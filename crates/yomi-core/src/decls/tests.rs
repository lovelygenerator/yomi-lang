//! Unit tests of declaration files.
use super::*;
use crate::{Ctx, Package, PackageReg};

fn minimal(extra: &str) -> String {
    format!("{{\"format\":\"yomi-decls\",\"version\":1{extra}}}")
}

fn kind_of(json: &str) -> DeclErrorKind {
    Registry::from_decls(json).expect_err(json).kind
}

#[test]
fn empty_registry_can_be_exported_and_reread() {
    let reg = Registry::new();
    let json = reg.export_decls();
    let back = Registry::from_decls(&json).expect("読める");
    assert_eq!(back.export_decls(), json);
    assert!(back.is_decl_only());
    assert!(!reg.is_decl_only());
}

struct Audio;
impl Package for Audio {
    const NAME: &'static str = "audio";
    type State = ();
    type Persistent = ();
    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("audio.bgm", |_: &mut Ctx<Audio>, _n: String| {})
            .doc("BGM を再生する \"引用\"\n改行")
            .param_names(&["name"])
            .alias("sound.bgm");
        reg.query("audio.playing", |_: &Ctx<Audio>| -> bool { false });
        reg.var::<f64>("audio.volume", crate::Kind::Persistent, 0.5)
            .doc("音量");
    }
}

fn sample() -> Registry {
    let mut r = Registry::new();
    // Registration order differs from name order
    r.stop::<(String,), ()>("zeta.say").unwrap();
    r.stop::<(i64,), i64>("alpha.roll").unwrap();
    r.package::<Audio>().unwrap();
    r.bind(Sugar::Plain, "zeta.say").unwrap();
    r.ignore(Sugar::Choice).unwrap();
    r
}

#[test]
fn export_is_sorted_by_name_and_stable_for_same_registry() {
    let json = sample().export_decls();
    assert_eq!(json, sample().export_decls());
    let alpha = json.find("alpha.roll").unwrap();
    let bgm = json.find("\"audio.bgm\"").unwrap();
    let playing = json.find("audio.playing").unwrap();
    let zeta = json.find("zeta.say\"").unwrap();
    assert!(alpha < bgm && bgm < playing && playing < zeta);
    // Sugar: bindings are names, ignored ones are null, and those with neither are omitted
    assert!(json.contains("\"plain\": \"zeta.say\""));
    assert!(json.contains("\"choice\": null"));
    assert!(!json.contains("\"quote\""));
}

#[test]
fn exported_declarations_round_trip_to_same_string() {
    let json = sample().export_decls();
    let back = Registry::from_decls(&json).unwrap();
    assert_eq!(back.export_decls(), json);
    // Aliases, descriptions, parameter names and kinds are kept
    let cmds = back.commands();
    let bgm = cmds.iter().find(|c| c.name == "audio.bgm").unwrap();
    assert_eq!(bgm.aliases, ["sound.bgm"]);
    assert_eq!(bgm.doc.as_deref(), Some("BGM を再生する \"引用\"\n改行"));
    assert_eq!(bgm.params, [Some("name".to_string())]);
    assert_eq!(bgm.kind, CommandKind::Effect);
    assert_eq!(back.packages().count(), 1);
    assert_eq!(back.vars()[0].doc.as_deref(), Some("音量"));
}

#[test]
fn export_uses_2_space_indent_and_sorted_keys() {
    let json = sample().export_decls();
    assert!(json.starts_with("{\n  \"commands\": [\n"));
    assert!(json.contains("\n  \"format\": \"yomi-decls\",\n"));
    assert!(json.contains("\"returns\": null"));
    assert!(json.contains("\"default\": 0.5"));
    assert!(json.ends_with("}\n"));
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = root.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(root["version"], 2);
}

#[test]
fn syntax_error_columns_count_characters() {
    let e = Registry::from_decls("{\n  \"あいう\": ?\n}").unwrap_err();
    assert_eq!(e.kind, DeclErrorKind::Syntax { line: 2, col: 10 });
}

#[test]
fn later_duplicate_keys_win_and_unrepresentable_numbers_are_syntax_errors() {
    Registry::from_decls("{\"format\":\"yomi-decls\",\"version\":3,\"version\":2}").unwrap();
    let huge = minimal(
        ",\"variables\":[{\"name\":\"x\",\"type\":\"float\",\"kind\":\"save\",\"default\":1e400}]",
    );
    assert!(matches!(kind_of(&huge), DeclErrorKind::Syntax { .. }));
}

