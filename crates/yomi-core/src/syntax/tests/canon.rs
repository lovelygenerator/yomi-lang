//! The standard form of expressions (the input of the position hash)

use super::*;

fn canon(expr: &str) -> String {
    let src = format!("label a {{\n  let x = {expr}\n}}\n");
    let (m, d) = parse(&src, "m");
    assert!(d.is_empty(), "{expr}: {d:?}");
    match &m.labels[0].body[0] {
        Stmt::Let { init, .. } => crate::syntax::canon::expr(init),
        other => panic!("{other:?}"),
    }
}

#[test]
fn spaces_and_redundant_parentheses_disappear() {
    assert_eq!(canon("1+2"), "1 + 2");
    assert_eq!(canon("  1   +   2  "), "1 + 2");
    assert_eq!(canon("(1 + 2) + 3"), "1 + 2 + 3");
    assert_eq!(canon("1 + (2 * 3)"), "1 + 2 * 3");
    assert_eq!(canon("((x))"), "x");
    assert_eq!(canon("f( a ,b )"), "f(a, b)");
}

#[test]
fn needed_parentheses_stay() {
    assert_eq!(canon("a - (b - c)"), "a - (b - c)");
    assert_eq!(canon("(a - b) - c"), "a - b - c");
    assert_eq!(canon("(a + b) * c"), "(a + b) * c");
    assert_eq!(canon("-(a + b)"), "-(a + b)");
    assert_eq!(canon("!(a && b)"), "!(a && b)");
    assert_eq!(canon("(a + b).len()"), "(a + b).len()");
    assert_eq!(canon("(a < b) == c"), "a < b == c");
    assert_eq!(canon("a == (b == c)"), "a == (b == c)");
    assert_eq!(canon("(a == b) == c"), "(a == b) == c");
    assert_eq!(canon("--x"), "--x");
}

#[test]
fn literals_have_one_form() {
    assert_eq!(canon("1_000"), "1000");
    assert_eq!(canon("007"), "7");
    assert_eq!(canon("1.50"), "1.5");
    assert_eq!(canon("3.0"), "3.0");
    assert_eq!(
        canon("map { \"a\" : 1 , \"b\":2, }"),
        "map { \"a\": 1, \"b\": 2 }"
    );
    assert_eq!(canon("map {  }"), "map {}");
    assert_eq!(canon("[ 1 ,2 ]"), "[1, 2]");
    assert_eq!(canon("Some( 1 )"), "Some(1)");
}

#[test]
fn lambdas_and_strings_are_written_in_the_standard_form() {
    assert_eq!(canon("xs.map( |a ,b|a+b )"), "xs.map(|a, b| a + b)");
    assert_eq!(canon("|| 1"), "|| 1");
    assert_eq!(canon("1 + (|a| a)"), "1 + (|a| a)");
    // Characters in string literals stay as written, and the inside of `${ }` is in the standard form
    assert_eq!(
        canon("\"a  \\n ${ x+1 } \\$ b\""),
        "\"a  \\n ${x + 1} \\$ b\""
    );
}

#[test]
fn identifiers_are_nfc() {
    let decomposed = "\u{304B}\u{3099}";
    assert_eq!(canon(&format!("x{decomposed} + 1")), "x\u{304C} + 1");
}
