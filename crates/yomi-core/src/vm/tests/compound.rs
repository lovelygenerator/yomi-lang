//! Tests of compound values: running list, Option, struct, enum and match, value semantics, save, load and rollback.

use super::util::{self, *};
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Choice], |_| {})
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

fn compile_codes(src: &str) -> Vec<&'static str> {
    util::error_codes(src, &registry())
}

fn run_all(vm: &mut Vm<'_>) -> Vec<String> {
    util::run_all(vm, 200)
}

fn texts(src: &str) -> Vec<String> {
    util::texts_with(src, &registry(), VmConfig::default(), 200)
}

// ---------------------------------------------------------------------------
// Syntax and types
// ---------------------------------------------------------------------------

#[test]
fn parses_multiline_struct_and_enum_declarations() {
    let src = "struct P {\n  hp: int,\n  name: str,\n}\nenum E {\n  A,\n  B(int),\n}\nlabel start {\n  let p = P { name: \"a\", hp: 1 }\n  ${p.hp}${p.name}\n}\n";
    assert_eq!(texts(src), ["1a", "<end>"]);
}

#[test]
fn empty_list_and_none_error_when_type_undecidable() {
    assert_eq!(
        compile_codes("label start {\n  let a = []\n}\n"),
        ["cannot_infer_type"]
    );
    assert_eq!(
        compile_codes("label start {\n  let a = None\n}\n"),
        ["cannot_infer_type"]
    );
    assert!(
        compile_codes("label start {\n  let a: list<int> = []\n  let b: Option<int> = None\n}\n")
            .is_empty()
    );
}

#[test]
fn empty_list_type_comes_from_target_argument_or_comparison() {
    let src = "struct S { xs: list<int> }\nsave var a: list<str> = [\"x\"]\nlabel start {\n  a = []\n  let s = S { xs: [] }\n  if s.xs == [] {\n    空\n  }\n  if a == [] {\n    空2\n  }\n}\n";
    assert_eq!(texts(src), ["空", "空2", "<end>"]);
}

#[test]
fn list_elements_of_mixed_types_are_error() {
    assert_eq!(
        compile_codes("label start {\n  let a = [1, \"x\"]\n}\n"),
        ["type_mismatch"]
    );
}

#[test]
fn statement_methods_cannot_be_used_in_expressions() {
    assert_eq!(
        compile_codes("label start {\n  let a = [1]\n  let b = a.push(2)\n}\n"),
        ["statement_method_in_expression"]
    );
}

#[test]
fn statement_methods_only_on_assignable_places() {
    // Cannot push to a list of string literals (it becomes a plain line)
    let t = texts("label start {\n  [1, 2].push(3)\n}\n");
    assert_eq!(t, ["[1, 2].push(3)", "<end>"]);
}

#[test]
fn recursive_types_are_errors() {
    assert_eq!(
        compile_codes("struct R { r: list<R> }\nlabel start {\n  end\n}\n"),
        ["recursive_type"]
    );
    assert_eq!(
        compile_codes("enum T { Leaf, Node(T, T) }\nlabel start {\n  end\n}\n"),
        ["recursive_type"]
    );
}

#[test]
fn same_named_types_are_errors() {
    assert_eq!(
        compile_codes("struct A { x: int }\nenum A { B }\nlabel start {\n  end\n}\n"),
        ["duplicate_name"]
    );
}

#[test]
fn struct_literal_in_if_condition_needs_parentheses() {
    let src = "struct P { hp: int }\nlabel start {\n  let p = P { hp: 1 }\n  if p == (P { hp: 1 }) {\n    同じ\n  }\n}\n";
    assert_eq!(texts(src), ["同じ", "<end>"]);
}

#[test]
fn too_deeply_nested_types_are_errors() {
    let mut src = String::from("struct S0 { x: int }\n");
    for i in 1..40 {
        src.push_str(&format!("struct S{i} {{ s: S{} }}\n", i - 1));
    }
    src.push_str("label start {\n  end\n}\n");
    assert!(compile_codes(&src).contains(&"too_deep"));
}

