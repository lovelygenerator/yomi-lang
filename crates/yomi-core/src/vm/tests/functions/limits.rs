//! Limits

use super::*;

#[test]
fn call_depth_exceeded_at_recursion_depth_limit() {
    let f = "fn deep(n: int) -> int {\n  return deep(n + 1)\n}\n";
    let t = texts(&format!("{f}label start {{\n  ${{deep(0)}}\n}}\n"));
    assert_eq!(t, ["<call_depth_exceeded>"]);
    // It runs if the depth is exactly at the limit
    let f =
        "fn down(n: int) -> int {\n  if n == 0 {\n    return 0\n  }\n  return 1 + down(n - 1)\n}\n";
    let src = format!("{f}label start {{\n  ${{down(254)}}\n}}\n");
    assert_eq!(texts(&src), ["254", "<end>"]);
    // The limit can be changed with `max_call_depth`
    let src = format!("{f}label start {{\n  ${{down(10)}}\n}}\n");
    assert_eq!(
        texts_with(&src, VmConfig::default().max_call_depth(5)),
        ["<call_depth_exceeded>"]
    );
    assert_eq!(
        texts_with(&src, VmConfig::default().max_call_depth(11)),
        ["10", "<end>"]
    );
}

#[test]
fn call_depth_counts_label_calls_and_fns_together() {
    // Call `fn` 3 levels on top of 2 levels of label `call` (5 in total)
    let src = "fn down(n: int) -> int {\n  if n == 0 {\n    return 0\n  }\n  return 1 + down(n - 1)\n}\nlabel start {\n  call a()\n}\nlabel a {\n  call b()\n}\nlabel b {\n  ${down(2)}\n}\n";
    // Call depth: a (frame 1), b (frame 2), down(2), down(1), down(0) (3 levels of fn)
    assert_eq!(
        texts_with(src, VmConfig::default().max_call_depth(5)),
        ["2", "<end>"]
    );
    assert_eq!(
        texts_with(src, VmConfig::default().max_call_depth(4)),
        ["<call_depth_exceeded>"]
    );
}

#[test]
fn recursion_at_full_depth_limit_runs_on_1_mib_stack() {
    crate::tests::on_small_stack(|| {
        let f = "fn down(n: int) -> int {\n  if n == 0 {\n    return 0\n  }\n  return 1 + down(n - 1)\n}\nfn deep(n: int) -> int {\n  return deep(n + 1)\n}\n";
        // Exactly at the limit (256), and over it
        assert_eq!(
            texts(&format!("{f}label start {{\n  ${{down(255)}}\n}}\n")),
            ["255", "<end>"]
        );
        assert_eq!(
            texts(&format!("{f}label start {{\n  ${{deep(0)}}\n}}\n")),
            ["<call_depth_exceeded>"]
        );
        // Even with a larger limit, the host's stack does not overflow (it runs on an explicit stack)
        assert_eq!(
            texts_with(
                &format!("{f}label start {{\n  ${{down(20000)}}\n}}\n"),
                VmConfig::default().max_call_depth(30_000)
            ),
            ["20000", "<end>"]
        );
        // Deeply nested expressions, and nested arguments
        let mut nest = String::from("1");
        for _ in 0..20 {
            nest = format!("down(1 + {nest})");
        }
        let t = texts(&format!("{f}label start {{\n  ${{{nest}}}\n}}\n"));
        assert_eq!(t.last().map(String::as_str), Some("<end>"), "{t:?}");
    });
}

#[test]
fn instruction_limit_counts_instructions_inside_fn() {
    let f = "fn spin() -> int {\n  let i = 0\n  while true {\n    i += 1\n  }\n  return i\n}\nfn work(n: int) -> int {\n  let t = 0\n  for i in range(0, n) {\n    t += i\n  }\n  return t\n}\n";
    // A loop that never ends
    let t = texts_with(
        &format!("{f}label start {{\n  ${{spin()}}\n}}\n"),
        VmConfig::default().max_steps_per_run(Some(10_000)),
    );
    assert_eq!(t, ["<step_limit_exceeded>"]);
    // Even if the calling label has few instructions, the `fn`'s instructions reach the limit
    let src = format!("{f}label start {{\n  ${{work(1000)}}\n}}\n");
    assert_eq!(
        texts_with(&src, VmConfig::default().max_steps_per_run(Some(500))),
        ["<step_limit_exceeded>"]
    );
    assert_eq!(
        texts_with(&src, VmConfig::default().max_steps_per_run(Some(100_000))),
        ["499500", "<end>"]
    );
    // Recounted for each `run`
    let src = format!("{f}label start {{\n  ${{work(100)}} 一 #a\n  ${{work(100)}} 二 #b\n}}\n");
    let t = texts_with(&src, VmConfig::default().max_steps_per_run(Some(2500)));
    assert_eq!(t, ["4950 一", "4950 二", "<end>"]);
}