#[test]
fn nesting_parses_up_to_limit_depth() {
    let at_limit = format!(
        "{}{}",
        "[".repeat(MAX_JSON_DEPTH),
        "]".repeat(MAX_JSON_DEPTH)
    );
    assert_eq!(kind_of(&at_limit), DeclErrorKind::NotDecls);
    let over = format!(
        "{}{}",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    assert_eq!(kind_of(&over), DeclErrorKind::TooDeep);
    // Brackets inside strings are not counted
    let in_str = minimal(&format!(
        ",\"doc\":\"{}\\\"{}\"",
        "[".repeat(500),
        "{".repeat(500)
    ));
    Registry::from_decls(&in_str).unwrap();
}

#[test]
fn unknown_fields_are_ignored_and_different_version_is_rejected() {
    let ok = minimal(",\"future\":{\"a\":[1,2,3]},\"commands\":[]");
    Registry::from_decls(&ok).unwrap();
    assert_eq!(
        kind_of("{\"format\":\"yomi-decls\",\"version\":3}"),
        DeclErrorKind::UnsupportedVersion(3)
    );
    assert_eq!(
        kind_of("{\"format\":\"yomi-decls\"}"),
        DeclErrorKind::MissingField
    );
    assert_eq!(
        kind_of("{\"format\":\"other\",\"version\":1}"),
        DeclErrorKind::NotDecls
    );
    assert_eq!(kind_of("[]"), DeclErrorKind::NotDecls);
    assert_eq!(
        kind_of("{\"format\":\"yomi-decls\",\"version\":\"1\"}"),
        DeclErrorKind::WrongType
    );
}

#[test]
fn broken_json_errors_with_position_without_panic() {
    for bad in [
        "",
        "{",
        "}",
        "{\"a\"}",
        "{\"a\":}",
        "[1,]",
        "{\"a\":1,}",
        "nul",
        "tru",
        "\"abc",
        "\"\\x\"",
        "\"\\u12\"",
        "\"\\ud800\"",
        "01",
        "1.",
        "-",
        "1e",
        "{} x",
        "\u{feff}{}",
        "{\"format\":\"yomi-decls\",\"version\":1,\"types\":[{]}",
    ] {
        let e = Registry::from_decls(bad).expect_err(bad);
        assert!(
            matches!(
                e.kind,
                DeclErrorKind::Syntax { .. } | DeclErrorKind::NotDecls
            ),
            "{bad:?}: {e}"
        );
    }
    let e = Registry::from_decls("{\n  \"a\": ?\n}").unwrap_err();
    assert_eq!(e.kind, DeclErrorKind::Syntax { line: 2, col: 8 });
}

#[test]
fn huge_input_and_deep_nesting_are_rejected_without_panic() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    assert_eq!(kind_of(&deep), DeclErrorKind::TooDeep);
    let deep_obj = format!("{}1{}", "{\"a\":".repeat(5_000), "}".repeat(5_000));
    assert_eq!(kind_of(&deep_obj), DeclErrorKind::TooDeep);
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(MAX_DECLS_BYTES));
    assert_eq!(kind_of(&big), DeclErrorKind::TooLarge);
    // Nesting of type names
    let ty = format!("{}int{}", "list<".repeat(100_000), ">".repeat(100_000));
    let json = minimal(&format!(
        ",\"variables\":[{{\"name\":\"a.b\",\"type\":\"{ty}\",\"kind\":\"save\",\"default\":[]}}]"
    ));
    assert_eq!(kind_of(&json), DeclErrorKind::TooDeep);
    // A type exceeding the nesting limit of types (32) is a registration error
    let ty = format!("{}int{}", "list<".repeat(33), ">".repeat(33));
    let json = minimal(&format!(
        ",\"variables\":[{{\"name\":\"a.b\",\"type\":\"{ty}\",\"kind\":\"save\",\"default\":[]}}]"
    ));
    assert!(matches!(
        kind_of(&json),
        DeclErrorKind::Registry(RegistryError::TypeTooDeep(_))
    ));
    // A large number of elements
    let many = format!("[{}0]", "0,".repeat(1_000_000));
    let json = minimal(&format!(",\"x\":{many}"));
    Registry::from_decls(&json).unwrap();
}

