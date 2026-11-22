//! Function values (references to `fn` and lambdas), higher-order methods of `list`, and `each`

use super::*;

const DOUBLE: &str = "fn double(x: int) -> int {\n  return x * 2\n}\nfn apply(f: fn(int) -> int, x: int) -> int {\n  return f(x)\n}\n";

#[test]
fn fn_names_and_lambdas_can_be_passed_and_called_as_function_values() {
    assert_eq!(one(DOUBLE, "  ${apply(double, 5)}"), "10");
    assert_eq!(one(DOUBLE, "  ${apply(|x| x + 1, 5)}"), "6");
    assert_eq!(one(DOUBLE, "  ${apply(|x: int| x * x, 5)}"), "25");
    // Store in a variable with a type annotation and call it
    assert_eq!(
        one(
            DOUBLE,
            "  let f: fn(int) -> int = double\n  let g = |s: str, n: int| \"${s}${n}\"\n  ${f(4)} ${g(\"a\", 1)}"
        ),
        "8 a1"
    );
    // No parameters
    assert_eq!(one("", "  let f = || 42\n  ${f()}"), "42");
}

#[test]
fn captures_are_by_value_and_unaffected_by_later_outer_changes() {
    let body = "  let k = 10\n  let f = |x: int| x + k\n  k = 100\n  let xs = [1]\n  let g = |x: int| xs.len() + x\n  xs.push(2)\n  ${f(1)} ${g(0)} ${k} ${xs}";
    assert_eq!(one("", body), "11 1 100 [1, 2]");
    // Nested lambdas also capture outer variables
    let nested =
        "  let k = 3\n  let xs = [1, 2]\n  let ys = xs.map(|x| xs.map(|y| x * y + k))\n  ${ys}";
    assert_eq!(one("", nested), "[[4, 5], [5, 7]]");
}

#[test]
fn lambdas_read_module_variables_at_call_time() {
    let src = "save var m: int = 1\nlabel start {\n  let xs = [1, 2]\n  m = 5\n  ${xs.map(|x| x + m)}\n}\n";
    assert_eq!(texts(src), ["[6, 7]", "<end>"]);
}

#[test]
fn list_higher_order_methods() {
    let xs = "  let xs = [3, 1, 4, 1, 5]\n";
    assert_eq!(
        one("", &format!("{xs}  ${{xs.map(|x| x * 2)}}")),
        "[6, 2, 8, 2, 10]"
    );
    assert_eq!(
        one("", &format!("{xs}  ${{xs.map(|x| \"<${{x}}>\")}}")),
        "[<3>, <1>, <4>, <1>, <5>]"
    );
    assert_eq!(
        one("", &format!("{xs}  ${{xs.filter(|x| x > 2)}}")),
        "[3, 4, 5]"
    );
    assert_eq!(
        one(
            "",
            &format!("{xs}  ${{xs.any(|x| x > 4)}} ${{xs.any(|x| x > 5)}}")
        ),
        "true false"
    );
    assert_eq!(
        one(
            "",
            &format!("{xs}  ${{xs.all(|x| x > 0)}} ${{xs.all(|x| x > 1)}}")
        ),
        "true false"
    );
    assert_eq!(
        one(
            "",
            &format!("{xs}  ${{xs.find(|x| x > 3)}} ${{xs.find(|x| x > 9)}}")
        ),
        "Some(4) None"
    );
    assert_eq!(one("", &format!("{xs}  ${{xs.count(|x| x == 1)}}")), "2");
    assert_eq!(
        one("", &format!("{xs}  ${{xs.fold(0, |a, x| a + x)}}")),
        "14"
    );
    assert_eq!(
        one(
            "",
            &format!("{xs}  ${{xs.fold(\"\", |a, x| \"${{a}}${{x}}\")}}")
        ),
        "31415"
    );
    // An empty list, chaining, and fn names
    assert_eq!(
        one(
            DOUBLE,
            "  let e: list<int> = []\n  ${e.map(double)} ${e.any(|x| true)} ${e.all(|x| false)}"
        ),
        "[] false true"
    );
    assert_eq!(
        one(
            DOUBLE,
            &format!("{xs}  ${{xs.filter(|x| x != 1).map(double).fold(0, |a, x| a + x)}}")
        ),
        "24"
    );
}

