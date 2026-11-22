//! Saves and persistent data

use super::*;

pub(super) const SCRIPT_COUNTER: &str =
    "import counter\nlabel start {\n  @counter.inc(5)\n  @counter.add_total(2)\n  あ #a\n}\n";
pub(super) const SCRIPT_PLAIN: &str = "label start {\n  あ #a\n}\n";

pub(super) fn save_of(src: &str, reg: &Registry) -> (Vec<u8>, Vec<u8>) {
    let prog = compile_ok(src, reg);
    let mut vm = Vm::new(&prog, reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    (vm.save().unwrap(), vm.save_persistent())
}

#[test]
fn state_and_persistent_slices_round_trip() {
    let reg = full_registry();
    let (save, persistent) = save_of(SCRIPT_COUNTER, &reg);
    let prog = compile_ok(SCRIPT_COUNTER, &reg);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (vm, report) = Vm::load(&prog, &reg, cfg, &save).unwrap();
    assert_eq!(vm.package_state::<Counter>().unwrap().n, 5);
    assert_eq!(vm.package_persistent::<Counter>().unwrap().total, 2);
    assert!(report.ignored_slices.is_empty());
    assert!(report.migrated_slices.is_empty());
    assert!(report.defaulted_slices.is_empty());
    let p = report.persistent.unwrap();
    assert!(p.ignored_slices.is_empty() && p.migrated_slices.is_empty());
    assert!(p.defaulted_slices.is_empty());
}

#[test]
fn slices_saved_but_unregistered_are_ignored_and_registered_but_unsaved_get_defaults() {
    // Save: with counter. Load: counter not registered → ignored
    let full = full_registry();
    let (save, persistent) = save_of(SCRIPT_COUNTER, &full);
    let bare = host_registry();
    let prog = compile_ok(SCRIPT_PLAIN, &bare);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (vm, report) = Vm::load(&prog, &bare, cfg, &save).unwrap();
    // The save contains the slices of the three packages with state (`counter`, `other`, `mirror`)
    assert_eq!(report.ignored_slices, ["counter", "other", "mirror"]);
    assert!(report.defaulted_slices.is_empty());
    // Only `counter` has a persistent slice
    assert_eq!(
        report.persistent.as_ref().unwrap().ignored_slices,
        ["counter"]
    );
    assert!(vm.is_stopped());

    // Save: without counter. Load: with counter → default value
    let (save, persistent) = save_of(SCRIPT_PLAIN, &bare);
    let prog = compile_ok(SCRIPT_PLAIN, &full);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (vm, report) = Vm::load(&prog, &full, cfg, &save).unwrap();
    assert_eq!(report.defaulted_slices, ["counter", "other", "mirror"]);
    assert_eq!(
        report.persistent.as_ref().unwrap().defaulted_slices,
        ["counter"]
    );
    assert_eq!(
        vm.package_state::<Counter>(),
        Some(&CounterState::default())
    );
    assert_eq!(
        vm.package_persistent::<Counter>(),
        Some(&CounterPersistent::default())
    );
}

pub(super) const SCRIPT_VER: &str =
    "import ver\nlabel start {\n  @ver.bump()\n  @ver.bump()\n  @ver.visit()\n  あ #a\n}\n";

#[test]
fn slices_with_different_version_load_via_migrate_and_are_reported() {
    let mut old = host_registry();
    old.package::<VerV1>().unwrap();
    let (save, persistent) = save_of(SCRIPT_VER, &old);
    let mut new = host_registry();
    new.package::<VerV2>().unwrap();
    let prog = compile_ok(SCRIPT_VER, &new);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (vm, report) = Vm::load(&prog, &new, cfg, &save).unwrap();
    assert_eq!(report.migrated_slices, ["ver"]);
    assert_eq!(report.persistent.as_ref().unwrap().migrated_slices, ["ver"]);
    assert_eq!(
        vm.package_state::<VerV2>(),
        Some(&VerStateV2 {
            label: "旧版から".to_string(),
            count: 2
        })
    );
    assert_eq!(
        vm.package_persistent::<VerV2>(),
        Some(&VerPersistentV2 {
            visits: 1,
            note: "旧版から".to_string()
        })
    );
    // Loading with the same version does not call migrate
    let (save2, persistent2) = save_of(SCRIPT_VER, &new);
    let cfg = VmConfig::default().persistent_data(persistent2);
    let (_, report) = Vm::load(&prog, &new, cfg, &save2).unwrap();
    assert!(report.migrated_slices.is_empty());
    let _ = (VerStateV1 { count: 0 }, VerPersistentV1 { visits: 0 });
}

/// A package without state that has only a rule version (version 1).
pub(super) struct RulesV1;
impl Package for RulesV1 {
    const NAME: &'static str = "rules";
    type State = ();
    type Persistent = ();
    fn register(_: &mut PackageReg<'_, Self>) {}
}

/// The same name with the rule version raised to 2.
pub(super) struct RulesV2;
impl Package for RulesV2 {
    const NAME: &'static str = "rules";
    const RULES_VERSION: u32 = 2;
    type State = ();
    type Persistent = ();
    fn register(_: &mut PackageReg<'_, Self>) {}
}

#[test]
fn packages_with_different_rule_version_load_and_are_reported() {
    let mut old = host_registry();
    old.package::<RulesV1>().unwrap();
    let (save, _) = save_of(SCRIPT_PLAIN, &old);
    let mut new = host_registry();
    new.package::<RulesV2>().unwrap();
    let prog = compile_ok(SCRIPT_PLAIN, &new);
    let (_, report) = Vm::load(&prog, &new, VmConfig::default(), &save).unwrap();
    assert_eq!(report.changed_rules, ["rules"]);
    // The rule version is separate from the version of the state slice (migrate is not called)
    assert!(report.migrated_slices.is_empty());
    // With the same version, it does not appear
    let (_, report) = Vm::load(&prog, &old, VmConfig::default(), &save).unwrap();
    assert!(report.changed_rules.is_empty());
}

#[test]
fn packages_absent_from_save_are_not_in_rules_report() {
    let (save, _) = save_of(SCRIPT_PLAIN, &host_registry());
    let mut new = host_registry();
    new.package::<RulesV2>().unwrap();
    let prog = compile_ok(SCRIPT_PLAIN, &new);
    let (_, report) = Vm::load(&prog, &new, VmConfig::default(), &save).unwrap();
    assert!(report.changed_rules.is_empty());
}

#[test]
fn load_fails_with_package_slice_when_migrate_returns_err() {
    // Save with version 2 and read with a version 1 registry (which has no migrate to an older version)
    let mut new = host_registry();
    new.package::<VerV2>().unwrap();
    let (save, persistent) = save_of(SCRIPT_VER, &new);
    let mut old = host_registry();
    old.package::<VerV1>().unwrap();
    let prog = compile_ok(SCRIPT_VER, &old);
    let e = Vm::load(&prog, &old, VmConfig::default(), &save).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::PackageSlice("ver".to_string()));
    // Persistent data is the same. `Vm::new` fails with `PersistentData`, and `load_persistent` does not change the VM
    let cfg = VmConfig::default().persistent_data(persistent.clone());
    let e = Vm::new(&prog, &old, cfg).unwrap_err();
    assert_eq!(
        e.kind,
        VmErrorKind::PersistentData(LoadErrorKind::PackageSlice("ver".to_string()))
    );
    let mut vm = Vm::new(&prog, &old, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let before = vm.package_persistent::<VerV1>().cloned();
    let e = vm.load_persistent(&persistent).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::PackageSlice("ver".to_string()));
    assert_eq!(vm.package_persistent::<VerV1>().cloned(), before);
}

/// Finds the same sequence as `from` in the bytes and rewrites it to `to` (fixing the checksum).
pub(super) fn tamper(bytes: &[u8], needle: &[u8], edit: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let at = out
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("見つかる");
    edit(&mut out[at..at + needle.len()]);
    let n = out.len() - 4;
    let crc = crc32(&out[..n]);
    out[n..].copy_from_slice(&crc.to_le_bytes());
    out
}

pub(super) fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

#[test]
fn broken_slices_fail_with_package_slice_without_panic() {
    let reg = full_registry();
    let (save, _) = save_of(SCRIPT_COUNTER, &reg);
    let prog = compile_ok(SCRIPT_COUNTER, &reg);
    // One slice is a sequence of `name (with length 7), version, bytes`. Find the start of the `counter` slice
    let head = [7, b'c', b'o', b'u', b'n', b't', b'e', b'r', 1];
    // If the package name changes, it is ignored as a slice without registration (does not fail)
    let renamed = tamper(&save, &head, |b| b[7] = b'X');
    let (_, report) = Vm::load(&prog, &reg, VmConfig::default(), &renamed).unwrap();
    assert_eq!(report.ignored_slices, ["counteX"]);
    assert_eq!(report.defaulted_slices, ["counter"]);
    // An unknown version (`migrate` returns `Err`)
    let versioned = tamper(&save, &head, |b| b[8] = 7);
    let e = Vm::load(&prog, &reg, VmConfig::default(), &versioned).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::PackageSlice("counter".to_string()));
    // Break the body (the string becomes invalid UTF-8)
    let broken = tamper(&save, b"inc 5", |b| b[0] = 0xFF);
    let e = Vm::load(&prog, &reg, VmConfig::default(), &broken).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::PackageSlice("counter".to_string()));
}

