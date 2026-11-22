//! `yomi assign-ids` (appends automatic stable IDs to lines without one; section 5.8 of `spec/host/05-texts.md`).
//!
//! The plan is made by the core (`yomi_core::plan_ids`). This collects the files and, only after everything compiles,
//! rewrites them all (writing to temporary files and renaming). The original text is unchanged except where `#id` is
//! added (including line endings and BOM; files that are not UTF-8 cannot be read, so they are not rewritten).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use lingokit::{FluentArgs, Localizer};
use yomi_core::{CompileOptions, IdRng, Insertion, Severity, StopKind, apply_insertions, plan_ids};

use crate::diag::{diagnostic_line, error_line};
use crate::modules::{self, SourceFile};
use crate::texts::collect_error_line;

/// The options of `assign-ids`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The script files to rewrite (for a directory, all `.yomi` under it).
    pub paths: Vec<PathBuf>,
    /// The directory to search for modules.
    pub root: Option<PathBuf>,
    /// The declaration file (the built-in registry if absent).
    pub decls: Option<PathBuf>,
    /// The seed of the random generator for IDs (the time if absent).
    pub seed: Option<u64>,
    /// Returns 1 if there are lines without IDs (does not write).
    pub check: bool,
    /// Shows the IDs to add and their positions without writing.
    pub dry_run: bool,
}

/// The default seed (from the startup time and the process ID).
fn default_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
}

/// The name of a kind of line.
pub(crate) fn kind_name(loc: &Localizer, kind: StopKind) -> String {
    let key = match kind {
        StopKind::Plain => "assign-ids-kind-plain",
        StopKind::Quote => "assign-ids-kind-quote",
        StopKind::Choice => "assign-ids-kind-choice",
        _ => "assign-ids-kind-command",
    };
    loc.fmt(key, None)
}

/// Collects the paths of the files to rewrite (for directories, the `.yomi` under them).
pub(crate) fn targets(paths: &[PathBuf]) -> Result<Vec<PathBuf>, modules::CollectError> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.is_dir() {
            out.extend(modules::scripts_under(p)?);
        } else {
            out.push(p.clone());
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    Ok(out)
}

/// The name of the temporary file before writing.
fn temp_path(path: &Path, n: usize) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(
        ".{name}.yomi-assign-{}-{n}.tmp",
        std::process::id()
    ))
}

/// Writes all contents to temporary files, then renames them. On failure, removes the temporary files.
pub(crate) fn write_all_files(
    files: &[(PathBuf, String)],
) -> Result<(), (PathBuf, std::io::Error)> {
    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, (path, content)) in files.iter().enumerate() {
        let tmp = temp_path(path, i);
        let result = std::fs::write(&tmp, content).and_then(|()| {
            // Keep the permissions of the original file
            match std::fs::metadata(path) {
                Ok(m) => std::fs::set_permissions(&tmp, m.permissions()),
                Err(_) => Ok(()),
            }
        });
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            for (t, _) in &temps {
                let _ = std::fs::remove_file(t);
            }
            return Err((path.clone(), e));
        }
        temps.push((tmp, path.clone()));
    }
    for (tmp, path) in &temps {
        if let Err(e) = std::fs::rename(tmp, path) {
            for (t, _) in &temps {
                let _ = std::fs::remove_file(t);
            }
            return Err((path.clone(), e));
        }
    }
    Ok(())
}

/// `yomi assign-ids`. Returns the exit code (0 is success; 1 with `--check` when there are lines without IDs, on compile
/// errors, and on write failures).
pub fn run<O: Write, E: Write>(
    loc: &Localizer,
    options: &Options,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let reg = match crate::decls::registry_for(loc, options.decls.as_deref()) {
        Ok(r) => r,
        Err(line) => {
            let _ = writeln!(err, "{line}");
            return 1;
        }
    };
    let Some((files, target_modules)) =
        collect_targets(loc, &options.paths, options.root.as_deref(), err)
    else {
        return 1;
    };
    let sources = modules::sources(&files);
    let mut rng = IdRng::new(options.seed.unwrap_or_else(default_seed));
    let plan = match plan_ids(
        &sources,
        &reg,
        &modules::options(&files, CompileOptions::default()),
        &mut rng,
    ) {
        Ok(p) => p,
        Err(diags) => {
            for d in diags.iter().filter(|d| d.severity == Severity::Error) {
                let _ = writeln!(
                    err,
                    "{}",
                    diagnostic_line(loc, &modules::path_of(&files, &d.module), d)
                );
            }
            return 1;
        }
    };
    apply_plan(loc, options, &files, &target_modules, &plan, out, err)
}

