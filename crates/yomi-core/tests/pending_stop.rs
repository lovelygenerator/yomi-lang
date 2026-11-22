//! Reading the stop point of a save without restoring the VM (`spec/host/04-save.md`, 4.9).

use yomi_core::{
    LoadErrorKind, Registry, Sugar, Value, Vm, VmConfig, Yield, compile, pending_stop,
};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

const SCRIPT: &str = "label start {\n  一行目 #one\n  二行目\n}\n";

fn stopped_save(cfg: VmConfig, steps: usize) -> Vec<u8> {
    let reg = registry();
    let program = compile(SCRIPT, "main", &reg).unwrap();
    let mut vm = Vm::new(&program, &reg, cfg).unwrap();
    for _ in 0..steps {
        assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
        vm.resume(yomi_core::Input::Return(Value::Unit)).unwrap();
    }
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm.save().unwrap()
}

#[test]
fn reads_the_command_arguments_and_key_of_the_stop_point() {
    let p = pending_stop(&stopped_save(VmConfig::default(), 0))
        .unwrap()
        .unwrap();
    assert_eq!(p.label, "main.start");
    assert_eq!(p.key.as_deref(), Some("main.start#one"));
    assert_eq!(p.command, "ui.say");
    assert_eq!(p.args, [Value::Str("一行目".into())]);
    assert_eq!(p.schema_version, 1);
}

#[test]
fn a_stop_point_without_an_id_has_no_key_and_the_last_stop_is_read() {
    let p = pending_stop(&stopped_save(VmConfig::default(), 1))
        .unwrap()
        .unwrap();
    assert_eq!(p.key, None);
    assert_eq!(p.args, [Value::Str("二行目".into())]);
}

#[test]
fn a_save_without_history_gives_none() {
    let mut cfg = VmConfig::default();
    cfg.history_limit = 0;
    assert_eq!(pending_stop(&stopped_save(cfg, 0)).unwrap(), None);
}

#[test]
fn broken_bytes_and_persistent_data_are_load_errors() {
    let bytes = stopped_save(VmConfig::default(), 0);
    let mut broken = bytes.clone();
    let last = broken.len() - 1;
    broken[last] ^= 0xFF;
    assert_eq!(
        pending_stop(&broken).unwrap_err().kind,
        LoadErrorKind::ChecksumMismatch
    );
    assert_eq!(
        pending_stop(&bytes[..4]).unwrap_err().kind,
        LoadErrorKind::BadMagic
    );
    assert_eq!(pending_stop(b"").unwrap_err().kind, LoadErrorKind::BadMagic);
    // Persistent data has a different magic
    let reg = registry();
    let program = compile(SCRIPT, "main", &reg).unwrap();
    let vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    let persistent = vm.save_persistent();
    assert_eq!(
        pending_stop(&persistent).unwrap_err().kind,
        LoadErrorKind::BadMagic
    );
}
