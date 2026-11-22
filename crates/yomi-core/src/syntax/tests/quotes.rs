//! Identities, quote lines and choices

use super::*;

#[test]
fn identity_declarations_and_quote_lines_can_be_parsed() {
    let src = "identity 案内人 = \"あんないにん\"\nlabel start {\n  案内人「やあ」 #hi\n  「ひとり」\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(m.identities.len(), 1);
    match &m.labels[0].body[0] {
        Stmt::Quote {
            identity,
            parts,
            id,
            ..
        } => {
            assert_eq!(identity.as_ref().map(|i| i.name.as_str()), Some("案内人"));
            assert_eq!(crate::syntax::canon::body(parts), "「やあ」");
            assert_eq!(id.as_ref().map(|i| i.id.as_str()), Some("hi"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &m.labels[0].body[1],
        Stmt::Quote { identity: None, .. }
    ));
}

#[test]
fn multiline_quotes_join_by_removing_newlines_and_leading_whitespace_and_tabs() {
    let src =
        "identity 案内人 = \"案内人\"\nlabel start {\n  案内人「あ\n     い\n\tう」\n  次\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::Quote { parts, .. } => {
            assert_eq!(crate::syntax::canon::body(parts), "「あいう」")
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&m.labels[0].body[1], Stmt::Plain { .. }));
}

#[test]
fn line_starting_with_unclosed_bracket_is_plain_line() {
    // ⑤: if the matching `」` is not at the end of the line, it is not a quote line (`\」` is not counted)
    let src = "label start {\n  「a」と「b」\n  「a」 と\n  「\\」は数えない」\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    let body = &m.labels[0].body;
    assert!(matches!(&body[0], Stmt::Plain { .. }));
    assert!(matches!(&body[1], Stmt::Plain { .. }));
    assert!(matches!(&body[2], Stmt::Quote { .. }));
}

#[test]
fn unclosed_bracket_after_identity_and_text_after_close_are_errors() {
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  案内人「あ\n}\n";
    assert!(codes(src).contains(&DiagCode::UnterminatedQuote));
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  案内人「あ」う\n}\n";
    assert!(codes(src).contains(&DiagCode::TextAfterQuote));
}

#[test]
fn choice_options_can_be_parsed() {
    let src = "label start {\n  choice {\n    「a」 once #a => { x = 1; y = 2 }\n    「b」 if true #b => 本文\n    「c」 => {\n      jump start\n    }\n  }\n}\n";
    let (m, d) = parse(src, "m");
    assert!(d.is_empty(), "{d:?}");
    match &m.labels[0].body[0] {
        Stmt::Choice { options, .. } => {
            assert_eq!(options.len(), 3);
            assert!(options[0].once.is_some() && options[0].cond.is_none());
            assert_eq!(options[0].body.len(), 2);
            assert!(options[1].cond.is_some() && options[1].once.is_none());
            assert_eq!(options[1].id.as_ref().map(|i| i.id.as_str()), Some("b"));
            assert_eq!(options[2].body.len(), 1);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn choice_without_options_is_error() {
    assert!(codes("label start {\n  choice {\n  }\n}\n").contains(&DiagCode::EmptyChoice));
}

#[test]
fn disallowed_statements_in_option_body_are_errors() {
    let src = "label start {\n  choice {\n    「a」 => if true { end }\n  }\n}\n";
    assert!(codes(src).contains(&DiagCode::UnexpectedToken));
}

#[test]
fn deeply_nested_choice_stops_with_error_without_exhausting_stack() {
    let mut src = String::from("label start {\n");
    for _ in 0..500 {
        src.push_str("choice {\n「a」 => ");
    }
    src.push_str("end\n");
    let (_, d) = parse(&src, "m");
    assert!(!d.is_empty());
}
