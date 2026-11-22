//! 7.2 Classifying input, and expressions

use super::*;

#[test]
fn expressions_return_value_and_type_in_display_form() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(eval(&mut repl, "favor"), "3");
    assert_eq!(eval(&mut repl, "items"), "[\"お面\"]");
    assert_eq!(eval(&mut repl, "mood_of(favor)"), "Mood.Normal");
    assert_eq!(eval(&mut repl, "true"), "true");
    assert_eq!(eval(&mut repl, "Some(3)"), "Some(3)");
    assert_eq!(eval(&mut repl, "map { \"a\": 1 }"), "map { \"a\": 1 }");
    assert_eq!(eval(&mut repl, "2.5 * 2.0"), "5.0");
    assert_eq!(
        eval(&mut repl, "Prize { name: \"お面\", hp: 3 }"),
        "Prize { name: \"お面\", hp: 3 }"
    );
    let s = repl.submit("favor + 1").unwrap();
    assert_eq!(s.kind, InputKind::Expression(Ty::Int));
    repl.abort();
    let s = repl.submit("mood_of(1)").unwrap();
    assert_eq!(s.kind, InputKind::Expression(Ty::Enum("main.Mood".into())));
}

#[test]
fn expression_detection_uses_only_input_characters_and_undeclared_names_are_not_plain_lines() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    let d = submit_err(&mut repl, "gld");
    assert_eq!(codes(&d), ["unknown_name"]);
    assert_eq!(
        (d[0].module.as_str(), d[0].line, d[0].col),
        ("<repl>", 1, 1)
    );
    // Candidates with close spellings (also searches script variables and REPL variables)
    exec(&mut repl, "let gold = 7");
    let d = submit_err(&mut repl, "gld");
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["gold".to_string()][..])
    );
    // `\` makes a plain line
    let s = repl.submit("\\はい").unwrap();
    assert_eq!(s.kind, InputKind::Statement);
    match repl.run().unwrap() {
        ReplYield::Stop { args, id, .. } => {
            assert_eq!(text_of(&args), "はい");
            assert_eq!(id, None);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn expressions_starting_with_keywords_are_read_as_expressions() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    for (input, shown) in [
        ("true", "true"),
        ("false", "false"),
        ("Some(3)", "Some(3)"),
        ("map { \"a\": 1, \"b\": 2 }", "map { \"a\": 1, \"b\": 2 }"),
    ] {
        assert_eq!(eval(&mut repl, input), shown);
    }
    // `None` has no decidable type, so a diagnostic (it does not become a plain line)
    assert_eq!(codes(&submit_err(&mut repl, "None")), ["cannot_infer_type"]);
}

#[test]
fn valueless_expressions_cannot_be_written_as_expressions() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "fn nothing() {\n  let x = 1\n}");
    let r = repl.submit("nothing()");
    match r {
        Err(d) => assert!(!d.is_empty()),
        Ok(s) => panic!("値のない呼び出しは式にならない: {s:?}"),
    }
}

#[test]
fn empty_lines_and_comments_do_nothing() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    for input in ["", "   ", "// メモ", "  // メモ", "\n\n"] {
        let s = repl.submit(input).unwrap();
        assert_eq!(s.kind, InputKind::Statement);
        assert!(matches!(
            repl.run().unwrap(),
            ReplYield::Done {
                value: None,
                ref declared,
                ended: false
            } if declared.is_empty()
        ));
    }
}
