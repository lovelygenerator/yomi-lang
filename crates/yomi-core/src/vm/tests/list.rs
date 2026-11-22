//! Tests of sharing and copy on write of `list` (`ListData`): value semantics, incremental tracking of total size, saving,
//! restoring and snapshots, and the performance of large `list`s and `map`s (only loose limits).

use super::util::{self, *};
use crate::program::Program;
use crate::registry::Sugar;
use crate::value::ListData;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Choice, Sugar::Quote], |_| {})
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

fn run_all(vm: &mut Vm<'_>) -> Vec<String> {
    util::run_all(vm, 5000)
}

fn texts(src: &str) -> Vec<String> {
    util::texts_with(src, &registry(), VmConfig::default(), 5000)
}

/// Whether the `list`s and `map`s in a value keep the same total size as recounting gives.
/// And whether the estimate of the value's size equals a naive recount.
fn deep_consistent(v: &Value) -> bool {
    fn naive(v: &Value) -> usize {
        match v {
            Value::Unit | Value::Int(_) | Value::Float(_) | Value::Bool(_) => 8,
            Value::Str(s) => 24 + s.len(),
            Value::Identity(i) => 48 + i.id.len() + i.name.len(),
            Value::List(items) => 24 + items.iter().map(naive).sum::<usize>(),
            Value::Map(entries) => {
                24 + entries
                    .iter()
                    .map(|(k, v)| naive(k) + naive(v))
                    .sum::<usize>()
            }
            Value::Option(None) => 8,
            Value::Option(Some(v)) => 8 + naive(v),
            Value::Struct(st) => {
                let fields = &st.fields;
                24 + fields.iter().map(|(_, v)| naive(v)).sum::<usize>()
            }
            Value::Enum(e) => 24 + e.args.iter().map(naive).sum::<usize>(),
            Value::Fn(fv) => 24 + fv.captures.iter().map(naive).sum::<usize>(),
        }
    }
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        if v.approx_bytes() != naive(v) {
            return false;
        }
        match v {
            Value::List(items) => {
                if !items.is_consistent() {
                    return false;
                }
                stack.extend(items.iter());
            }
            Value::Map(entries) => {
                if !entries.is_consistent() {
                    return false;
                }
                stack.extend(entries.iter().flat_map(|(k, v)| [k, v]));
            }
            Value::Option(Some(v)) => stack.push(v),
            Value::Struct(st) => stack.extend(st.fields.iter().map(|(_, v)| v)),
            Value::Enum(e) => stack.extend(e.args.iter()),
            _ => {}
        }
    }
    true
}

/// Whether all variables of the VM are consistent, and the incremental tracking of the total size of variables is correct.
fn vm_consistent(vm: &Vm<'_>) -> bool {
    vm.variables().iter().all(|v| deep_consistent(&v.value))
        && (!vm.total_valid || vm.total == vm.recount_total())
}

// ---------------------------------------------------------------------------
// The type itself
// ---------------------------------------------------------------------------

#[test]
fn list_clone_shares_contents_and_becomes_independent_on_update() {
    let a: ListData = vec![Value::Int(1), Value::Int(2)].into();
    let mut b = a.clone();
    assert!(a.shares_storage_with(&b));
    b.push(Value::Int(3));
    assert!(!a.shares_storage_with(&b));
    assert_eq!(a.len(), 2);
    assert_eq!(b.len(), 3);
    // Things not shared are updated in place (no more copying)
    let mut c = b.clone();
    c.push(Value::Int(4));
    let before = (b.len(), c.len());
    let mut d = c.clone();
    drop(d.clone());
    d.insert(0, Value::Int(0));
    d.remove(1);
    assert_eq!(before, (3, 4));
    assert_eq!(c.as_slice().len(), 4);
    assert!(a.is_consistent() && b.is_consistent() && c.is_consistent() && d.is_consistent());
}

#[test]
fn list_debug_and_serialization_match_vec() {
    let items = vec![Value::Int(1), Value::Str("a".into())];
    let list: ListData = items.clone().into();
    assert_eq!(format!("{list:?}"), format!("{items:?}"));
    assert_eq!(
        format!("{:?}", Value::List(list.clone())),
        format!("List({items:?})")
    );
    let bytes = postcard::to_allocvec(&list).unwrap();
    assert_eq!(bytes, postcard::to_allocvec(&items).unwrap());
    let back: ListData = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(back, list);
    assert!(back.is_consistent());
}

