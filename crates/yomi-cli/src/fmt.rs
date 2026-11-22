//! `yomi fmt` (formats scripts; section 6.13 of `spec/host/06-cli.md`).
//!
//! The formatting is done by the core (`yomi_core::format_source`). This collects the files and, only after every file
//! has been formatted, rewrites the changed ones all together (the same way as `assign-ids`).

use std::io::{Read, Write};
use std::path::PathBuf;

use lingokit::{FluentArgs, Localizer};
use yomi_core::{FormatError, Severity, format_source};

use crate::assign_ids::{collect_targets, write_all_files};
use crate::diag::{diagnostic_line, error_line};
use crate::modules::{self, SourceFile};
use crate::texts::collect_error_line;

/// The options of `fmt`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The script files or directories (`-` for the standard input).
    pub paths: Vec<PathBuf>,
    /// The directory to search for modules.
    pub root: Option<PathBuf>,
    /// Does not write; reports the files that need formatting.
    pub check: bool,
    /// Prints the result instead of writing.
    pub stdout: bool,
    /// The path the standard input stands for.
    pub stdin_path: Option<PathBuf>,
}

/// Formats one file. On failure, writes the reason to `err` and returns `None`.
fn format_file<E: Write>(
    loc: &Localizer,
    files: &[SourceFile],
    index: usize,
    err: &mut E,
) -> Option<String> {
    let f = &files[index];
    let modules: Vec<(&str, &str)> = modules::all_sources(files);
    match format_source(&modules, &f.module) {
        Ok(text) => Some(text),
        Err(FormatError::Syntax(diags)) => {
            for d in diags.iter().filter(|d| d.severity == Severity::Error) {
                let _ = writeln!(err, "{}", diagnostic_line(loc, &f.path, d));
            }
            None
        }
        Err(FormatError::Unverified) => {
            let _ = writeln!(
                err,
                "{}",
                error_line(loc, &f.path, &loc.fmt("fmt-unverified", None))
            );
            None
        }
    }
}

fn skipped_line(loc: &Localizer, path: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("path", path.to_string());
    loc.fmt("fmt-schema-skipped", Some(&args))
}

/// `yomi fmt`. Returns the exit code (0 is success; 1 with `--check` when some file needs formatting, on syntax errors,
/// and on write failures).
pub fn run<I: Read, O: Write, E: Write>(
    loc: &Localizer,
    options: &Options,
    stdin: &mut I,
    out: &mut O,
    err: &mut E,
) -> i32 {
    if options.paths.len() == 1 && options.paths[0].as_os_str() == "-" {
        return run_stdin(loc, options, stdin, out, err);
    }
    if options.stdout && options.paths.len() != 1 {
        let _ = writeln!(
            err,
            "{}",
            error_line(loc, "yomi", &loc.fmt("fmt-stdout-one", None))
        );
        return 1;
    }
    let Some((files, target_modules)) =
        collect_targets(loc, &options.paths, options.root.as_deref(), err)
    else {
        return 1;
    };
    let mut results: Vec<(usize, String)> = Vec::new();
    let mut failed = false;
    for &i in &target_modules {
        if files[i].is_frozen_schema() {
            let _ = writeln!(err, "{}", skipped_line(loc, &files[i].path));
            continue;
        }
        match format_file(loc, &files, i, err) {
            Some(text) => results.push((i, text)),
            None => failed = true,
        }
    }
    if failed {
        return 1;
    }
    if options.stdout {
        if let Some((_, text)) = results.first() {
            let _ = write!(out, "{text}");
        }
        return 0;
    }
    let changed: Vec<&(usize, String)> = results
        .iter()
        .filter(|(i, text)| *text != files[*i].source)
        .collect();
    let mut count = FluentArgs::new();
    count.set("count", changed.len() as i64);
    if options.check {
        for (i, _) in &changed {
            let _ = writeln!(err, "{}: needs formatting", files[*i].path);
        }
        if changed.is_empty() {
            let _ = writeln!(out, "{}", loc.fmt("fmt-nothing", None));
            return 0;
        }
        let _ = writeln!(out, "{}", loc.fmt("fmt-check-summary", Some(&count)));
        return 1;
    }
    if changed.is_empty() {
        let _ = writeln!(out, "{}", loc.fmt("fmt-nothing", None));
        return 0;
    }
    let contents: Vec<(PathBuf, String)> = changed
        .iter()
        .map(|(i, text)| (PathBuf::from(&files[*i].path), text.clone()))
        .collect();
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
                &loc.fmt("fmt-write-error", Some(&args))
            )
        );
        return 1;
    }
    let _ = writeln!(out, "{}", loc.fmt("fmt-summary", Some(&count)));
    0
}

/// `yomi fmt - --stdin-path <path>`: formats the standard input as the file at the path and prints the result.
fn run_stdin<I: Read, O: Write, E: Write>(
    loc: &Localizer,
    options: &Options,
    stdin: &mut I,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let Some(path) = &options.stdin_path else {
        let _ = writeln!(
            err,
            "{}",
            error_line(loc, "yomi", &loc.fmt("fmt-stdin-needs-path", None))
        );
        return 1;
    };
    let mut input = Vec::new();
    let text = match stdin
        .read_to_end(&mut input)
        .map(|_| String::from_utf8(input))
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            let _ = writeln!(err, "{}", error_line(loc, "-", &e.utf8_error().to_string()));
            return 1;
        }
        Err(e) => {
            let _ = writeln!(err, "{}", error_line(loc, "-", &e.to_string()));
            return 1;
        }
    };
    let mut files = match modules::collect(path, options.root.as_deref()) {
        Ok((files, _)) => files,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, path, e));
            }
            return 1;
        }
    };
    let canon = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    let want = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
    let Some(i) = files.iter().position(|f| canon(&f.path) == want) else {
        let mut args = FluentArgs::new();
        args.set("path", path.display().to_string());
        let _ = writeln!(
            err,
            "{}",
            error_line(
                loc,
                &path.display().to_string(),
                &loc.fmt("fmt-stdin-outside-root", Some(&args))
            )
        );
        return 1;
    };
    files[i].source = text.clone();
    if files[i].is_frozen_schema() {
        let _ = writeln!(err, "{}", skipped_line(loc, &files[i].path));
        let _ = write!(out, "{text}");
        return 0;
    }
    match format_file(loc, &files, i, err) {
        Some(formatted) => {
            let _ = write!(out, "{formatted}");
            0
        }
        None => 1,
    }
}