#[test]
fn too_deep_string_interpolation_nesting_is_error_without_exhausting_stack() {
    for n in [30, 5000] {
        let inner = format!("{}1{}", "\"${".repeat(n), "}\"".repeat(n));
        for line in [
            format!("v ${{{inner}}}"),
            format!("let s = {inner}"),
            format!("「${{{inner}}}」"),
        ] {
            let src = format!("label start {{\n  {line}\n}}\n");
            assert!(compile_codes(&src).contains(&"too_deep"), "{n}: {line:.20}");
        }
    }
    // Shallow nesting can be written
    let src = "label start {\n  let s = \"a${\"b${1 + 2}\"}\"\n  ${s}\n}\n";
    assert_eq!(texts(src), ["ab3", "<end>"]);
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

#[test]
fn list_index_indexed_assignment_len_and_contains_work() {
    let src = "label start {\n  let xs = [10, 20, 30]\n  xs[1] = 21\n  xs[2] += 5\n  ${xs[0]} ${xs[1]} ${xs[2]} ${xs.len()}\n  if xs.contains(35) {\n    あり\n  }\n  if !xs.contains(1) {\n    なし\n  }\n}\n";
    assert_eq!(texts(src), ["10 21 35 3", "あり", "なし", "<end>"]);
}

#[test]
fn push_remove_and_clear_work() {
    let src = "label start {\n  let xs: list<int> = []\n  xs.push(1)\n  xs.push(2)\n  xs.push(3)\n  xs.remove(0)\n  ${xs}\n  xs.clear()\n  ${xs.len()}\n}\n";
    assert_eq!(texts(src), ["[2, 3]", "0", "<end>"]);
}

#[test]
fn out_of_range_index_is_runtime_error() {
    assert_eq!(
        texts("label start {\n  let xs = [1]\n  let n = xs[1]\n}\n"),
        ["<index_out_of_range>"]
    );
    assert_eq!(
        texts("label start {\n  let xs = [1]\n  xs[0 - 1] = 5\n}\n"),
        ["<index_out_of_range>"]
    );
    assert_eq!(
        texts("label start {\n  let xs: list<int> = []\n  xs.remove(0)\n}\n"),
        ["<index_out_of_range>"]
    );
}

#[test]
fn assigned_copy_is_independent_of_original() {
    let src = "struct P { xs: list<int> }\nlabel start {\n  let a = P { xs: [1] }\n  let b = a\n  b.xs.push(2)\n  b.xs[0] = 9\n  ${a.xs} ${b.xs}\n  let c = [a, b]\n  c[0].xs.push(7)\n  ${a.xs} ${c[0].xs} ${c[1].xs}\n}\n";
    assert_eq!(texts(src), ["[1] [9, 2]", "[1] [1, 7] [9, 2]", "<end>"]);
}

#[test]
fn variable_fields_are_read_without_copying_whole_value_and_are_independent() {
    let src = "struct In { xs: list<int>, n: int }\nstruct Out { name: str, inner: In }\nsave var g: Out = Out { name: \"g\", inner: In { xs: [1], n: 5 } }\nlabel start {\n  let o = Out { name: \"o\", inner: In { xs: [2], n: 7 } }\n  let ys = o.inner.xs\n  ys.push(3)\n  o.inner.n += 1\n  ${o.inner.n} ${o.inner.xs} ${ys} ${o.name}\n  let zs = g.inner.xs\n  g.inner.xs.push(9)\n  ${g.inner.n} ${g.inner.xs} ${zs} ${g.name}\n}\n";
    assert_eq!(texts(src), ["8 [2] [2, 3] o", "5 [1, 9] [1] g", "<end>"]);
    // `variable.a.b` becomes one instruction (`LoadField`) and does not use `GetField`
    let prog = compile_ok(src);
    let code = &prog.labels[0].reg;
    assert!(code.ops.iter().any(|op| matches!(
        op,
        RegOp::LoadField { path, .. } if code.paths[*path].len() == 2
    )));
    assert!(
        !code
            .ops
            .iter()
            .any(|op| matches!(op, RegOp::GetField { .. }))
    );
}

#[test]
fn assignment_to_nested_places_works() {
    let src = "struct In { n: int }\nstruct Out { ins: list<In> }\nlabel start {\n  let o = Out { ins: [In { n: 1 }, In { n: 2 }] }\n  o.ins[1].n += 40\n  o.ins[0].n = 5\n  ${o.ins[0].n} ${o.ins[1].n}\n}\n";
    assert_eq!(texts(src), ["5 42", "<end>"]);
}

#[test]
fn struct_fields_are_evaluated_in_written_order() {
    let src = "struct Pair { a: int, b: int }\nlabel start {\n  let p = Pair { b: 1, a: 2 }\n  ${p.a} ${p.b}\n}\n";
    assert_eq!(texts(src), ["2 1", "<end>"]);
}

#[test]
fn struct_and_enum_equality() {
    let src = "enum E { A, B(int) }\nstruct P { e: E, xs: list<int> }\nlabel start {\n  let p = P { e: E.B(1), xs: [1] }\n  let q = P { e: E.B(1), xs: [1] }\n  let r = P { e: E.B(2), xs: [1] }\n  if p == q {\n    等しい\n  }\n  if p != r {\n    違う\n  }\n  if E.A != E.B(0) {\n    違う2\n  }\n}\n";
    assert_eq!(texts(src), ["等しい", "違う", "違う2", "<end>"]);
}

#[test]
fn option_equality_and_match() {
    let src = "label start {\n  let a: Option<int> = Some(3)\n  let b: Option<int> = None\n  if a != b {\n    違う\n  }\n  if b == None {\n    なし\n  }\n  match a {\n    Some(n) => ${n}\n    None => 無\n  }\n}\n";
    assert_eq!(texts(src), ["違う", "なし", "3", "<end>"]);
}

#[test]
fn match_tries_top_down_and_bindings_live_only_in_arm() {
    let src = "enum E { A(int), B }\nlabel start {\n  let x = E.A(4)\n  match x {\n    A(n) => ${n}\n    B => b\n  }\n  let n = 99\n  ${n}\n}\n";
    assert_eq!(texts(src), ["4", "99", "<end>"]);
}

#[test]
fn match_arms_can_have_multiple_statements() {
    let src = "label start {\n  let o: Option<int> = Some(1)\n  match o {\n    Some(n) => {\n      ${n}\n      次\n    }\n    None => {\n      なし\n    }\n  }\n}\n";
    assert_eq!(texts(src), ["1", "次", "<end>"]);
}

#[test]
fn match_exhaustiveness_checks() {
    let codes = |body: &str| {
        compile_codes(&format!(
            "enum E {{ A, B(int), C }}\nlabel start {{\n  let x = E.A\n  let o: Option<int> = None\n{body}}}\n"
        ))
    };
    assert_eq!(
        codes("  match x {\n    A => 1\n    B(n) => 2\n  }\n"),
        ["non_exhaustive_match"]
    );
    assert!(codes("  match x {\n    A => 1\n    B(n) => 2\n    C => 3\n  }\n").is_empty());
    assert!(codes("  match x {\n    A => 1\n    _ => 2\n  }\n").is_empty());
    assert_eq!(
        codes("  match o {\n    Some(n) => 1\n  }\n"),
        ["non_exhaustive_match"]
    );
    assert_eq!(
        codes("  match 1 {\n    1 => 1\n  }\n"),
        ["non_exhaustive_match"]
    );
    assert_eq!(
        codes("  match true {\n    true => 1\n  }\n"),
        ["non_exhaustive_match"]
    );
    assert!(codes("  match true {\n    true => 1\n    false => 2\n  }\n").is_empty());
}

#[test]
fn unreachable_patterns_warn_and_compile() {
    let src = "label start {\n  match 1 {\n    _ => 1\n    2 => 2\n  }\n}\n";
    let (_, warnings) =
        crate::compile_modules_with_warnings(&[("main", src)], &registry()).expect("通る");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code.as_str(), "unreachable_pattern");
}

