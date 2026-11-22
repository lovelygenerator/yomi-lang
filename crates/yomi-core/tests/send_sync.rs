//! Types that the host passes to other threads or shares between threads are `Send` and `Sync`.

use yomi_core::{
    CompileOptions, Diagnostic, LoadReport, Program, Registry, Repl, Snapshot, Value, Vm, VmConfig,
};

fn send<T: Send>() {}
fn sync<T: Sync>() {}

#[test]
fn shared_types_are_send_and_sync() {
    send::<Program>();
    sync::<Program>();
    send::<Registry>();
    sync::<Registry>();
    send::<Value>();
    sync::<Value>();
    send::<Snapshot>();
    sync::<Snapshot>();
    send::<VmConfig>();
    send::<CompileOptions>();
    send::<Diagnostic>();
    send::<LoadReport>();
}

#[test]
fn vm_and_repl_are_send_and_sync() {
    send::<Vm<'static>>();
    sync::<Vm<'static>>();
    send::<Repl<'static>>();
}

#[test]
fn vm_runs_on_another_thread() {
    let reg = Registry::new();
    let program = yomi_core::compile("label start {\n  end\n}\n", "main", &reg).unwrap();
    std::thread::scope(|s| {
        s.spawn(|| {
            let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
            assert_eq!(vm.run().unwrap(), yomi_core::Yield::End);
        });
    });
}
