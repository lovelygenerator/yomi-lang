//! Syntax that receives the return value of a command

use super::*;

#[test]
fn let_rhs_command_call_parses_as_receiving_return_value() {
    let src = "label start {\n  let name: str = @input.text(\"名前は？\", \"\", 8) #n\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::Let { name, ty, init, .. } => {
            assert_eq!(name, "name");
            assert_eq!(ty.as_ref().map(|t| t.name.as_str()), Some("str"));
            match init {
                Expr::Command(c) => {
                    assert_eq!(c.name, "input.text");
                    assert_eq!(c.args.len(), 3);
                    assert_eq!(c.id.as_ref().map(|i| i.id.as_str()), Some("n"));
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn assignment_rhs_command_call_parses_as_receiving_return_value() {
    let src =
        "label start {\n  name = @input.text(\"名前は？\", \"\", 8)\n  m.v = @game.roll(6)\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    for st in &m.labels[0].body {
        match st {
            Stmt::Assign { value, .. } => assert!(matches!(value, Expr::Command(_))),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn at_mid_expression_parses_but_compilation_rejects_it() {
    // It goes into the syntax tree (compilation makes it `invalid_command_use`)
    for src in [
        "label start {\n  let a = 1 + @game.roll(2)\n}\n",
        "label start {\n  if @input.confirm(\"?\") {\n  }\n}\n",
        "label start {\n  @game.give(@game.ask(\"x\"), 1)\n}\n",
        "label start {\n  x += @game.roll(2)\n}\n",
    ] {
        let (_, d) = parse(src, "m");
        assert!(d.is_empty(), "{src}: {d:?}");
    }
}

#[test]
fn command_call_without_closing_paren_cannot_be_parsed() {
    assert_eq!(
        codes("label start {\n  let a = @game.roll(2\n}\n"),
        vec![DiagCode::UnclosedBracket, DiagCode::UnterminatedBlock]
    );
    assert_eq!(
        codes("label start {\n  let a = @\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
}
