//! Value semantics and comparison

use super::*;

#[test]
fn map_is_a_value_and_updates_after_copy_do_not_affect_original() {
    let body = "  let a: map<str, int> = map { \"x\": 1 }\n  let b = a\n  b[\"x\"] = 2\n  b[\"y\"] = 3\n  ${a} ${b}";
    assert_eq!(one(body), "map { x: 1 } map { x: 2, y: 3 }");
    // Copied for arguments, return values and fields too
    let src = "struct H { m: map<str, int> }\nlabel start {\n  let m: map<str, int> = map { \"x\": 1 }\n  let h = H { m: m }\n  h.m[\"x\"] = 9\n  let r = call bump(m)\n  ${m} ${h.m} ${r}\n}\n\nlabel bump(a: map<str, int>) -> map<str, int> {\n  a[\"x\"] += 100\n  return a\n}\n";
    assert_eq!(
        texts(src),
        ["map { x: 1 } map { x: 9 } map { x: 101 }", "<end>"]
    );
}

#[test]
fn equality_ignores_insertion_order() {
    let body = "  let a: map<str, int> = map { \"a\": 1, \"b\": 2 }\n  let b: map<str, int> = map { \"b\": 2, \"a\": 1 }\n  ${a == b} ${a != b}\n  b[\"a\"] = 5\n  ${a == b}\n  let c: map<str, int> = map { \"a\": 1 }\n  ${a == c}\n  let l1 = [a]\n  let l2 = [b]\n  b[\"a\"] = 1\n  l2 = [b]\n  ${l1 == l2}";
    assert_eq!(one(body), "true false|false|false|true");
}

#[test]
fn empty_maps_are_equal_and_comparable_with_untyped_empty_map() {
    let body =
        "  let a: map<str, int> = map {}\n  ${a == map {}}\n  a[\"x\"] = 1\n  ${a != map {}}";
    assert_eq!(one(body), "true|true");
}

#[test]
fn value_expansion_prints_map_in_fixed_notation() {
    assert_eq!(
        one("  let m = map { \"a\": [1, 2], \"b\": [] }\n  ${m}"),
        "map { a: [1, 2], b: [] }"
    );
}
