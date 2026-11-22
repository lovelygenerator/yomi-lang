//! `yomi schema freeze`, `yomi schema extract` and `yomi migrate-init` (sections 6.14 and 6.15 of `spec/host/06-cli.md`).
//!
//! The content is made by the core (`yomi_core::frozen_schema`, `extract_schema`, `migrate_template`); this reads and writes
//! the files.

use std::io::Write;
use std::path::{Path, PathBuf};

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    CompileOptions, ExtractError, Program, Severity, compile_modules_with_options, extract_schema,
    frozen_schema, migrate_template,
};

use crate::diag::{diagnostic_line, error_line, load_kind_message};
use crate::modules::{self, SourceFile};
use crate::texts::collect_error_line;

/// The options of `schema freeze` and `migrate-init`.
#[derive(Debug, Clone, Default)]
pub struct ProjectOptions {
    /// The directory to search for modules (the current directory if absent).
    pub root: Option<PathBuf>,
    /// The declaration file.
    pub decls: Option<PathBuf>,
    /// `migrate-init`: appends the template to `migrations.yomi`.
    pub write: bool,
}

fn fail<E: Write>(
    loc: &Localizer,
    err: &mut E,
    subject: &str,
    key: &str,
    args: Option<&FluentArgs>,
) -> i32 {
    let _ = writeln!(err, "{}", error_line(loc, subject, &loc.fmt(key, args)));
    1
}

/// Collects and compiles the project. On failure, writes the reason to `err`.
fn load<E: Write>(
    loc: &Localizer,
    options: &ProjectOptions,
    err: &mut E,
) -> Option<(PathBuf, Vec<SourceFile>, Program)> {
    let reg = match crate::decls::registry_for(loc, options.decls.as_deref()) {
        Ok(r) => r,
        Err(line) => {
            let _ = writeln!(err, "{line}");
            return None;
        }
    };
    let root = options.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let files = match modules::collect_dir(&root) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", collect_error_line(loc, &root, e));
            }
            return None;
        }
    };
    let sources = modules::sources(&files);
    let opts = modules::options(&files, CompileOptions::default());
    match compile_modules_with_options(&sources, &reg, &opts) {
        Ok((program, _)) => Some((root, files, program)),
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

/// `yomi schema freeze`.
pub fn freeze<O: Write, E: Write>(
    loc: &Localizer,
    options: &ProjectOptions,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let Some((root, _files, program)) = load(loc, options, err) else {
        return 1;
    };
    let text = frozen_schema(&program);
    let path = root
        .join("schema")
        .join(format!("v{}.yomi", program.schema_version()));
    let shown = path.display().to_string();
    let mut args = FluentArgs::new();
    args.set("path", shown.clone());
    if path.exists() {
        return match std::fs::read_to_string(&path) {
            Ok(old) if old == text => {
                let _ = writeln!(out, "{}", loc.fmt("schema-freeze-latest", Some(&args)));
                0
            }
            Ok(_) => fail(loc, err, &shown, "schema-freeze-changed", Some(&args)),
            Err(e) => {
                args.set("reason", e.to_string());
                fail(loc, err, &shown, "schema-write-error", Some(&args))
            }
        };
    }
    let result =
        std::fs::create_dir_all(root.join("schema")).and_then(|()| std::fs::write(&path, &text));
    if let Err(e) = result {
        args.set("reason", e.to_string());
        return fail(loc, err, &shown, "schema-write-error", Some(&args));
    }
    let _ = writeln!(out, "{shown}");
    0
}

/// `yomi schema extract`.
pub fn extract<O: Write, E: Write>(
    loc: &Localizer,
    save: &Path,
    persistent: Option<&Path>,
    to: Option<&Path>,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let read = |p: &Path, err: &mut E| match std::fs::read(p) {
        Ok(b) => Some(b),
        Err(e) => {
            let mut args = FluentArgs::new();
            args.set("path", p.display().to_string());
            args.set("reason", e.to_string());
            fail(
                loc,
                err,
                &p.display().to_string(),
                "schema-read-error",
                Some(&args),
            );
            None
        }
    };
    let Some(save_bytes) = read(save, err) else {
        return 1;
    };
    let persistent_bytes = match persistent {
        Some(p) => match read(p, err) {
            Some(b) => Some(b),
            None => return 1,
        },
        None => None,
    };
    let text = match extract_schema(Some(&save_bytes), persistent_bytes.as_deref()) {
        Ok(t) => t,
        Err(ExtractError::Load(e)) => {
            let _ = writeln!(
                err,
                "{}",
                error_line(
                    loc,
                    &save.display().to_string(),
                    &load_kind_message(loc, &e.kind)
                )
            );
            return 1;
        }
        Err(ExtractError::VersionsDiffer {
            save: s,
            persistent: p,
        }) => {
            let mut args = FluentArgs::new();
            args.set("save", i64::from(s));
            args.set("persistent", i64::from(p));
            return fail(
                loc,
                err,
                "yomi",
                "schema-extract-versions-differ",
                Some(&args),
            );
        }
        Err(_) => return fail(loc, err, "yomi", "schema-extract-nothing", None),
    };
    match to {
        None => {
            let _ = write!(out, "{text}");
            0
        }
        Some(path) => {
            let shown = path.display().to_string();
            let mut args = FluentArgs::new();
            args.set("path", shown.clone());
            if path.exists() {
                return fail(loc, err, &shown, "schema-extract-exists", Some(&args));
            }
            match std::fs::write(path, &text) {
                Ok(()) => {
                    let _ = writeln!(out, "{shown}");
                    0
                }
                Err(e) => {
                    args.set("reason", e.to_string());
                    fail(loc, err, &shown, "schema-write-error", Some(&args))
                }
            }
        }
    }
}

/// The version of a frozen schema file (`schema::v2` is 2).
fn frozen_version(f: &SourceFile) -> Option<u32> {
    f.module.strip_prefix("schema::v")?.parse().ok()
}

/// `yomi migrate-init`.
pub fn migrate_init<O: Write, E: Write>(
    loc: &Localizer,
    options: &ProjectOptions,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let Some((root, files, program)) = load(loc, options, err) else {
        return 1;
    };
    let latest = files
        .iter()
        .filter(|f| f.is_frozen_schema())
        .filter_map(|f| frozen_version(f).map(|v| (v, f)))
        .max_by_key(|(v, _)| *v);
    let Some((k, frozen)) = latest else {
        return fail(loc, err, "yomi", "migrate-init-no-frozen", None);
    };
    let mut args = FluentArgs::new();
    args.set("version", i64::from(k + 1));
    if program.schema_version() > k {
        return fail(loc, err, "yomi", "migrate-init-exists", Some(&args));
    }
    let template = match migrate_template(&program, &frozen.source, k) {
        Ok(t) => t,
        Err(diags) => {
            for d in diags.iter().filter(|d| d.severity == Severity::Error) {
                let _ = writeln!(err, "{}", diagnostic_line(loc, &frozen.path, d));
            }
            return 1;
        }
    };
    let Some(template) = template else {
        let _ = writeln!(out, "{}", loc.fmt("migrate-init-nothing", None));
        return 0;
    };
    if !options.write {
        let _ = write!(out, "{template}");
        return 0;
    }
    let path = root.join("migrations.yomi");
    let shown = path.display().to_string();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut text = existing.clone();
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&template);
    if let Err(e) = std::fs::write(&path, text) {
        let mut a = FluentArgs::new();
        a.set("path", shown.clone());
        a.set("reason", e.to_string());
        return fail(loc, err, &shown, "schema-write-error", Some(&a));
    }
    let _ = writeln!(out, "{shown}");
    0
}
