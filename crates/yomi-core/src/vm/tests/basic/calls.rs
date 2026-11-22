//! call, return and modules

use super::*;

use crate::compile::compile_modules;

pub(super) fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

/// Advances to the next stop point and returns the body (the first argument).
pub(super) fn next_text(vm: &mut Vm<'_>) -> Option<String> {
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => Some(args[0].to_text()),
        Yield::End => None,
    }
}

pub(super) const CALL_SRC: &str = "label start {\n  開始 #s\n  call a(1)\n  戻った #back\n}\n\nlabel a(n: int) {\n  a入口 ${n} #a1\n  call b(n + 1)\n  a出口 ${n} #a2\n}\n\nlabel b(n: int) {\n  b ${n} #b1\n}\n";

#[test]
fn call_stack_depth_grows_with_call_and_shrinks_on_return() {
    let reg = registry();
    let prog = compile_ok(CALL_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut seen = Vec::new();
    while let Some(t) = next_text(&mut vm) {
        seen.push((t, vm.call_depth(), vm.label_name().to_string()));
        vm.resume(Input::Return(Value::Unit)).unwrap();
    }
    assert_eq!(
        seen,
        vec![
            ("開始".to_string(), 0, "start".to_string()),
            ("a入口 1".to_string(), 1, "a".to_string()),
            ("b 2".to_string(), 2, "b".to_string()),
            ("a出口 1".to_string(), 1, "a".to_string()),
            ("戻った".to_string(), 0, "start".to_string()),
        ]
    );
    assert_eq!(vm.call_depth(), 0);
}

#[test]
fn save_and_restore_mid_call_keep_return_point_and_continuation() {
    let reg = registry();
    let prog = compile_ok(CALL_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        next_text(&mut vm).unwrap();
        vm.resume(Input::Return(Value::Unit)).unwrap();
    }
    // Now at a stop point inside b (depth 2)
    assert_eq!(next_text(&mut vm).as_deref(), Some("a出口 1"));
    // Save inside a (depth 1)
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    // The stop point is found by ID, and the call line by hash
    assert_eq!(report.resolved_by, ResolvedBy::Hash);
    assert_eq!(vm2.call_depth(), 1);
    assert_eq!(vm2.current(), vm.current());
    assert_eq!(vm2.save().unwrap(), bytes);
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm2).as_deref(), Some("戻った"));
    assert_eq!(vm2.call_depth(), 0);
}

#[test]
fn saving_and_restoring_mid_deep_call_restores_all_frames() {
    let reg = registry();
    let src = "label start {\n  call f(3)\n  おわり #end\n}\n\nlabel f(n: int) {\n  if n > 0 {\n    call f(n - 1)\n  } else {\n    底 #bottom\n  }\n  戻り ${n} #r\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("底"));
    assert_eq!(vm.call_depth(), 4);
    let (mut vm2, report) =
        Vm::load(&prog, &reg, VmConfig::default(), &vm.save().unwrap()).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Hash);
    assert_eq!(vm2.call_depth(), 4);
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    let mut rest = Vec::new();
    while let Some(t) = next_text(&mut vm2) {
        rest.push(t);
        vm2.resume(Input::Return(Value::Unit)).unwrap();
    }
    assert_eq!(rest, vec!["戻り 0", "戻り 1", "戻り 2", "戻り 3", "おわり"]);
}

#[test]
fn return_point_is_found_by_ordinal_even_if_caller_line_changed() {
    let reg = registry();
    let prog = compile_ok(CALL_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        next_text(&mut vm).unwrap();
        vm.resume(Input::Return(Value::Unit)).unwrap();
    }
    next_text(&mut vm).unwrap();
    let bytes = vm.save().unwrap();
    // The arguments of `call a(1)` in `start` changed (the hash changes)
    let edited = compile_ok(&CALL_SRC.replace("call a(1)", "call a(2)"));
    let (mut vm2, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Ordinal);
    assert_eq!(vm2.call_depth(), 1);
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm2).as_deref(), Some("戻った"));
}

#[test]
fn save_whose_call_line_was_removed_drops_call_stack_and_returns_to_label_start() {
    let reg = registry();
    let prog = compile_ok(CALL_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        next_text(&mut vm).unwrap();
        vm.resume(Input::Return(Value::Unit)).unwrap();
    }
    next_text(&mut vm).unwrap();
    let bytes = vm.save().unwrap();
    // `call a(1)` disappeared from `start`
    let edited = compile_ok(&CALL_SRC.replace("  call a(1)\n", ""));
    let (mut vm2, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm2.call_depth(), 0);
    assert!(!vm2.is_stopped());
    // From the start of the label saved in (a), with default arguments
    assert_eq!(vm2.label_name(), "a");
    assert_eq!(next_text(&mut vm2).as_deref(), Some("a入口 0"));
    // There is no caller, so it ends at the end of `a`
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm2).as_deref(), Some("b 1"));
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm2).as_deref(), Some("a出口 0"));
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(vm2.run().unwrap(), Yield::End);
}

