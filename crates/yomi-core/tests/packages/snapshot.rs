//! Snapshots

use super::*;

#[test]
fn snapshot_restores_state_slices_and_variables_but_not_persistent_ones() {
    let reg = full_registry();
    let prog = compile_ok(
        "import counter\nlabel start {\n  @counter.inc(1)\n  一 #a\n  @counter.inc(10)\n  @counter.add_total(3)\n  counter.step = 7\n  counter.best = 8\n  二 #b\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let snap: Snapshot = vm.snapshot().unwrap();
    advance(&mut vm);
    to_stop(&mut vm);
    assert_eq!(vm.package_state::<Counter>().unwrap().n, 11);
    assert_eq!(vm.package_persistent::<Counter>().unwrap().total, 3);
    vm.restore(&snap).unwrap();
    assert_eq!(
        vm.package_state::<Counter>().unwrap(),
        &CounterState {
            n: 1,
            log: vec!["inc 1".to_string()]
        }
    );
    assert_eq!(vm.package_persistent::<Counter>().unwrap().total, 3);
    assert_eq!(vm.get_var("counter.step"), Some(Value::Int(1)));
    assert_eq!(vm.get_var("counter.best"), Some(Value::Int(8)));
}
