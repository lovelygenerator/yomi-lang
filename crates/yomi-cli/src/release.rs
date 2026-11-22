//! `yomi release` (section 6.16 of `spec/host/06-cli.md`): the checks before a release, then freezing the schema.

use std::io::Write;
use std::path::{Path, PathBuf};

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    CompileOptions, FormatError, LoadPolicy, Program, Severity, Vm, VmConfig,
    compile_modules_with_options, format_source,
};

use crate::assign_ids::kind_name;
use crate::diag::{diagnostic_line, error_line, load_kind_message};
use crate::modules::{self, SourceFile};
use crate::schema::{self, ProjectOptions};
use crate::texts::collect_error_line;

/// The options of `release`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The directory to search for modules.
    pub root: Option<PathBuf>,
    /// The declaration file.
    pub decls: Option<PathBuf>,
    /// The saves and persistent data to check loading of.
    pub saves: Vec<PathBuf>,
    /// Only checks (does not write the frozen schema).
    pub dry_run: bool,
}

/// The files under the given paths (directories: every file under them), in path order.
fn files_under(paths: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let p = e?.path();
            if p.is_dir() {
                walk(&p, out)?;
            } else {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut inner = Vec::new();
            walk(p, &mut inner)?;
            inner.sort();
            out.extend(inner);
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

/// The label the VM starts from when checking loads: `main.start` if there is one, otherwise the first label.
fn start_label(files: &[SourceFile], program: &Program) -> Option<String> {
    let mut first = None;
    for f in files
        .iter()
        .filter(|f| !f.is_migrations() && !f.is_frozen_schema())
    {
        let labels = program.labels_of(&f.module);
        if f.module == "main" && labels.contains(&"start") {
            return Some("main.start".to_string());
        }
        if first.is_none() {
            first = labels.first().map(|l| format!("{}.{l}", f.module));
        }
    }
    first
}

/// Step 3a: loads each file with `Strict`. Returns the number that loaded and the number that failed.
fn check_loads<O: Write, E: Write>(
    loc: &Localizer,
    options: &Options,
    files: &[SourceFile],
    program: &Program,
    reg: &yomi_core::Registry,
    out: &mut O,
    err: &mut E,
) -> (usize, usize) {
    let paths = match files_under(&options.saves) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{}", error_line(loc, "yomi", &e.to_string()));
            return (0, 1);
        }
    };
    let Some(start) = start_label(files, program) else {
        let _ = writeln!(
            err,
            "{}",
            error_line(loc, "yomi", &loc.fmt("release-no-label", None))
        );
        return (0, 1);
    };
    let cfg = || {
        VmConfig::default()
            .start(&start)
            .load_policy(LoadPolicy::Strict)
    };
    let (mut ok, mut bad) = (0, 0);
    for path in paths {
        let shown = path.display().to_string();
        let report = |err: &mut E, msg: String| {
            let _ = writeln!(err, "{}", error_line(loc, &shown, &msg));
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                report(err, e.to_string());
                bad += 1;
                continue;
            }
        };
        let result = match bytes.get(..4) {
            Some(b"YOMS") => Vm::load(program, reg, cfg(), &bytes)
                .map(|_| ())
                .map_err(|e| load_kind_message(loc, &e.kind)),
            Some(b"YOMP") => match Vm::new(program, reg, cfg()) {
                Ok(mut vm) => vm
                    .load_persistent(&bytes)
                    .map(|_| ())
                    .map_err(|e| load_kind_message(loc, &e.kind)),
                Err(e) => Err(e.to_string()),
            },
            _ => Err(loc.fmt("release-not-a-save", None)),
        };
        match result {
            Ok(()) => ok += 1,
            Err(msg) => {
                report(err, msg);
                bad += 1;
            }
        }
    }
    if bad == 0 {
        let mut args = FluentArgs::new();
        args.set("count", ok as i64);
        let _ = writeln!(out, "{}", loc.fmt("release-loads-ok", Some(&args)));
    }
    (ok, bad)
}

/// `yomi release`. Returns the exit code.
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
    let root = options.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let files = match modules::collect_dir(&root) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, &root, e));
            }
            return 1;
        }
    };
    // 1. strict check: warnings are errors too
    let sources = modules::sources(&files);
    let opts = modules::options(&files, CompileOptions::default());
    let (program, warnings) = match compile_modules_with_options(&sources, &reg, &opts) {
        Ok(r) => r,
        Err(diags) => {
            for d in &diags {
                let mut d = d.clone();
                d.severity = Severity::Error;
                let _ = writeln!(
                    err,
                    "{}",
                    diagnostic_line(loc, &modules::path_of(&files, &d.module), &d)
                );
            }
            return 1;
        }
    };
    if !warnings.is_empty() {
        for d in &warnings {
            let mut d = d.clone();
            d.severity = Severity::Error;
            let _ = writeln!(
                err,
                "{}",
                diagnostic_line(loc, &modules::path_of(&files, &d.module), &d)
            );
        }
        return 1;
    }
    // 2. stable IDs
    let missing = program.missing_ids();
    if !missing.is_empty() {
        for m in &missing {
            let mut args = FluentArgs::new();
            let module_path = modules::path_of(&files, &m.module);
            args.set("path", module_path);
            args.set("line", i64::from(m.line));
            args.set("col", i64::from(m.col));
            args.set("label", m.label.clone());
            args.set("kind", kind_name(loc, m.kind));
            let _ = writeln!(err, "{}", loc.fmt("assign-ids-missing-line", Some(&args)));
        }
        return 1;
    }
    // 2a. formatting
    let all = modules::all_sources(&files);
    let mut unformatted = false;
    for f in files.iter().filter(|f| !f.is_frozen_schema()) {
        match format_source(&all, &f.module) {
            Ok(text) if text == f.source => {}
            Ok(_) => {
                let _ = writeln!(err, "{}: needs formatting", f.path);
                unformatted = true;
            }
            Err(FormatError::Unverified) => {
                let _ = writeln!(
                    err,
                    "{}",
                    error_line(loc, &f.path, &loc.fmt("fmt-unverified", None))
                );
                unformatted = true;
            }
            Err(FormatError::Syntax(_)) => unformatted = true,
        }
    }
    if unformatted {
        return 1;
    }
    // 3a. loading saves
    if !options.saves.is_empty() {
        let (_, bad) = check_loads(loc, options, &files, &program, &reg, out, err);
        if bad > 0 {
            return 1;
        }
    }
    // 4. freeze
    let project = ProjectOptions {
        root: options.root.clone(),
        decls: options.decls.clone(),
        write: false,
    };
    if options.dry_run {
        let path = root
            .join("schema")
            .join(format!("v{}.yomi", program.schema_version()));
        let _ = writeln!(out, "{}", path.display());
        let _ = writeln!(out, "{}", loc.fmt("release-dry-ok", None));
        return 0;
    }
    if schema::freeze(loc, &project, out, err) != 0 {
        return 1;
    }
    let _ = writeln!(out, "{}", loc.fmt("release-ok", None));
    0
}