#[test]
fn patterns_of_wrong_type_are_errors() {
    assert_eq!(
        compile_codes("label start {\n  match 1 {\n    \"a\" => 1\n    _ => 2\n  }\n}\n"),
        ["type_mismatch"]
    );
}

#[test]
fn interpolation_prints_compound_values_in_fixed_notation() {
    let src = "struct P { n: str, xs: list<int> }\nenum E { A, B(int, str) }\nlabel start {\n  let o: Option<str> = Some(\"s\")\n  ${P { n: \"a\", xs: [1, 2] }} ${E.B(1, \"x\")} ${E.A} ${o}\n}\n";
    assert_eq!(
        texts(src),
        ["P { n: a, xs: [1, 2] } E.B(1, x) E.A Some(s)", "<end>"]
    );
}

#[test]
fn too_large_value_is_runtime_error() {
    let src = "label start {\n  let s = \"aaaaaaaa\"\n  let xs = [s]\n  while true {\n    s = \"${s}${s}\"\n    xs = [s, s]\n  }\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(1 << 12)).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
}

// ---------------------------------------------------------------------------
// Save, load and rollback
// ---------------------------------------------------------------------------

const STATE_SRC: &str = "enum Mode { Idle, Run(int) }\nstruct Chara { name: str, hp: int, items: list<str> }\nsave var c: Chara = Chara { name: \"案内人\", hp: 10, items: [] }\nsave var mode: Mode = Mode.Idle\nsave var maybe: Option<list<int>> = None\nlabel start {\n  c.hp -= 1\n  c.items.push(\"薬\")\n  mode = Mode.Run(3)\n  maybe = Some([1, 2])\n  一 #a\n  c.hp -= 1\n  c.items.push(\"剣\")\n  maybe = None\n  二 #b\n  ${c} ${mode} ${maybe} #c\n}\n";

