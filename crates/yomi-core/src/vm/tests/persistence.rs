//! Tests of completing saves (persistent data and so on): `persistent var`, persistent data, rollback, random generator,
//! history.

use super::util::{self, *};
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Quote, Sugar::Choice], |r| {
        r.stop::<(String, i64), ()>("game.give").unwrap();
    })
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

/// Advances by one stop point (runs and returns the body of the stop point it stopped at; `None` at the end).
fn advance(vm: &mut Vm<'_>) -> Option<String> {
    let y = vm.run().expect("run できる");
    if matches!(y, Yield::End) {
        return None;
    }
    Some(text_of(&y))
}

/// Runs to the end and lists the bodies (up to `limit` entries).
fn rest(vm: &mut Vm<'_>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    if vm.is_stopped() {
        resume(vm);
    }
    for _ in 0..limit {
        match vm.run() {
            Ok(Yield::End) => {
                out.push("<end>".to_string());
                break;
            }
            Ok(y) => {
                out.push(text_of(&y));
                resume(vm);
            }
            Err(e) => {
                out.push(format!("<{}>", e.kind.code()));
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// persistent var
// ---------------------------------------------------------------------------

const PERSIST_SRC: &str = "persistent var vol: int = 3\nsave var gold: int = 10\nlabel start {\n  vol += 1\n  gold += 5\n  音量${vol} 金${gold} #a\n  vol = vol * 2\n  gold = 0\n  音量${vol} #b\n}\n";

#[test]
fn persistent_var_can_be_read_and_written_and_is_marked_persistent_in_variables() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(advance(&mut vm).as_deref(), Some("音量4 金15"));
    resume(&mut vm);
    assert_eq!(advance(&mut vm).as_deref(), Some("音量8"));
    let vars = vm.variables();
    let vol = vars.iter().find(|v| v.name == "vol").unwrap();
    let gold = vars.iter().find(|v| v.name == "gold").unwrap();
    assert!(vol.persistent && !gold.persistent && !vol.local);
    assert_eq!(vol.value, Value::Int(8));
}

#[test]
fn persistent_var_can_be_read_and_written_with_module_qualified_name() {
    let reg = registry();
    let a = "import b\nlabel start {\n  b.vol = b.vol + 1\n  ${b.vol} #x\n}\n";
    let b = "persistent var vol: int = 40\nlabel unused {\n  end\n}\n";
    let prog = crate::compile_modules(&[("a", a), ("b", b)], &reg).expect("コンパイルできる");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("a.start")).unwrap();
    assert_eq!(advance(&mut vm).as_deref(), Some("41"));
    // Keys of persistent data include module names
    let bytes = vm.save_persistent();
    let mut vm2 = Vm::new(&prog, &reg, VmConfig::default().start("a.start")).unwrap();
    vm2.load_persistent(&bytes).unwrap();
    assert_eq!(var(&vm2, "vol"), Value::Int(41));
}

#[test]
fn saves_contain_neither_persistent_var_nor_passed_stop_points() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let first = vm.save().unwrap();
    // Rewriting only the persistent data does not change the bytes of the save
    let mut other = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut other);
    let mut pb = other.save_persistent();
    // Make persistent data with a different value (vol to 99)
    let mut vm99 = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm99.globals[0] = Value::Int(99);
    pb.clear();
    pb.extend(vm99.save_persistent());
    other.load_persistent(&pb).unwrap();
    assert_eq!(var(&other, "vol"), Value::Int(99));
    // The current stop point of `other` has its arguments rebuilt, so the body changes. The save is at the same position with
    // the same save vars
    let second = other.save().unwrap();
    // Only the last record of the history may differ, so read back and compare the variables (`save var`s)
    let (a, _) = Vm::load(&prog, &reg, VmConfig::default(), &first).unwrap();
    let (b, _) = Vm::load(&prog, &reg, VmConfig::default(), &second).unwrap();
    assert_eq!(var(&a, "gold"), var(&b, "gold"));
    // Without persistent data, vol is the declared initial value in both
    assert_eq!(var(&a, "vol"), Value::Int(3));
    assert_eq!(var(&b, "vol"), Value::Int(3));
}