#[test]
fn list_converts_back_to_vec_without_changing_shared_original() {
    let a: ListData = vec![Value::Int(1), Value::Int(2)].into();
    let b = a.clone();
    let v = b.into_vec();
    assert_eq!(v, vec![Value::Int(1), Value::Int(2)]);
    assert_eq!(a.len(), 2);
    assert_eq!(a.into_vec(), v);
    // `Deref`, `IntoIterator`, `FromIterator`
    let l: ListData = (0..3).map(Value::Int).collect();
    assert_eq!(l.iter().count(), 3);
    assert_eq!(l.first(), Some(&Value::Int(0)));
    assert_eq!(l.into_iter().count(), 3);
}

#[test]
fn list_size_estimate_matches_naive_count_even_nested() {
    let inner: ListData = vec![Value::Int(1), Value::Str("abc".into())].into();
    let outer = Value::List(vec![Value::List(inner.clone()), Value::List(inner)].into());
    assert!(deep_consistent(&outer));
    assert_eq!(outer.approx_bytes(), 24 + 2 * (24 + 8 + 24 + 3));
    assert_eq!(Value::List(ListData::new()).approx_bytes(), 24);
}

// ---------------------------------------------------------------------------
// Value semantics (even when shared, updating one does not affect the other)
// ---------------------------------------------------------------------------

#[test]
fn copying_list_by_assignment_keeps_updates_independent() {
    let src = "label start {\n  let a = [1, 2, 3]\n  let b = a\n  b.push(4)\n  b[0] = 100\n  a.remove(1)\n  ${a} ${b}\n  let c = b\n  c.clear()\n  ${b} ${c}\n  let d = a\n  d.insert(0, 7)\n  ${a} ${d}\n}\n";
    assert_eq!(
        texts(src),
        [
            "[1, 3] [100, 2, 3, 4]",
            "[100, 2, 3, 4] []",
            "[1, 3] [7, 1, 3]",
            "<end>"
        ]
    );
}

#[test]
fn passing_list_to_fn_does_not_change_callers_list() {
    let src = "fn grow(xs: list<int>) -> int {\n  xs.push(9)\n  xs[0] = 0\n  return xs.len()\n}\nlabel start {\n  let a = [1, 2]\n  let n = grow(a)\n  ${n} ${a}\n  let m = grow(a)\n  ${m} ${a}\n}\n";
    assert_eq!(texts(src), ["3 [1, 2]", "3 [1, 2]", "<end>"]);
}

#[test]
fn list_returned_from_fn_is_independent_of_original() {
    let src = "save var base: list<int> = [1, 2]\nfn same() -> list<int> {\n  return base\n}\nlabel start {\n  let a = same()\n  a.push(3)\n  ${base} ${a}\n  base.push(5)\n  ${base} ${a}\n}\n";
    assert_eq!(
        texts(src),
        ["[1, 2] [1, 2, 3]", "[1, 2, 5] [1, 2, 3]", "<end>"]
    );
}

#[test]
fn updating_target_list_during_for_iterates_original_contents() {
    let src = "label start {\n  let xs = [1, 2, 3]\n  let seen: list<int> = []\n  for x in xs {\n    xs.push(x * 10)\n    xs[0] = 99\n    seen.push(x)\n  }\n  ${xs} ${seen}\n  for y in xs {\n    xs.clear()\n  }\n  ${xs}\n}\n";
    assert_eq!(
        texts(src),
        ["[99, 2, 3, 10, 20, 30] [1, 2, 3]", "[]", "<end>"]
    );
}

#[test]
fn nested_lists_are_independent_inside_too() {
    let src = "label start {\n  let g = [[1], [2, 3]]\n  let h = g\n  h[0].push(5)\n  h[1][0] = 7\n  ${g} ${h}\n  let row = g[1]\n  row.push(8)\n  ${g} ${row}\n  g[0] = row\n  row.clear()\n  ${g} ${h} ${row}\n}\n";
    assert_eq!(
        texts(src),
        [
            "[[1], [2, 3]] [[1, 5], [7, 3]]",
            "[[1], [2, 3]] [2, 3, 8]",
            "[[2, 3, 8], [2, 3]] [[1, 5], [7, 3]] []",
            "<end>"
        ]
    );
}