#[test]
fn default_instruction_limit_is_50_million() {
    assert_eq!(VmConfig::default().max_steps_per_run, Some(50_000_000));
}

#[test]
fn non_terminating_recursive_fn_stops_at_instruction_or_depth_limit() {
    // With a larger depth limit, it stops at the instruction limit
    let f = "fn deep(n: int) -> int {\n  return deep(n + 1)\n}\n";
    let t = texts_with(
        &format!("{f}label start {{\n  ${{deep(0)}}\n}}\n"),
        VmConfig::default()
            .max_call_depth(u32::MAX)
            .max_steps_per_run(Some(100_000))
            .max_total_bytes(None),
    );
    assert_eq!(t, ["<step_limit_exceeded>"]);
}

#[test]
fn memory_limit_exceeded_at_total_variable_size_limit() {
    // Module variables
    let src = "save var a: list<int> = []\nlabel start {\n  let i = 0\n  while true {\n    a.push(i)\n    i += 1\n  }\n}\n";
    let t = texts_with(src, VmConfig::default().max_total_bytes(Some(10_000)));
    assert_eq!(t, ["<memory_limit_exceeded>"]);
    // Stops at the total limit even if the limit for one value (`value_too_large`) is not reached
    let src = "save var a: str = \"\"\nsave var b: str = \"\"\nsave var c: str = \"\"\nlabel start {\n  let s = \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"\n  a = s\n  b = s\n  c = s\n  ここ\n}\n";
    let t = texts_with(
        src,
        VmConfig::default()
            .max_value_bytes(200)
            .max_total_bytes(Some(400)),
    );
    assert_eq!(t, ["<memory_limit_exceeded>"]);
    // It runs if the limit is large enough
    let t = texts_with(
        src,
        VmConfig::default()
            .max_value_bytes(200)
            .max_total_bytes(Some(4000)),
    );
    assert_eq!(t, ["ここ", "<end>"]);
    // `None` means no limit
    let t = texts_with(
        src,
        VmConfig::default()
            .max_value_bytes(200)
            .max_total_bytes(None),
    );
    assert_eq!(t, ["ここ", "<end>"]);
    // The default is 64 MiB
    assert_eq!(VmConfig::default().max_total_bytes, Some(64 * 1024 * 1024));
}

#[test]
fn total_limit_counts_locals_and_fn_frames() {
    // Local variables
    let src = "label start {\n  let a: list<int> = []\n  let i = 0\n  while true {\n    a.push(i)\n    i += 1\n  }\n}\n";
    assert_eq!(
        texts_with(src, VmConfig::default().max_total_bytes(Some(5_000))),
        ["<memory_limit_exceeded>"]
    );
    // Push an argument with a large value on each recursion (each frame fits the limit for small values, but the total
    // exceeds the limit)
    let src = "fn grow(xs: list<int>, n: int) -> int {\n  if n == 0 {\n    return xs.len()\n  }\n  return grow(xs, n - 1)\n}\nlabel start {\n  let xs = range(0, 100)\n  ${grow(xs, 200)}\n}\n";
    // About 824 bytes per frame (100-element list = 24 + 800). About 165 KB at 200 levels
    assert_eq!(
        texts_with(src, VmConfig::default().max_total_bytes(Some(50_000))),
        ["<memory_limit_exceeded>"]
    );
    assert_eq!(
        texts_with(src, VmConfig::default().max_total_bytes(Some(500_000))),
        ["100", "<end>"]
    );
    // On return, the frame's share decreases (no matter how many times recursion repeats, it does not pile up)
    let src = "fn grow(xs: list<int>, n: int) -> int {\n  if n == 0 {\n    return xs.len()\n  }\n  return grow(xs, n - 1)\n}\nlabel start {\n  let xs = range(0, 100)\n  let t = 0\n  for i in range(0, 50) {\n    t += grow(xs, 20)\n  }\n  ${t}\n}\n";
    assert_eq!(
        texts_with(src, VmConfig::default().max_total_bytes(Some(40_000))),
        ["5000", "<end>"]
    );
}

