//! REPL tests (checks of `apply` that cannot be triggered by normal use).

use super::*;
use crate::compile::compile;
use crate::registry::Registry;
use crate::vm::{Input, Vm, VmConfig, Yield};

const SRC: &str = "save var favor: int = 3\nsave var items: list<int> = [1]\n\
label start {\n  let n = 5\n  \\あ${favor / n}\n}\n";

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(crate::Sugar::Plain, "ui.say").unwrap();
    r
}

fn global_index(vm: &Vm<'_>, name: &str) -> usize {
    vm.program_ref()
        .globals
        .iter()
        .position(|g| g.name == name)
        .unwrap()
}

/// Calls `apply` with a copy where `favor` is rewritten to `value`.
fn apply_with(config: VmConfig, value: Value) -> (Result<Applied, ApplyError>, Vec<u8>, Vec<u8>) {
    let reg = registry();
    let prog = compile(SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, config).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    let mut repl = Repl::attach(&vm).unwrap();
    let i = global_index(&repl.core.vm, "favor");
    repl.core.vm.set_global(i, value);
    let before = vm.save().unwrap();
    let r = repl.apply(&mut vm);
    (r, before, vm.save().unwrap())
}

#[test]
fn apply_rejects_mismatched_types_and_keeps_script() {
    let (r, before, after) = apply_with(VmConfig::default(), Value::Str("x".into()));
    match r {
        Err(ApplyError::Invalid { name, reason }) => {
            assert_eq!(name.name, "favor");
            assert_eq!(reason, ApplyInvalid::Type);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(before, after);
}

#[test]
fn apply_rejects_too_large_values_and_keeps_script() {
    let big = Value::List((0..1000).map(Value::Int).collect::<Vec<_>>().into());
    let reg = registry();
    let prog = compile(SRC, "main", &reg).unwrap();
    let cfg = VmConfig::default().max_value_bytes(64);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    vm.run().unwrap();
    let mut repl = Repl::attach(&vm).unwrap();
    let i = global_index(&repl.core.vm, "items");
    repl.core.vm.set_global(i, big);
    let before = vm.save().unwrap();
    match repl.apply(&mut vm) {
        Err(ApplyError::Invalid { name, reason }) => {
            assert_eq!(name.name, "items");
            assert_eq!(reason, ApplyInvalid::ValueTooLarge);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(vm.save().unwrap(), before);
}

#[test]
fn apply_rejects_total_over_limit_as_memory_limit() {
    let big = Value::List((0..200).map(Value::Int).collect::<Vec<_>>().into());
    let reg = registry();
    let prog = compile(SRC, "main", &reg).unwrap();
    let cfg = VmConfig::default().max_total_bytes(Some(400));
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    vm.run().unwrap();
    let mut repl = Repl::attach(&vm).unwrap();
    let i = global_index(&repl.core.vm, "items");
    repl.core.vm.set_global(i, big);
    let before = vm.save().unwrap();
    match repl.apply(&mut vm) {
        Err(ApplyError::Invalid { reason, .. }) => assert_eq!(reason, ApplyInvalid::MemoryLimit),
        other => panic!("{other:?}"),
    }
    assert_eq!(vm.save().unwrap(), before);
}

#[test]
fn apply_rejects_too_deep_values_as_not_savable() {
    let mut v = Value::Int(1);
    for _ in 0..200 {
        v = Value::List(vec![v].into());
    }
    let reg = registry();
    let prog = compile(
        "save var deep: list<int> = [1]\nlabel start {\n  \\あ\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let mut repl = Repl::attach(&vm).unwrap();
    let i = global_index(&repl.core.vm, "deep");
    repl.core.vm.set_global(i, v);
    let before = vm.save().unwrap();
    match repl.apply(&mut vm) {
        // Type checking may fail first (either way, the script is unchanged)
        Err(ApplyError::Invalid { reason, .. }) => {
            assert!(matches!(
                reason,
                ApplyInvalid::Type | ApplyInvalid::NotSavable
            ));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(vm.save().unwrap(), before);
}

#[test]
fn script_continues_normally_after_successful_apply() {
    let (r, _, _) = apply_with(VmConfig::default(), Value::Int(6));
    let applied = r.unwrap();
    assert_eq!(applied.variables.len(), 1);
    let reg = registry();
    let prog = compile(SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::End));
}
