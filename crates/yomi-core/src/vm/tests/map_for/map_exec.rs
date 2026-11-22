//! Running maps

use super::*;

#[test]
fn iteration_is_insertion_order_and_assigning_existing_key_keeps_position() {
    let body = "  let m: map<str, int> = map { \"a\": 1, \"b\": 2, \"c\": 3 }\n  m[\"b\"] = 20\n  m[\"a\"] += 5\n  m[\"d\"] = 4\n  ${m.keys()} ${m.values()}";
    assert_eq!(one(body), "[a, b, c, d] [6, 20, 3, 4]");
}

#[test]
fn removing_and_reinserting_goes_to_end() {
    let body = "  let m: map<int, str> = map { 1: \"a\", 2: \"b\", 3: \"c\" }\n  m.remove(1)\n  m.remove(99)\n  ${m.keys()}\n  m[1] = \"z\"\n  ${m.keys()} ${m.values()}";
    assert_eq!(one(body), "[2, 3]|[2, 3, 1] [b, c, z]");
}

#[test]
fn get_has_len_is_empty_clear() {
    let body = "  let m: map<str, int> = map { \"a\": 1 }\n  ${m.get(\"a\")} ${m.get(\"x\")} ${m.has(\"a\")} ${m.has(\"x\")} ${m.len()} ${m.is_empty()}\n  m.clear()\n  ${m.len()} ${m.is_empty()} ${m}";
    assert_eq!(one(body), "Some(1) None true false 1 false|0 true map {}");
}

#[test]
fn index_read_errors_at_runtime_when_key_missing() {
    let t = texts(
        "label start {\n  let m: map<str, int> = map { \"a\": 1 }\n  ${m[\"a\"]}\n  ${m[\"b\"]}\n  後\n}\n",
    );
    assert_eq!(t, ["1", "<key_not_found>"]);
}

#[test]
fn index_assignment_appends_when_missing_and_updates_value_when_present() {
    let body =
        "  let m: map<str, int> = map {}\n  m[\"x\"] = 1\n  m[\"y\"] = 2\n  m[\"x\"] = 3\n  ${m}";
    assert_eq!(one(body), "map { x: 3, y: 2 }");
}

#[test]
fn compound_assignment_and_field_update_error_at_runtime_when_key_missing() {
    let t = texts("label start {\n  let m: map<str, int> = map {}\n  m[\"a\"] += 1\n  後\n}\n");
    assert_eq!(t, ["<key_not_found>"]);
    let t = texts(
        "struct P { hp: int }\nlabel start {\n  let m: map<str, P> = map {}\n  m[\"a\"].hp = 1\n  後\n}\n",
    );
    assert_eq!(t, ["<key_not_found>"]);
    // If the key exists, the inside of the value can be updated
    let t = texts(
        "struct P { hp: int, xs: list<int> }\nlabel start {\n  let m: map<str, P> = map { \"a\": P { hp: 1, xs: [] } }\n  m[\"a\"].hp += 4\n  m[\"a\"].xs.push(7)\n  ${m}\n}\n",
    );
    assert_eq!(t, ["map { a: P { hp: 5, xs: [7] } }", "<end>"]);
}

#[test]
fn updates_element_of_list_inside_map() {
    let body = "  let m: map<str, list<int>> = map { \"a\": [1, 2] }\n  m[\"a\"][1] = 9\n  m[\"a\"].remove(0)\n  ${m}";
    assert_eq!(one(body), "map { a: [9] }");
}

#[test]
fn keys_can_also_be_struct_enum_bool_identity() {
    let src = "struct P { x: int, y: int }\nenum D { N, S(int) }\nidentity 案内人 = \"あんないにん\"\nlabel start {\n  let a: map<P, str> = map {}\n  a[P { x: 1, y: 2 }] = \"一\"\n  a[P { x: 1, y: 2 }] += \"\"\n  ${a}\n}\n";
    // `+=` on `str` cannot be used
    assert_eq!(compile_codes(src), ["invalid_operation"]);
    let body = "  let a: map<P, str> = map {}\n  a[P { x: 1, y: 2 }] = \"一\"\n  a[P { x: 1, y: 2 }] = \"二\"\n  a[P { x: 2, y: 1 }] = \"三\"\n  let d: map<D, int> = map { D.S(1): 1, D.N: 2, D.S(2): 3 }\n  d[D.S(1)] += 10\n  let b: map<bool, int> = map { true: 1 }\n  b[false] = 0\n  let i: map<Identity, int> = map { 案内人: 1 }\n  ${a} ${d} ${b} ${i}";
    let src = format!(
        "struct P {{ x: int, y: int }}\nenum D {{ N, S(int) }}\nidentity 案内人 = \"あんないにん\"\nlabel start {{\n{body}\n}}\n"
    );
    assert_eq!(
        texts(&src),
        [
            "map { P { x: 1, y: 2 }: 二, P { x: 2, y: 1 }: 三 } map { D.S(1): 11, D.N: 2, D.S(2): 3 } map { true: 1, false: 0 } map { あんないにん: 1 }",
            "<end>"
        ]
    );
}

#[test]
fn duplicate_literal_keys_update_with_later_value_and_keep_first_position() {
    assert_eq!(
        one("  let m = map { 1: \"a\", 2: \"b\", 1: \"c\" }\n  ${m}"),
        "map { 1: c, 2: b }"
    );
    // Same when the key is an expression
    assert_eq!(
        one("  let k = 1\n  let m = map { k: \"a\", 2: \"b\", k + 0: \"c\" }\n  ${m}"),
        "map { 1: c, 2: b }"
    );
}
