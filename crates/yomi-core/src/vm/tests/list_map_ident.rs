//! Tests of language finishing touches (`insert`, the `map` index, identifiers): `insert` on lists, the `map` index,
//! identifier normalization (NFC) and UAX #31.

use super::util::{self, *};
use crate::compile::compile;
use crate::diag::DiagArg;
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Choice, Sugar::Quote], |_| {})
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

fn compile_codes(src: &str) -> Vec<&'static str> {
    util::diag_codes(src, &registry())
}

fn run_all(vm: &mut Vm<'_>) -> Vec<String> {
    util::run_all(vm, 5000)
}

fn texts_with(src: &str, cfg: VmConfig) -> Vec<String> {
    util::texts_with(src, &registry(), cfg, 5000)
}

fn texts(src: &str) -> Vec<String> {
    texts_with(src, VmConfig::default())
}

// ---------------------------------------------------------------------------
// insert on lists
// ---------------------------------------------------------------------------

#[test]
fn insert_inserts_at_index() {
    let src = "label start {\n  let xs = [10, 30]\n  xs.insert(1, 20)\n  xs.insert(0, 5)\n  xs.insert(xs.len(), 40)\n  ${xs}\n}\n";
    assert_eq!(texts(src), ["[5, 10, 20, 30, 40]", "<end>"]);
}

#[test]
fn insert_can_insert_at_start_of_empty_list() {
    let src = "label start {\n  let xs: list<str> = []\n  xs.insert(0, \"a\")\n  ${xs}\n}\n";
    assert_eq!(texts(src), ["[a]", "<end>"]);
}

#[test]
fn insert_out_of_range_is_index_out_of_range() {
    for idx in ["-1", "3", "100"] {
        let src =
            format!("label start {{\n  let xs = [1, 2]\n  xs.insert({idx}, 9)\n  ${{xs}}\n}}\n");
        assert_eq!(texts(&src), ["<index_out_of_range>"], "添字 {idx}");
    }
    // Even out of range, the list does not change (do not continue after the error)
    let src = "label start {\n  let xs = [1, 2]\n  xs.insert(3, 9)\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind.code(), "index_out_of_range");
}

#[test]
fn insert_type_checking() {
    // The number and types of arguments
    assert_eq!(
        compile_codes("label start {\n  let xs = [1]\n  xs.insert(0)\n}\n"),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes("label start {\n  let xs = [1]\n  xs.insert(\"a\", 1)\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let xs = [1]\n  xs.insert(0, \"a\")\n}\n"),
        ["type_mismatch"]
    );
    // map has no insert
    assert_eq!(
        compile_codes("label start {\n  let m: map<int, int> = map {}\n  m.insert(0, 1)\n}\n"),
        ["unknown_method"]
    );
    // Cannot be called inside an expression
    assert_eq!(
        compile_codes("label start {\n  let xs = [1]\n  let n = xs.insert(0, 1)\n}\n"),
        ["statement_method_in_expression"]
    );
    // The correct form
    assert!(compile_codes("label start {\n  let xs = [1]\n  xs.insert(0, 1)\n}\n").is_empty());
}

#[test]
fn insert_has_value_semantics_and_does_not_affect_copies() {
    let src = "struct P { xs: list<int> }\nlabel start {\n  let a = [1, 3]\n  let b = a\n  b.insert(1, 2)\n  ${a} ${b}\n  let p = P { xs: [1] }\n  let q = p\n  q.xs.insert(0, 0)\n  ${p.xs} ${q.xs}\n  let ls = [[1], [2]]\n  ls[1].insert(0, 9)\n  ${ls}\n}\n";
    assert_eq!(
        texts(src),
        ["[1, 3] [1, 2, 3]", "[1] [0, 1]", "[[1], [9, 2]]", "<end>"]
    );
}

#[test]
fn insert_can_update_parameter_locals_in_fn() {
    let src = "fn ins(xs: list<int>) -> list<int> {\n  xs.insert(0, 7)\n  return xs\n}\nlabel start {\n  let a = [1]\n  ${ins(a)} ${a}\n}\n";
    assert_eq!(texts(src), ["[7, 1] [1]", "<end>"]);
    // Module variables cannot be updated inside a fn (same for insert)
    let src = "save var g: list<int> = []\nfn f() {\n  g.insert(0, 1)\n}\nlabel start {\n}\n";
    assert!(!compile_codes(src).is_empty());
}