#[test]
fn save_with_mismatched_frame_locals_drops_call_stack_and_returns_to_label_start() {
    let reg = registry();
    let src =
        "label start {\n  let k = 5\n  call a()\n  戻った ${k}\n}\n\nlabel a() {\n  a #a1\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    next_text(&mut vm).unwrap();
    let bytes = vm.save().unwrap();
    // The caller's local variables increased
    let edited = compile_ok(&src.replace("  let k = 5\n", "  let j = 1\n  let k = 5\n"));
    let (vm2, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm2.call_depth(), 0);
    // The caller's types changed
    let edited = compile_ok(&src.replace("let k = 5", "let k = \"s\""));
    let (vm3, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm3.call_depth(), 0);
    // If nothing changed, it goes back with the frames. Local variables go back too
    let (mut vm4, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    vm4.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm4).as_deref(), Some("戻った 5"));
}

#[test]
fn save_whose_call_target_differs_from_running_label_returns_to_label_start() {
    let reg = registry();
    // `a` returns no value and moves to a scene with `jump`. Save inside the scene
    let src = "label start {\n  call a(1)\n  戻った\n}\n\nlabel a(n: int) {\n  jump scene\n}\n\nlabel scene {\n  場面 #sc\n}\n\nlabel c(n: int) {\n}\n\nlabel d() -> int {\n  return 1\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("場面"));
    let bytes = vm.save().unwrap();
    // The call line changed to a call of another label that receives a value (the hash differs, so it is found by ordinal)
    let edited = compile_ok(&src.replace("  call a(1)\n", "  let r = call d()\n"));
    let (vm2, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm2.call_depth(), 0);
    // Inside a label that can only be entered with `call`, but the call line now calls another label
    let src = "label start {\n  call a(1)\n  戻った\n}\n\nlabel a(n: int) {\n  a #a1\n}\n\nlabel c(n: int) {\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("a"));
    let bytes = vm.save().unwrap();
    let edited = compile_ok(&src.replace("call a(1)", "call c(1)"));
    let (vm3, report) = Vm::load(&edited, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm3.call_depth(), 0);
}

#[test]
fn saving_at_stop_point_after_jump_finds_return_point_from_call_line() {
    let reg = registry();
    let src = "label start {\n  call a()\n  戻った\n}\n\nlabel a() {\n  a\n  jump scene\n}\n\nlabel scene {\n  場面 #sc\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("a"));
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("場面"));
    assert_eq!(vm.call_depth(), 1);
    let (mut vm2, report) =
        Vm::load(&prog, &reg, VmConfig::default(), &vm.save().unwrap()).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Hash);
    assert_eq!(vm2.label_name(), "scene");
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(next_text(&mut vm2).as_deref(), Some("戻った"));
}

#[test]
fn exceeding_call_depth_limit_is_runtime_error() {
    let reg = registry();
    let prog =
        compile_ok("label start {\n  call f(0)\n}\n\nlabel f(n: int) {\n  call f(n + 1)\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_call_depth(10)).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::CallDepthExceeded);
    assert_eq!(e.label.as_deref(), Some("f"));
    assert_eq!(vm.call_depth(), 10);
    // It runs up to exactly the limit depth
    let ok = compile_ok(
        "label start {\n  call f(0)\n  おわり\n}\n\nlabel f(n: int) {\n  if n < 9 {\n    call f(n + 1)\n  }\n}\n",
    );
    let mut vm = Vm::new(&ok, &reg, VmConfig::default().max_call_depth(10)).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("おわり"));
    // The default is 256
    assert_eq!(VmConfig::default().max_call_depth, 256);
}

#[test]
fn recursive_call_does_not_use_rust_stack() {
    // Even recursion at depth 100000 does not overflow the stack and stops at the limit
    let reg = registry();
    let prog =
        compile_ok("label start {\n  call f(0)\n}\n\nlabel f(n: int) {\n  call f(n + 1)\n}\n");
    let cfg = VmConfig::default()
        .max_call_depth(100_000)
        .max_steps_per_run(None);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::CallDepthExceeded);
    // It also stops at the instruction limit
    let mut vm = Vm::new(
        &prog,
        &reg,
        VmConfig::default()
            .max_call_depth(u32::MAX)
            .max_steps_per_run(Some(10_000)),
    )
    .unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::StepLimitExceeded);
}

#[test]
fn end_discards_call_stack_and_ends() {
    let reg = registry();
    let prog = compile_ok("label start {\n  call a()\n  来ない\n}\n\nlabel a() {\n  end\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.run().unwrap(), Yield::End);
    assert_eq!(vm.call_depth(), 0);
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Ended);
}