#[test]
fn higher_order_methods_work_in_fn_and_can_recurse() {
    let f = "fn sum(xs: list<list<int>>) -> int {\n  return xs.map(|ys| ys.fold(0, |a, y| a + y)).fold(0, |a, x| a + x)\n}\nfn depth(n: int) -> int {\n  if n == 0 {\n    return 0\n  }\n  return [n - 1].map(depth)[0] + 1\n}\n";
    assert_eq!(one(f, "  ${sum([[1, 2], [3]])} ${depth(10)}"), "6 10");
    // Recursion beyond the limit stops at the depth limit
    let t = texts(&format!("{f}label start {{\n  ${{depth(1000000)}}\n}}\n"));
    assert_eq!(t, ["<call_depth_exceeded>"]);
}

#[test]
fn runtime_errors_in_lambdas_are_reported_as_is() {
    let t = texts("label start {\n  let xs = [1, 0]\n  ${xs.map(|x| 10 / x)}\n}\n");
    assert_eq!(t, ["<division_by_zero>"]);
}

#[test]
fn each_is_sugar_for_for_and_body_can_stop() {
    let src = "label start {\n  let xs = [\"a\", \"b\", \"c\"]\n  xs.each { x =>\n    ${x}\n    if x == \"b\" {\n      break\n    }\n  }\n  [1, 2].map(|v| v * 3).each { v => // 注釈\n    ${v}\n  }\n  終\n}\n";
    assert_eq!(texts(src), ["a", "b", "3", "6", "終", "<end>"]);
}

#[test]
fn each_can_be_saved_and_loaded_midway() {
    let reg = registry();
    let src =
        "label start {\n  let xs = [1, 2, 3]\n  xs.each { x =>\n    ${x} #a\n  }\n  終 #b\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "1");
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), "2");
    let bytes = vm.save().unwrap();
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(run_all(&mut vm2), ["3", "終", "<end>"]);
}

#[test]
fn function_values_out_of_scope_are_not_saved_or_listed() {
    let reg = registry();
    let src = "save var v: int = 0\nlabel start {\n  if true {\n    let f = |x: int| x + 1\n    v = f(1)\n  }\n  一 ${v} #a\n  二 #b\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一 2");
    let names: Vec<_> = vm.variables().into_iter().map(|v| v.name).collect();
    assert_eq!(names, ["v"]);
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert!(report.type_changed_locals.is_empty());
    assert_eq!(var(&vm2, "v"), Value::Int(2));
    assert_eq!(run_all(&mut vm2), ["二", "<end>"]);
}