/// Collects the target scripts and the modules. Modules are collected from under the directory of the first script
/// (changeable with `--root`). Returns the collected modules and the indices of the modules that correspond to the target
/// files. On failure, writes to `err`.
pub(crate) fn collect_targets<E: Write>(
    loc: &Localizer,
    paths: &[PathBuf],
    root: Option<&Path>,
    err: &mut E,
) -> Option<(Vec<SourceFile>, Vec<usize>)> {
    let targets = match targets(paths) {
        Ok(t) => t,
        Err(e) => {
            let entry = paths.first().cloned().unwrap_or_default();
            let _ = writeln!(err, "{}", collect_error_line(loc, &entry, &e));
            return None;
        }
    };
    let Some(first) = targets.first() else {
        let _ = writeln!(
            err,
            "{}",
            error_line(loc, "yomi", &loc.fmt("assign-ids-no-files", None))
        );
        return None;
    };
    let root = match (root, paths.first()) {
        (Some(r), _) => Some(r.to_path_buf()),
        (None, Some(p)) if p.is_dir() => Some(p.clone()),
        _ => None,
    };
    let (files, _entry): (Vec<SourceFile>, usize) = match modules::collect(first, root.as_deref()) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, first, e));
            }
            return None;
        }
    };
    // Each target file is one of the collected modules
    let canon = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    let collected: HashMap<PathBuf, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (canon(&f.path), i))
        .collect();
    let mut target_modules: Vec<usize> = Vec::new();
    for t in &targets {
        match collected.get(&std::fs::canonicalize(t).unwrap_or_else(|_| t.clone())) {
            Some(&i) => target_modules.push(i),
            None => {
                let mut args = FluentArgs::new();
                args.set("path", t.display().to_string());
                args.set(
                    "root",
                    root.as_deref()
                        .or_else(|| first.parent())
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                );
                let _ = writeln!(
                    err,
                    "{}",
                    error_line(
                        loc,
                        &t.display().to_string(),
                        &loc.fmt("run-entry-outside-root", Some(&args))
                    )
                );
                return None;
            }
        }
    }
    Some((files, target_modules))
}

/// Reports the planned IDs for `--check` / `--dry-run`, otherwise writes them into the target files. Returns the exit
/// code.
fn apply_plan<O: Write, E: Write>(
    loc: &Localizer,
    options: &Options,
    files: &[SourceFile],
    target_modules: &[usize],
    plan: &[Insertion],
    out: &mut O,
    err: &mut E,
) -> i32 {
    // Only for the target modules to rewrite (module name → file index)
    let by_module: HashMap<&str, usize> = target_modules
        .iter()
        .map(|&i| (files[i].module.as_str(), i))
        .collect();
    let mine: Vec<&Insertion> = plan
        .iter()
        .filter(|i| by_module.contains_key(i.module.as_str()))
        .collect();
    let mut changed: Vec<usize> = mine.iter().map(|i| by_module[i.module.as_str()]).collect();
    changed.sort_unstable();
    changed.dedup();
    let line_args = |i: &Insertion| {
        let mut args = FluentArgs::new();
        args.set("path", files[by_module[i.module.as_str()]].path.clone());
        args.set("line", i64::from(i.line));
        args.set("col", i64::from(i.col));
        args.set("id", i.id.clone());
        args.set("label", i.label.clone());
        args.set("kind", kind_name(loc, i.kind));
        args
    };
    let mut count = FluentArgs::new();
    count.set("count", mine.len() as i64);
    count.set("files", changed.len() as i64);

    if options.check {
        for i in &mine {
            let _ = writeln!(
                err,
                "{}",
                loc.fmt("assign-ids-missing-line", Some(&line_args(i)))
            );
        }
        if mine.is_empty() {
            let _ = writeln!(out, "{}", loc.fmt("assign-ids-nothing", None));
            return 0;
        }
        let _ = writeln!(out, "{}", loc.fmt("assign-ids-check-summary", Some(&count)));
        return 1;
    }
    if mine.is_empty() {
        let _ = writeln!(out, "{}", loc.fmt("assign-ids-nothing", None));
        return 0;
    }
    if options.dry_run {
        for i in &mine {
            let _ = writeln!(
                out,
                "{}",
                loc.fmt("assign-ids-plan-line", Some(&line_args(i)))
            );
        }
        let _ = writeln!(out, "{}", loc.fmt("assign-ids-dry-summary", Some(&count)));
        return 0;
    }
    let mut contents: Vec<(PathBuf, String)> = Vec::new();
    for &fi in &changed {
        let f = &files[fi];
        let mine_here: Vec<Insertion> = mine
            .iter()
            .filter(|i| i.module == f.module)
            .map(|i| (*i).clone())
            .collect();
        contents.push((
            PathBuf::from(&f.path),
            apply_insertions(&f.source, &mine_here),
        ));
    }
    if let Err((path, e)) = write_all_files(&contents) {
        let mut args = FluentArgs::new();
        args.set("path", path.display().to_string());
        args.set("reason", e.to_string());
        let _ = writeln!(
            err,
            "{}",
            error_line(
                loc,
                &path.display().to_string(),
                &loc.fmt("assign-ids-write-error", Some(&args))
            )
        );
        return 1;
    }
    let _ = writeln!(out, "{}", loc.fmt("assign-ids-summary", Some(&count)));
    0
}