#[test]
fn compound_variables_continue_identically_after_save_and_reload() {
    let reg = registry();
    let prog = compile_ok(STATE_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一");
    let bytes = vm.save().unwrap();
    let rest_a = run_all(&mut vm);
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert!(report.type_changed_vars.is_empty() && report.changed_types.is_empty());
    assert_eq!(var(&vm2, "maybe"), var_of_some_list());
    assert_eq!(run_all(&mut vm2), rest_a);
    assert_eq!(
        rest_a.last().map(String::as_str),
        Some("<end>"),
        "{rest_a:?}"
    );
    assert!(
        rest_a.contains(
            &"Chara { name: 案内人, hp: 8, items: [薬, 剣] } Mode.Run(3) None".to_string()
        )
    );
}

fn var_of_some_list() -> Value {
    Value::Option(Some(Box::new(Value::List(
        vec![Value::Int(1), Value::Int(2)].into(),
    ))))
}

#[test]
fn saved_bytes_are_same_from_same_state() {
    let reg = registry();
    let prog = compile_ok(STATE_SRC);
    let mut a = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut b = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    a.run().unwrap();
    b.run().unwrap();
    assert_eq!(a.save().unwrap(), b.save().unwrap());
}

#[test]
fn compound_values_in_stop_args_and_history_are_saved() {
    let src = "label start {\n  let xs = [\"a\", \"b\"]\n  let n = xs.len()\n  ${xs}\n  * ${n}個 #c1\n  * もう一つ\n  終わり\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "[a, b]");
    resume(&mut vm);
    let y = vm.run().unwrap();
    assert!(matches!(y, Yield::Stop { .. }));
    let bytes = vm.save().unwrap();
    let hist = vm.history().to_vec();
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(vm2.history(), &hist[..]);
    assert_eq!(vm2.current(), vm.current());
}

#[test]
fn compound_values_stay_as_of_snapshot_after_rollback() {
    let reg = registry();
    let prog = compile_ok(STATE_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    let c0 = var(&vm, "c");
    let first = run_all(&mut vm);
    assert_ne!(var(&vm, "c"), c0);
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "c"), c0);
    assert_eq!(run_all(&mut vm), first);
}

#[test]
fn compound_local_values_can_be_saved_and_reloaded() {
    let src = "struct P { n: int }\nlabel start {\n  let ps = [P { n: 1 }, P { n: 2 }]\n  ps[0].n = 5\n  前 #a\n  ${ps[0].n}${ps[1].n}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(run_all(&mut vm2), ["52", "<end>"]);
}

fn load_with(old: &str, new: &str) -> (Vm<'static>, LoadReport) {
    // For tests, leak the program and registry to extend their lifetimes
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
    Vm::load(
        new_prog,
        reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
        &bytes,
    )
    .unwrap()
}

#[test]
fn changed_struct_fields_reset_variables_of_that_type() {
    let old = "struct P { hp: int }\nsave var p: P = P { hp: 5 }\nsave var k: int = 1\nlabel start {\n  p.hp = 7\n  k = 2\n  一 #a\n  二\n}\n";
    let new = "struct P { hp: int, mp: int }\nsave var p: P = P { hp: 1, mp: 2 }\nsave var k: int = 1\nlabel start {\n  p.hp = 7\n  k = 2\n  一 #a\n  二\n}\n";
    let (vm, report) = load_with(old, new);
    assert_eq!(report.changed_types, ["main.P"]);
    assert_eq!(report.type_changed_vars, ["main.p"]);
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(var(&vm, "k"), Value::Int(2));
    assert_eq!(var(&vm, "p").to_text(), "P { hp: 1, mp: 2 }");
}