#[test]
fn without_persistent_data_loading_save_resets_persistent_var() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let save = vm.save().unwrap();
    let persistent = vm.save_persistent();
    let (vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &save).unwrap();
    assert_eq!(var(&vm2, "vol"), Value::Int(3));
    assert_eq!(var(&vm2, "gold"), Value::Int(15));
    assert!(report.persistent.is_none());
    // With persistent data, it is the value when saved
    let (vm3, report) = Vm::load(
        &prog,
        &reg,
        VmConfig::default().persistent_data(persistent),
        &save,
    )
    .unwrap();
    assert_eq!(var(&vm3, "vol"), Value::Int(4));
    assert_eq!(
        report.persistent,
        Some(PersistentReport {
            from_schema_version: 1,
            to_schema_version: 1,
            ..PersistentReport::default()
        })
    );
}

#[test]
fn persistent_data_saves_and_restores_passed_stop_points() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    resume(&mut vm);
    advance(&mut vm);
    assert_eq!(vm.seen_points(), vec!["main.start#a", "main.start#b"]);
    let bytes = vm.save_persistent();
    let mut fresh = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(fresh.seen_points().is_empty());
    fresh.load_persistent(&bytes).unwrap();
    assert_eq!(fresh.seen_points(), vec!["main.start#a", "main.start#b"]);
    assert_eq!(var(&fresh, "vol"), Value::Int(8));
    // Saving again gives the same bytes
    assert_eq!(fresh.save_persistent(), bytes);
}

#[test]
fn persistent_data_given_at_vm_creation_sets_persistent_var_and_seen_from_start() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save_persistent();
    let cfg = VmConfig::default().persistent_data(bytes);
    let mut next = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(var(&next, "vol"), Value::Int(4));
    assert_eq!(next.seen_points(), vec!["main.start#a"]);
    // Second playthrough: carry over the persistent data and start from the beginning
    assert_eq!(advance(&mut next).as_deref(), Some("音量5 金15"));
    // A VM cannot be created with broken persistent data
    let bad = VmConfig::default().persistent_data(vec![1, 2, 3]);
    let e = Vm::new(&prog, &reg, bad).unwrap_err();
    assert_eq!(e.kind, VmErrorKind::PersistentData(LoadErrorKind::BadMagic));
}

#[test]
fn persistent_data_replaces_seen_points_setting() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save_persistent();
    let cfg = VmConfig::default().seen_points(["main.other#z"]);
    let mut other = Vm::new(&prog, &reg, cfg).unwrap();
    assert_eq!(other.seen_points(), vec!["main.other#z"]);
    other.load_persistent(&bytes).unwrap();
    assert_eq!(other.seen_points(), vec!["main.start#a"]);
}

