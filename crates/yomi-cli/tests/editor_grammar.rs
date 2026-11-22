//! Checks that the grammar of the editor extension (`editors/vscode`) matches the table of reserved words.
//!
//! If the extension's files are missing (such as a public repository that did not export the extension), it finishes
//! without checking anything.

use std::path::PathBuf;

use yomi_core::{KEYWORDS, KeywordKind};

fn read(rel: &str) -> Option<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "editors",
        "vscode",
        rel,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(path).ok()
}

/// The words in word alternations (`\b(a|b|…)\b`) among the `match`es of `repository` names in the grammar.
fn words_of(grammar: &serde_json::Value, rule: &str) -> Vec<String> {
    let m = grammar["repository"][rule]["match"]
        .as_str()
        .expect("match がある");
    let inner = m
        .strip_prefix("\\b(")
        .and_then(|s| s.strip_suffix(")\\b"))
        .expect("単語の選択の形");
    inner.split('|').map(str::to_string).collect()
}

#[test]
fn grammar_keywords_and_type_names_match_keyword_table() {
    let Some(text) = read("syntaxes/yomi.tmLanguage.json") else {
        return;
    };
    let grammar: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let mut keywords = words_of(&grammar, "keyword");
    let mut types = words_of(&grammar, "type");
    keywords.sort();
    types.sort();
    let mut want_keywords: Vec<String> = KEYWORDS
        .iter()
        .filter(|k| k.kind != KeywordKind::Type)
        .map(|k| k.word.to_string())
        .collect();
    let mut want_types: Vec<String> = KEYWORDS
        .iter()
        .filter(|k| k.kind == KeywordKind::Type)
        .map(|k| k.word.to_string())
        .collect();
    want_keywords.sort();
    want_types.sort();
    assert_eq!(keywords, want_keywords);
    assert_eq!(types, want_types);
}

#[test]
fn extension_config_files_are_valid_json() {
    for rel in ["package.json", "language-configuration.json"] {
        if let Some(text) = read(rel) {
            serde_json::from_str::<serde_json::Value>(&text)
                .unwrap_or_else(|e| panic!("{rel}: {e}"));
        }
    }
}
