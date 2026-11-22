//! Tests: misuse of the API

use super::*;

#[test]
fn calls_not_matching_state_return_error_without_panic() {
    let reg = registry();
    let prog = compile_ok("label start {\n  あ\n  @game.roll(1)\n}\n", &reg);
    let mut vm = Vm::new(&prog, &reg, config()).unwrap();
    // resume and save before stopping
    assert_eq!(
        vm.resume(Input::Return(Value::Unit)).unwrap_err().kind,
        VmErrorKind::NotStopped
    );
    assert_eq!(vm.save().unwrap_err().kind, VmErrorKind::NotStopped);
    assert!(vm.current().is_none());
    // run while stopped
    assert!(matches!(vm.run(), Ok(Yield::Stop { .. })));
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::AlreadyStopped);
    // Input of the wrong type
    let e = vm.resume(Input::Return(Value::Int(1))).unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
    // It can continue even after a wrong input
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(vm.run(), Ok(Yield::Stop { .. })));
    let e = vm.resume(Input::Return(Value::Unit)).unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
    vm.resume(Input::Return(Value::Int(2))).unwrap();
    assert_eq!(vm.run().unwrap(), Yield::End);
    // After the end
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Ended);
    assert_eq!(vm.save().unwrap_err().kind, VmErrorKind::NotStopped);
}
