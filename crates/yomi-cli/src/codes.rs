//! `yomi codes` (the list of codes of diagnostics, runtime errors, load errors and so on;
//! `spec/language/06-diagnostics.md`).
//!
//! By default, one line each with the group, code, severity, arguments and a one-line description in the current
//! language, separated by TAB. `--json` gives a machine-readable form independent of language (group, code, severity,
//! argument names).

use std::io::Write;

use lingokit::{FluentArgs, Localizer};
use serde_json::{Value, json};
use yomi_core::{CodeInfo, ErrorCodeInfo, Severity, all_codes, core_command_error_codes};

/// The stable English name of a severity.
fn severity_code(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        _ => "warning",
    }
}

/// The text of the description (arguments are filled in as `<name>`).
pub(crate) fn describe(loc: &Localizer, c: &CodeInfo) -> String {
    let mut args = FluentArgs::new();
    for a in c.args {
        args.set(*a, format!("<{a}>"));
    }
    loc.fmt(&format!("{}{}", c.group.ftl_prefix(), c.code), Some(&args))
}

/// One line of the human-readable list.
pub fn human_line(loc: &Localizer, c: &CodeInfo) -> String {
    let severity = match c.severity {
        Severity::Error => loc.fmt("severity-error", None),
        _ => loc.fmt("severity-warning", None),
    };
    format!(
        "{}\t{}\t{}\t{}\t{}",
        c.group.name(),
        c.code,
        severity,
        c.args.join(","),
        describe(loc, c).replace(['\n', '\t'], " ")
    )
}

/// The machine-readable list (JSON).
pub fn json(codes: &[CodeInfo]) -> String {
    let items: Vec<Value> = codes
        .iter()
        .map(|c| {
            json!({
                "group": c.group.name(),
                "code": c.code,
                "severity": severity_code(c.severity),
                "args": c.args,
            })
        })
        .collect();
    document(items)
}

/// Writes the whole list (`format`, `version`, `codes`) formatted. Adds a newline at the end.
fn document(items: Vec<Value>) -> String {
    let root = json!({ "format": "yomi-codes", "version": 1, "codes": items });
    let mut out = serde_json::to_string_pretty(&root).expect("JSON の値は、いつも書き出せる");
    out.push('\n');
    out
}

/// Kinds of command error codes (`--packages`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandKind {
    /// Errors of standard package commands (`yomi_std::error_codes()`).
    Package,
    /// Errors of core commands (`core_command_error_codes()`).
    Core,
}

impl CommandKind {
    /// The stable English name (the "group" column of the human-readable list, and `kind` in JSON).
    pub fn name(self) -> &'static str {
        match self {
            CommandKind::Package => "package",
            CommandKind::Core => "core",
        }
    }
}

/// All command error codes added by `--packages` (standard packages, then the core).
pub fn command_codes() -> Vec<(CommandKind, &'static ErrorCodeInfo)> {
    yomi_std::error_codes()
        .iter()
        .map(|c| (CommandKind::Package, c))
        .chain(
            core_command_error_codes()
                .iter()
                .map(|c| (CommandKind::Core, c)),
        )
        .collect()
}

/// The text of the description of a command error (arguments are filled in as `<name>`; empty if there is no
/// translation).
pub fn command_message(loc: &Localizer, c: &ErrorCodeInfo) -> String {
    let mut args = FluentArgs::new();
    for a in c.args {
        args.set(*a, format!("<{a}>"));
    }
    let key = crate::std_errors::ftl_key(c.code);
    if loc.has_own(&key) {
        loc.fmt(&key, Some(&args))
    } else {
        String::new()
    }
}

/// One line of the human-readable list of command errors (group, code, severity, arguments, description; the same 5
/// columns as language diagnostic codes).
pub fn command_human_line(loc: &Localizer, kind: CommandKind, c: &ErrorCodeInfo) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        kind.name(),
        c.code,
        loc.fmt("severity-error", None),
        c.args.join(","),
        command_message(loc, c).replace(['\n', '\t'], " ")
    )
}

/// The machine-readable list with `--packages` (JSON). Each entry has `kind` (`language`, `package`, `core`). Language
/// diagnostic codes also have `group`. It is separate from the form without `--packages` ([`json`](fn@json)), which stays
/// unchanged.
pub fn json_with_commands(codes: &[CodeInfo]) -> String {
    let mut items: Vec<Value> = codes
        .iter()
        .map(|c| {
            json!({
                "kind": "language",
                "group": c.group.name(),
                "code": c.code,
                "severity": severity_code(c.severity),
                "args": c.args,
            })
        })
        .collect();
    items.extend(command_codes().into_iter().map(|(kind, c)| {
        json!({
            "kind": kind.name(),
            "code": c.code,
            "severity": "error",
            "args": c.args,
        })
    }));
    document(items)
}

/// `yomi codes [--json] [--packages]`. `--packages` prints command error codes in addition to language diagnostic codes.
pub fn run<O: Write>(loc: &Localizer, as_json: bool, packages: bool, out: &mut O) -> i32 {
    let codes = all_codes();
    let text = if as_json {
        if packages {
            json_with_commands(&codes)
        } else {
            json(&codes)
        }
    } else {
        let mut s = String::new();
        for c in &codes {
            s.push_str(&human_line(loc, c));
            s.push('\n');
        }
        if packages {
            for (kind, c) in command_codes() {
                s.push_str(&command_human_line(loc, kind, c));
                s.push('\n');
            }
        }
        s
    };
    match out.write_all(text.as_bytes()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