#[test]
fn detects_types_with_fewer_fields_and_changed_enum_variants() {
    let old = "struct P { hp: int, mp: int }\nenum E { A, B }\nsave var p: P = P { hp: 1, mp: 1 }\nsave var e: E = E.B\nlabel start {\n  一 #a\n}\n";
    let new = "struct P { hp: int }\nenum E { A, C }\nsave var p: P = P { hp: 9 }\nsave var e: E = E.C\nlabel start {\n  一 #a\n}\n";
    let (vm, report) = load_with(old, new);
    assert_eq!(report.changed_types, ["main.E", "main.P"]);
    assert_eq!(report.type_changed_vars, ["main.p", "main.e"]);
    assert_eq!(var(&vm, "e").to_text(), "E.C");
}

#[test]
fn types_containing_changed_types_and_lists_of_them_count_as_changed() {
    let old = "struct In { a: int }\nstruct Out { i: In }\nsave var o: Out = Out { i: In { a: 1 } }\nsave var l: list<In> = [In { a: 2 }]\nsave var ok: list<int> = [1]\nlabel start {\n  一 #a\n}\n";
    let new = "struct In { a: str }\nstruct Out { i: In }\nsave var o: Out = Out { i: In { a: \"x\" } }\nsave var l: list<In> = []\nsave var ok: list<int> = [1]\nlabel start {\n  一 #a\n}\n";
    let (vm, report) = load_with(old, new);
    assert_eq!(report.changed_types, ["main.In", "main.Out"]);
    assert_eq!(report.type_changed_vars, ["main.o", "main.l"]);
    assert_eq!(var(&vm, "ok").to_text(), "[1]");
}

#[test]
fn report_is_empty_when_declarations_did_not_change() {
    let (_, report) = load_with(STATE_SRC, STATE_SRC);
    assert!(report.changed_types.is_empty());
    assert!(report.type_changed_vars.is_empty());
    assert_eq!(report.resolved_by, ResolvedBy::Id);
}

#[test]
fn broken_saves_are_rejected_even_inside_compound_values() {
    let reg = registry();
    let prog = compile_ok(STATE_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // Changing any single byte does not panic; it is either read or rejected
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0xff;
        let _ = Vm::load(&prog, &reg, VmConfig::default(), &b);
    }
    for n in 0..bytes.len() {
        let _ = Vm::load(&prog, &reg, VmConfig::default(), &bytes[..n]);
    }
}

#[test]
fn persistent_data_can_hold_compound_values() {
    let src = "persistent var best: list<int> = []\nlabel start {\n  best.push(3)\n  一 #a\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save_persistent();
    let mut vm2 = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let report = vm2.load_persistent(&bytes).unwrap();
    assert!(report.changed_types.is_empty());
    assert_eq!(var(&vm2, "best").to_text(), "[3]");
}

#[test]
fn host_declared_variables_can_be_list() {
    let mut r = registry();
    assert!(
        r.var::<Vec<i64>>("host.scores", Kind::Save, vec![1])
            .is_ok()
    );
    assert!(r.var::<i64>("host.gold", Kind::Save, 1).is_ok());
}

#[test]
fn values_at_depth_limit_save_and_reload_on_1_mib_stack() {
    crate::tests::on_small_stack(|| {
        let depth = 20;
        let ty = format!("{}int{}", "list<".repeat(depth), ">".repeat(depth));
        let lit = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        let src = format!("save var a: {ty} = {lit}\nlabel start {{\n  一 #a\n  ${{a}}\n}}\n");
        let reg = registry();
        let prog = compile_ok(&src);
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        vm.run().unwrap();
        let bytes = vm.save().unwrap();
        let snap = vm.snapshot().unwrap();
        let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
        assert_eq!(var(&vm2, "a"), var(&vm, "a"));
        vm.restore(&snap).unwrap();
        let t = run_all(&mut vm2);
        assert_eq!(t.last().map(String::as_str), Some("<end>"));
    });
}