#[test]
fn persistent_data_compatibility_defaults_added_drops_removed_and_resets_type_changed() {
    let reg = registry();
    let old = compile_ok(
        "persistent var keep: int = 1\npersistent var gone: int = 2\npersistent var typed: int = 3\nlabel start {\n  keep = 11\n  gone = 12\n  typed = 13\n  あ #a\n}\n",
    );
    let mut vm = Vm::new(&old, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save_persistent();
    let new = compile_ok(
        "persistent var keep: int = 100\npersistent var typed: str = \"既定\"\npersistent var added: bool = true\nlabel start {\n  あ #a\n}\n",
    );
    let mut vm2 = Vm::new(
        &new,
        &reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
    )
    .unwrap();
    let report = vm2.load_persistent(&bytes).unwrap();
    assert_eq!(report.added_vars, vec!["main.added"]);
    assert_eq!(report.removed_vars, vec!["main.gone"]);
    assert_eq!(report.type_changed_vars, vec!["main.typed"]);
    assert_eq!(var(&vm2, "keep"), Value::Int(11));
    assert_eq!(var(&vm2, "typed"), Value::Str("既定".into()));
    assert_eq!(var(&vm2, "added"), Value::Bool(true));
}

#[test]
fn broken_persistent_data_cannot_be_loaded_and_vm_is_unchanged() {
    let reg = registry();
    let prog = compile_ok(PERSIST_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let good = vm.save_persistent();
    let before_vars = vm.variables();
    let before_seen = vm.seen_points();
    let mut bad_crc = good.clone();
    let n = bad_crc.len();
    bad_crc[n - 6] ^= 0xff;
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    let mut bad_version = good.clone();
    bad_version[4..8].copy_from_slice(&9u32.to_le_bytes());
    let cases: Vec<(Vec<u8>, LoadErrorKind)> = vec![
        (bad_crc, LoadErrorKind::ChecksumMismatch),
        (bad_magic, LoadErrorKind::BadMagic),
        (bad_version, LoadErrorKind::UnsupportedVersion(9)),
        (Vec::new(), LoadErrorKind::BadMagic),
        (good[..5].to_vec(), LoadErrorKind::BadMagic),
        // The bytes of a save are not persistent data
        (vm.save().unwrap(), LoadErrorKind::BadMagic),
    ];
    for (bytes, kind) in cases {
        let e = vm.load_persistent(&bytes).unwrap_err();
        assert_eq!(e.kind, kind);
        assert_eq!(vm.variables(), before_vars);
        assert_eq!(vm.seen_points(), before_seen);
    }
    // Persistent data cannot be read as a save either
    let e = Vm::load(&prog, &reg, VmConfig::default(), &good).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::BadMagic);
}

#[test]
fn persistent_data_with_nan_is_broken() {
    let reg = registry();
    let prog = compile_ok("persistent var x: float = 1.5\nlabel start {\n  あ #a\n}\n");
    let vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut bytes = vm.save_persistent();
    let one_and_half = 1.5f64.to_le_bytes();
    let nan = f64::NAN.to_le_bytes();
    let at = bytes
        .windows(8)
        .position(|w| w == one_and_half)
        .expect("値のバイト列がある");
    bytes[at..at + 8].copy_from_slice(&nan);
    let n = bytes.len() - 4;
    let crc = crate::vm::save::crc32(&bytes[..n]);
    bytes[n..].copy_from_slice(&crc.to_le_bytes());
    let mut vm2 = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm2.load_persistent(&bytes).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::Corrupt);
}

const ORDER_SRC: &str = "persistent var vol: int = 3\nsave var gold: int = 10\nlabel start {\n  gold += 5\n  音量${vol} ${random(1, 1000)} #a\n  vol = vol + 1\n  金${gold} #b\n}\n";

#[test]
fn save_and_persistent_data_give_same_state_in_either_order() {
    let reg = registry();
    let prog = compile_ok(ORDER_SRC);
    // Play and save (after saving, only the persistent data advances)
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(5)).unwrap();
    advance(&mut vm);
    let save = vm.save().unwrap();
    resume(&mut vm);
    advance(&mut vm);
    let persistent = vm.save_persistent();
    assert_eq!(var(&vm, "vol"), Value::Int(4));

    // Order 1: persistent data first (passed in the settings)
    let (a, report_a) = Vm::load(
        &prog,
        &reg,
        VmConfig::default().persistent_data(persistent.clone()),
        &save,
    )
    .unwrap();
    // Order 2: save first, persistent data afterwards
    let (mut b, _) = Vm::load(&prog, &reg, VmConfig::default(), &save).unwrap();
    let report_b = b.load_persistent(&persistent).unwrap();
    assert_eq!(report_a.persistent, Some(report_b));
    assert_eq!(a.current(), b.current());
    assert_eq!(a.variables(), b.variables());
    assert_eq!(a.seen_points(), b.seen_points());
    assert_eq!(a.history(), b.history());
    assert_eq!(a.save().unwrap(), b.save().unwrap());
    assert_eq!(a.save_persistent(), b.save_persistent());
    // The arguments were rebuilt with the new persistent data (vol = 4). The random value is the same as when saved
    assert!(text_of(a.current().unwrap()).starts_with("音量4 "));
    let mut a = a;
    assert_eq!(rest(&mut a, 10), rest(&mut b, 10));
}