#[test]
fn insert_stops_at_value_size_limit() {
    // The size of a list = 24 + 8 × number of elements
    let src = "label start {\n  let xs: list<int> = []\n  xs.insert(0, 1)\n  xs.insert(0, 2)\n  xs.insert(0, 3)\n  ${xs}\n}\n";
    let t = texts_with(src, VmConfig::default().max_value_bytes(24 + 24));
    assert_eq!(t, ["[3, 2, 1]", "<end>"]);
    let t = texts_with(src, VmConfig::default().max_value_bytes(24 + 23));
    assert_eq!(t, ["<value_too_large>"]);
}

#[test]
fn insert_counts_toward_total_memory_limit() {
    let src = "save var a: list<int> = []\nlabel start {\n  let i = 0\n  while true {\n    a.insert(0, i)\n    i += 1\n  }\n}\n";
    let t = texts_with(src, VmConfig::default().max_total_bytes(Some(10_000)));
    assert_eq!(t, ["<memory_limit_exceeded>"]);
}

const INSERT_STATE: &str = "save var xs: list<str> = [\"a\"]\nlabel start {\n  xs.insert(0, \"b\")\n  一 #a\n  xs.insert(1, \"c\")\n  二 #b\n  ${xs}\n}\n";

#[test]
fn inserted_save_var_continues_identically_after_save_and_reload() {
    let reg = registry();
    let prog = compile_ok(INSERT_STATE);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一");
    let bytes = vm.save().unwrap();
    let rest = run_all(&mut vm);
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(var(&vm2, "xs").to_text(), "[b, a]");
    assert_eq!(run_all(&mut vm2), rest);
    assert_eq!(rest, ["二", "[b, c, a]", "<end>"]);
}

#[test]
fn inserted_list_goes_back_with_rollback() {
    let reg = registry();
    let prog = compile_ok(INSERT_STATE);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    let before = var(&vm, "xs");
    let first = run_all(&mut vm);
    assert_ne!(var(&vm, "xs"), before);
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "xs"), before);
    assert_eq!(run_all(&mut vm), first);
}

// ---------------------------------------------------------------------------
// The map index
// ---------------------------------------------------------------------------

#[test]
fn map_keeps_insertion_order_and_update_keeps_position() {
    let src = "label start {\n  let m: map<str, int> = map {}\n  m[\"b\"] = 1\n  m[\"a\"] = 2\n  m[\"c\"] = 3\n  m[\"a\"] = 20\n  ${m} ${m.keys()} ${m.values()}\n}\n";
    assert_eq!(
        texts(src),
        ["map { b: 1, a: 20, c: 3 } [b, a, c] [1, 20, 3]", "<end>"]
    );
}

#[test]
fn map_lookup_and_insertion_order_stay_correct_after_removal() {
    let src = "label start {\n  let m: map<int, str> = map {}\n  for i in range(0, 6) {\n    m[i] = \"v${i}\"\n  }\n  m.remove(2)\n  m.remove(99)\n  ${m.keys()} ${m.has(3)} ${m.has(2)} ${m[5]} ${m.get(4)}\n  m[2] = \"again\"\n  m.remove(0)\n  ${m.keys()} ${m[2]} ${m[1]} ${m.len()}\n}\n";
    assert_eq!(
        texts(src),
        [
            "[0, 1, 3, 4, 5] true false v5 Some(v4)",
            "[1, 3, 4, 5, 2] again v1 5",
            "<end>"
        ]
    );
}

#[test]
fn cloned_maps_are_independent() {
    let src = "label start {\n  let a: map<str, int> = map { \"x\": 1 }\n  let b = a\n  b[\"y\"] = 2\n  b[\"x\"] = 10\n  a.remove(\"x\")\n  ${a} ${b}\n  let c = [a, b]\n  c[0][\"z\"] = 3\n  ${a} ${c[0]} ${c[1]}\n}\n";
    assert_eq!(
        texts(src),
        [
            "map {} map { x: 10, y: 2 }",
            "map {} map { z: 3 } map { x: 10, y: 2 }",
            "<end>"
        ]
    );
}

#[test]
fn map_equality_ignores_insertion_order() {
    let src = "label start {\n  let a: map<int, int> = map { 1: 10, 2: 20 }\n  let b: map<int, int> = map {}\n  b[2] = 20\n  b[1] = 10\n  ${a == b}\n  b[1] = 11\n  ${a == b} ${a != b}\n}\n";
    assert_eq!(texts(src), ["true", "false true", "<end>"]);
}

