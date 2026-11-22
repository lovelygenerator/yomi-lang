use super::*;

const DOC: &str = r#"{"name":"太郎","age":20,"score":1.5,"ok":true,"none":null,
    "items":[{"name":"a","n":1},{"name":"b","n":2}],"x.y":{"z":[10,20]}}"#;

fn get_str(s: &str, p: &str) -> Option<String> {
    match at(s, p)? {
        Value::String(x) => Some(x),
        _ => None,
    }
}

#[test]
fn path_parsing_accepts_keys_indices_and_quoted_keys() {
    assert_eq!(parse_path("").unwrap(), vec![]);
    assert_eq!(
        parse_path("a.b[2][\"x.y\"].c").unwrap(),
        vec![
            Seg::Key("a".into()),
            Seg::Key("b".into()),
            Seg::Index(2),
            Seg::Key("x.y".into()),
            Seg::Key("c".into())
        ]
    );
    assert_eq!(parse_path("[0].名前").unwrap().len(), 2);
    assert_eq!(parse_path("[\"\"]").unwrap(), vec![Seg::Key(String::new())]);
    assert_eq!(
        parse_path("[\"a\\\"b\"]").unwrap(),
        vec![Seg::Key("a\"b".into())]
    );
}

#[test]
fn malformed_paths_find_nothing() {
    for p in [
        ".a",
        "a..b",
        "a.",
        "a[",
        "[-1]",
        "[x]",
        "[]",
        "a[0]b",
        "a\"b",
        "[\"a\"",
        "[99999999999999999999]",
    ] {
        assert!(parse_path(p).is_none(), "{p}");
        assert!(!at(DOC, p).is_some(), "{p}");
    }
}

#[test]
fn getters_return_values_of_the_matching_type_only() {
    assert_eq!(get_str(DOC, "name").as_deref(), Some("太郎"));
    assert_eq!(get_str(DOC, "items[1].name").as_deref(), Some("b"));
    assert_eq!(get_str(DOC, "x.y"), None);
    assert_eq!(get_str(DOC, "[\"x.y\"].z"), None);
    assert_eq!(at(DOC, "age").unwrap().as_i64(), Some(20));
    assert_eq!(at(DOC, "score").unwrap().as_i64(), None);
    assert_eq!(at(DOC, "age").unwrap().as_f64(), Some(20.0));
    assert_eq!(at(DOC, "ok").unwrap().as_bool(), Some(true));
    assert_eq!(at(DOC, "name").unwrap().as_bool(), None);
    assert_eq!(at(DOC, "[\"x.y\"].z[1]").unwrap().as_i64(), Some(20));
    assert!(at(DOC, "items[2]").is_none());
    assert!(at(DOC, "missing").is_none());
}

#[test]
fn null_is_a_value_and_kinds_are_named() {
    assert!(at(DOC, "none").is_some());
    let kinds: Vec<_> = ["none", "ok", "age", "name", "items", ""]
        .iter()
        .map(|p| kind(&at(DOC, p).unwrap()))
        .collect();
    assert_eq!(
        kinds,
        ["null", "bool", "number", "string", "array", "object"]
    );
}

#[test]
fn integer_forms_are_distinguished_from_floats() {
    assert_eq!(at("1e3", "").unwrap().as_i64(), None);
    assert_eq!(at("1.0", "").unwrap().as_i64(), None);
    assert_eq!(
        at("9223372036854775807", "").unwrap().as_i64(),
        Some(i64::MAX)
    );
    assert_eq!(at("9223372036854775808", "").unwrap().as_i64(), None);
    assert_eq!(
        dump(&at("18446744073709551615", "").unwrap()).unwrap(),
        "18446744073709551615"
    );
    let big = at("123456789012345678901234567890", "").unwrap();
    assert_eq!(big.as_i64(), None);
    assert!(big.as_f64().is_some());
}

#[test]
fn validity_follows_json_and_the_depth_limit() {
    assert!(parse_doc(" [1, 2] ").is_some());
    assert!(parse_doc("1 2").is_none());
    assert!(parse_doc("").is_none());
    assert!(parse_doc("{'a':1}").is_none());
    assert!(parse_doc("1e999").is_none());
    let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert!(parse_doc(&ok).is_some());
    assert!(parse_doc(&deep).is_none());
    assert!(parse_doc(&format!("[\"{}\"]", "[".repeat(500))).is_some());
}

#[test]
fn len_and_keys_cover_arrays_and_objects() {
    assert_eq!(at(DOC, "items").unwrap().as_array().map(Vec::len), Some(2));
    let root = at(DOC, "").unwrap();
    assert_eq!(
        keys(root.as_object().unwrap()),
        ["age", "items", "name", "none", "ok", "score", "x.y"]
    );
}

#[test]
fn duplicate_keys_keep_the_last_one() {
    assert_eq!(at(r#"{"a":1,"a":2}"#, "a").unwrap().as_i64(), Some(2));
}

#[test]
fn output_is_compact_with_sorted_keys() {
    assert_eq!(
        dump(&at(r#"{ "b": [1, 2.5], "a": "x" }"#, "").unwrap()).unwrap(),
        r#"{"a":"x","b":[1,2.5]}"#
    );
}

#[test]
fn quote_escapes_only_what_json_requires() {
    assert_eq!(
        Value::String("a\"b\\c\n\u{1}日本".into()).to_string(),
        "\"a\\\"b\\\\c\\n\\u0001日本\""
    );
    let q = Value::String("x\ty".into()).to_string();
    assert_eq!(get_str(&q, "").as_deref(), Some("x\ty"));
}

#[test]
fn set_replaces_adds_and_appends() {
    assert!(set(DOC, "age", "21").unwrap().contains("\"age\":21"));
    assert_eq!(set("{}", "a", "1").as_deref(), Some(r#"{"a":1}"#));
    assert_eq!(set("[1,2]", "[1]", "9").as_deref(), Some("[1,9]"));
    assert_eq!(set("[1,2]", "[2]", "9").as_deref(), Some("[1,2,9]"));
    assert_eq!(set("[1,2]", "[3]", "9"), None);
    assert_eq!(
        set(r#"{"a":{"b":1}}"#, "a.b", "[true]").as_deref(),
        Some(r#"{"a":{"b":[true]}}"#)
    );
    assert_eq!(set("1", "", "{\"a\":1}").as_deref(), Some(r#"{"a":1}"#));
    assert_eq!(set("{}", "a.b", "1"), None);
    assert_eq!(set("[]", "a", "1"), None);
    assert_eq!(set("{}", "a", "not json"), None);
    assert_eq!(set("nope", "a", "1"), None);
    assert_eq!(
        set("{}", "a", &format!("[{}]", "1,".repeat(3) + "2")).as_deref(),
        Some(r#"{"a":[1,1,1,2]}"#)
    );
}

#[test]
fn set_rejects_a_result_nested_too_deeply() {
    let d = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(set(&d(100), &"[0]".repeat(99), &d(100)).is_none());
    assert!(set(&d(60), &"[0]".repeat(59), &d(60)).is_some());
}

#[test]
fn remove_deletes_keys_and_shifts_array_elements() {
    assert_eq!(
        remove(r#"{"a":1,"b":2}"#, "a").as_deref(),
        Some(r#"{"b":2}"#)
    );
    assert_eq!(remove("[1,2,3]", "[0]").as_deref(), Some("[2,3]"));
    assert_eq!(remove("[1]", "[1]"), None);
    assert_eq!(remove("{}", "a"), None);
    assert_eq!(remove("{}", ""), None);
    assert_eq!(remove("nope", "a"), None);
}
