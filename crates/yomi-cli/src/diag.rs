//! Makes the text of diagnostics, runtime errors and load errors.
//!
//! The core holds only codes and arguments. Here, text is made from FTL with the code as the key.
//! Only functions that return strings live here; output to the terminal is done by the caller.

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    DiagArg, DiagCode, Diagnostic, LoadError, LoadErrorKind, Severity, TextError, TextErrorKind,
    TextIssue, TextIssueKind, VmError, VmErrorKind,
};

/// Turns diagnostic arguments into Fluent arguments.
pub fn fluent_args(args: &[(&'static str, DiagArg)]) -> FluentArgs<'static> {
    let mut out = FluentArgs::new();
    for (name, value) in args {
        match value {
            DiagArg::Str(s) => out.set(*name, s.clone()),
            DiagArg::Int(n) => out.set(*name, *n),
            DiagArg::List(items) => out.set(*name, items.join(", ")),
        }
    }
    out
}

/// Makes the text of a diagnostic from its code and arguments (`diag-<code>`).
///
/// If there is a `suggestions` argument (candidate close names), appends "did you mean" (`diag-did-you-mean`).
pub fn code_message(loc: &Localizer, code: DiagCode, args: &[(&'static str, DiagArg)]) -> String {
    let id = format!("diag-{}", code.as_str());
    let message = loc.fmt(&id, Some(&fluent_args(args)));
    let suggestions = args.iter().find_map(|(name, value)| match value {
        DiagArg::List(items) if *name == "suggestions" && !items.is_empty() => Some(items),
        _ => None,
    });
    let Some(items) = suggestions else {
        return message;
    };
    let mut out = FluentArgs::new();
    out.set("message", message);
    out.set("suggestions", items.join(", "));
    loc.fmt("diag-did-you-mean", Some(&out))
}

/// The text of a diagnostic.
pub fn diagnostic_message(loc: &Localizer, d: &Diagnostic) -> String {
    code_message(loc, d.code, &d.args)
}

/// The name of a severity.
fn severity_name(loc: &Localizer, severity: Severity) -> String {
    match severity {
        Severity::Error => loc.fmt("severity-error", None),
        _ => loc.fmt("severity-warning", None),
    }
}

/// One line `<file>:<line>:<col>: <severity>: <text>`.
pub fn diagnostic_line(loc: &Localizer, file: &str, d: &Diagnostic) -> String {
    format!(
        "{file}:{}:{}: {}: {}",
        d.line,
        d.col,
        severity_name(loc, d.severity),
        diagnostic_message(loc, d)
    )
}

/// The arguments of a runtime error (values per kind).
fn vm_kind_args(loc: &Localizer, kind: &VmErrorKind) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    match kind {
        VmErrorKind::BadInput { expected, found } => {
            args.set("expected", expected.name());
            args.set("found", found.name());
        }
        VmErrorKind::ChoiceOutOfRange { index, count } => {
            args.set("index", *index);
            args.set("count", *count);
        }
        VmErrorKind::StartLabelNotFound(name)
        | VmErrorKind::CommandMissing(name)
        | VmErrorKind::UnknownVariable(name)
        | VmErrorKind::SliceSerialize(name) => {
            args.set("name", name.clone());
        }
        VmErrorKind::CommandFailed { code, args: a } => {
            // `vm-command_failed` (for codes without text) prints the code and arguments as they are
            args.set("code", crate::std_errors::raw(code, a));
        }
        VmErrorKind::PersistentData(load) => args.set("reason", load_kind_message(loc, load)),
        VmErrorKind::KeyNotFound { key } => args.set("key", key.clone()),
        VmErrorKind::IndexOutOfRange { index, len } => {
            args.set("index", *index);
            args.set("len", *len);
        }
        VmErrorKind::InvalidRandomRange { min, max } => {
            args.set("min", *min);
            args.set("max", *max);
        }
        _ => {}
    }
    args
}

/// The text of a runtime error (`vm-<code>`; with the location if known).
pub fn vm_error_message(loc: &Localizer, e: &VmError) -> String {
    vm_kind_message(
        loc,
        &e.kind,
        (e.module.as_deref(), e.label.as_deref()),
        e.line,
    )
}

/// Makes the text of a runtime error from its kind and location (module, label).
///
/// If the module is known, the label is shown as `module.label` (the same as how it is written in scripts).
pub fn vm_kind_message(
    loc: &Localizer,
    kind: &VmErrorKind,
    place: (Option<&str>, Option<&str>),
    line: Option<u32>,
) -> String {
    let message = match kind.command_error() {
        // Package errors use text per code (`std-error-<code>`). If there is none, the code and arguments as they are
        Some((code, args)) => crate::std_errors::message(loc, code, args),
        None => {
            let id = format!("vm-{}", kind.code());
            loc.fmt(&id, Some(&vm_kind_args(loc, kind)))
        }
    };
    let (module, label) = place;
    let Some(label) = label else {
        return message;
    };
    let label = match module {
        Some(m) => format!("{m}.{label}"),
        None => label.to_string(),
    };
    let mut args = FluentArgs::new();
    args.set("message", message);
    args.set("label", label);
    let key = match line {
        Some(line) => {
            args.set("line", i64::from(line));
            "vm-located"
        }
        None => "vm-located-label",
    };
    loc.fmt(key, Some(&args))
}

/// The text of a load error (`load-<code>`).
pub fn load_error_message(loc: &Localizer, e: &LoadError) -> String {
    load_kind_message(loc, &e.kind)
}

/// Makes the text of a load error from its kind.
pub fn load_kind_message(loc: &Localizer, kind: &LoadErrorKind) -> String {
    let id = format!("load-{}", kind.code());
    let mut args = FluentArgs::new();
    match kind {
        LoadErrorKind::UnsupportedVersion(v) => args.set("version", i64::from(*v)),
        LoadErrorKind::LabelNotFound(name)
        | LoadErrorKind::ModuleMismatch(name)
        | LoadErrorKind::CommandMissing(name)
        | LoadErrorKind::PackageSlice(name) => {
            args.set("name", name.clone());
        }
        LoadErrorKind::SaveFromNewerVersion { saved, current } => {
            args.set("saved", i64::from(*saved));
            args.set("current", i64::from(*current));
        }
        LoadErrorKind::SaveVersionUnsupported { saved, min } => {
            args.set("saved", i64::from(*saved));
            args.set("min", i64::from(*min));
        }
        LoadErrorKind::SchemaMismatch(diffs) => {
            let lines: Vec<String> = diffs
                .iter()
                .map(|d| format!("{}: {} -> {}", d.name, d.saved, d.current))
                .collect();
            args.set("vars", lines.join("; "));
        }
        LoadErrorKind::MigrateFailed(names) => args.set("vars", names.join(", ")),
        _ => {}
    }
    loc.fmt(&id, Some(&args))
}

/// One line `<file>: <error>: <text>` (for errors without a location).
pub fn error_line(loc: &Localizer, file: &str, message: &str) -> String {
    format!("{file}: {}: {message}", loc.fmt("severity-error", None))
}

/// The text of a text table error (in a translation or a line of the table file) (`text-error-<code>`).
pub fn text_error_message(loc: &Localizer, e: &TextError) -> String {
    let id = format!("text-error-{}", e.kind.code());
    let mut args = FluentArgs::new();
    match &e.kind {
        TextErrorKind::InvalidEscape { found } | TextErrorKind::InvalidPlaceholder { found } => {
            args.set("found", found.clone());
        }
        TextErrorKind::UnknownDirective { name } => args.set("name", name.clone()),
        _ => {}
    }
    args.set("key", e.key.clone().unwrap_or_default());
    loc.fmt(&id, Some(&args))
}

/// One line for an error in a table file (`<table>:<line>: <severity>: <text>`; the line is omitted if there is none).
pub fn text_error_line(loc: &Localizer, file: &str, e: &TextError) -> String {
    let place = match e.line {
        Some(n) => format!("{file}:{n}"),
        None => file.to_string(),
    };
    error_line(loc, &place, &text_error_message(loc, e))
}

/// The text of a result of checking a text table against the scripts (`text-issue-<code>`).
pub fn text_issue_message(loc: &Localizer, i: &TextIssue) -> String {
    let id = format!("text-issue-{}", i.kind.code());
    let mut args = FluentArgs::new();
    args.set("key", i.key.clone());
    if let TextIssueKind::BadNumber { number, count } = &i.kind {
        args.set("number", i64::from(*number));
        args.set("count", i64::from(*count));
    }
    loc.fmt(&id, Some(&args))
}