#[test]
fn struct_and_enum_keys_can_be_looked_up_by_index() {
    let src = "struct K { a: int, b: str }\nenum E { X, Y(int) }\nlabel start {\n  let m: map<K, int> = map {}\n  m[K { a: 1, b: \"x\" }] = 1\n  m[K { a: 1, b: \"y\" }] = 2\n  m[K { a: 1, b: \"x\" }] = 3\n  ${m.len()} ${m[K { a: 1, b: \"x\" }]}\n  let e: map<E, str> = map {}\n  e[E.Y(1)] = \"a\"\n  e[E.X] = \"b\"\n  e[E.Y(2)] = \"c\"\n  ${e.has(E.Y(1))} ${e.has(E.Y(3))} ${e[E.X]}\n}\n";
    assert_eq!(texts(src), ["2 3", "true false b", "<end>"]);
}

#[test]
fn map_index_works_after_load_and_supports_insert_and_remove() {
    let src = "save var m: map<str, int> = map {}\nlabel start {\n  m[\"a\"] = 1\n  m[\"b\"] = 2\n  m[\"c\"] = 3\n  一 #a\n  m.remove(\"a\")\n  m[\"d\"] = 4\n  m[\"c\"] = 30\n  ${m} ${m.has(\"a\")} ${m[\"b\"]}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    let Value::Map(m) = var(&vm2, "m") else {
        panic!("map のはず")
    };
    assert!(m.is_consistent());
    assert_eq!(
        m.entries().position(|(k, _)| *k == Value::Str("c".into())),
        Some(2)
    );
    assert_eq!(
        run_all(&mut vm2),
        ["map { b: 2, c: 30, d: 4 } false 2", "<end>"]
    );
    // Still in the v9 form (saving again gives the same bytes)
    let (vm3, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(vm3.save().unwrap(), bytes);
}

#[test]
fn map_size_estimate_depends_only_on_entries() {
    let m = Value::Map(
        vec![
            (Value::Str("a".into()), Value::Int(1)),
            (Value::Str("bc".into()), Value::Int(2)),
        ]
        .into(),
    );
    // 24 + (24+1 + 8) + (24+2 + 8)
    assert_eq!(m.approx_bytes(), 24 + 33 + 34);
    assert_eq!(Value::Map(MapData::new()).approx_bytes(), 24);
}

#[test]
fn large_map_insert_lookup_and_update_finish_in_realistic_time() {
    let src = "save var m: map<int, int> = map {}\nlabel start {\n  for i in range(0, 20000) {\n    m[i] = i\n  }\n  let hit = 0\n  for i in range(0, 20000) {\n    if m.has(i * 2) {\n      hit += 1\n    }\n    m[i] += 1\n  }\n  ${m.len()} ${hit} ${m[19999]} ${m.get(5)}\n}\n";
    let t0 = std::time::Instant::now();
    let t = texts_with(src, VmConfig::default().max_steps_per_run(None));
    assert_eq!(t, ["20000 10000 20000 Some(6)", "<end>"]);
    // A linear search would take tens of seconds or more. A loose limit so that slow CI does not fail
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(20),
        "遅すぎる: {:?}",
        t0.elapsed()
    );
}

#[test]
fn snapshot_and_rollback_of_large_map_are_not_extremely_slow() {
    let src = "save var m: map<int, int> = map {}\nlabel start {\n  for i in range(0, 20000) {\n    m[i] = i\n  }\n  一 #a\n  m[0] = 1\n  二 #b\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let t0 = std::time::Instant::now();
    for _ in 0..200 {
        let snap = vm.snapshot().unwrap();
        vm.restore(&snap).unwrap();
        // Read-only operations do not copy
        let _ = var(&vm, "m");
    }
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(20),
        "遅すぎる: {:?}",
        t0.elapsed()
    );
}

// ---------------------------------------------------------------------------
// Identifiers: UAX #31 and NFC
// ---------------------------------------------------------------------------

/// The decomposed form of "が" (か + dakuten).
const GA_D: &str = "\u{304B}\u{3099}";
/// The composed form of "が".
const GA_C: &str = "\u{304C}";

