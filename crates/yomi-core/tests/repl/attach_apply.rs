//! attach and apply

use super::*;

pub(super) fn stopped_at<'a>(prog: &'a Program, reg: &'a Registry, label: &str) -> Vm<'a> {
    let mut vm = Vm::new(prog, reg, config().start(label)).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm
}

#[test]
fn attach_runs_on_copy_of_stopped_vm_and_leaves_vm() {
    let reg = registry();
    let prog = program(&reg);
    let vm = stopped_at(&prog, &reg, "talk");
    let before = (vm.save().unwrap(), vm.current().cloned(), vm.variables());
    let mut repl = Repl::attach(&vm).unwrap();
    // The local variables of the stop point are visible
    assert_eq!(eval(&mut repl, "gold"), "100");
    exec(&mut repl, "gold += 5");
    exec(&mut repl, "favor += 1");
    exec(&mut repl, "best = 9");
    exec(&mut repl, "@counter.inc(4)");
    exec(&mut repl, "let x = random(1, 100)");
    repl.submit("call greet(1)").unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Stop { .. }));
    repl.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Done { .. }));
    assert_eq!(eval(&mut repl, "gold"), "105");
    assert_eq!(eval(&mut repl, "favor"), "4");
    // The script does not change at all
    assert_eq!(
        (vm.save().unwrap(), vm.current().cloned(), vm.variables()),
        before
    );
    assert_eq!(
        vm.save_persistent(),
        stopped_at(&prog, &reg, "talk").save_persistent()
    );
    drop(repl);
    // The continuation is also the same as without using the REPL
    let mut a = vm;
    let mut b = stopped_at(&prog, &reg, "talk");
    a.resume(Input::Return(Value::Unit)).unwrap();
    b.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(a.run().unwrap(), b.run().unwrap());
    assert_eq!(a.save().unwrap(), b.save().unwrap());
}

#[test]
fn cannot_attach_to_vm_that_is_not_stopped() {
    let reg = registry();
    let prog = program(&reg);
    let vm = Vm::new(&prog, &reg, config()).unwrap();
    assert_eq!(Repl::attach(&vm).unwrap_err().kind, VmErrorKind::NotStopped);
}

#[test]
fn attach_rejects_let_with_same_name_as_stop_point_local() {
    let reg = registry();
    let prog = program(&reg);
    let vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    assert_eq!(
        codes(&submit_err(&mut repl, "let gold = 1")),
        ["duplicate_name"]
    );
    // Names of the current module (the script's module) can be written as they are (labels can also be `call`ed)
    assert_eq!(eval(&mut repl, "mood_of(1)"), "Mood.Sulky");
}

#[test]
fn apply_writes_back_save_vars_and_locals_and_rebuilds_stop_point() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    let old = vm.current().cloned().unwrap();
    assert!(matches!(&old, Yield::Stop { args, .. } if text_of(args) == "「信頼度は3、割ると20」"));
    exec(&mut repl, "favor += 4");
    exec(&mut repl, "d = 10");
    exec(&mut repl, "gold = 1");
    exec(&mut repl, "best = 5");
    exec(&mut repl, "let junk = 1");
    exec(&mut repl, "@counter.inc(3)");
    // The script does not change until `apply`
    assert_eq!(vm.current().cloned().unwrap(), old);
    let applied = repl.apply(&mut vm).unwrap();
    let names: Vec<String> = applied.variables.iter().map(|v| v.name.clone()).collect();
    assert_eq!(names, ["favor", "gold", "d"]);
    assert_eq!(applied.variables[0].value, Value::Int(7));
    assert_eq!(applied.variables[0].module.as_deref(), Some("main"));
    assert!(applied.variables[1].local);
    assert_eq!(applied.skipped.len(), 1);
    assert_eq!(applied.skipped[0].to_string(), "main.best");
    // Script variables, and the arguments of the stop point
    let get = |vm: &Vm<'_>, n: &str| vm.variables().into_iter().find(|v| v.name == n).unwrap();
    assert_eq!(get(&vm, "favor").value, Value::Int(7));
    assert_eq!(get(&vm, "gold").value, Value::Int(1));
    assert_eq!(get(&vm, "best").value, Value::Int(0));
    let Some(Yield::Stop { args, .. }) = vm.current() else {
        panic!()
    };
    assert_eq!(text_of(args), "「信頼度は7、割ると10」");
    // The current stop point in the history is rebuilt too
    let Some(last) = vm.history().last() else {
        panic!()
    };
    assert_eq!(last.args, *args);
    // Package state is not written back
    assert_eq!(vm.package_state::<common::Counter>().unwrap().n, 0);
    // Saves and snapshots after applying
    let snap = vm.snapshot().unwrap();
    let bytes = vm.save().unwrap();
    let (vm2, _) = Vm::load(&prog, &reg, config().start("talk"), &bytes).unwrap();
    assert_eq!(
        vm2.variables()
            .iter()
            .find(|v| v.name == "favor")
            .unwrap()
            .value,
        Value::Int(7)
    );
    drop(snap);
    // It can continue running
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
}

