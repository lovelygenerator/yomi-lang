//! `yomi texts check` and `yomi texts template` (tools for text tables that replace bodies; `spec/host/05-texts.md`).
//!
//! Collects and compiles the scripts, and checks a text table against them or makes a template. Output to the terminal is
//! done through strings returned to the caller (`main.rs`) and writes to standard output and standard error.

use std::io::Write;
use std::path::Path;

use lingokit::{FluentArgs, Localizer};
use yomi_core::{Program, Severity, TextIssue, TextTable, compile_modules_with_options};

use crate::diag::{diagnostic_line, error_line, text_error_line, text_issue_message};
use crate::modules::{self, CollectError, SourceFile};
use crate::session::registry;

/// Reads and parses a table file. On failure, returns a line to write to standard error (`path:line: error: …`).
pub fn read_table(loc: &Localizer, path: &Path) -> Result<TextTable, String> {
    let shown = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|e| {
        let mut args = FluentArgs::new();
        args.set("path", shown.clone());
        args.set("reason", e.to_string());
        error_line(loc, &shown, &loc.fmt("run-read-error", Some(&args)))
    })?;
    TextTable::from_file(&source).map_err(|e| text_error_line(loc, &shown, &e))
}

/// Collects (the entry file and the modules in the same directory) and compiles the scripts. On failure, writes the
/// diagnostics to `err` and returns `None`. Warnings are dropped (the purpose is checking the table).
fn compile_script<E: Write>(
    loc: &Localizer,
    script: &Path,
    root: Option<&Path>,
    err: &mut E,
) -> Option<Program> {
    let (files, _entry): (Vec<SourceFile>, usize) = match modules::collect(script, root) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, script, e));
            }
            return None;
        }
    };
    let sources = modules::sources(&files);
    match compile_modules_with_options(
        &sources,
        &registry(),
        &modules::options(&files, Default::default()),
    ) {
        Ok((program, _warnings)) => Some(program),
        Err(diags) => {
            for d in diags.iter().filter(|d| d.severity == Severity::Error) {
                let _ = writeln!(
                    err,
                    "{}",
                    diagnostic_line(loc, &modules::path_of(&files, &d.module), d)
                );
            }
            None
        }
    }
}

/// Puts the reason the modules could not be collected on one line.
pub(crate) fn collect_error_line(
    loc: &Localizer,
    entry: impl AsRef<Path>,
    e: &CollectError,
) -> String {
    let mut args = FluentArgs::new();
    match e {
        CollectError::Read { path, reason } => {
            args.set("path", path.clone());
            args.set("reason", reason.clone());
            error_line(loc, path, &loc.fmt("run-read-error", Some(&args)))
        }
        CollectError::EntryOutsideRoot { path, root } => {
            args.set("path", path.clone());
            args.set("root", root.clone());
            error_line(
                loc,
                &entry.as_ref().display().to_string(),
                &loc.fmt("run-entry-outside-root", Some(&args)),
            )
        }
    }
}

/// The line for one check result (`<table>: <severity>: <text>`).
pub fn issue_line(loc: &Localizer, table_path: &str, issue: &TextIssue) -> String {
    let severity = match issue.severity {
        Severity::Error => loc.fmt("severity-error", None),
        _ => loc.fmt("severity-warning", None),
    };
    format!(
        "{table_path}: {severity}: {}",
        text_issue_message(loc, issue)
    )
}

/// `yomi texts check <script> <table>`. Prints missing translations, extra translations and number errors to standard
/// error. 1 if there are errors, 0 if there are only warnings.
pub fn check<O: Write, E: Write>(
    loc: &Localizer,
    script: &Path,
    table_path: &Path,
    root: Option<&Path>,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let Some(program) = compile_script(loc, script, root, err) else {
        return 1;
    };
    let table = match read_table(loc, table_path) {
        Ok(t) => t,
        Err(line) => {
            let _ = writeln!(err, "{line}");
            return 1;
        }
    };
    let shown = table_path.display().to_string();
    let issues = table.check(&program);
    let (mut errors, mut warnings) = (0i64, 0i64);
    for i in &issues {
        match i.severity {
            Severity::Error => errors += 1,
            _ => warnings += 1,
        }
        let _ = writeln!(err, "{}", issue_line(loc, &shown, i));
    }
    let mut args = FluentArgs::new();
    args.set("errors", errors);
    args.set("warnings", warnings);
    let key = if issues.is_empty() {
        "texts-check-ok"
    } else {
        "texts-check-summary"
    };
    let _ = writeln!(out, "{}", loc.fmt(key, Some(&args)));
    i32::from(errors > 0)
}

/// `yomi texts template <script>`. Writes to standard output a template table whose values are the originals, from the
/// body lines with IDs.
pub fn template<O: Write, E: Write>(
    loc: &Localizer,
    script: &Path,
    root: Option<&Path>,
    language: &str,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let Some(program) = compile_script(loc, script, root, err) else {
        return 1;
    };
    let table = TextTable::template(&program, language);
    match write!(out, "{}", table.to_file()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
