//! Stop commands with return value checks (`stop_validated`)

use super::*;

pub(super) const ASK_SCRIPT: &str = "import cued\nlabel start {\n  @cued.ask(5) #ask\n  あと\n}\n";

#[test]
fn checked_stop_commands_accept_values_meeting_constraints() {
    let reg = cued_registry();
    let prog = compile_ok(ASK_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    vm.resume(Input::Return(Value::Int(5))).unwrap();
    to_stop(&mut vm);
}

#[test]
fn checked_stop_commands_make_resume_with_violating_value_runtime_error_restorable_from_snapshot() {
    let reg = cued_registry();
    let prog = compile_ok(ASK_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let snap = vm.snapshot().unwrap();
    let e = vm.resume(Input::Return(Value::Int(6))).unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::CommandFailed {
            code: "cued.over_max".to_string(),
            args: vec![("max", DiagArg::Int(5)), ("value", DiagArg::Int(6))],
        }
    );
    assert!(e.label.is_some(), "位置が付く: {e:?}");
    // After a runtime error, it cannot continue
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
    // Restoring from a snapshot allows returning a value again
    vm.restore(&snap).unwrap();
    vm.resume(Input::Return(Value::Int(0))).unwrap();
    to_stop(&mut vm);
}

#[test]
fn checked_stop_commands_reject_wrong_type_as_bad_input() {
    let reg = cued_registry();
    let prog = compile_ok(ASK_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let e = vm
        .resume(Input::Return(Value::Str("x".into())))
        .unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
    // A type error is the host's misuse. The VM stays at the stop point
    vm.resume(Input::Return(Value::Int(1))).unwrap();
}

#[test]
fn checked_stop_commands_error_on_argument_check_before_stopping() {
    let reg = cued_registry();
    let prog = compile_ok(
        "import cued\nlabel start {\n  @cued.ask(-1)\n  あと\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::CommandFailed {
            code: "cued.negative_max".to_string(),
            args: vec![("max", DiagArg::Int(-1))],
        }
    );
}
