//! Unit tests of parsing and generation (the macro itself is called in `tests/derive.rs` of `yomi-core`).

use crate::gen_code::generate;
use crate::parse::{Item, ItemKind, Tok, parse};

/// A simple tokenizer (for tests; symbols one character at a time, brackets become Groups).
fn lex(src: &str) -> Vec<Tok> {
    fn go(chars: &[char], i: &mut usize, close: Option<char>) -> Vec<Tok> {
        let mut out = Vec::new();
        while *i < chars.len() {
            let c = chars[*i];
            if Some(c) == close {
                *i += 1;
                return out;
            }
            if c.is_whitespace() {
                *i += 1;
            } else if c.is_alphanumeric() || c == '_' {
                let st = *i;
                while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
                    *i += 1;
                }
                out.push(Tok::Ident(chars[st..*i].iter().collect()));
            } else if c == '"' {
                let st = *i;
                *i += 1;
                while chars[*i] != '"' {
                    *i += 1;
                }
                *i += 1;
                out.push(Tok::Literal(chars[st..*i].iter().collect()));
            } else if let Some(end) = match c {
                '(' => Some(')'),
                '{' => Some('}'),
                '[' => Some(']'),
                _ => None,
            } {
                *i += 1;
                let inner = go(chars, i, Some(end));
                out.push(Tok::Group(c, inner));
            } else {
                *i += 1;
                let joint =
                    chars.get(*i).is_some_and(|n| "<>:-".contains(*n)) && "<>:-".contains(c);
                out.push(Tok::Punct(c, joint));
            }
        }
        out
    }
    let chars: Vec<char> = src.chars().collect();
    go(&chars, &mut 0, None)
}

fn ok(src: &str) -> Item {
    parse(&lex(src)).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn ng(src: &str) -> String {
    parse(&lex(src)).expect_err(src)
}

#[test]
fn structs_with_named_fields_can_be_parsed() {
    let item = ok(
        "#[yomi(namespace = \"game\")] pub struct Chara { pub name: String, hp: Vec<i64>, m: YomiMap<String, i64> }",
    );
    assert_eq!(item.full_name, "game.Chara");
    let ItemKind::Struct(fields) = item.kind else {
        panic!()
    };
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].0, "name");
    // Commas inside types do not split fields
    assert_eq!(fields[2].0, "m");
}

#[test]
fn enums_with_unit_and_tuple_variants_can_be_parsed() {
    let item = ok(
        "#[yomi(namespace = \"n\", name = \"気分\")] enum Mood { Happy, Angry(i64), Pair(String, Option<i64>,), }",
    );
    assert_eq!(item.rust_name, "Mood");
    assert_eq!(item.full_name, "n.気分");
    let ItemKind::Enum(v) = item.kind else {
        panic!()
    };
    assert_eq!(v.len(), 3);
    assert!(v[0].1.is_empty());
    assert_eq!(v[2].1.len(), 2);
}

#[test]
fn names_can_include_namespace() {
    let item = ok("#[yomi(name = \"n.T\")] struct A {}");
    assert_eq!(item.full_name, "n.T");
    assert!(ng("#[yomi(name = \"n.T\", namespace = \"m\")] struct A {}").contains("併記"));
}

#[test]
fn other_attributes_are_ignored_and_doc_comments_are_fine() {
    let item = ok(
        "#[derive(Clone)] #[doc = \"説明\"] #[yomi(namespace = \"n\")] struct A { #[doc = \"x\"] a: i64 }",
    );
    assert_eq!(item.rust_name, "A");
}

#[test]
fn unparseable_forms_are_rejected_with_japanese_message() {
    assert!(ng("#[yomi(namespace = \"n\")] struct A<T> { a: T }").contains("ジェネリクス"));
    assert!(ng("#[yomi(namespace = \"n\")] struct A(i64);").contains("タプル"));
    assert!(ng("#[yomi(namespace = \"n\")] struct A;").contains("フィールドのない"));
    assert!(ng("#[yomi(namespace = \"n\")] enum E { A { x: i64 } }").contains("波括弧"));
    assert!(ng("#[yomi(namespace = \"n\")] enum E { A = 1 }").contains("判別値"));
    assert!(ng("#[yomi(namespace = \"n\")] enum E {}").contains("バリアントが 1 つもない"));
    assert!(ng("#[yomi(namespace = \"n\", color = \"x\")] struct A {}").contains("color"));
    assert!(ng("#[yomi(namespace = n)] struct A {}").contains("#[yomi"));
    assert!(ng("struct A { a: i64 }").contains("名前空間"));
    assert!(
        ng("#[yomi(namespace = \"n\")] struct A { #[yomi(name = \"x\")] a: i64 }")
            .contains("型にだけ")
    );
    assert!(ng("#[yomi(namespace = \"n\")] union U { a: i64 }").contains("union"));
    assert!(ng("#[yomi(namespace = \"n\")] fn f() {}").contains("struct と enum"));
    assert!(ng("#[yomi(namespace = \"n\")] struct A { a }").contains("名前: 型"));
    assert!(ng("#[yomi(namespace = \"a.b\")] struct A {}").contains("`.`"));
}

#[test]
fn generated_code_uses_absolute_paths_and_includes_type_names() {
    let code = generate(&ok("#[yomi(namespace = \"n\")] struct A { a: i64 }"));
    assert!(code.contains("::yomi_core::YomiDecl for A"));
    assert!(code.contains("\"n.A\""));
    assert!(code.contains("self.a"));
    let code = generate(&ok("#[yomi(namespace = \"n\")] enum E { U, T(i64) }"));
    assert!(code.contains("Self::T(a0)"));
    assert!(!code.contains(" yomi_core::"), "絶対パスだけ");
}