#[test]
fn list_fields_of_struct_are_independent_of_copies() {
    let src = "struct Bag { items: list<str>, n: int }\nlabel start {\n  let a = Bag { items: [\"x\"], n: 1 }\n  let b = a\n  b.items.push(\"y\")\n  b.n += 1\n  ${a.items} ${a.n} ${b.items} ${b.n}\n  let xs = a.items\n  xs.push(\"z\")\n  a.items[0] = \"w\"\n  ${a.items} ${xs}\n}\n";
    assert_eq!(texts(src), ["[x] 1 [x, y] 2", "[w] [x, z]", "<end>"]);
}

#[test]
fn list_values_in_map_are_independent_of_copies() {
    let src = "label start {\n  let m: map<str, list<int>> = map {}\n  m[\"a\"] = []\n  m[\"a\"].push(1)\n  let n = m\n  n[\"a\"].push(2)\n  n[\"b\"] = [9]\n  m[\"a\"][0] = 5\n  ${m} ${n}\n}\n";
    assert_eq!(
        texts(src),
        ["map { a: [5] } map { a: [1, 2], b: [9] }", "<end>"]
    );
}

#[test]
fn list_equality_and_contains_same_whether_shared_or_not() {
    let src = "label start {\n  let a = [1, 2]\n  let b = a\n  let c = [1, 2]\n  ${a == b} ${a == c}\n  b.push(3)\n  ${a == b} ${a == c} ${a.contains(2)} ${b.contains(3)}\n  let g = [a, c]\n  ${g.contains(c)}\n}\n";
    assert_eq!(
        texts(src),
        ["true true", "false true true true", "true", "<end>"]
    );
}

// ---------------------------------------------------------------------------
// Total size of variables (incremental)
// ---------------------------------------------------------------------------

#[test]
fn list_and_map_total_sizes_match_recount_after_updates() {
    let src = "struct S { xs: list<int>, m: map<int, list<int>> }\nsave var g: list<list<int>> = [[1], [2]]\nsave var s: S = S { xs: [1], m: map {} }\nlabel start {\n  g[0].push(3)\n  g[1][0] = 8\n  g.push([4, 5])\n  g[2].insert(1, 6)\n  g[2].remove(0)\n  s.xs.push(7)\n  s.m[1] = [1]\n  s.m[1].push(2)\n  s.m[2] = []\n  s.m[2].push(3)\n  s.m.remove(2)\n  s.m[1][0] = 9\n  g[0].clear()\n  g.remove(1)\n  let t = s\n  t.xs.push(1)\n  t.m[3] = [3]\n  一 #a\n  ${g} ${s.xs} ${s.m} ${t.m}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    assert!(vm_consistent(&vm), "{y:?}");
    assert_eq!(
        run_all(&mut vm),
        [
            "[[], [6, 5]] [1, 7] map { 1: [9, 2] } map { 1: [9, 2], 3: [3] }",
            "<end>"
        ]
    );
    assert!(vm_consistent(&vm));
}

