//! Statements continued over several physical lines (an open bracket at the end of a line)

use super::*;
use crate::syntax::ast::Pos;

fn body(src: &str) -> Vec<Stmt> {
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    m.labels[0].body.clone()
}

#[test]
fn list_map_and_struct_literals_continue_over_lines() {
    let src = "struct C { name: str, hp: int }\nlabel a {\n  let xs = [\n    1,\n    2,\n  ]\n  let m = map {\n    \"a\": 1,\n  }\n  let c = C {\n    name: \"x\",\n    hp: 1,\n  }\n}\n";
    assert_eq!(body(src).len(), 3);
}

#[test]
fn call_arguments_continue_and_allow_a_trailing_comma() {
    let src = "label a {\n  @f(\n    1,\n    2,\n  )\n  @f(1, 2,)\n}\n";
    assert_eq!(body(src).len(), 2);
}

#[test]
fn blank_and_comment_lines_between_continuation_lines_are_skipped() {
    let src = "label a {\n  let xs = [ // first\n\n    // note\n    1, // one\n    2\n  ]\n}\n";
    assert_eq!(body(src).len(), 1);
}

#[test]
fn brackets_inside_strings_and_comments_are_not_counted() {
    let src = "label a {\n  let s = \"(((\" // [[\n  let t = f(\"]\")\n}\n";
    assert_eq!(body(src).len(), 2);
}

#[test]
fn block_header_continues_but_its_brace_does_not() {
    let src =
        "label a {\n  if f(\n      1, 2) {\n    end\n  }\n  xs.each { x =>\n    end\n  }\n}\n";
    assert_eq!(body(src).len(), 2);
}

#[test]
fn plain_and_quote_lines_are_not_continued() {
    let src = "label a {\n  こんにちは（笑\n  次の行\n}\n";
    assert_eq!(body(src).len(), 2);
}

#[test]
fn diagnostics_keep_the_physical_line_and_column() {
    let src = "label a {\n  let xs = [\n    1,\n    ?,\n  ]\n}\n";
    let (_, d) = parse(src, "m");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!((d[0].line, d[0].col), (4, 5));
}

#[test]
fn unclosed_bracket_at_the_end_of_the_file_is_reported_at_the_bracket() {
    let src = "label a {\n  let xs = [\n    1,\n";
    let (_, d) = parse(src, "m");
    let d: Vec<_> = d
        .iter()
        .filter(|d| d.code == DiagCode::UnclosedBracket)
        .collect();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(
        Pos {
            line: d[0].line,
            col: d[0].col
        },
        Pos { line: 2, col: 12 }
    );
}

#[test]
fn continuation_is_limited_to_256_lines() {
    let mut src = String::from("label a {\n  let xs = [\n");
    for _ in 0..300 {
        src.push_str("    1,\n");
    }
    src.push_str("  ]\n}\n");
    assert!(codes(&src).contains(&DiagCode::UnclosedBracket));
    let mut ok = String::from("label a {\n  let xs = [\n");
    for _ in 0..255 {
        ok.push_str("    1,\n");
    }
    ok.push_str("  ]\n}\n");
    assert!(codes(&ok).is_empty());
}

#[test]
fn module_variable_and_option_lines_continue() {
    let src = "save var xs: list<int> = [\n  1,\n  2,\n]\nlabel a {\n  choice {\n    「はい」 => @f(\n      1,\n    )\n  }\n}\n";
    let (_, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
}