#[test]
fn apply_writes_only_changed_variables_and_skips_reverted_ones() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    exec(&mut repl, "favor += 1");
    exec(&mut repl, "favor -= 1");
    exec(&mut repl, "let q = 1");
    let applied = repl.apply(&mut vm).unwrap();
    assert!(applied.variables.is_empty() && applied.skipped.is_empty());
    // When only a `persistent var` differs
    exec(&mut repl, "best = 3");
    let applied = repl.apply(&mut vm).unwrap();
    assert!(applied.variables.is_empty());
    assert_eq!(applied.skipped.len(), 1);
}

#[test]
fn apply_is_all_or_nothing_and_keeps_script_when_args_cannot_be_rebuilt() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    let before = (vm.save().unwrap(), vm.current().cloned(), vm.variables());
    exec(&mut repl, "favor = 50");
    exec(&mut repl, "d = 0");
    let e = repl.apply(&mut vm).unwrap_err();
    match e {
        ApplyError::StopArgs(e) => assert_eq!(e.kind, VmErrorKind::DivisionByZero),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        (vm.save().unwrap(), vm.current().cloned(), vm.variables()),
        before
    );
    // `vm` does not become `poisoned`
    vm.resume(Input::Return(Value::Unit)).unwrap();
    // The REPL can still be used
    assert_eq!(eval(&mut repl, "favor"), "50");
}

#[test]
fn apply_fails_for_advanced_or_other_vm_and_without_script() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    exec(&mut repl, "favor = 9");
    let mut other = stopped_at(&prog, &reg, "talk");
    assert_eq!(repl.apply(&mut other).unwrap_err(), ApplyError::VmChanged);
    // A VM that has advanced
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    assert_eq!(repl.apply(&mut vm).unwrap_err(), ApplyError::VmChanged);
    // A REPL without a script
    let mut solo = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(solo.apply(&mut vm).unwrap_err(), ApplyError::NotAttached);
    let mut empty = Repl::empty(&reg, config()).unwrap();
    assert_eq!(empty.apply(&mut vm).unwrap_err(), ApplyError::NotAttached);
}

#[test]
fn reset_after_apply_returns_to_copy_of_applied_script() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    exec(&mut repl, "favor += 1");
    exec(&mut repl, "let t = 1");
    repl.apply(&mut vm).unwrap();
    exec(&mut repl, "favor += 10");
    repl.reset().unwrap();
    assert_eq!(eval(&mut repl, "favor"), "4");
    assert_eq!(codes(&submit_err(&mut repl, "t")), ["unknown_name"]);
    // An `apply` that applied nothing does not change the base
    exec(&mut repl, "favor += 1");
    exec(&mut repl, "favor -= 1");
    repl.apply(&mut vm).unwrap();
    exec(&mut repl, "favor += 3");
    repl.reset().unwrap();
    assert_eq!(eval(&mut repl, "favor"), "4");
    // A second apply also works
    exec(&mut repl, "favor += 3");
    repl.apply(&mut vm).unwrap();
    assert_eq!(
        vm.variables()
            .iter()
            .find(|v| v.name == "favor")
            .unwrap()
            .value,
        Value::Int(7)
    );
}

#[test]
fn apply_cannot_be_used_while_repl_input_is_running() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = stopped_at(&prog, &reg, "talk");
    let mut repl = Repl::attach(&vm).unwrap();
    exec(&mut repl, "favor = 8");
    repl.submit("\\ここで止まる").unwrap();
    assert_eq!(repl.apply(&mut vm).unwrap_err(), ApplyError::NotAttached);
    repl.abort();
    assert_eq!(repl.apply(&mut vm).unwrap().variables.len(), 1);
}
