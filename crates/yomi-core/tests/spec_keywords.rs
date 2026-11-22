//! Checks that the reserved words of the spec (section 1.6 of `spec/language/01-lexical.md`) match the core's
//! `KEYWORDS`.
//!
//! The spec files are relative to the workspace root (`CARGO_MANIFEST_DIR/../../spec/…`). If the files are missing (such
//! as a public repository exported with only the spec), it finishes without comparing against the spec.

use std::collections::BTreeSet;
use std::path::PathBuf;

use yomi_core::{CONTEXTUAL_KEYWORDS, KEYWORDS, is_reserved, keyword};

fn spec_section() -> Option<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "spec",
        "language",
        "01-lexical.md",
    ]
    .iter()
    .collect();
    let text = std::fs::read_to_string(path).ok()?;
    let section = text.split("## 1.6 ").nth(1)?;
    Some(section.split("\n## ").next()?.to_string())
}

fn spec_keywords() -> Option<Vec<String>> {
    let section = spec_section()?;
    let block = section.split("```").nth(1)?;
    Some(block.split_whitespace().map(str::to_string).collect())
}

#[test]
fn spec_keywords_match_table_in_same_order() {
    let Some(spec) = spec_keywords() else {
        return;
    };
    let table: Vec<&str> = KEYWORDS.iter().map(|k| k.word).collect();
    assert_eq!(spec, table);
}

#[test]
fn table_has_no_duplicates_and_all_identifier_shaped() {
    let words: BTreeSet<&str> = KEYWORDS.iter().map(|k| k.word).collect();
    assert_eq!(words.len(), KEYWORDS.len());
    for k in KEYWORDS {
        assert!(
            k.word.chars().all(|c| c.is_ascii_alphabetic()),
            "{}",
            k.word
        );
        assert_eq!(keyword(k.word), Some(k));
    }
}

#[test]
fn contextual_words_are_not_keywords_and_appear_in_spec_1_6() {
    for c in CONTEXTUAL_KEYWORDS {
        assert!(!is_reserved(c.word), "{}", c.word);
    }
    for w in ["random", "seen", "visited", "range"] {
        assert!(!is_reserved(w), "{w}");
    }
    let Some(section) = spec_section() else {
        return;
    };
    for c in CONTEXTUAL_KEYWORDS {
        assert!(section.contains(&format!("`{}`", c.word)), "{}", c.word);
    }
}
