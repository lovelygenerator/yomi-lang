//! Basics

use super::*;

#[test]
fn parses_plain_lines_assignments_and_if() {
    let src = "save var n: int = 0\nlabel start {\n  n += 1\n  こんにちは ${n} 回目 #hello\n  if n < 3 {\n    jump start\n  } else {\n    end\n  }\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(m.vars.len(), 1);
    assert_eq!(m.labels.len(), 1);
    let body = &m.labels[0].body;
    assert!(matches!(body[0], Stmt::Assign { .. }));
    match &body[1] {
        Stmt::Plain { parts, id, .. } => {
            assert_eq!(id.as_ref().map(|i| i.id.as_str()), Some("hello"));
            assert!(matches!(parts[1], StrPart::Interp(Expr::Name(..))));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(body[2], Stmt::If { .. }));
}

#[test]
fn else_if_chains_can_be_parsed() {
    let src = "label a {\n if true {\n end\n } else if false {\n end\n } else {\n end\n }\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::If {
            arms, else_body, ..
        } => {
            assert_eq!(arms.len(), 2);
            assert_eq!(else_body.len(), 1);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn long_else_if_chains_do_not_recurse_deeply() {
    // A chain is not nesting of blocks. It can be read even if long (the limit applies only to nesting)
    let mut src = String::from("label a {\n  if x == 0 {\n  }");
    for i in 1..20_000 {
        src.push_str(&format!(" else if x == {i} {{\n  }}"));
    }
    src.push_str(" else {\n  }\n}\n");
    let (m, d) = parse(&src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::If { arms, .. } => assert_eq!(arms.len(), 20_000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_outside_labels_is_error() {
    assert_eq!(codes("こんにちは\n"), vec![DiagCode::StatementOutsideLabel]);
}

#[test]
fn unclosed_block_is_error() {
    assert_eq!(
        codes("label a {\n  end\n"),
        vec![DiagCode::UnterminatedBlock]
    );
}

#[test]
fn invalid_escapes_are_errors() {
    assert_eq!(
        codes("label a {\n  a\\qb\n}\n"),
        vec![DiagCode::InvalidEscape]
    );
}

#[test]
fn chained_comparisons_are_errors() {
    assert_eq!(
        codes("label a {\n  if 1 < 2 < 3 {\n  }\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn full_width_space_stays_in_bodies_and_is_error_in_code() {
    let (m, d) = parse("label a {\n　字下げ\n}\n", "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::Plain { raw, .. } => assert!(raw.starts_with('\u{3000}')),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        codes("label a {\n  let x =\u{3000}1\n}\n"),
        vec![DiagCode::FullWidthSpace]
    );
}

#[test]
fn trailing_stable_id_is_not_part_of_text() {
    let (m, _) = parse("label a {\n  朝です #m1\n  価格は\\#100 #m2\n}\n", "m");
    match &m.labels[0].body[1] {
        Stmt::Plain { parts, id, .. } => {
            assert_eq!(id.as_ref().unwrap().id, "m2");
            assert_eq!(
                parts,
                &vec![StrPart::Text(
                    "価格は#100".to_string(),
                    "価格は\\#100".to_string()
                )]
            );
        }
        other => panic!("{other:?}"),
    }
}

pub(super) fn plain_text(src: &str) -> String {
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::Plain { parts, .. } => match &parts[0] {
            StrPart::Text(t, _) => t.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn lines_with_equals_and_greater_than_are_plain_lines() {
    assert_eq!(plain_text("label start {\n  a => b\n}\n"), "a => b");
}

#[test]
fn leading_backslash_writes_plain_line_starting_with_keyword() {
    assert_eq!(
        plain_text("label start {\n  \\in the room\n}\n"),
        "in the room"
    );
    assert_eq!(plain_text("label start {\n  \\@home\n}\n"), "@home");
    // When characters that form escapes follow, as before
    assert_eq!(plain_text("label start {\n  \\=x\n}\n"), "=x");
}

pub(super) fn only_stmt(src: &str) -> Stmt {
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    m.labels[0].body[0].clone()
}

pub(super) fn init_of(src: &str) -> Expr {
    match only_stmt(src) {
        Stmt::Let { init, .. } => init,
        other => panic!("{other:?}"),
    }
}

#[test]
fn float_literals_can_be_parsed() {
    assert!(matches!(
        init_of("label a {\n  let x = 1.5\n}\n"),
        Expr::Float(x, _) if x == 1.5
    ));
    assert!(matches!(
        init_of("label a {\n  let x = 1_000.2_5\n}\n"),
        Expr::Float(x, _) if x == 1000.25
    ));
    // Without digits after the decimal point, it is a `.` after an integer (no name follows either, so it is an unexpected
    // token)
    assert_eq!(
        codes("label a {\n  let x = 1.\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
    // There is no exponent notation (the `e3` of `1.5e3` is left over)
    assert_eq!(
        codes("label a {\n  let x = 1.5e3\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn floats_with_edge_digit_separators_or_out_of_range_are_not_numbers() {
    for lit in ["1_.5", "1.5_", "1.5__"] {
        assert_eq!(
            codes(&format!("label a {{\n  let x = {lit}\n}}\n")),
            vec![DiagCode::InvalidNumber],
            "{lit}"
        );
    }
    let huge = format!("{}.0", "9".repeat(400));
    assert_eq!(
        codes(&format!("label a {{\n  let x = {huge}\n}}\n")),
        vec![DiagCode::InvalidNumber]
    );
}

#[test]
fn parenthesis_right_after_name_is_a_call() {
    match init_of("label a {\n  let x = to_int(1.5 + y, 2)\n}\n") {
        Expr::Call(name, args, _) => {
            assert_eq!(name, "to_int");
            assert_eq!(args.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        init_of("label a {\n  let x = f()\n}\n"),
        Expr::Call(_, args, _) if args.is_empty()
    ));
    // Calls can be combined with operators as part of expressions
    assert!(matches!(
        init_of("label a {\n  let x = -f(1) * 2\n}\n"),
        Expr::Binary(..)
    ));
    // Calls of things that are not names cannot be used yet
    for src in ["(1 + 2)(3)", "f(1)(2)", "1(2)"] {
        assert_eq!(
            codes(&format!("label a {{\n  let x = {src}\n}}\n")),
            vec![DiagCode::NotYetSupported],
            "{src}"
        );
    }
    // Dots and indices can be read as postfix expressions
    assert!(matches!(
        init_of("label a {\n  let x = a.b.c\n}\n"),
        Expr::Field(..)
    ));
    assert!(matches!(
        init_of("label a {\n  let x = a[0]\n}\n"),
        Expr::Index(..)
    ));
    // `package.name(args)` is a call of a query command
    match init_of("label a {\n  let x = math.abs(-3, y)\n}\n") {
        Expr::QueryCall(pkg, name, args, _) => {
            assert_eq!((pkg.as_str(), name.as_str()), ("math", "abs"));
            assert_eq!(args.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        init_of("label a {\n  let x = text.len()\n}\n"),
        Expr::QueryCall(_, _, args, _) if args.is_empty()
    ));
    // The arguments are not closed
    assert_eq!(
        codes("label a {\n  let x = f(1,\n}\n"),
        vec![DiagCode::UnclosedBracket, DiagCode::UnterminatedBlock]
    );
    // A trailing `,` is allowed, but two are not
    assert!(codes("label a {\n  let x = f(1,)\n}\n").is_empty());
    assert_eq!(
        codes("label a {\n  let x = f(1,,)\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn nesting_of_calls_has_limit_too() {
    let deep = format!("{}1{}", "f(".repeat(100), ")".repeat(100));
    assert_eq!(
        codes(&format!("label a {{\n  let x = {deep}\n}}\n")),
        vec![DiagCode::TooDeep]
    );
}

#[test]
fn while_break_and_continue_can_be_parsed() {
    let src =
        "label a {\n  while x < 3 {\n    if x == 1 {\n      continue\n    }\n    break\n  }\n}\n";
    match only_stmt(src) {
        Stmt::While { body, .. } => {
            assert!(matches!(body[0], Stmt::If { .. }));
            assert!(matches!(body[1], Stmt::Break(_)));
        }
        other => panic!("{other:?}"),
    }
    // An error if a token follows `break`
    assert_eq!(
        codes("label a {\n  while true {\n    break 1\n  }\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn nesting_of_while_has_limit_too() {
    let src = format!(
        "label a {{\n{}{}}}\n",
        "while true {\n".repeat(80),
        "}\n".repeat(80)
    );
    assert_eq!(codes(&src), vec![DiagCode::BlockTooDeep]);
}

#[test]
fn token_after_label_closing_brace_is_error() {
    assert_eq!(
        codes("label a {\n} junk\nlabel b {\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
    // A comment is fine
    assert!(codes("label a {\n} // 終わり\n").is_empty());
    // A token ending with `{` skips that block (no cascading)
    assert_eq!(
        codes("label a {\n} else {\n  あ\n}\nlabel b {\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn token_after_else_is_error_but_body_is_read() {
    // `else { junk`: the body is read (the `}` does not close the label)
    let src = "label a {\n  if true {\n  } else { junk\n    あ\n  }\n}\n";
    let (m, d) = parse(src, "m");
    assert_eq!(
        d.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagCode::UnexpectedToken]
    );
    match &m.labels[0].body[0] {
        Stmt::If { else_body, .. } => assert_eq!(else_body.len(), 1),
        other => panic!("{other:?}"),
    }
    // `else x {`
    let src = "label a {\n  if true {\n  } else x {\n    あ\n  }\n}\n";
    let (m, d) = parse(src, "m");
    assert_eq!(
        d.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagCode::UnexpectedToken]
    );
    match &m.labels[0].body[0] {
        Stmt::If { else_body, .. } => assert_eq!(else_body.len(), 1),
        other => panic!("{other:?}"),
    }
    // Only `else` (there is no `{` on the next line)
    assert_eq!(
        codes("label a {\n  if true {\n  } else\n  あ\n}\n"),
        vec![DiagCode::UnexpectedEol]
    );
}

#[test]
fn plain_line_ending_with_brace_does_not_skip_block() {
    // Even with an escape error, the following lines are read as statements
    assert_eq!(
        codes("label a {\n  壊れた \\q {\n  jump nowhere\n}\nlabel b {\n}\n"),
        vec![DiagCode::InvalidEscape]
    );
    // The header of a real block is skipped only on error
    assert_eq!(
        codes("label a {\n  if ) {\n    jump nowhere\n  }\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}

#[test]
fn keywords_that_cannot_start_statements_are_unexpected_tokens() {
    for w in [
        "true", "false", "in", "var", "int", "float", "bool", "str", "list", "map", "Option",
        "Identity", "Some", "None",
    ] {
        assert_eq!(
            codes(&format!("label a {{\n  {w} x\n}}\n")),
            vec![DiagCode::UnexpectedToken],
            "{w}"
        );
    }
    // A `\` at the start of a line makes it a plain line
    assert!(codes("label a {\n  \\true story\n}\n").is_empty());
}

#[test]
fn declarations_inside_labels_have_dedicated_code() {
    for w in [
        "save",
        "persistent",
        "identity",
        "import",
        "label",
        "fn",
        "enum",
        "struct",
    ] {
        let (_, d) = parse(&format!("label a {{\n  {w} x\n}}\n"), "m");
        assert_eq!(d.len(), 1, "{w}");
        assert_eq!(d[0].code, DiagCode::DeclarationNotAllowed, "{w}");
        assert_eq!(d[0].arg_str("what"), Some(w));
    }
    // The block of the header's `{` is skipped
    assert_eq!(
        codes("label a {\n  fn f() {\n    jump x\n  }\n}\n"),
        vec![DiagCode::DeclarationNotAllowed]
    );
}

#[test]
fn failed_declaration_names_are_recorded_as_poisoned() {
    let src = "save var a: int = )\nlabel b\nlabel c {\n  let x = )\n  let y: int =\n}\n";
    let (m, d) = parse(src, "m");
    assert_eq!(d.len(), 4);
    let mut names: Vec<&str> = m.poisoned.iter().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["a", "b", "x", "y"]);
}

#[test]
fn duplicate_let_diagnostic_uses_name_column() {
    let (m, _) = parse("label a {\n  let   xx = 1\n}\n", "m");
    match &m.labels[0].body[0] {
        Stmt::Let { name_pos, pos, .. } => {
            assert_eq!((pos.line, pos.col), (2, 3));
            assert_eq!((name_pos.line, name_pos.col), (2, 9));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn columns_count_characters_and_long_lines_are_fast() {
    // Finding the column for each character does not recount from the start of the line (it used to take quadratic time)
    let body = "あ".repeat(300_000);
    let src = format!("label a {{\n  let s = \"{body}\" + 1\n}}\n");
    let start = std::time::Instant::now();
    let (_, d) = parse(&src, "m");
    assert!(d.is_empty(), "{d:?}");
    // The column of a diagnostic after a string is in characters
    let src = format!("label a {{\n  let s = \"{body}\" )\n}}\n");
    let (_, d) = parse(&src, "m");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].col, 300_000 + 12 + 2);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "遅すぎる: {:?}",
        start.elapsed()
    );
}

#[test]
fn columns_match_on_long_ascii_only_lines() {
    let body = "a".repeat(200_000);
    let src = format!("label a {{\n  let s = \"{body}\" )\n}}\n");
    let (_, d) = parse(&src, "m");
    assert_eq!(d[0].col, 200_000 + 12 + 2);
}

#[test]
fn label_parameter_and_return_types_can_be_parsed() {
    let (m, d) = parse(
        "label add(a: int, b: int) -> int {\n  return a + b\n}\nlabel none() {\n}\nlabel plain {\n}\n",
        "m",
    );
    assert!(d.is_empty(), "{d:?}");
    let add = &m.labels[0];
    let names: Vec<_> = add
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.ty.name.as_str()))
        .collect();
    assert_eq!(names, vec![("a", "int"), ("b", "int")]);
    assert_eq!(add.ret.as_ref().map(|t| t.name.as_str()), Some("int"));
    assert!(matches!(add.body[0], Stmt::Return { value: Some(_), .. }));
    assert!(m.labels[1].params.is_empty() && m.labels[1].ret.is_none());
    assert!(m.labels[2].params.is_empty() && m.labels[2].ret.is_none());
}

#[test]
fn call_can_be_statement_let_or_assignment_rhs() {
    let src = "label a {\n  call f(1, 2)\n  let x = call m.g()\n  x = call f(x)\n  return\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    let body = &m.labels[0].body;
    match &body[0] {
        Stmt::Call(c) => {
            assert_eq!(c.target.written(), "f");
            assert_eq!(c.args.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    match &body[1] {
        Stmt::Let {
            init: Expr::LabelCall(c),
            ..
        } => assert_eq!(c.target.written(), "m.g"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &body[2],
        Stmt::Assign {
            value: Expr::LabelCall(_),
            ..
        }
    ));
    assert!(matches!(body[3], Stmt::Return { value: None, .. }));
}

#[test]
fn call_mid_expression_is_error() {
    for src in [
        "1 + call f()",
        "(call f())",
        "f(call g())",
        "\"a${call f()}\"",
    ] {
        let full = format!("label a {{\n  let x = {src}\n}}\n");
        assert_eq!(codes(&full), vec![DiagCode::CallInExpression], "{src}");
    }
    // The right-hand side of a compound assignment is also an expression
    assert_eq!(
        codes("label a {\n  x += call f()\n}\n"),
        vec![DiagCode::CallInExpression]
    );
    assert_eq!(
        codes("label a {\n  if call f() {\n  }\n}\n"),
        vec![DiagCode::CallInExpression]
    );
}

#[test]
fn module_qualified_names_can_be_read_in_expressions_assignments_and_jump() {
    let (m, d) = parse(
        "label a {\n  let x = m.v + 1\n  m.v += 2\n  jump m.other\n}\n",
        "m",
    );
    assert!(d.is_empty(), "{d:?}");
    let body = &m.labels[0].body;
    match &body[0] {
        Stmt::Let {
            init: Expr::Binary(_, l, _, _),
            ..
        } => assert!(matches!(&**l, Expr::Qualified(m, v, _) if m == "m" && v == "v")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &body[1],
        Stmt::Assign { module: Some((m, _)), name, .. } if m == "m" && name == "v"
    ));
    assert!(matches!(
        &body[2],
        Stmt::Jump { target, .. } if target.written() == "m.other"
    ));
    // Even with consecutive dots, if it is not in assignment form, it is a plain line
    let (m, d) = parse("label a {\n  a.b を読む\n}\n", "m");
    assert!(d.is_empty(), "{d:?}");
    assert!(matches!(m.labels[0].body[0], Stmt::Plain { .. }));
}

#[test]
fn import_only_at_top() {
    let (m, d) = parse("import a\nimport b\nlabel x {\n}\n", "m");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        m.imports
            .iter()
            .map(|i| i.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let (m, d) = parse("save var n: int = 0\nimport a\n", "m");
    assert_eq!(
        d.iter()
            .map(|d| (d.code, d.line, d.col))
            .collect::<Vec<_>>(),
        vec![(DiagCode::ImportNotFirst, 2, 1)]
    );
    // Imports that were read are kept even if misplaced (so "undeclared" does not cascade)
    assert_eq!(m.imports.len(), 1);
    // An import without a name, and reserved words
    assert_eq!(codes("import\n"), vec![DiagCode::UnexpectedEol]);
    assert_eq!(codes("import label\n"), vec![DiagCode::UnexpectedToken]);
}

#[test]
fn label_header_errors_report_one_error_and_skip_body() {
    for header in [
        "label a(",
        "label a(x)",
        "label a(x:)",
        "label a(x: int",
        "label a(x: int,) -> int",
        "label a() ->",
        "label a() -> int int",
    ] {
        let src = format!("{header} {{\n  あ\n  jump a\n}}\nlabel b {{\n}}\n");
        let (m, d) = parse(&src, "m");
        assert_eq!(d.len(), 1, "{header}: {d:?}");
        assert_eq!(m.labels.len(), 1, "{header}");
        assert_eq!(m.labels[0].name, "b");
    }
}