#[test]
fn total_record_stays_correct_through_assignments_updates_for_and_label_transitions() {
    // On every run, check at the start of `run` that the record matches a recount (`debug_assert` in debug builds).
    // Here, various updates are done across stop points.
    let src = "struct P { items: list<str>, m: map<str, int> }\nsave var p: P = P { items: [], m: map {} }\nsave var big: list<str> = []\nfn make(n: int) -> list<str> {\n  let out: list<str> = []\n  for i in range(0, n) {\n    out.push(\"要素${i}\")\n  }\n  return out\n}\nlabel start {\n  let local = make(5)\n  p.items.push(\"a\")\n  p.m[\"k\"] = 1\n  p.m[\"j\"] = 2\n  p.m[\"k\"] += 5\n  一 #a\n  p.items = make(3)\n  p.items.remove(1)\n  p.m.remove(\"k\")\n  big = make(10)\n  二 #b\n  for s in local {\n    big.push(s)\n  }\n  p.m.clear()\n  p.items.clear()\n  三 #c\n  let r = call sub(make(4))\n  ${r} #d\n  jump other\n}\nlabel sub(xs: list<str>) -> int {\n  let ys = xs\n  ys.push(\"z\")\n  停 #s\n  return ys.len()\n}\nlabel other {\n  big = []\n  終 #e\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut snaps = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::End => break,
            _ => {
                assert_eq!(vm.total, vm.recount_total());
                snaps.push(vm.snapshot().unwrap());
                resume(&mut vm);
            }
        }
    }
    // Restore and do it again (after restoring, recount)
    for s in &snaps {
        vm.restore(s).unwrap();
        let t = run_all(&mut vm);
        assert_eq!(t.last().map(String::as_str), Some("<end>"));
    }
}

#[test]
fn value_size_limit_applies_to_fn_return_values() {
    let f = "fn big() -> str {\n  let s = \"xxxxxxxxxxxxxxxxxxxx\"\n  return \"${s}${s}${s}${s}${s}\"\n}\n";
    let t = texts_with(
        &format!("{f}label start {{\n  ${{big()}}\n}}\n"),
        VmConfig::default().max_value_bytes(99),
    );
    assert_eq!(t, ["<value_too_large>"]);
}

#[test]
fn runtime_error_location_is_fn_name_and_line_in_fn() {
    let src =
        "fn bad(x: int) -> int {\n  let a = 1\n  return a / x\n}\nlabel start {\n  ${bad(0)}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
    assert_eq!(e.label.as_deref(), Some("bad"));
    assert_eq!(e.line, Some(3));
    assert_eq!(e.module.as_deref(), Some("main"));
}

#[test]
fn after_runtime_error_can_go_back_to_snapshot_and_continue() {
    let src = "fn bad(x: int) -> int {\n  return 10 / x\n}\nsave var z: int = 0\nlabel start {\n  一 #a\n  z = bad(z)\n  二 #b\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
    // The state stopped inside a `fn` disappears when restored
    vm.restore(&snap).unwrap();
    assert!(vm.fn_frames.is_empty() && vm.cur_fn.is_none());
    vm.set_var("none.x", Value::Int(1)).unwrap_err();
    resume(&mut vm);
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::DivisionByZero);
}

#[test]
fn load_returns_to_label_start_when_stop_args_fail_inside_fn() {
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    // When saved, it does not divide by 0, but the script changed and the `fn` in the arguments divides by 0
    let old = "fn q(x: int) -> int {\n  return 10 / (x + 1)\n}\nsave var d: int = 0\nlabel start {\n  一 ${q(d)} #a\n}\n";
    let new = "fn q(x: int) -> int {\n  return 10 / (x + 0)\n}\nsave var d: int = 0\nlabel start {\n  一 ${q(d)} #a\n}\n";
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一 10");
    let bytes = vm.save().unwrap();
    let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
    let (mut vm2, report) = Vm::load(new_prog, reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert!(!vm2.is_stopped());
    // Running from the start gives the same error
    assert_eq!(vm2.run().unwrap_err().kind, VmErrorKind::DivisionByZero);
}