#[test]
fn function_types_cannot_be_in_saved_places() {
    // Module variables, persistent variables, struct fields, enum arguments
    assert_eq!(
        compile_codes("save var f: fn(int) -> int = 0\nlabel start {\n}\n"),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes("persistent var f: list<fn() -> int> = []\nlabel start {\n}\n"),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes(
            "struct S {\n  f: fn(int) -> int\n}\nenum E {\n  A(Option<fn() -> int>)\n}\nlabel start {\n}\n"
        ),
        ["fn_not_saveable", "fn_not_saveable"]
    );
    // Using local variables of function types after a stop point, a choice or `call`
    assert_eq!(
        compile_codes("label start {\n  let f = |x: int| x\n  一\n  ${f(1)}\n}\n"),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes(
            "label start {\n  let fs = [|x: int| x]\n  choice {\n    「A」 => ${fs.len()}\n  }\n}\n"
        ),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes(
            "label start {\n  let f = |x: int| x\n  call other()\n  let g = f\n}\nlabel other {\n  一\n}\n"
        ),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes(
            "label start {\n  call other(|x| x)\n}\nlabel other(f: fn(int) -> int) {\n  一\n  ${f(1)}\n}\n"
        ),
        ["fn_not_saveable"]
    );
    // Using a variable from outside the loop inside a loop containing a stop point (on the next iteration, it is used after
    // the stop point)
    assert_eq!(
        compile_codes(
            "label start {\n  let f = |x: int| x\n  for i in [1, 2] {\n    ${f(i)}\n  }\n}\n"
        ),
        ["fn_not_saveable"]
    );
    assert_eq!(
        compile_codes(
            "label start {\n  let f = |x: int| x\n  let i = 0\n  while f(i) < 2 {\n    一\n    i += 1\n  }\n}\n"
        ),
        ["fn_not_saveable"]
    );
    // A stop point inside a loop that iterates a list of function values
    assert_eq!(
        compile_codes("label start {\n  for g in [|x: int| x] {\n    一\n  }\n}\n"),
        ["fn_not_saveable"]
    );
    // Even if reassigned, an error if there is a path that uses it without going through that assignment
    assert_eq!(
        compile_codes(
            "label start {\n  let f = |x: int| x\n  一\n  if true {\n    f = |x: int| x + 1\n  }\n  ${f(1)}\n}\n"
        ),
        ["fn_not_saveable"]
    );
    // Updates that are not whole-value assignments (push and the like) use the previous value
    assert_eq!(
        compile_codes(
            "label start {\n  let fs = [|x: int| x]\n  一\n  fs.push(|x: int| x + 1)\n}\n"
        ),
        ["fn_not_saveable"]
    );
    // It can be placed if reassigned after the stop point before use (assigned in the same block, or a block that encloses
    // the use)
    assert_eq!(
        one(
            "",
            "  let f = |x: int| x\n  一\n  f = |x: int| x + 1\n  if true {\n    ${f(1)}\n  }"
        ),
        "一|2"
    );
    assert_eq!(
        one(
            "",
            "  let f = |x: int| x\n  for i in [1, 2] {\n    f = |x: int| x + i\n    ${f(10)}\n  }"
        ),
        "11|12"
    );
    // It can be placed if used only before the stop point, declared inside the loop, or in a label without stop points
    assert_eq!(
        one("", "  let f = |x: int| x * 2\n  ${f(1) + f(2)}\n  二"),
        "6|二"
    );
    assert_eq!(
        one(
            "",
            "  for i in [1, 2] {\n    let f = |x: int| x + i\n    ${f(10)}\n  }"
        ),
        "11|12"
    );
    assert_eq!(
        one(
            "",
            "  call other(|x| x + 1)\n  済\n}\nlabel other(f: fn(int) -> int) {\n  let g = f\n  return"
        ),
        "済"
    );
}

