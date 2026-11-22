//! `yomi decls` and `yomi check` (declaration files; section 2.8 of `spec/host/02-registry.md`).
//!
//! `decls` writes the declaration file of the built-in registry (the same as `yomi run`). `check` compiles scripts and
//! prints diagnostics (errors and warnings). With `--decls`, it checks with a registry made from a declaration file
//! (without the host's Rust code).

use std::io::Write;
use std::path::Path;

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    CompileOptions, DeclError, DeclErrorKind, Registry, Severity, compile_modules_with_options,
};

use crate::diag::{diagnostic_line, error_line};
use crate::modules::{self, SourceFile};
use crate::session::registry;
use crate::texts::collect_error_line;

/// `yomi decls`. Writes the declaration file of the built-in registry to standard output.
pub fn export<O: Write>(out: &mut O) -> i32 {
    match write!(out, "{}", registry().export_decls()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// The text of a declaration file error (`decl-error-<code>`; with the location if there is one).
pub fn decl_error_message(loc: &Localizer, e: &DeclError) -> String {
    let id = format!("decl-error-{}", e.kind.code());
    let mut args = FluentArgs::new();
    match &e.kind {
        DeclErrorKind::Syntax { line, col } => {
            args.set("line", i64::from(*line));
            args.set("col", i64::from(*col));
        }
        DeclErrorKind::UnsupportedVersion(v) => args.set("version", *v),
        DeclErrorKind::InvalidType(t) => args.set("found", t.clone()),
        DeclErrorKind::Registry(r) => args.set("reason", r.to_string()),
        _ => {}
    }
    let message = loc.fmt(&id, Some(&args));
    if e.path.is_empty() {
        return message;
    }
    let mut at = FluentArgs::new();
    at.set("message", message);
    at.set("path", e.path.clone());
    loc.fmt("decl-error-at", Some(&at))
}

/// With `--decls`, the registry of the declaration file; otherwise the CLI's registry. On failure, returns a line to write
/// to standard error.
pub fn registry_for(loc: &Localizer, decls: Option<&Path>) -> Result<Registry, String> {
    match decls {
        Some(p) => read_decls(loc, p),
        None => Ok(registry()),
    }
}

/// Reads a declaration file into a registry. On failure, returns a line to write to standard error.
pub fn read_decls(loc: &Localizer, path: &Path) -> Result<Registry, String> {
    let shown = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|e| {
        let mut args = FluentArgs::new();
        args.set("path", shown.clone());
        args.set("reason", e.to_string());
        error_line(loc, &shown, &loc.fmt("run-read-error", Some(&args)))
    })?;
    Registry::from_decls(&source).map_err(|e| error_line(loc, &shown, &decl_error_message(loc, &e)))
}

/// `yomi check <script> [--decls <file>]`. Writes diagnostics to standard error and counts to standard output. 1 if there
/// are errors.
pub fn check<O: Write, E: Write>(
    loc: &Localizer,
    script: &Path,
    root: Option<&Path>,
    decls: Option<&Path>,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let reg = match registry_for(loc, decls) {
        Ok(r) => r,
        Err(line) => {
            let _ = writeln!(err, "{line}");
            return 1;
        }
    };
    let (files, entry): (Vec<SourceFile>, usize) = match modules::collect(script, root) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, script, e));
            }
            return 1;
        }
    };
    let sources = modules::sources(&files);
    let options = modules::options(&files, CompileOptions::default())
        .entry_label(&format!("{}.start", files[entry].module));
    let diags = match compile_modules_with_options(&sources, &reg, &options) {
        Ok((_, warnings)) => warnings,
        Err(diags) => diags,
    };
    let (mut errors, mut warnings) = (0i64, 0i64);
    for d in &diags {
        match d.severity {
            Severity::Error => errors += 1,
            _ => warnings += 1,
        }
        let _ = writeln!(
            err,
            "{}",
            diagnostic_line(loc, &modules::path_of(&files, &d.module), d)
        );
    }
    let mut args = FluentArgs::new();
    args.set("errors", errors);
    args.set("warnings", warnings);
    let key = if diags.is_empty() {
        "check-ok"
    } else {
        "check-summary"
    };
    let _ = writeln!(out, "{}", loc.fmt(key, Some(&args)));
    i32::from(errors > 0)
}
