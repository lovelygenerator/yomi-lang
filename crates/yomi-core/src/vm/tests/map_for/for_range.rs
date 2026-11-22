//! for and range

use super::*;

#[test]
fn for_binds_list_elements_in_order() {
    assert_eq!(
        one("  for x in [\"a\", \"b\", \"c\"] {\n    ${x}\n  }"),
        "a|b|c"
    );
}

#[test]
fn for_iterates_copy_taken_at_start() {
    let body =
        "  let xs = [1, 2, 3]\n  for x in xs {\n    xs.push(x * 10)\n    xs[0] = 99\n  }\n  ${xs}";
    assert_eq!(one(body), "[99, 2, 3, 10, 20, 30]");
    // Changing the variable while iterating does not affect the iteration
    let body =
        "  let xs = [1, 2, 3]\n  let n = 0\n  for x in xs {\n    xs = []\n    n += x\n  }\n  ${n}";
    assert_eq!(one(body), "6");
}

#[test]
fn for_break_and_continue() {
    let body = "  for i in range(0, 10) {\n    if i == 2 {\n      continue\n    }\n    if i == 5 {\n      break\n    }\n    ${i}\n  }\n  後";
    assert_eq!(one(body), "0|1|3|4|後");
}

#[test]
fn nested_for_inner_break_and_continue_affect_only_inner_loop() {
    let body = "  for i in range(0, 3) {\n    for j in range(0, 3) {\n      if j == 1 {\n        continue\n      }\n      if j == 2 {\n        break\n      }\n      ${i}${j}\n    }\n  }";
    assert_eq!(one(body), "00|10|20");
}

#[test]
fn for_and_while_can_nest() {
    let body = "  let n = 0\n  while n < 2 {\n    for k in range(0, 2) {\n      ${n}${k}\n    }\n    n += 1\n  }";
    assert_eq!(one(body), "00|01|10|11");
}

#[test]
fn return_inside_for() {
    let src = "label start {\n  let r = call find(5)\n  ${r}\n  let r2 = call find(100000)\n  ${r2}\n}\n\nlabel find(limit: int) -> int {\n  for v in range(0, 100) {\n    if v * v > limit {\n      return v\n    }\n  }\n  return -1\n}\n";
    assert_eq!(texts(src), ["3", "-1", "<end>"]);
}

#[test]
fn empty_list_and_empty_range_never_iterate() {
    let body = "  let e: list<int> = []\n  for x in e {\n    ありえない\n  }\n  for x in range(3, 3) {\n    ありえない\n  }\n  for x in range(5, 1) {\n    ありえない\n  }\n  後";
    assert_eq!(one(body), "後");
}

#[test]
fn for_variable_is_body_local_and_does_not_clash_with_same_named_let() {
    assert_eq!(
        compile_codes("label start {\n  let x = 1\n  for x in [1] {\n  }\n}\n"),
        ["duplicate_name"]
    );
    assert_eq!(
        compile_codes("label start {\n  for x in [1] {\n    let x = 2\n  }\n}\n"),
        ["duplicate_name"]
    );
    // After the end, the same name can be used
    assert_eq!(one("  for x in [1] {\n  }\n  let x = 7\n  ${x}"), "7");
    assert_eq!(
        compile_codes("label start {\n  for x in [1] {\n  }\n  ${x}\n}\n"),
        ["unknown_name"]
    );
    // A following for with the same variable name can also be written
    assert_eq!(
        one("  for x in [1] {\n    ${x}\n  }\n  for x in [2] {\n    ${x}\n  }"),
        "1|2"
    );
}

#[test]
fn for_expression_must_be_list() {
    assert_eq!(
        compile_codes("label start {\n  let m: map<int, int> = map {}\n  for x in m {\n  }\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  for x in 5 {\n  }\n}\n"),
        ["type_mismatch"]
    );
}

#[test]
fn for_header_syntax_errors() {
    for src in [
        "label start {\n  for 3 in [1] {\n  }\n}\n",
        "label start {\n  for x [1] {\n  }\n}\n",
        "label start {\n  for x in {\n  }\n}\n",
        "label start {\n  for in [1] {\n  }\n}\n",
        "label start {\n  for x in [1]\n}\n",
    ] {
        let codes = compile_codes(src);
        assert!(!codes.is_empty(), "{src}");
        assert!(!codes.contains(&"not_yet_supported"), "{src}");
    }
}

#[test]
fn for_target_can_be_parenthesized_struct_literal() {
    let src = "struct P { xs: list<int> }\nlabel start {\n  for x in (P { xs: [1, 2] }).xs {\n    ${x}\n  }\n}\n";
    assert_eq!(texts(src), ["1", "2", "<end>"]);
}

#[test]
fn map_iterates_with_keys_and_values() {
    let body = "  let m: map<str, int> = map { \"b\": 2, \"a\": 1 }\n  for k in m.keys() {\n    ${k}${m[k]}\n  }\n  let s = 0\n  for v in m.values() {\n    s += v\n  }\n  ${s}";
    assert_eq!(one(body), "b2|a1|3");
}

#[test]
fn range_edges_empty_and_negative() {
    assert_eq!(
        one("  ${range(0, 3)} ${range(2, 2)} ${range(3, 1)} ${range(-2, 1)}"),
        "[0, 1, 2] [] [] [-2, -1, 0]"
    );
    // It runs without overflowing even at the edges of the type
    assert_eq!(
        one("  ${range(9223372036854775806, 9223372036854775807)}"),
        "[9223372036854775806]"
    );
    assert_eq!(
        one("  ${range((-9223372036854775807 - 1), -9223372036854775807)}"),
        "[-9223372036854775808]"
    );
}

#[test]
fn range_size_is_checked_against_max_value_bytes() {
    let reg = registry();
    let prog = compile_ok("label start {\n  let xs = range(0, 100000)\n  後\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(1000)).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
    // Even for a huge range, it is rejected before building (does not exhaust memory)
    let prog = compile_ok(
        "label start {\n  let xs = range((-9223372036854775807 - 1), 9223372036854775807)\n  後\n}\n",
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
    // It can be built if within the limit
    let prog = compile_ok("label start {\n  let xs = range(0, 100)\n  ${xs.len()}\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(24 + 800)).unwrap();
    assert_eq!(run_all(&mut vm), ["100", "<end>"]);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(24 + 799)).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
}

#[test]
fn range_argument_checks_and_shadowed_builtin() {
    assert_eq!(
        compile_codes("label start {\n  let x = range(1)\n}\n"),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes("label start {\n  let x = range(1, 2.5)\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        warning_codes("label start {\n  let range = 3\n  ${range}\n}\n"),
        ["shadowed_builtin"]
    );
}

#[test]
fn for_and_range_stop_at_instruction_limit_like_while() {
    let reg = registry();
    let prog =
        compile_ok("label start {\n  for i in range(0, 1000000) {\n    let a = i\n  }\n  後\n}\n");
    let mut vm = Vm::new(
        &prog,
        &reg,
        VmConfig::default().max_steps_per_run(Some(1000)),
    )
    .unwrap();
    assert_eq!(run_all(&mut vm), ["<step_limit_exceeded>"]);
}

#[test]
fn loop_variables_and_iteration_state_hide_hidden_ones_from_variables() {
    let src = "label start {\n  for x in [1, 2] {\n    停止 #a\n  }\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let names: Vec<String> = vm.variables().into_iter().map(|v| v.name).collect();
    assert_eq!(names, ["x"]);
    assert_eq!(var(&vm, "x"), Value::Int(1));
}