#[test]
fn when_persistent_load_cannot_rebuild_stop_point_args_it_errors_and_vm_is_unchanged() {
    let reg = registry();
    let src = "persistent var d: int = 1\nlabel start {\n  結果${10 / d} #a\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(advance(&mut vm).as_deref(), Some("結果10"));
    let zero_src = src.replace("= 1\n", "= 0\n");
    let zero_prog = compile_ok(&zero_src);
    let zero = Vm::new(&zero_prog, &reg, VmConfig::default()).unwrap();
    let bytes = zero.save_persistent();
    let before = (vm.variables(), vm.current().cloned(), vm.save().unwrap());
    let e = vm.load_persistent(&bytes).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::StopArgsFailed);
    assert_eq!(
        (vm.variables(), vm.current().cloned(), vm.save().unwrap()),
        before
    );
    // Nothing changed, so it can continue
    assert_eq!(rest(&mut vm, 5), vec!["<end>"]);
}

#[test]
fn load_report_returns_removed_variables_and_type_changed_locals() {
    let reg = registry();
    let old = compile_ok(
        "save var keep: int = 1\nsave var gone: int = 2\nlabel start {\n  let x = 5\n  あ ${x} #a\n}\n",
    );
    let mut vm = Vm::new(&old, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    // gone disappears, and the type of local variable x changes
    let new =
        compile_ok("save var keep: int = 1\nlabel start {\n  let x = \"五\"\n  あ ${x} #a\n}\n");
    let (vm2, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.removed_vars, vec!["main.gone"]);
    assert_eq!(report.type_changed_locals, vec!["main.start.x"]);
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert!(!vm2.is_stopped());
    assert!(report.type_changed_vars.is_empty() && report.added_vars.is_empty());
}

#[test]
fn load_report_includes_type_changed_locals_of_call_stack_frames() {
    let reg = registry();
    let old = "label start {\n  let a = 1\n  call f()\n}\nlabel f {\n  あ #p\n}\n";
    let prog = compile_ok(old);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    let new = "label start {\n  let a = true\n  call f()\n}\nlabel f {\n  あ #p\n}\n";
    let prog2 = compile_ok(new);
    let (_, report) = Vm::load(&prog2, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.type_changed_locals, vec!["main.start.a"]);
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
}

#[test]
fn save_with_save_var_turned_persistent_drops_it_and_reads_from_persistent_data() {
    let reg = registry();
    let old = compile_ok("save var v: int = 1\nlabel start {\n  v = 7\n  あ${v} #a\n}\n");
    let mut vm = Vm::new(&old, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    let new = compile_ok("persistent var v: int = 1\nlabel start {\n  v = 7\n  あ${v} #a\n}\n");
    let (vm2, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.removed_vars, vec!["main.v"]);
    assert_eq!(var(&vm2, "v"), Value::Int(1));
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

const ROLLBACK_SRC: &str = "persistent var best: int = 0\nsave var n: int = 0\nlabel start {\n  let k = 100\n  n += 1\n  best += 10\n  一 ${n} ${best} #a\n  n += 1\n  best += 10\n  call sub(k)\n  三 ${n} ${best} #c\n  jump other\n}\nlabel sub(m: int) {\n  n += 100\n  二 ${m} #b\n}\nlabel other {\n  四 ${n} #d\n}\n";

#[test]
fn cannot_snapshot_when_not_stopped() {
    let reg = registry();
    let prog = compile_ok(ROLLBACK_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.snapshot().unwrap_err().kind, VmErrorKind::NotStopped);
    advance(&mut vm);
    assert!(vm.snapshot().is_ok());
    resume(&mut vm);
    assert_eq!(vm.snapshot().unwrap_err().kind, VmErrorKind::NotStopped);
}

#[test]
fn rollback_restores_save_state_but_not_persistent_var_or_seen() {
    let reg = registry();
    let prog = compile_ok(ROLLBACK_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(9)).unwrap();
    advance(&mut vm); // 1
    let snap = vm.snapshot().unwrap();
    let at_snap = (
        var(&vm, "n"),
        vm.visited_labels(),
        vm.history().to_vec(),
        vm.current().cloned(),
    );
    // Advance (into a subroutine, and to another label)
    resume(&mut vm);
    advance(&mut vm); // 2 (inside call)
    assert_eq!(vm.call_depth(), 1);
    resume(&mut vm);
    advance(&mut vm); // 3
    resume(&mut vm);
    advance(&mut vm); // 4
    assert_eq!(var(&vm, "best"), Value::Int(20));
    assert_eq!(
        vm.seen_points(),
        vec!["main.other#d", "main.start#a", "main.start#c", "main.sub#b"]
    );
    vm.restore(&snap).unwrap();
    // The save state is as of the snapshot
    assert_eq!(var(&vm, "n"), at_snap.0);
    assert_eq!(vm.visited_labels(), at_snap.1);
    assert_eq!(vm.history(), &at_snap.2[..]);
    assert_eq!(vm.current().cloned(), at_snap.3);
    assert_eq!(vm.call_depth(), 0);
    assert_eq!(vm.label_name(), "start");
    // Persistent data does not go back
    assert_eq!(var(&vm, "best"), Value::Int(20));
    assert_eq!(
        vm.seen_points(),
        vec!["main.other#d", "main.start#a", "main.start#c", "main.sub#b"]
    );
}

#[test]
fn continuation_after_rollback_matches_save_and_load() {
    let reg = registry();
    let prog = compile_ok(ROLLBACK_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(3)).unwrap();
    advance(&mut vm);
    let snap = vm.snapshot().unwrap();
    let save = vm.save().unwrap();
    let persistent_then = vm.save_persistent();
    let want = {
        let (mut fresh, _) = Vm::load(
            &prog,
            &reg,
            VmConfig::default().persistent_data(persistent_then.clone()),
            &save,
        )
        .unwrap();
        (
            rest(&mut fresh, 20),
            fresh.history().to_vec(),
            fresh.save_persistent(),
        )
    };
    // Advance and then go back. Persistent data does not go back, so only that part is put back to the saved one before
    // comparing
    resume(&mut vm);
    advance(&mut vm);
    resume(&mut vm);
    advance(&mut vm);
    vm.restore(&snap).unwrap();
    vm.load_persistent(&persistent_then).unwrap();
    assert_eq!(vm.save().unwrap(), save);
    let got = (
        rest(&mut vm, 20),
        vm.history().to_vec(),
        vm.save_persistent(),
    );
    assert_eq!(got, want);
}

#[test]
fn rollback_restores_chosen_once() {
    let reg = registry();
    let src = "label start {\n  choice {\n    「a」 once #a => {\n      あ #x\n    }\n    「b」 #b => {\n      い #y\n    }\n  }\n  jump start\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    vm.resume(Input::Return(Value::Int(0))).unwrap(); // a
    vm.run().unwrap();
    assert_eq!(vm.chosen_once(), vec!["main.start#a".to_string()]);
    vm.restore(&snap).unwrap();
    assert!(vm.chosen_once().is_empty());
    // The list of the choice is also restored, and it can resume
    vm.resume(Input::Return(Value::Int(0))).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "あ");
}

#[test]
fn can_restore_from_snapshot_and_continue_after_runtime_error() {
    let reg = registry();
    let src = "save var d: int = 1\nlabel start {\n  あ #a\n  d = 0\n  ${10 / d}\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "d"), Value::Int(1));
    assert_eq!(rest(&mut vm, 5), vec!["<division_by_zero>"]);
}

#[test]
fn snapshot_of_another_program_cannot_be_restored() {
    let reg = registry();
    let a = compile_ok("label start {\n  あ #a\n}\n");
    let b = compile_ok("save var x: int = 1\nlabel start {\n  あ #a\n}\nlabel two {\n  い\n}\n");
    let mut va = Vm::new(&a, &reg, VmConfig::default()).unwrap();
    advance(&mut va);
    let snap = va.snapshot().unwrap();
    let mut vb = Vm::new(&b, &reg, VmConfig::default()).unwrap();
    advance(&mut vb);
    let before = vb.save().unwrap();
    assert_eq!(
        vb.restore(&snap).unwrap_err().kind,
        VmErrorKind::SnapshotMismatch
    );
    assert_eq!(vb.save().unwrap(), before);
}

#[test]
fn snapshot_ring_drops_oldest_beyond_capacity() {
    let reg = registry();
    let prog =
        compile_ok("save var n: int = 0\nlabel start {\n  n += 1\n  あ ${n}\n  jump start\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut log = SnapshotLog::new(3);
    assert!(log.is_empty() && log.capacity() == 3);
    for _ in 0..5 {
        advance(&mut vm);
        log.push(vm.snapshot().unwrap());
        resume(&mut vm);
    }
    assert_eq!(log.len(), 3);
    // Newest first: n = 5, 4, 3
    for want in [5, 4, 3] {
        let snap = log.pop_back().unwrap();
        vm.restore(&snap).unwrap();
        assert_eq!(var(&vm, "n"), Value::Int(want));
    }
    assert!(log.pop_back().is_none());
    // A limit of 0 holds nothing
    let mut none = SnapshotLog::new(0);
    resume(&mut vm);
    advance(&mut vm);
    none.push(vm.snapshot().unwrap());
    assert!(none.is_empty() && none.back().is_none());
    log.push(vm.snapshot().unwrap());
    log.clear();
    assert!(log.is_empty());
}

// ---------------------------------------------------------------------------
// Random numbers
// ---------------------------------------------------------------------------

const RANDOM_SRC: &str = "save var n: int = 0\nlabel start {\n  n = random(1, 6)\n  a ${n} #a\n  n = random(-3, 3)\n  b ${n} #b\n  c ${random(10, 20)} ${random(0, 0)} #c\n}\n";

#[test]
fn random_returns_integers_in_inclusive_range() {
    let reg = registry();
    let prog = compile_ok(
        "save var n: int = 0\nsave var seenlow: bool = false\nsave var seenhigh: bool = false\nlabel start {\n  let i = 0\n  while i < 300 {\n    n = random(1, 3)\n    if n < 1 || n > 3 {\n      バグ\n    }\n    if n == 1 {\n      seenlow = true\n    }\n    if n == 3 {\n      seenhigh = true\n    }\n    i += 1\n  }\n  終 ${seenlow} ${seenhigh} #e\n}\n",
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(advance(&mut vm).as_deref(), Some("終 true true"));
}

#[test]
fn same_seed_same_sequence_different_seed_different_sequence() {
    let reg = registry();
    let prog = compile_ok(RANDOM_SRC);
    let run = |seed: u64| {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(seed)).unwrap();
        let mut out = Vec::new();
        while let Some(t) = advance(&mut vm) {
            out.push(t);
            resume(&mut vm);
        }
        out
    };
    assert_eq!(run(42), run(42));
    let any_different = (1..20).any(|s| run(s) != run(42));
    assert!(any_different);
}

#[test]
fn random_min_greater_than_max_is_runtime_error() {
    let reg = registry();
    let prog = compile_ok("label start {\n  let n = random(5, 4)\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::InvalidRandomRange { min: 5, max: 4 });
    assert_eq!(e.kind.code(), "invalid_random_range");
    assert_eq!(e.label.as_deref(), Some("start"));
    assert_eq!(e.line, Some(2));
}

#[test]
fn rng_state_is_saved_and_restore_continues_same_sequence() {
    let reg = registry();
    let prog = compile_ok(RANDOM_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(77)).unwrap();
    advance(&mut vm);
    resume(&mut vm);
    advance(&mut vm); // b
    let bytes = vm.save().unwrap();
    // A restored VM continues from the save's random state, regardless of the seed
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default().seed(1), &bytes).unwrap();
    assert_eq!(vm2.current(), vm.current());
    assert_eq!(rest(&mut vm, 10), rest(&mut vm2, 10));
}

#[test]
fn random_in_stop_args_gives_same_value_on_load_without_overadvancing() {
    let reg = registry();
    let prog = compile_ok(RANDOM_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(1234)).unwrap();
    advance(&mut vm);
    resume(&mut vm);
    advance(&mut vm);
    resume(&mut vm);
    let c = advance(&mut vm).unwrap(); // c: random twice in the arguments
    let bytes = vm.save().unwrap();
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(text_of(vm2.current().unwrap()), c);
    // Saving again gives the same bytes (the random state stays as of when computing the arguments began)
    assert_eq!(vm2.save().unwrap(), bytes);
    // The values of `${expr}` in the history are the same too
    assert_eq!(vm2.history(), vm.history());
}

#[test]
fn rollback_restores_random_and_draws_same_sequence_again() {
    let reg = registry();
    let prog = compile_ok(RANDOM_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().seed(2)).unwrap();
    advance(&mut vm);
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    let first = rest(&mut vm, 10);
    vm.restore(&snap).unwrap();
    let second = rest(&mut vm, 10);
    assert_eq!(first, second);
}

#[test]
fn save_with_broken_all_zero_rng_state_is_broken() {
    let reg = registry();
    let prog = compile_ok("label start {\n  あ #a\n}\n");
    let vm = Vm::new(&prog, &reg, VmConfig::default().seed(0)).unwrap();
    let mut vm = vm;
    advance(&mut vm);
    let mut bytes = vm.save().unwrap();
    // Make a body whose random state (two u64s, variable-length integers) is zero
    let rng = vm.args_rng.state();
    let mut body = postcard::to_stdvec(&rng).unwrap();
    let at = bytes
        .windows(body.len())
        .position(|w| w == body.as_slice())
        .expect("乱数の状態がある");
    body.iter_mut().for_each(|b| *b = 0);
    bytes[at..at + body.len()].copy_from_slice(&body);
    let n = bytes.len() - 4;
    let crc = crate::vm::save::crc32(&bytes[..n]);
    bytes[n..].copy_from_slice(&crc.to_le_bytes());
    let e = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert!(matches!(
        e.kind,
        LoadErrorKind::Corrupt | LoadErrorKind::ChecksumMismatch
    ));
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

const HISTORY_SRC: &str = "identity 案内人 = \"あんないにん\"\nsave var n: int = 7\nlabel start {\n  素 ${n} と ${n + 1} #p1\n  案内人「やあ ${n * 2}」 #p2\n  @game.give(\"薬${n}\", n)\n  choice {\n    「はい ${n}」 #yes => {\n      終 #e\n    }\n    「いいえ」 #no => {\n      終 #e2\n    }\n  }\n}\n";

#[test]
fn each_stop_point_records_key_command_args_and_interpolated_values_in_history() {
    let reg = registry();
    let prog = compile_ok(HISTORY_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(vm.history().is_empty());
    advance(&mut vm);
    assert_eq!(vm.history().len(), 1);
    resume(&mut vm);
    advance(&mut vm);
    resume(&mut vm);
    advance(&mut vm); // game.give
    resume(&mut vm);
    advance(&mut vm); // choice
    let h = vm.history();
    assert_eq!(h.len(), 4);
    assert_eq!(h[0].key.as_deref(), Some("main.start#p1"));
    assert_eq!(h[0].command, "ui.say");
    assert_eq!(h[0].args, vec![Value::Str("素 7 と 8".into())]);
    assert_eq!(h[0].values, vec![vec!["7".to_string(), "8".to_string()]]);
    assert_eq!(h[1].key.as_deref(), Some("main.start#p2"));
    assert_eq!(h[1].command, "ui.say_as");
    assert_eq!(h[1].values, vec![vec!["14".to_string()]]);
    // Stop commands: one per string literal argument
    assert_eq!(h[2].key, None);
    assert_eq!(h[2].command, "game.give");
    assert_eq!(h[2].args, vec![Value::Str("薬7".into()), Value::Int(7)]);
    assert_eq!(h[2].values, vec![vec!["7".to_string()]]);
    // Choices: one per option included in the list. No key
    assert_eq!(h[3].key, None);
    assert_eq!(h[3].command, "ui.choose");
    assert_eq!(
        h[3].values,
        vec![vec!["7".to_string()], Vec::<String>::new()]
    );
}

#[test]
fn history_drops_oldest_beyond_limit_and_records_nothing_at_0() {
    let reg = registry();
    let prog =
        compile_ok("save var n: int = 0\nlabel start {\n  n += 1\n  あ ${n}\n  jump start\n}\n");
    for (limit, want_first) in [(3usize, 8i64), (1, 10), (200, 1)] {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default().history_limit(limit)).unwrap();
        let steps = if limit == 200 { 5 } else { 10 };
        for _ in 0..steps {
            advance(&mut vm);
            resume(&mut vm);
        }
        let want_first = if limit == 200 { 1 } else { want_first };
        assert_eq!(vm.history().len(), limit.min(steps));
        assert_eq!(
            vm.history()[0].args[0],
            Value::Str(format!("あ {want_first}").into()),
            "limit={limit}"
        );
    }
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().history_limit(0)).unwrap();
    advance(&mut vm);
    assert!(vm.history().is_empty());
    assert!(vm.save().is_ok());
}

#[test]
fn history_is_saved_and_restored() {
    let reg = registry();
    let prog = compile_ok(HISTORY_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        advance(&mut vm);
        resume(&mut vm);
    }
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(vm2.history(), vm.history());
    assert_eq!(vm2.history().len(), 4);
    // Reading with a setting with a small limit keeps only the newer ones
    let (vm3, _) = Vm::load(&prog, &reg, VmConfig::default().history_limit(2), &bytes).unwrap();
    assert_eq!(vm3.history(), &vm.history()[2..]);
}

#[test]
fn history_goes_back_with_rollback() {
    let reg = registry();
    let prog = compile_ok(HISTORY_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    advance(&mut vm);
    assert_eq!(vm.history().len(), 2);
    vm.restore(&snap).unwrap();
    assert_eq!(vm.history().len(), 1);
    resume(&mut vm);
    advance(&mut vm);
    assert_eq!(vm.history().len(), 2);
}

#[test]
fn history_modified_after_snapshot_reverts_on_restore() {
    let reg = registry();
    let prog = compile_ok(HISTORY_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    // Advance to the choice stop point (plain line, quote line, @game.give, choice)
    for _ in 0..3 {
        advance(&mut vm);
        resume(&mut vm);
    }
    advance(&mut vm);
    let snap = vm.snapshot().unwrap();
    let before = vm.history().to_vec();
    assert_eq!(before.last().unwrap().ret, None);
    // After resuming, the return value goes into the record of the current stop point (a record shared with a snapshot is
    // copied before modifying)
    resume(&mut vm);
    assert_eq!(vm.history().last().unwrap().ret, Some(Value::Int(0)));
    assert_eq!(before.last().unwrap().ret, None);
    vm.restore(&snap).unwrap();
    assert_eq!(vm.history(), before.as_slice());
    assert_eq!(vm.history().last().unwrap().ret, None);
    // Records that were not modified stay shared with the snapshot
    assert!(std::sync::Arc::ptr_eq(&vm.history()[0], &before[0]));
}

#[test]
fn history_args_with_option_lists_and_identity_survive_save_and_restore() {
    let reg = registry();
    let prog = compile_ok(HISTORY_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        advance(&mut vm);
        resume(&mut vm);
    }
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    let h = vm2.history();
    assert_eq!(
        h[1].args[0],
        Value::Option(Some(Box::new(Value::Identity(std::sync::Arc::new(
            crate::Identity {
                id: "案内人".into(),
                name: "あんないにん".into()
            }
        )))))
    );
    assert_eq!(
        h[3].args,
        vec![
            Value::List(
                vec![
                    Value::Str("「はい 7」".into()),
                    Value::Str("「いいえ」".into())
                ]
                .into()
            ),
            Value::List(
                vec![
                    Value::Option(Some(Box::new(Value::Str("yes".into())))),
                    Value::Option(Some(Box::new(Value::Str("no".into()))))
                ]
                .into()
            )
        ]
    );
}

#[test]
fn save_format_version_is_12_and_magic_is_yoms() {
    let reg = registry();
    let prog = compile_ok("label start {\n  あ #a\n}\n");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    advance(&mut vm);
    let bytes = vm.save().unwrap();
    assert_eq!(&bytes[..4], b"YOMS");
    assert_eq!(&bytes[4..8], &12u32.to_le_bytes());
    let p = vm.save_persistent();
    assert_eq!(&p[..4], b"YOMP");
    assert_eq!(&p[4..8], &5u32.to_le_bytes());
}
