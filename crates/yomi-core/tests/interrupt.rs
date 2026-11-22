//! Interrupting a running VM (`spec/host/01-execution.md`, 1.9a).

use std::sync::OnceLock;
use std::time::Duration;

use yomi_core::{
    CommandError, Ctx, InterruptHandle, Package, PackageReg, Registry, Repl, Sugar, Vm, VmConfig,
    VmErrorKind, Yield, compile,
};

const SPIN: &str = "label start {\n  let i = 0\n  while true {\n    i += 1\n  }\n}\n";

/// The token that `watch.trip` interrupts (a command cannot reach the VM's token otherwise).
static TRIPPED: OnceLock<InterruptHandle> = OnceLock::new();

struct Watcher;

impl Package for Watcher {
    const NAME: &'static str = "watch";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("watch.spin", |ctx: &mut Ctx<Watcher>| {
            while !ctx.interrupted() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err::<(), _>(CommandError::interrupted())
        });
        reg.effect("watch.trip", |_: &mut Ctx<Watcher>| {
            TRIPPED.get().unwrap().interrupt();
        });
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.package::<Watcher>().unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

#[test]
fn interrupt_stops_a_running_loop_from_another_thread() {
    let reg = registry();
    let program = compile(SPIN, "main", &reg).unwrap();
    let cfg = VmConfig::default().max_steps_per_run(None);
    let mut vm = Vm::new(&program, &reg, cfg).unwrap();
    let handle = vm.interrupt_handle();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        handle.interrupt();
    });
    let e = vm.run().unwrap_err();
    stopper.join().unwrap();
    assert_eq!(e.kind, VmErrorKind::Interrupted);
    assert_eq!(e.kind.code(), "interrupted");
    // Like any runtime error, the VM cannot go on
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
}

#[test]
fn interrupt_requested_before_run_stops_at_the_start() {
    let reg = registry();
    let program = compile("label start {\n  end\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    vm.interrupt_handle().interrupt();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Interrupted);
}

#[test]
fn interrupt_requested_while_stopped_is_seen_by_the_next_run() {
    let reg = registry();
    let program = compile("label start {\n  一 #a\n  二 #b\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm.interrupt_handle().interrupt();
    vm.resume(yomi_core::Input::Return(yomi_core::Value::Unit))
        .unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Interrupted);
}

#[test]
fn request_stays_until_reset_and_a_shared_token_stops_all_vms() {
    let reg = registry();
    let program = compile("label start {\n  end\n}\n", "main", &reg).unwrap();
    let token = InterruptHandle::new();
    let cfg = VmConfig::default().interrupt(token.clone());
    let mut a = Vm::new(&program, &reg, cfg.clone()).unwrap();
    let mut b = Vm::new(&program, &reg, cfg).unwrap();
    token.interrupt();
    assert_eq!(a.run().unwrap_err().kind, VmErrorKind::Interrupted);
    assert_eq!(b.run().unwrap_err().kind, VmErrorKind::Interrupted);
    assert!(token.is_interrupted());
    token.reset();
    let mut c = Vm::new(&program, &reg, VmConfig::default().interrupt(token.clone())).unwrap();
    assert_eq!(c.run().unwrap(), Yield::End);
}

#[test]
fn a_command_can_see_the_interrupt_and_return_interrupted() {
    let reg = registry();
    let program = compile(
        "import watch\nlabel start {\n  @watch.spin()\n}\n",
        "main",
        &reg,
    );
    let program = match program {
        Ok(p) => p,
        Err(d) => panic!("{d:?}"),
    };
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    let handle = vm.interrupt_handle();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        handle.interrupt();
    });
    let e = vm.run().unwrap_err();
    stopper.join().unwrap();
    // `CommandError::interrupted` is not `CommandFailed`
    assert_eq!(e.kind, VmErrorKind::Interrupted);
}

#[test]
fn interrupt_is_seen_when_a_command_returns() {
    let reg = registry();
    let program = compile(
        "import watch\nlabel start {\n  @watch.trip()\n  end\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let token = TRIPPED.get_or_init(InterruptHandle::new).clone();
    let mut vm = Vm::new(&program, &reg, VmConfig::default().interrupt(token)).unwrap();
    // The command raises the interrupt itself, and the script is too short for the periodic check
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Interrupted);
}

#[test]
fn interrupt_is_seen_inside_fn() {
    let reg = registry();
    let src = "fn spin() -> int {\n  let i = 0\n  while true {\n    i += 1\n  }\n  return i\n}\nlabel start {\n  ${spin()}\n}\n";
    let program = compile(src, "main", &reg).unwrap();
    let cfg = VmConfig::default().max_steps_per_run(None);
    let mut vm = Vm::new(&program, &reg, cfg).unwrap();
    let handle = vm.interrupt_handle();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        handle.interrupt();
    });
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Interrupted);
    stopper.join().unwrap();
}

#[test]
fn step_limit_still_applies_with_the_interrupt_check() {
    let reg = registry();
    let program = compile(SPIN, "main", &reg).unwrap();
    for limit in [1_u64, 3, 1023, 1024, 1025, 5000] {
        let cfg = VmConfig::default().max_steps_per_run(Some(limit));
        let mut vm = Vm::new(&program, &reg, cfg).unwrap();
        assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::StepLimitExceeded);
    }
}

#[test]
fn repl_interrupt_discards_the_input() {
    let reg = registry();
    let program = compile("label start {\n  end\n}\n", "main", &reg).unwrap();
    let cfg = VmConfig::default().max_steps_per_run(None);
    let mut repl = Repl::from_program(&program, &reg, cfg).unwrap();
    let handle = repl.interrupt_handle();
    repl.submit("let n = 1").unwrap();
    handle.interrupt();
    assert_eq!(repl.run().unwrap_err().kind, VmErrorKind::Interrupted);
    handle.reset();
    repl.submit("let n = 2").unwrap();
    assert!(repl.run().is_ok());
}