#[test]
fn random_update_sequences_keep_total_size_consistent_and_do_not_affect_other_owners() {
    // Repeat pseudo-random updates to a list, a list of lists, a map and struct fields, and check that the copy (`snap`) does
    // not change and that the total size stays consistent
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let mut body = String::new();
    for n in 0..400 {
        let i = next(3);
        let v = next(100);
        let line = match next(12) {
            0 => format!("g.push([{v}])"),
            1 => format!("if g.len() > {i} {{\n    g[{i}].push({v})\n  }}"),
            2 => format!(
                "if g.len() > {i} {{\n    if g[{i}].len() > 0 {{\n      g[{i}][0] = {v}\n    }}\n  }}"
            ),
            3 => format!("if g.len() > {i} {{\n    g.remove({i})\n  }}"),
            4 => format!("s.m[{i}] = [{v}]"),
            5 => format!("if s.m.has({i}) {{\n    s.m[{i}].push({v})\n  }}"),
            6 => format!("s.m.remove({i})"),
            7 => format!("s.xs.push({v})"),
            8 => format!("if s.xs.len() > {i} {{\n    s.xs.remove({i})\n  }}"),
            9 => format!("if g.len() > {i} {{\n    g[{i}].clear()\n  }}"),
            10 => format!("if g.len() > {i} {{\n    g.insert({i}, [{v}, {v}])\n  }}"),
            _ => format!("let c{n} = s\n  c{n}.xs.push(1)\n  c{n}.m[9] = [9]"),
        };
        body.push_str("  ");
        body.push_str(&line);
        body.push('\n');
    }
    let src = format!(
        "struct S {{ xs: list<int>, m: map<int, list<int>> }}\nsave var g: list<list<int>> = [[1], [2]]\nsave var s: S = S {{ xs: [1], m: map {{}} }}\nlabel start {{\n  let snap = g\n  let snap_s = s\n  let before = \"${{snap}} ${{snap_s}}\"\n{body}  一 #a\n  ${{before == \"${{snap}} ${{snap_s}}\"}}\n}}\n"
    );
    let reg = registry();
    let prog = compile_ok(&src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    assert!(vm_consistent(&vm));
    assert_eq!(run_all(&mut vm), ["true", "<end>"]);
}

#[test]
fn list_push_over_value_limit_is_error_and_within_limit_is_in_place() {
    // Fine if 24 + 8 * n is within the limit (limit 24 + 8 * 4 = 56)
    let src = "label start {\n  let xs: list<int> = []\n  for i in range(0, 10) {\n    xs.push(i)\n  }\n  ${xs.len()}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let cfg = VmConfig::default().max_value_bytes(24 + 8 * 4);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
    // Additions inside nesting are checked against the size of the inner `list` (the updated `list`, not the outer one)
    let src = "label start {\n  let g = [[1, 2]]\n  g[0].push(3)\n  g[0].push(4)\n  ${g}\n  for i in range(0, 6) {\n    g[0].push(i)\n  }\n}\n";
    let prog = compile_ok(src);
    // The outer one is 24 + (24 + 8 * 4) = 80. The inner one exceeds at 8 elements (24 + 64 = 88)
    let cfg = VmConfig::default().max_value_bytes(80);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["[[1, 2, 3, 4]]", "<value_too_large>"]);
    assert!(vm_consistent(&vm));
}

#[test]
fn map_insert_over_value_size_limit_is_error() {
    let src = "label start {\n  let m: map<int, int> = map {}\n  for i in range(0, 10) {\n    m[i] = i\n  }\n  ${m.len()}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    // 16 bytes per pair. Up to 24 + 16 * 3
    let cfg = VmConfig::default().max_value_bytes(24 + 16 * 3);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
    let cfg = VmConfig::default().max_value_bytes(24 + 16 * 10);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["10", "<end>"]);
}

#[test]
fn total_memory_limit_counts_shared_lists_by_logical_size() {
    // Even if two variables hold the same `list`, each is counted (as estimated)
    let src = "label start {\n  let a = range(0, 100)\n  let b = a\n  ${b.len()}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let cfg = VmConfig::default().max_total_bytes(Some(24 + 8 * 100 + 100));
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["<memory_limit_exceeded>"]);
    let cfg = VmConfig::default().max_total_bytes(Some(2 * (24 + 8 * 100) + 100));
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(run_all(&mut vm), ["100", "<end>"]);
}

// ---------------------------------------------------------------------------
// Saving, restoring and snapshots
// ---------------------------------------------------------------------------

#[test]
fn list_restores_same_value_and_resaves_same_bytes() {
    let src = "save var g: list<list<int>> = [[1], [2]]\nsave var xs: list<str> = []\nlabel start {\n  g[0].push(3)\n  g.push([4])\n  xs.push(\"a\")\n  一 #a\n  ${g} ${xs}\n  g[1].push(9)\n  xs.push(\"b\")\n  ${g} ${xs}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(var(&vm2, "g"), var(&vm, "g"));
    assert!(vm_consistent(&vm2));
    assert_eq!(vm2.save().unwrap(), bytes);
    assert_eq!(
        run_all(&mut vm2),
        [
            "[[1, 3], [2], [4]] [a]",
            "[[1, 3], [2, 9], [4]] [a, b]",
            "<end>"
        ]
    );
    // Updates after restoring are also independent of the copy
    assert!(vm_consistent(&vm2));
}