/// A state whose serialization fails.
#[derive(Clone, Default, Deserialize)]
pub(super) struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("直列化できない"))
    }
}

pub(super) struct Odd;

impl Package for Odd {
    const NAME: &'static str = "odd";
    type State = Unserializable;
    type Persistent = Unserializable;

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("odd.touch", |_: &mut Ctx<Odd>| {});
    }
}

#[test]
fn unserializable_state_errors_on_save_and_is_skipped_in_persistent_data() {
    let mut reg = host_registry();
    reg.package::<Odd>().unwrap();
    let prog = compile_ok(
        "import odd\nlabel start {\n  @odd.touch()\n  あ #a\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let e = vm.save().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::SliceSerialize("odd".to_string()));
    // Persistent data does not panic; it is made skipping what cannot be serialized
    let bytes = vm.save_persistent();
    assert!(Vm::new(&prog, &reg, VmConfig::default().persistent_data(bytes)).is_ok());
}

#[test]
fn host_declared_variables_go_into_save_and_persistent_data_like_script_variables() {
    let reg = full_registry();
    let src = "import counter\nlabel start {\n  counter.step = 4\n  counter.best = 6\n  counter.title = \"題\"\n  あ #a\n}\n";
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    let save = vm.save().unwrap();
    let persistent = vm.save_persistent();
    let cfg = VmConfig::default().persistent_data(persistent.clone());
    let (loaded, report) = Vm::load(&prog, &reg, cfg, &save).unwrap();
    assert_eq!(loaded.get_var("counter.step"), Some(Value::Int(4)));
    assert_eq!(loaded.get_var("counter.best"), Some(Value::Int(6)));
    assert_eq!(
        loaded.get_var("counter.title"),
        Some(Value::Str("題".into()))
    );
    assert!(report.added_vars.is_empty() && report.removed_vars.is_empty());

    // A registry with more variables: added ones are filled with initial values and reported
    let mut more = full_registry();
    more.var::<i64>("counter.extra", Kind::Save, 42).unwrap();
    more.var::<i64>("counter.extra_p", Kind::Persistent, 43)
        .unwrap();
    let prog2 = compile_ok(src, &more);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (loaded, report) = Vm::load(&prog2, &more, cfg, &save).unwrap();
    assert_eq!(loaded.get_var("counter.extra"), Some(Value::Int(42)));
    assert_eq!(loaded.get_var("counter.extra_p"), Some(Value::Int(43)));
    assert_eq!(report.added_vars, ["counter.extra"]);
    assert_eq!(report.persistent.unwrap().added_vars, ["counter.extra_p"]);

    // A registry with variables removed: they are discarded and reported
    let (save2, persistent2) = save_of(src, &more);
    let cfg = VmConfig::default().persistent_data(persistent2);
    let (_, report) = Vm::load(&prog, &reg, cfg, &save2).unwrap();
    assert_eq!(report.removed_vars, ["counter.extra"]);
    assert_eq!(report.persistent.unwrap().removed_vars, ["counter.extra_p"]);
}

#[test]
fn changed_host_variable_type_is_reported_and_returns_to_label_start() {
    let src = "import ui\nlabel start {\n  あ #a\n  い #b\n}\n";
    let mut old = host_registry();
    old.var::<i64>("ui.x", Kind::Save, 1).unwrap();
    old.var::<i64>("ui.y", Kind::Persistent, 2).unwrap();
    let (save, persistent) = save_of(src, &old);
    let mut changed = host_registry();
    changed
        .var::<String>("ui.x", Kind::Save, "初期".to_string())
        .unwrap();
    changed
        .var::<String>("ui.y", Kind::Persistent, "永続".to_string())
        .unwrap();
    let prog = compile_ok(src, &changed);
    let cfg = VmConfig::default().persistent_data(persistent);
    let (vm, report) = Vm::load(&prog, &changed, cfg, &save).unwrap();
    assert_eq!(report.type_changed_vars, ["ui.x"]);
    assert_eq!(report.persistent.unwrap().type_changed_vars, ["ui.y"]);
    assert_eq!(vm.get_var("ui.x"), Some(Value::Str("初期".into())));
    assert_eq!(vm.get_var("ui.y"), Some(Value::Str("永続".into())));
    // Same as script variables, it goes back to the start of the label (not stopped)
    assert!(!vm.is_stopped());
}
