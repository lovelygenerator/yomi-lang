//! `for k, v in m`, `for i, x in xs`, `each { k, v => }`, and `range` without building a list

use super::*;

#[test]
fn map_iterates_keys_and_values_in_insertion_order() {
    let body = "  let m = map { \"a\": 1, \"b\": 2, \"c\": 3 }\n  m.remove(\"b\")\n  m[\"d\"] = 4\n  for k, v in m {\n    ${k}=${v}\n  }";
    assert_eq!(one(body), "a=1|c=3|d=4");
}

#[test]
fn map_iteration_uses_copy_taken_at_start() {
    let body = "  let m = map { 1: 10, 2: 20 }\n  for k, v in m {\n    m[k * 100] = v\n    m.remove(2)\n    ${k}:${v}\n  }\n  ${m}";
    assert_eq!(one(body), "1:10|2:20|map { 1: 10, 100: 10, 200: 20 }");
}

#[test]
fn list_iterates_with_index_and_element() {
    assert_eq!(
        one("  for i, x in [\"a\", \"b\"] {\n    ${i}${x}\n  }"),
        "0a|1b"
    );
}

#[test]
fn each_also_takes_key_and_value() {
    let body = "  let m = map { \"x\": true }\n  m.each { k, v =>\n    ${k}${v}\n  }\n  [5, 6].each { i, x =>\n    ${i}-${x}\n  }";
    assert_eq!(one(body), "xtrue|0-5|1-6");
}

#[test]
fn two_variable_for_type_errors() {
    let src = "label start {\n  for a, b in 3 {\n  }\n}\n";
    assert!(
        compile_codes(src).contains(&"type_mismatch"),
        "{:?}",
        compile_codes(src)
    );
    // A `map` cannot be iterated with one variable (use `keys()` / `values()`)
    let src = "label start {\n  let m = map { 1: 2 }\n  for x in m {\n  }\n}\n";
    assert!(compile_codes(src).contains(&"type_mismatch"));
    let src = "label start {\n  for a, a in [1] {\n  }\n}\n";
    assert!(compile_codes(src).contains(&"duplicate_name"));
}

#[test]
fn key_and_value_variables_have_types() {
    let src = "label start {\n  let m = map { \"a\": 1 }\n  for k, v in m {\n    let n: int = k\n  }\n}\n";
    assert!(compile_codes(src).contains(&"type_mismatch"));
    let body = "  let m = map { \"a\": 1 }\n  for k, v in m {\n    let s: str = k\n    let n: int = v + 1\n    ${s}${n}\n  }";
    assert_eq!(one(body), "a2");
}

#[test]
fn range_iterates_without_building_list_and_respects_bounds_and_empty() {
    assert_eq!(
        one(
            "  for i in range(2, 5) {\n    ${i}\n  }\n  for i in range(3, 3) {\n    x\n  }\n  for i in range(4, 1) {\n    y\n  }\n  後"
        ),
        "2|3|4|後"
    );
    // Changing the variable in the body does not change the iteration
    assert_eq!(
        one("  for i in range(0, 3) {\n    i = 100\n    ${i}\n  }"),
        "100|100|100"
    );
    // No list is built, so even a large range can be iterated (if it breaks out partway)
    assert_eq!(
        one(
            "  for i in range(0, 10000000000) {\n    if i == 2 {\n      break\n    }\n    ${i}\n  }"
        ),
        "0|1"
    );
    // The range expressions are evaluated once before starting, in the order written
    assert_eq!(
        one("  let n = 2\n  for i in range(n, n + 2) {\n    n += 10\n    ${i}\n  }\n  ${n}"),
        "2|3|22"
    );
}

#[test]
fn saving_and_reloading_mid_two_variable_for_and_range_continues_identically() {
    let src = "label start {\n  let m = map { \"a\": 1, \"b\": 2, \"c\": 3 }\n  m.remove(\"a\")\n  for k, v in m {\n    m ${k}${v}\n  }\n  for i, x in [\"p\", \"q\"] {\n    l ${i}${x}\n  }\n  for i in range(7, 9) {\n    r ${i}\n  }\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let all = run_all(&mut Vm::new(&prog, &reg, VmConfig::default()).unwrap());
    assert_eq!(all, ["m b2", "m c3", "l 0p", "l 1q", "r 7", "r 8", "<end>"]);
    // Save at each stop point, read it back, and run to the end
    for stop in 1..all.len() - 1 {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        let mut seen = vec![text_of(&vm.run().unwrap())];
        for _ in 1..stop {
            vm.resume(Input::Return(Value::Unit)).unwrap();
            seen.push(text_of(&vm.run().unwrap()));
        }
        let bytes = vm.save().unwrap();
        let (mut loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
        loaded.resume(Input::Return(Value::Unit)).unwrap();
        seen.extend(run_all(&mut loaded));
        assert_eq!(seen, all, "停止点 {stop} でセーブ");
    }
}

#[test]
fn underscore_loop_variable_is_discard_name() {
    let body = "  let m = map { \"a\": 1, \"b\": 2 }\n  for k, _ in m {\n    ${k}\n  }\n  for _, v in m {\n    ${v}\n  }\n  for _, _ in m {\n    x\n  }\n  for _ in range(0, 2) {\n    y\n  }\n  m.each { _, v =>\n    e${v}\n  }";
    assert_eq!(one(body), "a|b|1|2|x|x|y|y|e1|e2");
    // It cannot be referred to from the body
    let src = "label start {\n  for _, v in [1] {\n    ${_}\n  }\n}\n";
    assert!(
        compile_codes(src).contains(&"unknown_name"),
        "{:?}",
        compile_codes(src)
    );
}