#[test]
fn snapshot_restores_list_contents_unaffected_by_later_updates() {
    let src = "save var xs: list<int> = [1]\nlabel start {\n  xs.push(2)\n  一 #a\n  xs.push(3)\n  xs[0] = 100\n  二 #b\n  xs.push(4)\n  三 #c\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap_a = vm.snapshot().unwrap();
    let at_a = var(&vm, "xs");
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    let snap_b = vm.snapshot().unwrap();
    let at_b = var(&vm, "xs");
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    assert_eq!(var(&vm, "xs").to_text(), "[100, 2, 3, 4]");
    // It goes back to the contents at the time it was taken, and the snapshot itself does not change (it can be restored any
    // number of times)
    for _ in 0..2 {
        vm.restore(&snap_b).unwrap();
        assert_eq!(var(&vm, "xs"), at_b);
        assert!(vm_consistent(&vm));
        vm.resume(Input::Return(Value::Unit)).unwrap();
        vm.run().unwrap();
        assert_eq!(var(&vm, "xs").to_text(), "[100, 2, 3, 4]");
        vm.restore(&snap_a).unwrap();
        assert_eq!(var(&vm, "xs"), at_a);
        assert_eq!(at_a.to_text(), "[1, 2]");
        assert!(vm_consistent(&vm));
    }
}

#[test]
fn updating_list_with_pre_rollback_value_does_not_change_snapshot() {
    let src = "save var g: list<list<int>> = [[1]]\nlabel start {\n  一 #a\n  g[0].push(2)\n  g.push([3])\n  二 #b\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    assert_eq!(var(&vm, "g").to_text(), "[[1, 2], [3]]");
    vm.restore(&snap).unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    assert_eq!(var(&vm, "g").to_text(), "[[1, 2], [3]]");
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "g").to_text(), "[[1]]");
}

// ---------------------------------------------------------------------------
// Performance (no pass/fail on time; loose limits only check that it is not quadratic)
// ---------------------------------------------------------------------------

/// It must not take longer than this even in debug builds (the previous quadratic implementation took 4 seconds even in
/// release).
const LOOSE_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

#[test]
fn twenty_thousand_element_list_index_len_push_for_finish_within_loose_limit() {
    let src = "save var xs: list<int> = []\nlabel start {\n  for i in range(0, 20000) {\n    xs.push(i)\n  }\n  let t = 0\n  for i in range(0, 20000) {\n    t += xs[i] + xs.len()\n  }\n  for i in range(0, 20000) {\n    xs[i] = xs[i] * 2\n  }\n  let u = 0\n  for x in xs {\n    u += x\n  }\n  ${t} ${u} ${xs.len()} ${xs.contains(39998)}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let start = std::time::Instant::now();
    let out = run_all(&mut vm);
    assert!(
        start.elapsed() < LOOSE_LIMIT,
        "遅すぎる: {:?}",
        start.elapsed()
    );
    // t = 199990000 + 20000 * 20000, u = 2 * 199990000
    assert_eq!(out, ["599990000 399980000 20000 true", "<end>"]);
    assert!(vm_consistent(&vm));
}

#[test]
fn twenty_thousand_map_inserts_and_updates_finish_within_loose_limit() {
    let src = "save var m: map<int, int> = map {}\nlabel start {\n  for i in range(0, 20000) {\n    m[i] = i\n  }\n  for i in range(0, 20000) {\n    m[i] += 1\n  }\n  let t = 0\n  for i in range(0, 20000) {\n    t += m[i]\n  }\n  ${m.len()} ${t}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let start = std::time::Instant::now();
    let out = run_all(&mut vm);
    assert!(
        start.elapsed() < LOOSE_LIMIT,
        "遅すぎる: {:?}",
        start.elapsed()
    );
    assert_eq!(out, ["20000 200010000", "<end>"]);
    assert!(vm_consistent(&vm));
}

