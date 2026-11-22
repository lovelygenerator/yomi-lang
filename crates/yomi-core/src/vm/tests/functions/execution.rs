//! Execution

use super::*;

#[test]
fn recursion_and_mutual_recursion() {
    let f = "fn fact(n: int) -> int {\n  if n <= 1 {\n    return 1\n  }\n  return n * fact(n - 1)\n}\nfn fib(n: int) -> int {\n  if n < 2 {\n    return n\n  }\n  return fib(n - 1) + fib(n - 2)\n}\nfn is_even(n: int) -> bool {\n  if n == 0 {\n    return true\n  }\n  return is_odd(n - 1)\n}\nfn is_odd(n: int) -> bool {\n  if n == 0 {\n    return false\n  }\n  return is_even(n - 1)\n}\n";
    assert_eq!(
        one(f, "  ${fact(10)} ${fib(15)} ${is_even(20)} ${is_odd(20)}"),
        "3628800 610 true false"
    );
}

#[test]
fn compound_args_and_return_values() {
    let f = "struct P { name: str, hp: int }\nenum Dir { N, Go(int) }\nfn heal(p: P, by: int) -> P {\n  p.hp += by\n  return p\n}\nfn name_of(m: map<str, P>, k: str) -> Option<str> {\n  match m.get(k) {\n    Some(p) => return Some(p.name)\n    None => return None\n  }\n}\nfn step(d: Dir) -> int {\n  match d {\n    N => return 0\n    Go(n) => return n\n  }\n}\nfn evens(xs: list<int>) -> list<int> {\n  let out: list<int> = []\n  for x in xs {\n    if x % 2 == 0 {\n      out.push(x)\n    }\n  }\n  return out\n}\n";
    assert_eq!(
        one(
            f,
            "  let p = P { name: \"案内人\", hp: 3 }\n  let q = heal(p, 4)\n  ${p} ${q}"
        ),
        "P { name: 案内人, hp: 3 } P { name: 案内人, hp: 7 }"
    );
    assert_eq!(
        one(
            f,
            "  let m = map { \"a\": P { name: \"x\", hp: 1 } }\n  ${name_of(m, \"a\")} ${name_of(m, \"b\")}"
        ),
        "Some(x) None"
    );
    assert_eq!(one(f, "  ${step(Dir.N)} ${step(Dir.Go(5))}"), "0 5");
    assert_eq!(one(f, "  ${evens([1, 2, 3, 4])}"), "[2, 4]");
}

#[test]
fn arguments_are_passed_by_value_and_updates_in_fn_do_not_affect_caller() {
    let f = "struct P { hp: int }\nfn bump(xs: list<int>, p: P, n: int) -> int {\n  xs.push(99)\n  xs[0] = 0\n  p.hp = 100\n  n += 1\n  return n\n}\n";
    assert_eq!(
        one(
            f,
            "  let xs = [1, 2]\n  let p = P { hp: 5 }\n  let n = 1\n  let r = bump(xs, p, n)\n  ${xs} ${p.hp} ${n} ${r}"
        ),
        "[1, 2] 5 1 2"
    );
    // Same when passing a module variable
    let src = format!(
        "save var g: list<int> = [1]\n{f}label start {{\n  let p = P {{ hp: 1 }}\n  let r = bump(g, p, 0)\n  ${{g}}\n}}\n"
    );
    assert_eq!(texts(&src), ["[1]", "<end>"]);
}

#[test]
fn while_for_match_and_early_return_in_fn() {
    let f = "fn find(xs: list<int>, want: int) -> int {\n  let i = 0\n  while i < xs.len() {\n    if xs[i] == want {\n      return i\n    }\n    i += 1\n  }\n  return -1\n}\nfn first_big(xs: list<int>) -> Option<int> {\n  for x in xs {\n    if x > 10 {\n      return Some(x)\n    }\n  }\n  return None\n}\nfn skip_sum(xs: list<int>) -> int {\n  let t = 0\n  for x in xs {\n    if x == 0 {\n      continue\n    }\n    if x < 0 {\n      break\n    }\n    t += x\n  }\n  return t\n}\nfn kind(n: int) -> str {\n  match n {\n    0 => return \"zero\"\n    1 => { return \"one\" }\n    _ => {\n      for i in range(0, 3) {\n        if i == n {\n          return \"small\"\n        }\n      }\n      return \"big\"\n    }\n  }\n}\n";
    assert_eq!(
        one(
            f,
            "  ${find([5, 6, 7], 7)} ${find([5], 1)} ${first_big([1, 20, 30])} ${first_big([1])} ${skip_sum([1, 0, 2, -1, 100])} ${kind(0)} ${kind(1)} ${kind(2)} ${kind(9)}"
        ),
        "2 -1 Some(20) None 3 zero one small big"
    );
}