#[test]
fn decomposed_and_composed_identifiers_are_same_name() {
    // Variables, labels, fn, struct, enum, fields, variants, parameter names, identity
    let src = "identity {D}x = \"がx\"\nstruct S{D} {\n  f{D}: int\n}\nenum E{D} {\n  V{D}(int)\n}\nfn fn{D}(a{D}: int) -> int {\n  return a{C} + 1\n}\nsave var v{D}: int = 1\nlabel start {\n  let x{D} = fn{C}(v{C})\n  let s = S{C} { f{C}: x{C} }\n  let e = E{C}.V{C}(s.f{D})\n  match e {\n    V{C}(n) => {\n      ${n}\n    }\n  }\n  {C}x「こんにちは」\n  jump l2{D}\n}\nlabel l2{C} {\n  終わり\n}\n";
    // The version written only in composed forms and the one written only in decomposed forms give the same output
    let composed = src.replace("{D}", GA_C).replace("{C}", GA_C);
    let decomposed = src.replace("{D}", GA_D).replace("{C}", GA_D);
    // A mixed version (declarations in decomposed form, uses in composed form)
    let mixed = src.replace("{D}", GA_D).replace("{C}", GA_C);
    let a = texts(&composed);
    assert_eq!(a, ["2", "「こんにちは」", "終わり", "<end>"], "{a:?}");
    assert_eq!(texts(&decomposed), a);
    assert_eq!(texts(&mixed), a);
}

#[test]
fn decomposed_identifier_scripts_give_same_save_bytes_and_position_hashes() {
    // The position hash is made from the original text of command arguments. Only identifiers are normalized to NFC;
    // string literals and bodies stay as written
    let src = "save var m{D}: str = \"x\"\nlabel start {\n  let t{D} = \"{D}\"\n  @ui.say(\"${m{D}}${t{D}}\")\n  {D}x「{D}」\n  本文{D} ${m{D}}\n}\nidentity {D}x = \"が\"\n";
    let comp = src.replace("{D}", GA_C);
    let decomp = src.replace("{D}", GA_D);
    // Only identifiers are decomposed (strings and bodies stay composed)
    let src2 = "save var m{D}: str = \"x\"\nlabel start {\n  let t{D} = \"{C}\"\n  @ui.say(\"${m{D}}${t{D}}\")\n  {D}x「{C}」\n  本文{C} ${m{D}}\n}\nidentity {D}x = \"が\"\n"
        .replace("{D}", GA_D)
        .replace("{C}", GA_C);
    let reg = registry();
    let mut hashes = Vec::new();
    let mut saves = Vec::new();
    for s in [&comp, &src2] {
        let prog = compile_ok(s);
        hashes.push(
            prog.labels
                .iter()
                .flat_map(|l| l.stops.iter().map(|st| st.hash))
                .collect::<Vec<_>>(),
        );
        // Run to the same stop point and take a save
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        vm.run().unwrap();
        saves.push(vm.save().unwrap());
    }
    assert_eq!(saves[0], saves[1], "セーブのバイト列も同じ");
    assert_eq!(
        hashes[0], hashes[1],
        "識別子の綴りの形でハッシュが変わらない"
    );
    assert_eq!(hashes[0].len(), 3);
    // Decomposed strings and bodies stay as written (not normalized), so the hash changes
    let prog_d = compile_ok(&decomp);
    let hd: Vec<u64> = prog_d
        .labels
        .iter()
        .flat_map(|l| l.stops.iter().map(|st| st.hash))
        .collect();
    assert_ne!(hd, hashes[0]);
}

fn stop_hashes(src: &str) -> Vec<u64> {
    compile_ok(src)
        .labels
        .iter()
        .flat_map(|l| l.stops.iter().map(|st| st.hash))
        .collect()
}

#[test]
fn position_hashes_ignore_spacing_redundant_parentheses_and_line_breaks_of_quotes() {
    let a = "identity 案内人 = \"案内人\"\nlabel start {\n  @ui.say(\"a ${1 + 1 * 2}\")\n  朝です\n  案内人「あいう」\n  call other(1_000, 2)\n}\nlabel other(a: int, b: int) {\n  終わり\n}\n";
    let b = "identity 案内人 = \"案内人\"\nlabel start {\n  @ui.say( (\"a ${ (1 + (1*2)) }\") )   // comment\n  朝です   \n  案内人「あ\n      い\n\tう」\n  call other( 1000 , 2 )\n}\nlabel other(a: int, b: int) {\n  終わり\n}\n";
    assert_eq!(stop_hashes(a), stop_hashes(b));
    assert_eq!(stop_hashes(a).len(), 4);
}

