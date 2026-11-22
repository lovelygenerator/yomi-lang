//! Tests of the behavior of small public types (`CommandError`, `RegistryError`, `StrValue`).

use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::Arc;

use yomi_core::{CommandError, CommandErrorKind, DiagArg, RegistryError, StrValue, Sugar};

#[test]
fn coded_failures_have_code_and_args_and_args_can_be_added() {
    let e = CommandError::with_code("input.out_of_range", [("min", DiagArg::Int(1))])
        .arg("max", 9i64)
        .arg("name", "回数");
    assert_eq!(e.code(), "input.out_of_range");
    assert_eq!(
        e.args(),
        &[
            ("min", DiagArg::Int(1)),
            ("max", DiagArg::Int(9)),
            ("name", DiagArg::Str("回数".into())),
        ]
    );
    assert!(matches!(e.kind(), CommandErrorKind::Failed { .. }));
    assert_eq!(
        e.to_string(),
        "input.out_of_range (min=1, max=9, name=回数)"
    );
}

#[test]
fn failures_without_args_show_only_code() {
    let e = CommandError::with_code("audio.missing", []);
    assert!(e.args().is_empty());
    assert_eq!(e.to_string(), "audio.missing");
}

#[test]
fn failures_like_core_ones_use_english_code_names_without_args() {
    let cases = [
        (
            CommandError::integer_overflow(),
            CommandErrorKind::IntegerOverflow,
            "integer_overflow",
        ),
        (
            CommandError::non_finite_float(),
            CommandErrorKind::NonFiniteFloat,
            "non_finite_float",
        ),
        (
            CommandError::float_out_of_range(),
            CommandErrorKind::FloatOutOfRange,
            "float_out_of_range",
        ),
        (
            CommandError::division_by_zero(),
            CommandErrorKind::DivisionByZero,
            "division_by_zero",
        ),
    ];
    for (e, kind, code) in cases {
        assert_eq!(e.kind(), &kind);
        assert_eq!(e.code(), code);
        assert!(e.args().is_empty());
        assert_eq!(e.to_string(), code);
        // Even with added arguments, they are not attached to the kinds that match the core's
        assert!(e.arg("x", 1i64).args().is_empty());
    }
}

#[test]
fn registration_errors_display_as_sentences_with_names_per_kind() {
    let s = |x: &str| x.to_string();
    let cases = [
        (
            RegistryError::DuplicateCommand(s("a.b")),
            "duplicate command: a.b",
        ),
        (
            RegistryError::UnknownCommand(s("a.b")),
            "unknown command: a.b",
        ),
        (
            RegistryError::SugarMismatch(s("a.say")),
            "type mismatch for sugar binding: a.say",
        ),
        (
            RegistryError::AlreadyBound(Sugar::Quote),
            "sugar already bound: quote",
        ),
        (RegistryError::InvalidName(s("1x")), "invalid name: 1x"),
        (
            RegistryError::DuplicatePackage(s("a")),
            "duplicate package: a",
        ),
        (
            RegistryError::MissingDependency {
                package: s("a"),
                dependency: s("b"),
            },
            "package a depends on b, which is not registered",
        ),
        (
            RegistryError::DependencyCycle(s("a")),
            "package a depends on itself",
        ),
        (
            RegistryError::QueryWithoutValue(s("a.q")),
            "query returns no value: a.q",
        ),
        (
            RegistryError::UnsupportedVarType(s("a.v")),
            "unsupported variable type: a.v",
        ),
        (
            RegistryError::InvalidVarValue(s("a.v")),
            "invalid initial value: a.v",
        ),
        (
            RegistryError::DuplicateType(s("a.T")),
            "duplicate type: a.T",
        ),
        (
            RegistryError::UnknownType {
                name: s("a.T"),
                used_by: s("a.f"),
            },
            "type a.T (used by a.f) is not registered",
        ),
        (
            RegistryError::RecursiveType(s("a.T")),
            "type a.T contains itself",
        ),
        (
            RegistryError::InvalidMapKey(s("a.T")),
            "invalid map key type in a.T",
        ),
        (
            RegistryError::TypeTooDeep(s("a.T")),
            "type nesting too deep in a.T",
        ),
        (
            RegistryError::InvalidTypeDecl(s("a.T")),
            "invalid type declaration: a.T",
        ),
        (RegistryError::NoCommand, "no command to attach the item to"),
        (
            RegistryError::InvalidParams(s("a.f")),
            "invalid parameter names or defaults: a.f",
        ),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
        let _: &dyn std::error::Error = &e;
    }
}

#[test]
fn str_values_are_equal_regardless_of_source() {
    let base = StrValue::from("あいう");
    let from_all = [
        StrValue::from(String::from("あいう")),
        StrValue::from(&String::from("あいう")),
        StrValue::from(Arc::<str>::from("あいう")),
        StrValue::from(Box::<str>::from("あいう")),
        StrValue::from(std::borrow::Cow::Borrowed("あいう")),
        StrValue::from(std::borrow::Cow::<str>::Owned("あいう".into())),
    ];
    for v in from_all {
        assert_eq!(v, base);
    }
    assert_eq!(StrValue::default().as_str(), "");
}

#[test]
fn str_values_read_as_str_and_compare_with_str() {
    let v = StrValue::from("名前");
    assert_eq!(v.as_str(), "名前");
    assert_eq!(v.len(), "名前".len());
    assert_eq!(AsRef::<str>::as_ref(&v), "名前");
    assert_eq!(Borrow::<str>::borrow(&v), "名前");
    assert!(v == *"名前");
    assert!(v == "名前");
    assert_eq!(v.to_string(), "名前");
    assert_eq!(format!("{v:?}"), "\"名前\"");
}

#[test]
fn str_values_look_up_tables_by_str_keys() {
    let mut m: HashMap<StrValue, i32> = HashMap::new();
    m.insert(StrValue::from("a"), 1);
    assert_eq!(m.get("a"), Some(&1));
}

#[test]
fn str_values_serialize_as_strings() {
    let v = StrValue::from("台詞");
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, "\"台詞\"");
    let back: StrValue = serde_json::from_str(&json).unwrap();
    assert_eq!(back, v);
}
