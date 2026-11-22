//! Tests of identifiers (UAX #31 and NFC) from the public API: naming rules of the registry, declaration files, scripts.

use yomi_core::{
    DeclErrorKind, Input, Kind, Registry, RegistryError, Sugar, Value, Vm, VmConfig, Yield, compile,
};

/// The decomposed form (か + dakuten) and composed form of "が".
const GA_D: &str = "\u{304B}\u{3099}";
const GA_C: &str = "\u{304C}";

#[test]
fn registry_names_are_checked_by_uax31_and_nfc_rules() {
    let mut r = Registry::new();
    // Japanese, accented Latin letters, digits (second character onward) and `_` pass
    for name in [
        "ui.say",
        "ゲーム.台本",
        "ゲーム.案内人2",
        "café._x",
        &format!("{GA_C}.{GA_C}"),
    ] {
        assert!(r.stop::<(), ()>(name).is_ok(), "{name}");
    }
    // Emoji, symbols, starting with a digit, whitespace, and a misplaced `.` are rejected
    for name in [
        "a.🐱",
        "🐱.a",
        "a.b-c",
        "a.1b",
        "a.b c",
        "a.b\u{3000}c",
        "a.",
        ".a",
        "a.b.c",
        "",
        "a.①",
    ] {
        assert!(
            matches!(r.stop::<(), ()>(name), Err(RegistryError::InvalidName(_))),
            "{name}"
        );
    }
    // A name that is not NFC is a registration error
    let decomposed = format!("{GA_D}.x");
    assert!(matches!(
        r.stop::<(), ()>(&decomposed),
        Err(RegistryError::InvalidName(_))
    ));
    let decomposed = format!("x.{GA_D}");
    assert!(matches!(
        r.stop::<(), ()>(&decomposed),
        Err(RegistryError::InvalidName(_))
    ));
    // The composed form passes, and the decomposed form that looks the same is not treated as a duplicate (it cannot be
    // registered in the first place)
    assert!(matches!(
        r.stop::<(), ()>(&format!("{GA_C}.{GA_C}")),
        Err(RegistryError::DuplicateCommand(_))
    ));
}

#[test]
fn host_variable_names_follow_same_rules() {
    let mut r = Registry::new();
    assert!(r.var::<i64>("ゲーム.所持金", Kind::Save, 0).is_ok());
    assert!(matches!(
        r.var::<i64>("ゲーム.所持金🐱", Kind::Save, 0),
        Err(RegistryError::InvalidName(_))
    ));
    assert!(matches!(
        r.var::<i64>(&format!("g.{GA_D}"), Kind::Save, 0),
        Err(RegistryError::InvalidName(_))
    ));
}

fn decls(name: &str, var: &str) -> String {
    format!(
        "{{\"format\":\"yomi-decls\",\"version\":1,\"packages\":[],\"commands\":[{{\"name\":\"{name}\",\"kind\":\"stop\",\"params\":[{{\"type\":\"str\"}}],\"returns\":null,\"aliases\":[]}}],\"variables\":[{{\"name\":\"{var}\",\"type\":\"int\",\"kind\":\"save\",\"default\":1,\"read_only\":false}}],\"types\":[],\"sugar\":{{}}}}"
    )
}

#[test]
fn declaration_file_names_are_checked_with_same_rules() {
    assert!(Registry::from_decls(&decls("ui.say", "game.gold")).is_ok());
    assert!(Registry::from_decls(&decls(&format!("{GA_C}.x"), &format!("g.{GA_C}"))).is_ok());
    for (cmd, var) in [
        ("ui.🐱", "game.gold"),
        ("ui.say", "game.go ld"),
        (&format!("{GA_D}.x") as &str, "game.gold"),
        ("ui.say", &format!("game.{GA_D}") as &str),
    ] {
        let e = Registry::from_decls(&decls(cmd, var)).expect_err(cmd);
        assert!(
            matches!(
                e.kind,
                DeclErrorKind::Registry(RegistryError::InvalidName(_))
            ),
            "{cmd} {var}: {e:?}"
        );
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

fn run_texts(src: &str) -> Vec<String> {
    let reg = registry();
    let prog = compile(src, "main", &reg).unwrap_or_else(|d| panic!("{d:?}"));
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut out = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::End => break,
            Yield::Stop { args, .. } => {
                out.push(args.last().map(Value::to_text).unwrap_or_default());
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            _ => {}
        }
    }
    out
}

#[test]
fn script_identifiers_work_same_in_decomposed_and_composed_forms() {
    let tpl = "save var 点{X}: int = 1\nlabel start {\n  点{X} += 2\n  ${点{Y}}\n  jump 次{Y}\n}\nlabel 次{X} {\n  届いた\n}\n";
    let a = run_texts(&tpl.replace("{X}", GA_C).replace("{Y}", GA_C));
    let b = run_texts(&tpl.replace("{X}", GA_D).replace("{Y}", GA_C));
    let c = run_texts(&tpl.replace("{X}", GA_C).replace("{Y}", GA_D));
    assert_eq!(a, ["3", "届いた"]);
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn invalid_identifier_chars_are_compile_errors() {
    for bad in ["let 🐱 = 1", "let a-b = 1", "let 1a = 1"] {
        let src = format!("label start {{\n  {bad}\n}}\n");
        assert!(compile(&src, "main", &registry()).is_err(), "{bad}");
    }
}