#[test]
fn field_errors_are_located_errors() {
    let e = Registry::from_decls(&minimal(
        ",\"commands\":[{\"name\":\"a.b\",\"kind\":\"stop\",\"params\":[{\"type\":\"nope\"}]}]",
    ))
    .unwrap_err();
    assert_eq!(e.path, "commands[0].params[0].type");
    assert_eq!(e.kind, DeclErrorKind::InvalidType("nope".into()));
    let e = Registry::from_decls(&minimal(",\"commands\":[{\"kind\":\"stop\"}]")).unwrap_err();
    assert_eq!(
        (e.kind, e.path.as_str()),
        (DeclErrorKind::MissingField, "commands[0].name")
    );
    let e = Registry::from_decls(&minimal(
        ",\"commands\":[{\"name\":\"a.b\",\"kind\":\"jump\"}]",
    ))
    .unwrap_err();
    assert_eq!(e.kind, DeclErrorKind::InvalidValue);
    // Colliding names
    let e = Registry::from_decls(&minimal(
        ",\"commands\":[{\"name\":\"a.b\",\"kind\":\"stop\"},{\"name\":\"a.b\",\"kind\":\"stop\"}]",
    ))
    .unwrap_err();
    assert!(matches!(
        e.kind,
        DeclErrorKind::Registry(RegistryError::DuplicateCommand(_))
    ));
    assert_eq!(e.path, "commands[1]");
    // Types that use each other
    let e = Registry::from_decls(&minimal(
        ",\"types\":[{\"name\":\"a.X\",\"kind\":\"struct\",\"fields\":[{\"name\":\"y\",\"type\":\"list<a.Y>\"}]},{\"name\":\"a.Y\",\"kind\":\"struct\",\"fields\":[{\"name\":\"x\",\"type\":\"a.X\"}]}]",
    ))
    .unwrap_err();
    assert!(matches!(
        e.kind,
        DeclErrorKind::Registry(RegistryError::RecursiveType(_))
    ));
    // Effect commands need a package
    let e = Registry::from_decls(&minimal(
        ",\"commands\":[{\"name\":\"a.b\",\"kind\":\"effect\"}]",
    ))
    .unwrap_err();
    assert_eq!(e.kind, DeclErrorKind::MissingField);
    // A default that does not match the type
    let e = Registry::from_decls(&minimal(
        ",\"variables\":[{\"name\":\"a.v\",\"type\":\"int\",\"kind\":\"save\",\"default\":1.5}]",
    ))
    .unwrap_err();
    assert_eq!(e.kind, DeclErrorKind::InvalidValue);
    assert_eq!(e.path, "variables[0].default");
}

#[test]
fn variable_defaults_of_all_types_can_be_written_and_reread() {
    let json = minimal(
        r#",
  "types": [
    { "name": "a.S", "kind": "struct", "fields": [{ "name": "x", "type": "int" }, { "name": "o", "type": "Option<Option<str>>" }] },
    { "name": "a.E", "kind": "enum", "variants": [{ "name": "U", "fields": [] }, { "name": "P", "fields": [{ "type": "float" }, { "type": "a.S" }] }] }
  ],
  "variables": [
    { "name": "a.i", "type": "int", "kind": "save", "default": -5 },
    { "name": "a.f", "type": "float", "kind": "save", "default": 3 },
    { "name": "a.b", "type": "bool", "kind": "persistent", "default": true },
    { "name": "a.s", "type": "str", "kind": "save", "default": "あ\n\"" },
    { "name": "a.id", "type": "Identity", "kind": "save", "default": { "id": "x", "name": "y" } },
    { "name": "a.l", "type": "list<a.S>", "kind": "save", "default": [{ "x": 1, "o": { "some": null } }, { "x": 2, "o": { "some": { "some": "z" } } }] },
    { "name": "a.m", "type": "map<str, a.E>", "kind": "save", "default": [["k", { "variant": "P", "args": [1.5, { "x": 0, "o": null }] }], ["j", { "variant": "U" }]] },
    { "name": "a.o", "type": "Option<int>", "kind": "save", "default": null }
  ]"#,
    );
    let r = Registry::from_decls(&json).unwrap();
    let out = r.export_decls();
    let r2 = Registry::from_decls(&out).unwrap();
    assert_eq!(r2.export_decls(), out);
    let f = r.vars().iter().find(|v| v.name == "a.f").unwrap();
    assert_eq!(f.init, Value::Float(3.0));
    assert!(out.contains("\"default\": 3.0"));
    // A map with duplicate keys, or arguments with a wrong count, are rejected
    for bad in [
        r#"{ "name": "a.m2", "type": "map<str, int>", "kind": "save", "default": [["k", 1], ["k", 2]] }"#,
        r#"{ "name": "a.e2", "type": "a.E", "kind": "save", "default": { "variant": "P", "args": [] } }"#,
        r#"{ "name": "a.e3", "type": "a.E", "kind": "save", "default": { "variant": "Q" } }"#,
        r#"{ "name": "a.i2", "type": "int", "kind": "save", "default": 1e3 }"#,
        r#"{ "name": "a.i3", "type": "int", "kind": "save", "default": 99999999999999999999 }"#,
        r#"{ "name": "a.f2", "type": "float", "kind": "save", "default": 1e999 }"#,
    ] {
        let j = json.replace("\"variables\": [", &format!("\"variables\": [{bad},"));
        assert!(Registry::from_decls(&j).is_err(), "{bad}");
    }
}