#[test]
fn position_hashes_change_with_body_text_and_string_literals() {
    let base = stop_hashes("label start {\n  @ui.say(\"a\")\n  朝です\n}\n");
    assert_ne!(
        base,
        stop_hashes("label start {\n  @ui.say(\"b\")\n  朝です\n}\n")
    );
    assert_ne!(
        base,
        stop_hashes("label start {\n  @ui.say(\"a\")\n  朝です。\n}\n")
    );
    // A different spelling of the same character in a string literal is a different literal
    assert_ne!(
        stop_hashes("label start {\n  @ui.say(\"a$\")\n}\n"),
        stop_hashes("label start {\n  @ui.say(\"a\\$\")\n}\n")
    );
}

#[test]
fn bodies_and_string_literals_are_not_normalized() {
    let src = "label start {\n  let s = \"{D}\"\n  本文{D}\n  ${s}\n}\n".replace("{D}", GA_D);
    let t = texts(&src);
    // Still decomposed (normalizing would change the bytes)
    assert_eq!(t[0].as_bytes(), format!("本文{GA_D}").as_bytes());
    assert_eq!(t[1].as_bytes(), GA_D.as_bytes());
    assert_ne!(t[1].as_bytes(), GA_C.as_bytes());
}

#[test]
fn characters_against_uax31_cannot_be_used_in_identifiers() {
    for bad in [
        "let 🐱 = 1",
        "let a🐱 = 1",
        "let a\u{3000}b = 1",
        "let a-b = 1",
        "let a+ = 1",
        "let 1a = 1",
        "let ¥ = 1",
        "let ① = 1",
        "let ‿a = 1",
    ] {
        let src = format!("label start {{\n  {bad}\n}}\n");
        let codes = compile_codes(&src);
        assert!(!codes.is_empty(), "通ってしまった: {bad}");
        assert!(
            codes.iter().all(|c| *c == "unexpected_token"
                || *c == "unexpected_eol"
                || *c == "unknown_name"
                || *c == "full_width_space"),
            "{bad}: {codes:?}"
        );
    }
}

#[test]
fn characters_following_uax31_can_be_used_in_identifiers() {
    // Hiragana, katakana, kanji, the long vowel mark, iteration marks, accented Latin letters, Greek letters, `_`, digits
    // (second character onward)
    for ok in [
        "あ",
        "ア",
        "案内人",
        "ゆーざー",
        "人々",
        "café",
        "αβγ",
        "_x",
        "x_1",
        "ｘ１",
        "日本語2",
    ] {
        let src = format!("label start {{\n  let {ok} = 1\n  ${{{ok}}}\n}}\n");
        assert_eq!(texts(&src), ["1", "<end>"], "{ok}");
    }
}

#[test]
fn latin_precomposed_and_decomposed_accents_are_same_name() {
    let src = "label start {\n  let caf\u{e9} = 1\n  ${cafe\u{301}}\n}\n";
    assert_eq!(texts(src), ["1", "<end>"]);
    // No NFKC conversion (full-width and half-width are different names)
    let src = "label start {\n  let a = 1\n  ${ａ}\n}\n";
    assert_eq!(compile_codes(src), ["unknown_name"]);
}

#[test]
fn diagnostic_names_and_suggestions_for_decomposed_identifiers_are_nfc() {
    // Candidates with close spellings are compared in NFC and returned in NFC
    let src = format!("label start {{\n  let {GA_D}ame = 1\n  ${{{GA_D}amx}}\n}}\n");
    let reg = registry();
    let Err(d) = compile(&src, "main", &reg) else {
        panic!("エラーのはず")
    };
    assert_eq!(d[0].code.as_str(), "unknown_name");
    let name = d[0].args.iter().find(|(k, _)| *k == "name").unwrap();
    assert_eq!(
        format!("{:?}", name.1),
        format!("{:?}", DiagArg::Str(format!("{GA_C}amx")))
    );
    let sug = d[0].args.iter().find(|(k, _)| *k == "suggestions").unwrap();
    assert_eq!(
        format!("{:?}", sug.1),
        format!("{:?}", DiagArg::List(vec![format!("{GA_C}ame")]))
    );
}

#[test]
fn module_names_are_normalized_to_nfc() {
    let reg = registry();
    let b = "label go {\n  届いた\n}\n";
    let main = format!("import {GA_C}\nlabel start {{\n  jump {GA_D}.go\n}}\n");
    let prog = crate::compile::compile_modules(&[("main", &main), (GA_D, b)], &reg)
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(prog.modules, ["main".to_string(), GA_C.to_string()]);
    let cfg = VmConfig::default().start("main.start");
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["届いた", "<end>"]);
    // Same even if the host passes the start position in decomposed form
    let cfg = VmConfig::default().start(&format!("{GA_D}.go"));
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["届いた", "<end>"]);
}
