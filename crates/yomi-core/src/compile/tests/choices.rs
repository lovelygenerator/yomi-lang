//! Quote lines, identities, choices, seen, visited

use super::*;

pub(super) fn full_registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<crate::Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

pub(super) fn full_diags(src: &str) -> Vec<Diagnostic> {
    match compile_with_warnings(src, "m", &full_registry()) {
        Ok((_, warnings)) => warnings,
        Err(d) => d,
    }
}

pub(super) fn full_codes(src: &str) -> Vec<DiagCode> {
    full_diags(src).iter().map(|d| d.code).collect()
}

#[test]
fn quote_line_expression_diagnostics_point_to_original_line_and_column_across_lines() {
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  案内人「あ\n     ${nope}」\n}\n";
    let d = full_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, DiagCode::UnknownName);
    // The `nope` in `     ${nope}」` (after 5 spaces and `${`)
    assert_eq!((d[0].line, d[0].col), (4, 8));
}

#[test]
fn quote_line_expressions_are_type_checked_even_when_sugar_ignored() {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.ignore(Sugar::Quote).unwrap();
    let src = "label start {\n  「${1 + \"a\"}」\n}\n";
    let d = match compile_with_warnings(src, "m", &r) {
        Ok((_, w)) => w,
        Err(d) => d,
    };
    assert!(
        d.iter()
            .any(|d| d.code == DiagCode::InvalidOperation || d.code == DiagCode::TypeMismatch),
        "{d:?}"
    );
}

#[test]
fn unbound_sugar_is_error() {
    let src = "label start {\n  「a」\n  choice {\n    「b」 => end\n  }\n}\n";
    let codes = errors(src);
    assert_eq!(codes, vec![DiagCode::UnboundSugar, DiagCode::UnboundSugar]);
}

#[test]
fn once_without_stable_id_is_error() {
    let src = "label start {\n  choice {\n    「a」 once => end\n  }\n}\n";
    assert_eq!(
        full_codes(src)
            .into_iter()
            .filter(|c| *c == DiagCode::OnceWithoutId)
            .count(),
        1
    );
}

#[test]
fn choice_option_stable_id_colliding_with_same_label_stop_point_is_error() {
    let src = "label start {\n  a #x\n  choice {\n    「a」 #x => end\n  }\n}\n";
    assert!(full_codes(src).contains(&DiagCode::DuplicateStableId));
}

#[test]
fn non_bool_option_condition_is_error() {
    let src = "label start {\n  choice {\n    「a」 if 1 => end\n  }\n}\n";
    assert!(full_codes(src).contains(&DiagCode::ConditionNotBool));
}

#[test]
fn plain_line_with_undeclared_name_and_bracket_warns_on_close_identity() {
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  案内入「おはよう」\n}\n";
    let d = full_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, DiagCode::IdentityTypo);
    assert_eq!(d[0].severity, Severity::Warning);
    assert_eq!(d[0].arg_str("name"), Some("案内入"));
}

#[test]
fn missing_seen_and_visited_literals_warn() {
    let src = "label start {\n  a #p\n  let _x = seen(\"start#p\") && visited(\"start\")\n  let _y = seen(\"start#q\") || visited(\"nowhere\")\n}\n";
    let d = full_diags(src);
    let got: Vec<_> = d.iter().map(|d| (d.code, d.line)).collect();
    assert_eq!(
        got,
        vec![
            (DiagCode::UnknownPosition, 4),
            (DiagCode::UnknownLabelLiteral, 4)
        ]
    );
}

#[test]
fn seen_and_visited_take_string_and_return_bool() {
    let src = "label start {\n  let a = seen(1)\n  let b: int = visited(\"start\")\n}\n";
    let c = full_codes(src);
    assert!(c.contains(&DiagCode::TypeMismatch), "{c:?}");
}

#[test]
fn nested_choice_and_break_inside_loop_compile() {
    let src = "label start {\n  while true {\n    choice {\n      「a」 => break\n      「b」 => choice {\n        「c」 => continue\n        「d」 => end\n      }\n    }\n  }\n}\n";
    assert_eq!(full_codes(src), vec![]);
}
