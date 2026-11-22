//! Determinism, and the script's Vm does not change

use super::*;

#[test]
fn same_input_sequences_give_same_results() {
    let reg = registry();
    let prog = program(&reg);
    let script = [
        "let a = random(1, 100)",
        "favor += a",
        "fn f(n: int) -> int {\n  return n + favor\n}",
        "let b = f(a)",
        "items.push(\"x\")",
        "random(1, 1000)",
        "@counter.inc(2)",
        "counter.get()",
    ];
    let run = || {
        let mut repl = Repl::from_program(&prog, &reg, config().seed(42)).unwrap();
        let mut out = Vec::new();
        for s in script {
            out.push(format!("{:?}", exec(&mut repl, s)));
        }
        (out, repl.variables())
    };
    assert_eq!(run(), run());
}

#[test]
fn script_vm_is_unchanged_by_using_repl() {
    let reg = registry();
    let prog = program(&reg);
    let play = |use_repl: bool| {
        let mut vm = Vm::new(&prog, &reg, config().seed(7)).unwrap();
        let mut log = Vec::new();
        loop {
            if use_repl {
                let mut repl = Repl::attach_or_start(&vm, &prog, &reg);
                exec(&mut repl, "favor += 100");
                exec(&mut repl, "let x = random(1, 100)");
                exec(&mut repl, "@counter.inc(1)");
            }
            let y = vm.run().unwrap();
            log.push(format!("{y:?}"));
            match y {
                Yield::Stop { ref args, .. } => {
                    let v = if matches!(args.first(), Some(Value::List(_))) {
                        Value::Int(0)
                    } else {
                        Value::Unit
                    };
                    vm.resume(Input::Return(v)).unwrap();
                }
                _ => break,
            }
        }
        (log, vm.save().ok(), vm.variables())
    };
    assert_eq!(play(false), play(true));
}

/// For tests: `attach` if there is a stopped VM, otherwise (before starting) `from_program`.
pub(super) trait AttachOrStart<'a> {
    fn attach_or_start(vm: &Vm<'a>, prog: &'a Program, reg: &'a Registry) -> Repl<'a>;
}

impl<'a> AttachOrStart<'a> for Repl<'a> {
    fn attach_or_start(vm: &Vm<'a>, prog: &'a Program, reg: &'a Registry) -> Repl<'a> {
        if vm.is_stopped() {
            Repl::attach(vm).unwrap()
        } else {
            Repl::from_program(prog, reg, VmConfig::default()).unwrap()
        }
    }
}
