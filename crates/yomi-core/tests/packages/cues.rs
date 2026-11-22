//! Rules of cues

use super::*;

pub(super) fn cues(vm: &Vm<'_>) -> Vec<String> {
    vm.package_state::<Cued>().unwrap().cues.clone()
}

pub(super) const CUED_SCRIPT: &str = "import cued\nlabel start {\n  @cued.cue(\"a\")\n  @cued.cue(\"b\")\n  一 #one\n  @cued.cue(\"c\")\n  二 #two\n  三 #three\n}\n";

#[test]
fn cues_hold_only_those_added_after_last_resume_and_empty_at_resume_start() {
    let reg = cued_registry();
    let prog = compile_ok(CUED_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    assert_eq!(cues(&vm), ["a", "b"]);
    // Empty at the time of resume (before calling run)
    advance(&mut vm);
    assert!(cues(&vm).is_empty());
    assert_eq!(vm.package_state::<Cued>().unwrap().count, 2);
    to_stop(&mut vm);
    assert_eq!(cues(&vm), ["c"]);
    advance(&mut vm);
    to_stop(&mut vm);
    assert!(cues(&vm).is_empty(), "コマンドを呼ばなかった区間の合図は空");
    assert_eq!(vm.package_state::<Cued>().unwrap().count, 3);
}

#[test]
fn failed_resume_does_not_empty_cues() {
    let reg = cued_registry();
    let prog = compile_ok(CUED_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    // Even if resumed with a value of the wrong type, it stays at the stop point and the cues remain
    assert!(vm.resume(Input::Return(Value::Int(1))).is_err());
    assert_eq!(cues(&vm), ["a", "b"]);
}

#[test]
fn cues_are_in_saves_but_empty_after_load_and_persistent_part_restores() {
    let reg = cued_registry();
    let prog = compile_ok(CUED_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    assert_eq!(cues(&vm), ["a", "b"]);
    let bytes = vm.save().unwrap();
    // The VM that saved stays as it is (saving does not change the state)
    assert_eq!(cues(&vm), ["a", "b"]);
    let (loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert!(loaded.is_stopped());
    assert!(cues(&loaded).is_empty(), "古い合図は再生されない");
    assert_eq!(loaded.package_state::<Cued>().unwrap().count, 2);
    // In the continuation after loading, new cues are added
    let mut loaded = loaded;
    advance(&mut loaded);
    to_stop(&mut loaded);
    assert_eq!(cues(&loaded), ["c"]);
}

#[test]
fn cues_are_in_snapshots_but_empty_after_rollback_and_persistent_part_restores() {
    let reg = cued_registry();
    let prog = compile_ok(CUED_SCRIPT, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let snap: Snapshot = vm.snapshot().unwrap();
    advance(&mut vm);
    to_stop(&mut vm);
    assert_eq!(cues(&vm), ["c"]);
    assert_eq!(vm.package_state::<Cued>().unwrap().count, 3);
    vm.restore(&snap).unwrap();
    assert!(cues(&vm).is_empty());
    assert_eq!(
        *vm.package_state::<Cued>().unwrap(),
        CuedState {
            cues: vec![],
            count: 2
        }
    );
    // Even after going back, it can continue from the same stop point
    advance(&mut vm);
    to_stop(&mut vm);
    assert_eq!(cues(&vm), ["c"]);
    assert_eq!(vm.package_state::<Cued>().unwrap().count, 3);
}

#[test]
fn default_clear_cues_does_nothing() {
    // `Counter` does not implement `clear_cues`. The state does not change across resume and run
    let reg = full_registry();
    let prog = compile_ok(
        "import counter\nlabel start {\n  @counter.inc(4)\n  あ\n  い\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let before = vm.package_state::<Counter>().unwrap().clone();
    advance(&mut vm);
    assert_eq!(*vm.package_state::<Counter>().unwrap(), before);
}
