//! Reading and writing from the host

use super::*;

#[test]
fn host_reads_state_slices_and_variables_and_writes_variables_at_stop_points() {
    let reg = full_registry();
    let prog = compile_ok(
        "import counter\nlabel start {\n  @counter.inc(4)\n  @counter.add_total(2)\n  あ #a\n  ${counter.step}\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    assert_eq!(vm.package_state::<Counter>().unwrap().n, 4);
    assert_eq!(vm.package_persistent::<Counter>().unwrap().total, 2);
    assert_eq!(
        vm.package_state::<Mirror>(),
        Some(&MirrorState { copied: 0 })
    );
    // An unregistered type is `None`
    let bare = host_registry();
    let prog2 = compile_ok("label start {\n  あ\n}\n", &bare);
    let vm2 = Vm::new(&prog2, &bare, VmConfig::default()).unwrap();
    assert!(vm2.package_state::<Counter>().is_none());
    assert!(vm2.package_persistent::<Counter>().is_none());

    // Variables: read, write (the host can also write read_only ones), wrong types, unknown names
    assert_eq!(vm.get_var("counter.step"), Some(Value::Int(1)));
    assert_eq!(vm.get_var("counter.nothing"), None);
    vm.set_var("counter.step", Value::Int(9)).unwrap();
    vm.set_var("counter.locked", Value::Bool(true)).unwrap();
    assert_eq!(vm.get_var("counter.locked"), Some(Value::Bool(true)));
    let e = vm
        .set_var("counter.step", Value::Str("a".into()))
        .unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
    let e = vm.set_var("counter.nothing", Value::Int(1)).unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::UnknownVariable("counter.nothing".into())
    );
    let e = vm
        .set_var("counter.step", Value::Float(f64::NAN))
        .unwrap_err();
    assert_eq!(e.kind, VmErrorKind::NonFiniteFloat);
    assert_eq!(vm.get_var("counter.step"), Some(Value::Int(9)));
    // Can be read from scripts
    advance(&mut vm);
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => assert_eq!(args, [Value::Str("9".into())]),
        other => panic!("{other:?}"),
    }
    // Host-declared variables also appear in `variables()`
    let v = vm.variables();
    assert!(
        v.iter()
            .any(|x| x.module.as_deref() == Some("counter") && x.name == "step")
    );
}

#[test]
fn command_err_becomes_positioned_runtime_error_and_cannot_continue() {
    let reg = full_registry();
    let prog = compile_ok(
        "import counter\nlabel start {\n  @counter.fail(\"だめ\")\n  あ\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::CommandFailed {
            code: "counter.failed".to_string(),
            args: vec![("message", DiagArg::Str("だめ".to_string()))],
        }
    );
    // The code and arguments can be extracted (no message text)
    let (code, args) = e.kind.command_error().unwrap();
    assert_eq!(code, "counter.failed");
    assert_eq!(args[0].0, "message");
    assert_eq!(e.label.as_deref(), Some("start"));
    assert_eq!(e.line, Some(3));
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
}

#[test]
fn programs_using_commands_not_in_registry_cannot_create_vm() {
    let full = full_registry();
    let prog = compile_ok(
        "import counter\nlabel start {\n  @counter.inc(1)\n  あ\n}\n",
        &full,
    );
    let bare = host_registry();
    let e = Vm::new(&prog, &bare, VmConfig::default()).unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::CommandMissing("counter.inc".to_string())
    );
    let prog = compile_ok(
        "import counter\nlabel start {\n  counter.step = 2\n  あ\n}\n",
        &full,
    );
    let e = Vm::new(&prog, &bare, VmConfig::default()).unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::CommandMissing("counter.step".to_string())
    );
}
