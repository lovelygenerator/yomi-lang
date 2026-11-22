//! 7.2.3 Statements and declarations

use super::*;

#[test]
fn assignments_and_updates_print_nothing_and_only_change_variables() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    let r = exec(&mut repl, "favor += 1");
    assert_eq!(
        r,
        ReplYield::Done {
            value: None,
            declared: vec![],
            ended: false
        }
    );
    exec(&mut repl, "items.push(\"札0\")");
    assert_eq!(eval(&mut repl, "favor"), "4");
    assert_eq!(eval(&mut repl, "items"), "[\"お面\", \"札0\"]");
}

#[test]
fn let_declares_variable_and_returns_value() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    match exec(&mut repl, "let bonus = 25") {
        ReplYield::Done {
            declared, value, ..
        } => {
            assert_eq!(value, None);
            assert_eq!(declared.len(), 1);
            match &declared[0] {
                yomi_core::Declared::Let { name, value, ty } => {
                    assert_eq!(
                        (name.as_str(), value, ty),
                        ("bonus", &Value::Int(25), &Ty::Int)
                    );
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(eval(&mut repl, "bonus * 2"), "50");
    // Type annotations can also be written the same as in scripts
    exec(&mut repl, "let names: list<str> = []");
    exec(&mut repl, "names.push(\"a\")");
    assert_eq!(eval(&mut repl, "names"), "[\"a\"]");
    // Not input that has the value of an expression
    assert!(matches!(
        repl.submit("let q = 1").unwrap().kind,
        InputKind::Statement
    ));
}

#[test]
fn lets_in_blocks_disappear_and_only_top_level_lets_remain() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "if true {\n  let inner = 1\n  favor += inner\n}");
    assert_eq!(eval(&mut repl, "favor"), "4");
    assert_eq!(codes(&submit_err(&mut repl, "inner")), ["unknown_name"]);
    assert!(
        repl.variables()
            .iter()
            .all(|v| v.name != "inner" && v.module.as_deref() != Some("<repl>"))
    );
}

#[test]
fn expressions_inside_if_while_for_match_are_not_printed_only_whole_expression_returns_value() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    let r = exec(&mut repl, "for i in range(0, 3) {\n  favor += i\n}");
    assert!(matches!(r, ReplYield::Done { value: None, .. }));
    assert_eq!(eval(&mut repl, "favor"), "6");
    let r = exec(
        &mut repl,
        "match mood_of(favor) {\n  Happy => favor = 0\n  _ => favor += 1\n}",
    );
    assert!(matches!(r, ReplYield::Done { value: None, .. }));
}

#[test]
fn declarations_add_fn_struct_enum_identity_import_and_print_nothing() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(
        exec(&mut repl, "fn bonus(n: int) -> int {\n  return n * 2\n}"),
        ReplYield::Done {
            value: None,
            declared: vec![yomi_core::Declared::Fn {
                name: "bonus".into()
            }],
            ended: false
        }
    );
    exec(&mut repl, "struct Pt { x: int, y: int }");
    exec(&mut repl, "enum Dir { N, S(int) }");
    exec(&mut repl, "identity 村長 = \"たろう\"");
    exec(&mut repl, "import counter");
    assert_eq!(eval(&mut repl, "bonus(favor)"), "6");
    assert_eq!(eval(&mut repl, "Pt { x: 1, y: 2 }"), "Pt { x: 1, y: 2 }");
    assert_eq!(eval(&mut repl, "Dir.S(3)"), "Dir.S(3)");
    let defs: Vec<String> = repl.defs().iter().map(ToString::to_string).collect();
    assert_eq!(
        defs,
        [
            "fn bonus(n: int) -> int",
            "struct Pt",
            "enum Dir",
            "identity 村長",
            "import counter"
        ]
    );
}

#[test]
fn import_can_be_written_any_time_and_repeating_does_nothing() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let a = 1");
    exec(&mut repl, "import other");
    exec(&mut repl, "import other");
    // Things the script has already `import`ed are not errors either
    exec(&mut repl, "import counter");
    assert_eq!(
        repl.defs()
            .iter()
            .filter(|d| d.to_string() == "import other")
            .count(),
        1
    );
    assert_eq!(
        codes(&submit_err(&mut repl, "import nothing")),
        ["unknown_module"]
    );
}

#[test]
fn multiline_declarations_and_empty_lines_in_blocks_can_be_entered() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "fn bonus(n: int) -> int {\n  return n * 2\n}");
    assert_eq!(eval(&mut repl, "bonus(favor)"), "6");
    exec(&mut repl, "if favor >= 3 {\n  favor = 0\n\n  favor += 1\n}");
    assert_eq!(eval(&mut repl, "favor"), "1");
}
