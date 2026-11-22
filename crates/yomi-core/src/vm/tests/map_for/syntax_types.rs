//! Syntax and type checking

use super::*;

#[test]
fn map_literals_and_type_annotations_can_be_parsed() {
    assert_eq!(
        one("  let m: map<str, int> = map { \"a\": 1, \"b\": 2, }\n  ${m}"),
        "map { a: 1, b: 2 }"
    );
    assert_eq!(
        one("  let m = map { 1: [\"x\"] }\n  ${m}"),
        "map { 1: [x] }"
    );
    assert_eq!(one("  let m: map<int, bool> = map {}\n  ${m}"), "map {}");
}

#[test]
fn key_types_are_scalars_and_struct_enum_of_key_types_and_identity() {
    let ok = "struct P { x: int, s: str }\nenum E { A, B(int, bool) }\nidentity 案内人 = \"案内人\"\nsave var a: map<int, int> = map {}\nsave var b: map<str, int> = map {}\nsave var c: map<bool, int> = map {}\nsave var d: map<P, int> = map {}\nsave var e: map<E, int> = map {}\nsave var f: map<Identity, int> = map {}\nlabel start {\n  一\n}\n";
    assert!(compile_codes(ok).is_empty());
    for bad in [
        "float",
        "list<int>",
        "map<int, int>",
        "Option<int>",
        "Bad",
        "Bad2",
    ] {
        let src = format!(
            "struct Bad {{ f: float }}\nenum Bad2 {{ A(list<int>) }}\nsave var a: map<{bad}, int> = map {{}}\nlabel start {{\n  一\n}}\n"
        );
        let codes = compile_codes(&src);
        assert_eq!(codes, ["invalid_map_key"], "{bad}");
    }
}

#[test]
fn map_keys_in_type_declarations_are_checked_regardless_of_order() {
    // `Later` is declared later but can be a key. `Bad` contains a float, so it cannot be a key
    let ok = "struct H { m: map<Later, int> }\nstruct Later { n: int }\nlabel start {\n  一\n}\n";
    assert!(compile_codes(ok).is_empty());
    let bad = "struct H { m: map<Bad, int> }\nstruct Bad { f: float }\nlabel start {\n  一\n}\n";
    assert_eq!(compile_codes(bad), ["invalid_map_key"]);
}

#[test]
fn map_type_argument_count_and_inference_of_empty_map() {
    assert_eq!(
        compile_codes("save var a: map<int> = map {}\nlabel start {\n  一\n}\n")[0],
        "arg_count"
    );
    assert_eq!(
        compile_codes("label start {\n  let m = map {}\n}\n"),
        ["cannot_infer_type"]
    );
    // Decided by the assignment target, an argument, a field or the other side of a comparison
    let src = "struct S { m: map<str, int> }\nsave var g: map<str, int> = map { \"a\": 1 }\nlabel start {\n  g = map {}\n  let s = S { m: map {} }\n  if g == map {} {\n    空\n  }\n  ${s.m.len()}\n}\n";
    assert_eq!(texts(src), ["空", "0", "<end>"]);
}

#[test]
fn map_type_mismatches_are_diagnosed() {
    assert_eq!(
        compile_codes("label start {\n  let m: map<str, int> = map { 1: 1 }\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let m: map<str, int> = map { \"a\": \"b\" }\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let m = map { 1: 1, 2: \"x\" }\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let m: map<str, int> = [1]\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let m = map { 1.5: 1 }\n}\n"),
        ["invalid_map_key"]
    );
}

#[test]
fn map_method_diagnostics() {
    let head = "label start {\n  let m: map<str, int> = map { \"a\": 1 }\n";
    assert_eq!(
        compile_codes(&format!("{head}  ${{m.nope()}}\n}}\n")),
        ["unknown_method"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  m.push(1)\n}}\n")),
        ["unknown_method"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  ${{m.remove(\"a\")}}\n}}\n")),
        ["statement_method_in_expression"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  ${{m.has(1)}}\n}}\n")),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  ${{m.get()}}\n}}\n")),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  m.remove(1)\n}}\n")),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  m.clear(1)\n}}\n")),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  let x = m[1]\n}}\n")),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!("{head}  m[1] = 2\n}}\n")),
        ["type_mismatch"]
    );
}

#[test]
fn list_is_empty_can_be_used() {
    assert_eq!(
        one("  let xs: list<int> = []\n  ${xs.is_empty()}\n  xs.push(1)\n  ${xs.is_empty()}"),
        "true|false"
    );
    let ms = "  let m: map<int, int> = map {}\n  ${m.is_empty()}\n  m[1] = 1\n  ${m.is_empty()}";
    assert_eq!(one(ms), "true|false");
}
