//! Turns runtime errors of package commands (`VmErrorKind::CommandFailed { code, args }`) into text in the display
//! language.
//!
//! The core holds only the code (`package.error_name`) and named arguments (section 2.4 of spec/host/02-registry.md).
//! The CLI shows the FTL `std-error-<code>` (with `.` in the code replaced by `-`), filling arguments as `$name`.
//! Codes of the standard packages (`yomi_std::error_codes()`) and of the core (`yomi_core::core_command_error_codes()`)
//! are in both en and ja (checked by tests). Codes not in FTL (those of the host or third-party packages) are printed as
//! the code and arguments as they are.

use lingokit::{FluentArgs, Localizer};
use yomi_core::DiagArg;

/// Makes the FTL key (`std-error-<code>`, with `.` in the code replaced by `-`) from a code.
pub fn ftl_key(code: &str) -> String {
    format!("std-error-{}", code.replace('.', "-"))
}

/// Puts the code and arguments on one line as they are (`code (name=value, …)`).
pub fn raw(code: &str, args: &[(&'static str, DiagArg)]) -> String {
    if args.is_empty() {
        return code.to_string();
    }
    let list: Vec<String> = args.iter().map(|(n, v)| format!("{n}={v}")).collect();
    format!("{code} ({})", list.join(", "))
}

/// The text of a command failure. Codes in FTL are in the display language; codes not in FTL print the code and arguments
/// as they are.
pub fn message(loc: &Localizer, code: &str, args: &[(&'static str, DiagArg)]) -> String {
    let key = ftl_key(code);
    if loc.has_own(&key) {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            match value {
                DiagArg::Int(n) => fluent.set(*name, *n),
                other => fluent.set(*name, other.to_string()),
            }
        }
        return loc.fmt(&key, Some(&fluent));
    }
    let mut fluent = FluentArgs::new();
    fluent.set("code", raw(code, args));
    loc.fmt("vm-command_failed", Some(&fluent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list of keys in the FTL file that start with `std-error-`.
    fn ftl_keys(ftl: &str) -> Vec<String> {
        ftl.lines()
            .filter_map(|l| l.strip_prefix("std-error-"))
            .filter_map(|l| l.split_once(" = "))
            .map(|(k, _)| format!("std-error-{k}"))
            .collect()
    }

    fn all_codes() -> Vec<(&'static str, &'static [&'static str])> {
        yomi_std::error_codes()
            .iter()
            .chain(yomi_core::core_command_error_codes())
            .map(|c| (c.code, c.args))
            .collect()
    }

    #[test]
    fn all_command_error_codes_have_text_in_en_and_ja() {
        for lang in ["en", "ja"] {
            let loc = Localizer::only(&crate::LOCALES, lang);
            for (code, names) in all_codes() {
                let key = ftl_key(code);
                assert!(loc.has_own(&key), "{lang} に {key} がない");
                let args: Vec<(&'static str, DiagArg)> =
                    names.iter().map(|n| (*n, DiagArg::Int(7))).collect();
                let text = message(&loc, code, &args);
                assert!(
                    !text.contains("{$") && text != key && !text.is_empty(),
                    "{lang} {code}: {text}"
                );
            }
        }
    }

    #[test]
    fn ftl_has_no_keys_without_codes() {
        let codes: Vec<String> = all_codes().iter().map(|(c, _)| ftl_key(c)).collect();
        for (lang, ftl) in [
            ("en", include_str!("../locales/en/main.ftl")),
            ("ja", include_str!("../locales/ja/main.ftl")),
        ] {
            let keys = ftl_keys(ftl);
            for k in &keys {
                assert!(codes.contains(k), "{lang}: {k} に対応するコードがない");
            }
            assert_eq!(
                keys.len(),
                codes.len(),
                "{lang}: キーの数がコードの数と違う"
            );
        }
    }

    #[test]
    fn unknown_codes_print_code_and_args_as_is() {
        let args = [("a", DiagArg::Int(1)), ("b", DiagArg::Str("x y".into()))];
        for lang in ["en", "ja"] {
            let loc = Localizer::only(&crate::LOCALES, lang);
            assert_eq!(
                message(&loc, "host.unknown", &args),
                "host.unknown (a=1, b=x y)"
            );
            assert_eq!(message(&loc, "host.plain", &[]), "host.plain");
        }
    }

    #[test]
    fn ja_text_fills_arguments_into_previous_text() {
        let loc = Localizer::only(&crate::LOCALES, "ja");
        let text = message(
            &loc,
            "text.slice_range",
            &[
                ("start", DiagArg::Int(2)),
                ("end", DiagArg::Int(1)),
                ("len", DiagArg::Int(3)),
            ],
        );
        assert_eq!(
            text,
            "text.slice の範囲 2..1 は 0 <= start <= end <= 3 でなければなりません"
        );
        let text = message(
            &loc,
            "text.result_too_large",
            &[
                ("command", DiagArg::Str("text.repeat".into())),
                ("limit", DiagArg::Int(16_777_216)),
            ],
        );
        assert_eq!(
            text,
            "text.repeat の結果が大きすぎます（上限は 16777216 バイト）"
        );
    }
}
