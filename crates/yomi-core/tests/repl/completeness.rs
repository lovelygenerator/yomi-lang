//! 7.5 completeness

use super::*;

pub(super) fn kind(input: &str) -> &'static str {
    match completeness(input) {
        Completeness::Complete => "complete",
        Completeness::Incomplete => "incomplete",
        Completeness::Invalid(_) => "invalid",
        _ => unreachable!(),
    }
}

#[test]
fn completeness_marks_finished_input_complete() {
    for input in [
        "",
        "   ",
        "\n",
        "// コメント",
        "favor",
        "favor += 1",
        "let x = 1",
        ":vars",
        "  :apply",
        "\\:x",
        "あいうえお",
        "「かっこだけ",
        "if x {\n}",
        "if x {\n  a = 1\n}",
        "if x {\n  a = 1\n} else {\n  a = 2\n}",
        "while true {\n}",
        "for i in xs {\n  y\n\n  z\n}",
        "match x {\n  1 => a = 1\n  _ => a = 2\n}",
        "choice {\n  「a」 => x = 1\n}",
        "fn f() {\n}",
        "struct S { a: int }",
        "struct S {\n  a: int\n}",
        "enum E {\n  A,\n  B(int),\n}",
        "import math",
        "identity 案内人 = \"リオン\"",
        "案内人「こんにちは」",
        "案内人「こんにちは\n続き」",
        "x = \"}\"",
        "x = [1, 2]",
        // The syntax is complete (`submit` reports it as something that cannot be written)
        "jump shop",
        "end",
        "return",
    ] {
        assert_eq!(
            kind(input),
            "complete",
            "{input:?}: {:?}",
            completeness(input)
        );
    }
}

#[test]
fn completeness_marks_unclosed_blocks_and_quotes_incomplete() {
    for input in [
        "if x {",
        "if x {\n  a = 1",
        "if x {\n  a = 1\n",
        "if x {\n  a = 1\n\n",
        "if x {\n\n\n",
        "if a {\n} else {",
        "while true {",
        "for i in xs {",
        "match x {",
        "match x {\n  1 => a = 1",
        "choice {",
        "choice {\n  「a」 => x = 1",
        "fn f() {",
        "fn f(n: int) -> int {\n  return n",
        "struct S {",
        "struct S {\n  a: int",
        "enum E {\n  A,",
        "if a {\n  if b {\n  }",
        "案内人「こんにちは",
        "案内人「こんにちは\n続き",
        "案内人「こんにちは\n\n",
        "choice {\n  「a",
        "choice {\n  「a」 => {",
    ] {
        assert_eq!(
            kind(input),
            "incomplete",
            "{input:?}: {:?}",
            completeness(input)
        );
    }
}

#[test]
fn completeness_marks_unfixable_input_invalid() {
    for (input, code) in [
        ("let x =", "unexpected_eol"),
        ("x = [1, 2", "unexpected_eol"),
        ("x = \"abc", "unterminated_string"),
        ("}", "unexpected_close"),
        ("if x {\n}\n}", "unexpected_close"),
        ("else {", "else_without_if"),
        ("if x {\n  let y =", "unexpected_eol"),
        ("save var x: int = 1", "repl_not_allowed"),
        ("persistent var x: int = 1", "repl_not_allowed"),
        ("label foo {", "repl_not_allowed"),
    ] {
        match completeness(input) {
            Completeness::Invalid(d) => {
                assert!(
                    d.iter().any(|d| d.code.as_str() == code),
                    "{input:?}: {code} がない: {d:?}"
                );
            }
            other => panic!("{input:?}: {other:?}"),
        }
    }
}

#[test]
fn completeness_invalid_diagnostics_are_in_input_position_order() {
    let Completeness::Invalid(d) = completeness("if x {\n  y = (\n  z = (\n}") else {
        panic!()
    };
    assert!(d.len() >= 2);
    assert!(
        d.windows(2)
            .all(|w| (w[0].line, w[0].col) <= (w[1].line, w[1].col))
    );
}

#[test]
fn bracket_line_without_identity_completes_as_plain_and_quote_over_256_lines_is_invalid() {
    assert_eq!(kind("「閉じない"), "complete");
    let mut long = String::from("案内人「はじまり\n");
    for _ in 0..255 {
        long.push_str("つづき\n");
    }
    // If the continuation lines are up to 256, more can still be written
    long.push_str("最後");
    assert_eq!(kind(&long), "incomplete");
    let mut longer = long.clone();
    longer.push_str("\nさらに\nもっと");
    assert_eq!(kind(&longer), "invalid");
}

#[test]
fn completeness_is_environment_independent_and_deterministic() {
    let reg = registry();
    let prog = program(&reg);
    let inputs = [
        "案内人「こんにちは",
        "if x {",
        "gld",
        "favor",
        ":vars",
        "let x =",
    ];
    let before: Vec<_> = inputs.iter().map(|i| completeness(i)).collect();
    // Changing the environment (declarations) does not change it
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "identity 村長 = \"x\"");
    exec(&mut repl, "let gld = 1");
    let after: Vec<_> = inputs.iter().map(|i| completeness(i)).collect();
    assert_eq!(before, after);
}

#[test]
fn submit_returns_unterminated_diagnostics_for_incomplete_input() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(
        codes(&submit_err(&mut repl, "if true {")),
        ["unterminated_block"]
    );
    assert_eq!(
        codes(&submit_err(&mut repl, "fn f() {")),
        ["unterminated_block"]
    );
    assert_eq!(
        codes(&submit_err(&mut repl, "案内人「こんにちは")),
        ["unterminated_quote"]
    );
    assert!(
        codes(&submit_err(&mut repl, "choice {\n  「a」 => {"))
            .iter()
            .all(|c| *c == "unterminated_block")
    );
    // Even if `submit` returns diagnostics, the state does not change
    assert_eq!(eval(&mut repl, "favor"), "3");
}