#[test]
fn reading_shared_list_twenty_thousand_times_does_not_copy() {
    // Even after read-only operations (index, len, contains, for, `${}`), the variable's `list` still shares its contents with
    // the copy taken
    let src = "save var xs: list<int> = []\nlabel start {\n  for i in range(0, 1000) {\n    xs.push(i)\n  }\n  let t = 0\n  for i in range(0, 1000) {\n    t += xs[i] + xs.len()\n  }\n  一 #a\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let (Value::List(a), Value::List(b)) = (var(&vm, "xs"), var(&vm, "xs")) else {
        panic!("list のはず")
    };
    assert!(a.shares_storage_with(&b));
}

// ---------------------------------------------------------------------------
// Random number extensions (`random_float`, `shuffle`)
// ---------------------------------------------------------------------------

/// The bodies when run to the end with the given seed.
fn texts_seeded(src: &str, seed: u64) -> Vec<String> {
    util::texts_with(
        src,
        &registry(),
        VmConfig {
            seed,
            ..VmConfig::default()
        },
        5000,
    )
}

#[test]
fn random_float_is_in_0_to_1_and_same_seed_gives_same_sequence() {
    let src = "label start {\n  let i = 0\n  let ok = true\n  let sum = 0.0\n  while i < 1000 {\n    let x = random_float()\n    if x < 0.0 || x >= 1.0 {\n      ok = false\n    }\n    sum += x\n    i += 1\n  }\n  ${ok} ${sum > 400.0 && sum < 600.0}\n  ${random_float()}\n}\n";
    let a = texts_seeded(src, 7);
    assert_eq!(a[0], "true true");
    assert_eq!(a, texts_seeded(src, 7));
    assert_ne!(a[1], texts_seeded(src, 8)[1]);
}

#[test]
fn shuffle_only_reorders_and_same_seed_gives_same_order() {
    let src = "label start {\n  let xs = range(0, 20)\n  let ys = xs\n  xs.shuffle()\n  let sorted = true\n  let i = 0\n  while i < 20 {\n    if !xs.contains(i) {\n      sorted = false\n    }\n    i += 1\n  }\n  ${xs.len()} ${sorted} ${ys == range(0, 20)} ${xs != ys}\n  ${xs}\n}\n";
    let a = texts_seeded(src, 1);
    // The length is the same, all elements are present, and the original value (`ys`) does not change
    assert_eq!(a[0], "20 true true true");
    assert_eq!(a, texts_seeded(src, 1));
    assert_ne!(a[1], texts_seeded(src, 2)[1]);
}

#[test]
fn shuffle_works_on_nested_places_and_in_fn_and_keeps_empty_and_single() {
    let src = "struct Deck { cards: list<str> }\nfn shuffled(xs: list<int>) -> list<int> {\n  let ys = xs\n  ys.shuffle()\n  return ys\n}\nlabel start {\n  let d = Deck { cards: [\"a\"] }\n  d.cards.shuffle()\n  let e: list<int> = []\n  e.shuffle()\n  let g = [[1, 2, 3]]\n  g[0].shuffle()\n  ${d.cards} ${e} ${g[0].len()} ${shuffled([5]).len()}\n}\n";
    assert_eq!(texts(src), ["[a] [] 3 1", "<end>"]);
}

#[test]
fn shuffle_unavailable_in_expressions_and_on_map() {
    let codes = util::error_codes(
        "label start {\n  let xs = [1]\n  let n = xs.shuffle()\n}\n",
        &registry(),
    );
    assert_eq!(codes, ["statement_method_in_expression"]);
    let codes = util::error_codes(
        "label start {\n  let m: map<str, int> = map {}\n  m.shuffle()\n}\n",
        &registry(),
    );
    assert_eq!(codes, ["unknown_method"]);
}

#[test]
fn shuffle_and_random_float_go_back_with_rollback() {
    let src = "label start {\n  let xs = range(0, 10)\n  一\n  xs.shuffle()\n  二 ${xs} ${random_float()}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一");
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    let first = text_of(&vm.run().unwrap());
    vm.restore(&snap).unwrap();
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), first);
}
