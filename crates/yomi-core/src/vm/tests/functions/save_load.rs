//! Save, load and rollback

use super::*;

#[test]
fn adding_fn_or_changing_body_does_not_break_save_position_resolution() {
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    let old = "save var s: int = 0\nfn bonus(x: int) -> int {\n  return x + 1\n}\nlabel start {\n  s = bonus(1)\n  一 ${s} #a\n  s = bonus(s)\n  二 ${s} #b\n  三 #c\n}\n";
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一 2");
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), "二 3");
    let bytes = vm.save().unwrap();
    for new in [
        // Fixed the body
        "save var s: int = 0\nfn bonus(x: int) -> int {\n  return x + 10\n}\nlabel start {\n  s = bonus(1)\n  一 ${s} #a\n  s = bonus(s)\n  二 ${s} #b\n  三 #c\n}\n",
        // Added a `fn` (both at the start and after the label)
        "fn extra() -> int {\n  return 0\n}\nsave var s: int = 0\nfn bonus(x: int) -> int {\n  return x + 1\n}\nfn more() {\n}\nlabel start {\n  s = bonus(1)\n  一 ${s} #a\n  s = bonus(s)\n  二 ${s} #b\n  三 #c\n}\nfn tail() {\n}\n",
        // Removed the `fn` and wrote it directly
        "save var s: int = 0\nlabel start {\n  s = 2\n  一 ${s} #a\n  s += 1\n  二 ${s} #b\n  三 #c\n}\n",
    ] {
        let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
        let (mut vm2, report) = Vm::load(new_prog, reg, VmConfig::default(), &bytes).unwrap();
        assert_eq!(report.resolved_by, ResolvedBy::Id, "{new}");
        assert!(report.added_vars.is_empty() && report.removed_vars.is_empty());
        assert!(vm2.is_stopped());
        // The values of saved variables stay as saved, regardless of the `fn` body
        assert_eq!(var(&vm2, "s"), Value::Int(3));
        assert_eq!(run_all(&mut vm2).last().map(String::as_str), Some("<end>"));
    }
}

#[test]
fn fn_locals_are_not_saved_and_fn_frames_are_empty_at_stop_points() {
    let src = "fn f(x: int) -> int {\n  let t = x * 2\n  return t\n}\nsave var v: int = 0\nlabel start {\n  v = f(4)\n  一 #a\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    // At a stop point there are no `fn` frames (the list of variables does not show `fn` local variables either)
    assert!(vm.fn_frames.is_empty() && vm.cur_fn.is_none());
    let names: Vec<_> = vm.variables().into_iter().map(|v| v.name).collect();
    assert_eq!(names, ["v"]);
    let bytes = vm.save().unwrap();
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(var(&vm2, "v"), Value::Int(8));
    // Adding `fn` does not change the save format version
    assert_eq!(&bytes[4..8], &crate::vm::save::FORMAT_VERSION.to_le_bytes());
}

#[test]
fn fn_call_results_continue_identically_after_rollback() {
    let src = "fn bump(x: int) -> int {\n  return x + random(1, 100)\n}\nsave var v: int = 0\nlabel start {\n  v = bump(v)\n  一 #a\n  v = bump(v)\n  二 #b\n  v = bump(v)\n  三 ${v} #c\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(3)).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    let first = run_all(&mut vm);
    vm.restore(&snap).unwrap();
    let again = run_all(&mut vm);
    assert_eq!(first, again);
}

#[test]
fn can_save_after_calling_fn_from_label_mid_call() {
    let src = "fn twice(x: int) -> int {\n  return x * 2\n}\nlabel start {\n  let r = call sub(3)\n  結果 ${r} #r\n}\nlabel sub(n: int) -> int {\n  一 ${twice(n)} #a\n  return twice(n) + 1\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一 6");
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    // Call lines have no stable ID, so they are resolved by hash
    assert_eq!(report.resolved_by, ResolvedBy::Hash);
    assert_eq!(run_all(&mut vm2), ["結果 7", "<end>"]);
}
