//! 7.3 State and scope

use super::*;

#[test]
fn name_collisions_follow_script_rules_as_duplicate_name() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    // The same name as a script variable, fn, type or identity
    for input in [
        "let favor = 1",
        "fn mood_of(a: int) -> int {\n  return a\n}",
        "struct Prize { a: int }",
        "enum Mood { A }",
        "identity 案内人 = \"x\"",
        "fn favor() -> int {\n  return 1\n}",
    ] {
        let d = submit_err(&mut repl, input);
        assert_eq!(codes(&d)[0], "duplicate_name", "{input:?}: {d:?}");
    }
    // A `let` with the same name as a label is not an error
    exec(&mut repl, "let shop = 1");
    assert_eq!(eval(&mut repl, "shop"), "1");
}

#[test]
fn repl_let_is_replaced_by_same_named_let_even_with_new_type() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let x = 1");
    assert_eq!(eval(&mut repl, "x"), "1");
    exec(&mut repl, "let x = \"あ\"");
    assert_eq!(eval(&mut repl, "x"), "\"あ\"");
    assert_eq!(repl.type_of("x").unwrap(), Ty::Str);
    assert_eq!(repl.variables().iter().filter(|v| v.name == "x").count(), 1);
    // Script variables cannot be replaced
    assert_eq!(
        codes(&submit_err(&mut repl, "let items = 1")),
        ["duplicate_name"]
    );
}

#[test]
fn repl_fn_replaces_body_only_with_same_types_otherwise_duplicate_name() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "fn f(n: int) -> int {\n  return n + 1\n}");
    assert_eq!(eval(&mut repl, "f(1)"), "2");
    exec(&mut repl, "fn f(n: int) -> int {\n  return n + 10\n}");
    assert_eq!(eval(&mut repl, "f(1)"), "11");
    assert_eq!(repl.defs().len(), 1);
    // Different parameter or return types are an error (the previous declaration stays)
    for input in [
        "fn f(n: str) -> int {\n  return 1\n}",
        "fn f(n: int) -> str {\n  return \"a\"\n}",
        "fn f() -> int {\n  return 1\n}",
    ] {
        let d = submit_err(&mut repl, input);
        assert_eq!(codes(&d), ["duplicate_name"], "{input:?}");
        assert_eq!(d[0].arg_str("name"), Some("f"));
    }
    assert_eq!(eval(&mut repl, "f(1)"), "11");
}

#[test]
fn struct_enum_identity_cannot_be_replaced() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "struct Pt { x: int }");
    exec(&mut repl, "enum E { A }");
    exec(&mut repl, "identity 村長 = \"a\"");
    for input in [
        "struct Pt { x: int, y: int }",
        "enum E { A, B }",
        "identity 村長 = \"b\"",
        "struct E { x: int }",
    ] {
        assert_eq!(
            codes(&submit_err(&mut repl, input)),
            ["duplicate_name"],
            "{input:?}"
        );
    }
}

#[test]
fn repl_type_diagnostics_show_repl_type_names() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "struct Prize2 { name: str }");
    let d = submit_err(&mut repl, "let p: int = Prize2 { name: \"a\" }");
    assert_eq!(codes(&d), ["type_mismatch"]);
    assert_eq!(d[0].arg_str("found"), Some("<repl>.Prize2"));
    assert_eq!(
        repl.type_of("Prize2 { name: \"a\" }").unwrap(),
        Ty::Struct("<repl>.Prize2".into())
    );
}

#[test]
fn repl_fn_bodies_cannot_see_repl_let_but_can_read_module_variables() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let z = 10");
    let d = submit_err(&mut repl, "fn g() -> int {\n  return z\n}");
    assert_eq!(codes(&d), ["unknown_name"]);
    exec(&mut repl, "fn h() -> int {\n  return favor * 2\n}");
    assert_eq!(eval(&mut repl, "h()"), "6");
    // The prohibition of writes inside `fn` is the same as in scripts
    let d = submit_err(&mut repl, "fn w() {\n  favor = 1\n}");
    assert_eq!(codes(&d), ["assign_in_fn"]);
    // A REPL `let` cannot have the same name as a `fn` declaration
    let d = submit_err(&mut repl, "fn z() -> int {\n  return 1\n}");
    assert_eq!(codes(&d), ["duplicate_name"]);
}

#[test]
fn repl_declarations_are_valid_only_in_repl_and_do_not_change_script_compilation() {
    let reg = registry();
    let prog = program(&reg);
    let before = prog.clone();
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let a = 1");
    exec(&mut repl, "fn f() -> int {\n  return 2\n}");
    exec(&mut repl, "struct Pt { x: int }");
    exec(&mut repl, "identity 村長 = \"x\"");
    assert_eq!(prog, before);
    assert_eq!(prog.module_names().collect::<Vec<_>>(), ["main"]);
    // Not visible in a new REPL
    let mut other = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(codes(&submit_err(&mut other, "a")), ["unknown_name"]);
    assert_eq!(codes(&submit_err(&mut other, "f()")), ["unknown_function"]);
}

#[test]
fn declaring_identity_enables_quote_lines_with_that_name() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    // Identities declared in the script can be used from the start
    repl.submit("案内人「こんにちは」").unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { args, .. } => {
            assert!(format!("{:?}", args[0]).contains("リオン"));
            assert_eq!(args[1].to_text(), "「こんにちは」");
        }
        other => panic!("{other:?}"),
    }
    repl.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Done { .. }));
    // A quote line with an undeclared name becomes a plain line (with a warning)
    exec(&mut repl, "identity 村長 = \"たろう\"");
    repl.submit("村長「やあ」").unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { args, .. } => {
            assert!(format!("{:?}", args[0]).contains("たろう"));
        }
        other => panic!("{other:?}"),
    }
}