#[test]
fn fn_calls_mid_expression_give_correct_values() {
    let f = "fn id(x: int) -> int {\n  return x\n}\nfn pick(n: int) -> int {\n  match n {\n    1 => return 10\n    _ => return 20\n  }\n}\nfn guard(x: int) -> bool {\n  match x {\n    0 => return false\n    _ => return true\n  }\n}\n";
    // Return from an arm of `match` (even if the subject remains on the stack, the caller's operands are not broken)
    assert_eq!(
        one(
            f,
            "  ${1 + pick(1) * 2 + pick(2) - id(3) * (id(4) + pick(1))}"
        ),
        "-1"
    );
    assert_eq!(one(f, "  ${[id(1), pick(1), id(3)]}"), "[1, 10, 3]");
    // Short-circuit evaluation does not call the `fn` on the right
    assert_eq!(
        one(
            f,
            "  ${guard(0) && guard(1)} ${guard(1) || guard(0)} ${guard(0) || guard(1)}"
        ),
        "false true true"
    );
}

#[test]
fn fn_calls_return_same_value_for_same_arguments() {
    let f = "fn f(x: int) -> int {\n  let acc = 0\n  for i in range(0, x) {\n    acc += i * i\n  }\n  return acc\n}\n";
    assert_eq!(one(f, "  ${f(5)} ${f(5)} ${f(6)}"), "30 30 55");
}

#[test]
fn order_of_declarations_in_fn_body_does_not_matter() {
    // A `fn` written later can be called from earlier
    let src = "label start {\n  ${later(2)}\n}\nfn later(x: int) -> int {\n  return x + 1\n}\n";
    assert_eq!(texts(src), ["3", "<end>"]);
}

#[test]
fn fn_in_stop_args_is_rebuilt_on_load() {
    let src = "fn label_of(k: int) -> str {\n  return \"#${k}\"\n}\nsave var n: int = 1\nlabel start {\n  一 ${label_of(n)} #a\n  n = 5\n  二 ${label_of(n)} #b\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一 #1");
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), "二 #5");
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(text_of(vm2.current().unwrap()), "二 #5");
    assert_eq!(run_all(&mut vm2), ["<end>"]);
}

#[test]
fn alternating_calls_to_differently_sized_fns_leave_no_previous_values() {
    let f = "fn small(n: int) -> int {\n  return n + 1\n}\nfn big(xs: list<int>, s: str) -> str {\n  let total = 0\n  for x in xs {\n    total += x * 2 + 1\n  }\n  let r = \"${s}:${total}\"\n  return r\n}\nfn names(k: int) -> list<str> {\n  let out: list<str> = []\n  let i = 0\n  while i < k {\n    out.push(\"n${i}\")\n    i += 1\n  }\n  return out\n}\n";
    assert_eq!(
        one(
            f,
            "  let a = small(1)\n  let b = big([1, 2, 3], \"x\")\n  let c = small(a)\n  let d = names(2)\n  let e = big([], \"y\")\n  let g = names(0)\n  ${a} ${b} ${c} ${d} ${e} ${g}"
        ),
        "2 x:15 3 [n0, n1] y:0 []"
    );
}

#[test]
fn lambda_captures_are_passed_correctly_on_each_call() {
    let f = "fn apply(g: fn(int) -> int, x: int) -> int {\n  return g(x)\n}\n";
    assert_eq!(
        one(
            f,
            "  let k = 10\n  let add = |x: int| x + k\n  let mul = |x: int| x * k\n  ${apply(add, 1)} ${apply(mul, 2)} ${apply(add, 3)}"
        ),
        "11 20 13"
    );
}