#[test]
fn return_without_caller_is_same_as_end() {
    let reg = registry();
    let prog = compile_ok("label start {\n  return\n  来ない\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.run().unwrap(), Yield::End);
    // Even when starting from a label with a return type, it ends if there is no caller
    let prog = compile_ok("label start() -> int {\n  return 1\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.run().unwrap(), Yield::End);
}

#[test]
fn return_values_can_be_used_in_caller_variables_and_arguments() {
    let src = "save var g: int = 0\nlabel start {\n  let a = call twice(3)\n  g = call twice(a)\n  call twice(g)\n  ${a} ${g}\n}\n\nlabel twice(x: int) -> int {\n  return x * 2\n}\n";
    assert_eq!(texts(src), vec!["6 12"]);
}

#[test]
fn can_start_with_module_given() {
    let reg = registry();
    let prog = compile_modules(
        &[
            ("main", "label start {\n  main\n}\n"),
            (
                "lib",
                "label start {\n  lib\n}\nlabel other {\n  lib other\n}\n",
            ),
        ],
        &reg,
    )
    .unwrap();
    let run = |start: &str| {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default().start(start)).unwrap();
        let t = next_text(&mut vm);
        (t, vm.module_name().to_string())
    };
    assert_eq!(run("main.start"), (Some("main".into()), "main".into()));
    assert_eq!(run("lib.start"), (Some("lib".into()), "lib".into()));
    assert_eq!(run("lib.other"), (Some("lib other".into()), "lib".into()));
    // With multiple modules, names without a module are not allowed
    for bad in ["start", "nothing.start", "lib.nothing", "lib."] {
        let e = Vm::new(&prog, &reg, VmConfig::default().start(bad)).unwrap_err();
        assert_eq!(
            e.kind,
            VmErrorKind::StartLabelNotFound(bad.to_string()),
            "{bad}"
        );
    }
    // With one module, just the label name is fine
    let single = compile_ok("label start {\n  あ\n}\nlabel x {\n}\n");
    assert!(Vm::new(&single, &reg, VmConfig::default().start("x")).is_ok());
    assert!(Vm::new(&single, &reg, VmConfig::default().start("main.x")).is_ok());
}

#[test]
fn cross_module_variables_are_saved_with_module_names() {
    let reg = registry();
    let sources = [
        (
            "main",
            "import lib\nsave var n: int = 1\nlabel start {\n  lib.n += 5\n  ${n} ${lib.n} #p\n}\n",
        ),
        ("lib", "save var n: int = 10\nlabel x {\n}\n"),
    ];
    let prog = compile_modules(&sources, &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    assert_eq!(next_text(&mut vm).as_deref(), Some("1 15"));
    let vars: Vec<_> = vm
        .variables()
        .into_iter()
        .map(|v| (v.module, v.name, v.value))
        .collect();
    assert_eq!(
        vars,
        vec![
            (Some("main".to_string()), "n".to_string(), Value::Int(1)),
            (Some("lib".to_string()), "n".to_string(), Value::Int(15)),
        ]
    );
    let bytes = vm.save().unwrap();
    let (vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(vm2.variables(), vm.variables());
    assert!(report.added_vars.is_empty());
    // A save with more variables in lib is reported with names
    let more = compile_modules(
        &[
            sources[0],
            (
                "lib",
                "save var n: int = 10\nsave var m: int = 3\nlabel x {\n}\n",
            ),
        ],
        &reg,
    )
    .unwrap();
    let (vm3, report) = Vm::load(&more, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.added_vars, vec!["lib.m"]);
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(vm3.variables()[2].value, Value::Int(3));
}

#[test]
fn save_with_missing_saved_module_is_module_mismatch_error() {
    let reg = registry();
    let a = compile("label start {\n  あ #p\n}\n", "a", &reg).unwrap();
    let mut vm = Vm::new(&a, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let b = compile("label start {\n  あ #p\n}\n", "b", &reg).unwrap();
    let e = Vm::load(&b, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::ModuleMismatch("a".to_string()));
    // If the module exists but the label does not, a label error
    let c = compile("label other {\n  あ #p\n}\n", "a", &reg).unwrap();
    let e = Vm::load(&c, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::LabelNotFound("start".to_string()));
}

#[test]
fn saves_of_versions_1_to_11_cannot_be_read() {
    let reg = registry();
    let prog = compile_ok("label start {\n  あ\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let mut bytes = vm.save().unwrap();
    assert_eq!(&bytes[4..8], &12u32.to_le_bytes());
    for old in 1u32..=11 {
        bytes[4..8].copy_from_slice(&old.to_le_bytes());
        let n = bytes.len() - 4;
        let crc = crate::vm::save::crc32(&bytes[..n]);
        bytes[n..].copy_from_slice(&crc.to_le_bytes());
        let e = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap_err();
        assert_eq!(e.kind, LoadErrorKind::UnsupportedVersion(old));
    }
}