#[test]
fn function_value_errors() {
    // The parameter type cannot be decided
    assert_eq!(
        compile_codes("label start {\n  let f = |x| x\n}\n"),
        ["cannot_infer_type"]
    );
    // A fn without a return value cannot be a value
    assert_eq!(
        compile_codes("fn noop(x: int) {\n}\nlabel start {\n  let f = noop\n}\n"),
        ["call_without_value"]
    );
    // Cannot be compared with `==`
    assert_eq!(
        compile_codes(&format!(
            "{DOUBLE}label start {{\n  let b = double == double\n}}\n"
        )),
        ["invalid_operation"]
    );
    // Type mismatches (the body, the number of arguments, values that are not functions)
    assert_eq!(
        compile_codes("label start {\n  let f: fn(str) -> int = |x| x\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let n = [1].map(|a, b| a)\n}\n"),
        ["arg_count", "cannot_infer_type"]
    );
    assert_eq!(
        compile_codes("label start {\n  let n = [1].filter(|a| a + 1)\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("label start {\n  let n = [1].map(3)\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!(
            "{DOUBLE}label start {{\n  let f = double\n  let n = f(1, 2)\n}}\n"
        )),
        ["arg_count"]
    );
    // The return type of a function type cannot be omitted
    assert_eq!(
        compile_codes("fn f(g: fn(int)) -> int {\n  return 0\n}\nlabel start {\n}\n"),
        ["unexpected_token"]
    );
    // The body of a lambda does not stop, the same as a fn (commands and label calls cannot be written in the expression)
    assert_eq!(
        compile_codes("label start {\n  let n = [1].map(|x| @ui.pause())\n}\n"),
        ["invalid_command_use"]
    );
}

const UNITS: &str = "struct Unit {\n  name: str,\n  hp: int,\n}\n";
const UNITS_LIST: &str = "  let us = [Unit { name: \"b\", hp: 5 }, Unit { name: \"a\", hp: 3 }, Unit { name: \"c\", hp: 5 }]\n";

#[test]
fn sort_by_is_stable_ascending_and_does_not_change_the_original() {
    let body = format!(
        "{UNITS_LIST}  ${{us.sort_by(|u| u.hp).map(|u| u.name)}} ${{us.map(|u| u.name)}} ${{us.sort_by(|u| u.name).map(|u| u.hp)}}"
    );
    assert_eq!(one(UNITS, &body), "[a, b, c] [b, a, c] [3, 5, 5]");
    // Keys of each type that can be ordered
    assert_eq!(
        one(
            "",
            "  ${[2, 1].sort_by(|x| x > 1)} ${[\"b\", \"a\"].sort_by(|s| s)} ${[2.5, 1.5].sort_by(|x| x)}"
        ),
        "[1, 2] [a, b] [1.5, 2.5]"
    );
    // Empty list
    assert_eq!(
        one("", "  let e: list<int> = []\n  ${e.sort_by(|x| x)}"),
        "[]"
    );
}

#[test]
fn sort_orders_ints_floats_strs_and_bools_and_reverse_reverses() {
    assert_eq!(
        one("", "  ${[3, -1, 2].sort()} ${[2.5, -1.0, 0.0].sort()}"),
        "[-1, 2, 3] [-1.0, 0.0, 2.5]"
    );
    // Strs are ordered by code point
    assert_eq!(
        one(
            "",
            "  ${[\"b\", \"あ\", \"a\", \"B\"].sort()} ${[true, false].sort()}"
        ),
        "[B, a, b, あ] [false, true]"
    );
    assert_eq!(
        one(
            "",
            "  let xs = [3, 1, 2]\n  ${xs.reverse()} ${xs} ${xs.sort().reverse()}"
        ),
        "[2, 1, 3] [3, 1, 2] [3, 2, 1]"
    );
    // `reverse` works for any element type
    assert_eq!(
        one(
            UNITS,
            &format!("{UNITS_LIST}  ${{us.reverse().map(|u| u.name)}}")
        ),
        "[c, a, b]"
    );
}

#[test]
fn min_by_and_max_by_pick_the_first_of_ties_and_none_for_an_empty_list() {
    let show = |o: &str| {
        format!(
            "{UNITS_LIST}  let e: list<Unit> = []\n  match {o} {{\n    Some(u) => {{\n      ${{u.name}}\n    }}\n    None => {{\n      none\n    }}\n  }}"
        )
    };
    assert_eq!(
        texts(&format!(
            "{UNITS}label start {{\n{}\n}}\n",
            show("us.max_by(|u| u.hp)")
        )),
        ["b", "<end>"]
    );
    assert_eq!(
        texts(&format!(
            "{UNITS}label start {{\n{}\n}}\n",
            show("us.min_by(|u| u.hp)")
        )),
        ["a", "<end>"]
    );
    assert_eq!(
        texts(&format!(
            "{UNITS}label start {{\n{}\n}}\n",
            show("e.max_by(|u| u.hp)")
        )),
        ["none", "<end>"]
    );
}

#[test]
fn sum_adds_ints_and_floats_and_is_zero_for_an_empty_list() {
    assert_eq!(one("", "  ${[3, 1, 2].sum()} ${[1.5, 2.5].sum()}"), "6 4.0");
    assert_eq!(
        one(
            "",
            "  let a: list<int> = []\n  let b: list<float> = []\n  ${a.sum()} ${b.sum()}"
        ),
        "0 0.0"
    );
    assert_eq!(
        texts("label start {\n  let xs = [9223372036854775807, 1]\n  ${xs.sum()}\n}\n"),
        ["<integer_overflow>"]
    );
    assert_eq!(
        texts(
            "label start {\n  let m = 100000000000000000000.0\n  let big = m * m * m * m * m * m * m * m * m * m * m * m * m * m * m * 100000000.0\n  let xs = [big, big]\n  ${xs.sum()}\n}\n"
        ),
        ["<non_finite_float>"]
    );
}

#[test]
fn sort_and_sum_reject_unsupported_element_types() {
    let units = |body: &str| format!("{UNITS}label start {{\n{UNITS_LIST}{body}\n}}\n");
    assert_eq!(
        compile_codes(&units("  let a = us.sort()")),
        ["unsupported_element_type"]
    );
    assert_eq!(
        compile_codes(&units("  let a = us.sort_by(|u| u)")),
        ["unsupported_element_type"]
    );
    assert_eq!(
        compile_codes(&units("  let a = us.min_by(|u| [u.hp])")),
        ["unsupported_element_type"]
    );
    assert_eq!(
        compile_codes(&units("  let a = us.sum()")),
        ["unsupported_element_type"]
    );
    assert_eq!(
        compile_codes("label start {\n  let a = [\"a\"].sum()\n}\n"),
        ["unsupported_element_type"]
    );
    // The number of arguments, and a misspelled method name
    assert_eq!(
        compile_codes("label start {\n  let a = [1].sort(1)\n}\n"),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes("label start {\n  let a: list<int> = [1].sort_on(|x: int| x)\n}\n"),
        ["unknown_method"]
    );
}

#[test]
fn lambda_literals_written_in_higher_order_methods_keep_their_meaning() {
    // Captures of several kinds and every higher-order method
    let body = "  let k = 3\n  let xs = [1, 2, 3, 4]\n  let names = [\"a\", \"bb\"]\n  ${xs.map(|x| x * k)} ${xs.filter(|x| x > k - 2)} ${xs.count(|x| x % 2 == 0)} ${xs.any(|x| x == k)} ${xs.all(|x| x < k)} ${xs.find(|x| x > k)} ${xs.fold(0, |a, x| a + x * k)} ${names.any(|n| n == \"bb\")} ${xs.sort_by(|x| 0 - x)} ${xs.min_by(|x| (x - k) * (x - k))} ${xs.max_by(|x| x % 3)}";
    assert_eq!(
        one("", body),
        "[3, 6, 9, 12] [2, 3, 4] 2 true false Some(4) 30 true [4, 3, 2, 1] Some(3) Some(2)"
    );
    // Inside `fn`, the inner lambda captures the outer parameter and the outer lambda's parameter
    let src = "fn f(xs: list<int>, k: int) -> list<list<int>> {\n  return xs.map(|x| xs.filter(|y| y + k > x))\n}\nlabel start {\n  ${f([1, 2], 0)}\n}\n";
    assert_eq!(texts(src), ["[[2], []]", "<end>"]);
    // Errors in the body are reported as before
    let t = texts("label start {\n  let xs = [9223372036854775807]\n  ${xs.map(|x| x + 1)}\n}\n");
    assert_eq!(t, ["<integer_overflow>"]);
}