#[test]
fn json_strings_read_escapes_and_surrogate_pairs() {
    let j = minimal(
        ",\"commands\":[{\"name\":\"a.b\",\"kind\":\"stop\",\"doc\":\"\\u3042\\ud83d\\ude00\\/\\t\"}]",
    );
    let r = Registry::from_decls(&j).unwrap();
    assert_eq!(r.commands()[0].doc.as_deref(), Some("あ😀/\t"));
    // Writing escapes control characters
    let out = r.export_decls();
    assert!(out.contains("\"doc\": \"あ😀/\\t\""));
}

#[test]
fn parameter_defaults_round_trip_and_version_1_reads_without_defaults() {
    let mut r = Registry::new();
    r.stop::<(String, i64, f64), ()>("game.give").unwrap();
    r.param_names("game.give", &["item", "count", "rate"])
        .unwrap();
    r.param_defaults(
        "game.give",
        &[("count", Value::Int(1)), ("rate", Value::Float(0.5))],
    )
    .unwrap();
    let json = r.export_decls();
    assert!(json.contains("\"default\": 1"));
    let back = Registry::from_decls(&json).unwrap();
    assert_eq!(back.export_decls(), json);
    let give = back
        .commands()
        .into_iter()
        .find(|c| c.name == "game.give")
        .unwrap();
    assert_eq!(
        give.defaults,
        vec![None, Some(Value::Int(1)), Some(Value::Float(0.5))]
    );
    // Version 1 (without defaults) can also be read
    let v1 = "{\"format\":\"yomi-decls\",\"version\":1,\"commands\":[{\"name\":\"game.give\",\"kind\":\"stop\",\"params\":[{\"name\":\"item\",\"type\":\"str\"}],\"returns\":null}]}";
    let old = Registry::from_decls(v1).unwrap();
    assert_eq!(old.commands()[0].defaults, vec![None]);
}

#[test]
fn defaults_are_named_trailing_and_type_correct() {
    let cmd = |params: &str| {
        minimal(&format!(
            ",\"commands\":[{{\"name\":\"game.give\",\"kind\":\"stop\",\"params\":[{params}],\"returns\":null}}]"
        ))
    };
    // No name
    let no_name = cmd("{\"type\":\"int\",\"default\":1}");
    assert_eq!(kind_of(&no_name), DeclErrorKind::InvalidValue);
    // Not at the end
    let middle =
        cmd("{\"name\":\"a\",\"type\":\"int\",\"default\":1},{\"name\":\"b\",\"type\":\"int\"}");
    assert_eq!(kind_of(&middle), DeclErrorKind::InvalidValue);
    // Wrong type
    let wrong = cmd("{\"name\":\"a\",\"type\":\"int\",\"default\":\"x\"}");
    assert_eq!(kind_of(&wrong), DeclErrorKind::WrongType);
}

#[test]
fn registry_defaults_are_named_type_correct_and_trailing() {
    let mut r = Registry::new();
    r.stop::<(String, i64), ()>("game.give").unwrap();
    // Not named
    assert!(
        r.param_defaults("game.give", &[("count", Value::Int(1))])
            .is_err()
    );
    r.param_names("game.give", &["item", "count"]).unwrap();
    // Wrong type, not at the end
    assert!(
        r.param_defaults("game.give", &[("count", Value::Bool(true))])
            .is_err()
    );
    assert!(
        r.param_defaults("game.give", &[("item", Value::Str("x".into()))])
            .is_err()
    );
    r.param_defaults("game.give", &[("count", Value::Int(1))])
        .unwrap();
}
