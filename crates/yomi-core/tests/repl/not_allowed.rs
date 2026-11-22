//! 7.2.3 Things that cannot be written, 7.7 repl_not_allowed

use super::*;

#[test]
fn not_allowed_things_are_repl_not_allowed_with_kind_in_what() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    let cases = [
        ("save var x: int = 1", "save"),
        ("persistent var y: int = 1", "persistent"),
        ("label foo {", "label"),
        ("jump shop", "jump"),
        ("end", "end"),
        ("return", "return"),
        ("if true {\n  return\n}", "return"),
        ("案内人「やあ」 #id1", "stable_id"),
        ("\\行 #id2", "stable_id"),
        ("choice {\n  「a」 #x1 => favor += 1\n}", "stable_id"),
        ("choice {\n  「a」 once #x1 => favor += 1\n}", "once"),
        ("let v = @game.ask(\"?\") #q1", "stable_id"),
    ];
    for (input, what) in cases {
        let d = submit_err(&mut repl, input);
        assert_eq!(d[0].code, DiagCode::ReplNotAllowed, "{input:?}: {d:?}");
        assert_eq!(d[0].arg_str("what"), Some(what), "{input:?}");
        assert_eq!(d[0].severity, Severity::Error);
        assert_eq!(d[0].module, "<repl>");
    }
    // The state does not change
    assert_eq!(eval(&mut repl, "favor"), "3");
}

#[test]
fn return_can_be_written_in_fn() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "fn f() -> int {\n  return 1\n}");
    assert_eq!(eval(&mut repl, "f()"), "1");
}

#[test]
fn diagnostic_position_is_line_and_column_in_whole_committed_input() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    let d = submit_err(&mut repl, "if true {\n  favor = nothing_here\n}");
    assert_eq!(codes(&d), ["unknown_name"]);
    assert_eq!((d[0].line, d[0].col), (2, 11));
    let d = submit_err(&mut repl, "fn f() -> int {\n  return \"a\"\n}");
    assert_eq!(d[0].line, 2);
}
