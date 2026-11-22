//! import

use super::*;

/// Parses an `import` line into a list of (path, name used in the script, whether there is an alias).
pub(super) fn imports_of(src: &str) -> (Vec<(String, String, bool)>, Vec<DiagCode>) {
    let (m, d) = parse(src, "m");
    (
        m.imports
            .iter()
            .map(|i| (i.path.clone(), i.local.clone(), i.aliased))
            .collect(),
        d.iter().map(|d| d.code).collect(),
    )
}

pub(super) fn t(path: &str, local: &str, aliased: bool) -> (String, String, bool) {
    (path.to_string(), local.to_string(), aliased)
}

#[test]
fn import_can_list_multiple_names_on_one_line() {
    let (v, d) = imports_of("import {audio, sprite, textbox}\n");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        v,
        vec![
            t("audio", "audio", false),
            t("sprite", "sprite", false),
            t("textbox", "textbox", false)
        ]
    );
}

#[test]
fn import_can_write_hierarchical_paths_and_name_groups() {
    let (v, d) = imports_of("import story::ch1\nimport story::{ch2, part::a, part::b}\n");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        v,
        vec![
            t("story::ch1", "ch1", false),
            t("story::ch2", "ch2", false),
            t("story::part::a", "a", false),
            t("story::part::b", "b", false),
        ]
    );
}

#[test]
fn import_braces_nest_and_allow_trailing_comma_and_spaces() {
    let (v, d) = imports_of("import  a :: { b ,  c::{ d, e, },\t}\n");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        v,
        vec![
            t("a::b", "b", false),
            t("a::c::d", "d", false),
            t("a::c::e", "e", false)
        ]
    );
}

#[test]
fn import_can_alias_with_as() {
    let (v, d) =
        imports_of("import a::util as autil\nimport b::{util as butil, x}\nimport audio as a\n");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        v,
        vec![
            t("a::util", "autil", true),
            t("b::util", "butil", true),
            t("b::x", "x", false),
            t("audio", "a", true),
        ]
    );
}

#[test]
fn import_name_starting_with_as_is_not_mistaken_for_alias() {
    let (v, d) = imports_of("import assets::{asset_a}\nimport assets2\n");
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        v,
        vec![
            t("assets::asset_a", "asset_a", false),
            t("assets2", "assets2", false)
        ]
    );
}

#[test]
fn misspelled_import_is_syntax_error() {
    for src in [
        "import\n",
        "import {}\n",
        "import {a b}\n",
        "import a::\n",
        "import a::{b\n",
        "import a::{b}}\n",
        "import a as\n",
        "import a as b as c\n",
        "import a::{b} as c\n",
        "import {,a}\n",
        "import end\n",
    ] {
        let (_, d) = imports_of(src);
        assert!(
            matches!(
                d[..],
                [DiagCode::UnexpectedToken] | [DiagCode::UnexpectedEol]
            ),
            "{src:?}: {d:?}"
        );
    }
}

#[test]
fn too_deep_import_braces_are_error() {
    let src = format!("import {}a{}\n", "{".repeat(40), "}".repeat(40));
    let (_, d) = imports_of(&src);
    assert!(!d.is_empty());
}

#[test]
fn scan_imports_expands_braces_and_paths_and_skips_aliases() {
    let src = "import audio\nimport story::{ch1, ch2 as c, part::{a, b,},}\n  import x::y as z\nimport\nimportant\n";
    assert_eq!(
        crate::syntax::parser::scan_imports(src),
        vec![
            "audio",
            "story::ch1",
            "story::ch2",
            "story::part::a",
            "story::part::b",
            "x::y"
        ]
    );
    assert_eq!(
        crate::syntax::parser::scan_imports("import {a, b}\n"),
        vec!["a", "b"]
    );
}
