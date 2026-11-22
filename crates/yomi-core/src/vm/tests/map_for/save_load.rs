//! Save, load and rollback

use super::*;

pub(super) const MAP_STATE: &str = "enum D { N, S }\nsave var inv: map<str, int> = map { \"薬\": 1, \"鍵\": 2 }\nsave var seen: map<D, list<str>> = map {}\nlabel start {\n  inv[\"剣\"] = 1\n  inv.remove(\"薬\")\n  inv[\"薬\"] = 7\n  seen[D.S] = [\"a\"]\n  一 #a\n  inv[\"盾\"] = 3\n  inv.remove(\"鍵\")\n  seen[D.N] = []\n  二 #b\n  ${inv} ${seen}\n}\n";

#[test]
fn map_variables_continue_in_insertion_order_after_save_and_reload() {
    let reg = registry();
    let prog = compile_ok(MAP_STATE);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(text_of(&vm.run().unwrap()), "一");
    let bytes = vm.save().unwrap();
    assert_eq!(var(&vm, "inv").to_text(), "map { 鍵: 2, 剣: 1, 薬: 7 }");
    let rest = run_all(&mut vm);
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert!(report.type_changed_vars.is_empty() && report.changed_types.is_empty());
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(var(&vm2, "inv").to_text(), "map { 鍵: 2, 剣: 1, 薬: 7 }");
    assert_eq!(run_all(&mut vm2), rest);
    assert_eq!(
        rest,
        [
            "二",
            "map { 剣: 1, 薬: 7, 盾: 3 } map { D.S: [a], D.N: [] }",
            "<end>"
        ]
    );
}

#[test]
fn saves_of_same_state_are_identical_bytes() {
    let reg = registry();
    let prog = compile_ok(MAP_STATE);
    let mut a = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut b = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    a.run().unwrap();
    b.run().unwrap();
    assert_eq!(a.save().unwrap(), b.save().unwrap());
}

#[test]
fn map_stays_as_of_snapshot_after_rollback() {
    let reg = registry();
    let prog = compile_ok(MAP_STATE);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    let before = var(&vm, "inv").to_text();
    let first = run_all(&mut vm);
    assert_ne!(var(&vm, "inv").to_text(), before);
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "inv").to_text(), before);
    assert_eq!(run_all(&mut vm), first);
}

#[test]
fn persistent_data_can_hold_maps() {
    let src = "persistent var best: map<str, int> = map {}\nlabel start {\n  best[\"b\"] = 1\n  best[\"a\"] = 2\n  一 #a\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save_persistent();
    assert_eq!(&bytes[4..8], &5u32.to_le_bytes());
    let mut vm2 = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let report = vm2.load_persistent(&bytes).unwrap();
    assert!(report.changed_types.is_empty());
    assert_eq!(var(&vm2, "best").to_text(), "map { b: 1, a: 2 }");
}

#[test]
fn variable_whose_type_inside_map_changed_is_reset_to_initial_value() {
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    let old = "save var m: map<str, int> = map { \"a\": 1 }\nsave var n: map<str, int> = map { \"a\": 1 }\nlabel start {\n  m[\"b\"] = 2\n  n[\"b\"] = 2\n  一 #a\n}\n";
    let new = "save var m: map<str, str> = map { \"x\": \"y\" }\nsave var n: map<str, int> = map { \"a\": 1 }\nlabel start {\n  一 #a\n}\n";
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
    let (vm2, report) = Vm::load(
        new_prog,
        reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
        &bytes,
    )
    .unwrap();
    assert_eq!(report.type_changed_vars, ["main.m"]);
    assert_eq!(var(&vm2, "m").to_text(), "map { x: y }");
    assert_eq!(var(&vm2, "n").to_text(), "map { a: 1, b: 2 }");
}

#[test]
fn changed_struct_key_or_value_of_map_type_appears_in_changed_types() {
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    let old = "struct K { a: int }\nstruct V { b: int }\nsave var m: map<K, V> = map {}\nsave var p: map<str, V> = map {}\nsave var ok: map<str, int> = map {}\nlabel start {\n  一 #a\n}\n";
    let new = "struct K { a: str }\nstruct V { b: int }\nsave var m: map<K, V> = map {}\nsave var p: map<str, V> = map {}\nsave var ok: map<str, int> = map {}\nlabel start {\n  一 #a\n}\n";
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
    let (_, report) = Vm::load(
        new_prog,
        reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
        &bytes,
    )
    .unwrap();
    assert_eq!(report.changed_types, ["main.K"]);
    assert_eq!(report.type_changed_vars, ["main.m"]);
}

#[test]
fn broken_saves_are_rejected_even_inside_map_contents() {
    let reg = registry();
    let prog = compile_ok(MAP_STATE);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0xff;
        let _ = Vm::load(&prog, &reg, VmConfig::default(), &b);
    }
    for n in 0..bytes.len() {
        let _ = Vm::load(&prog, &reg, VmConfig::default(), &bytes[..n]);
    }
}

#[test]
fn saved_map_with_duplicate_keys_is_rejected_as_broken() {
    use crate::vm::save::{SavedNode, SavedValue};
    // Two pairs: key 1 with value "a", and key 1 with value "b"
    let dup = SavedValue(vec![
        SavedNode::Int(1),
        SavedNode::Str("a".into()),
        SavedNode::Int(1),
        SavedNode::Str("b".into()),
        SavedNode::Map(2),
    ]);
    assert!(dup.decode().is_none());
    let ok = SavedValue(vec![
        SavedNode::Int(1),
        SavedNode::Str("a".into()),
        SavedNode::Int(2),
        SavedNode::Str("b".into()),
        SavedNode::Map(2),
    ]);
    assert_eq!(ok.decode().unwrap().to_text(), "map { 1: a, 2: b }");
    // Not enough children
    assert!(
        SavedValue(vec![SavedNode::Int(1), SavedNode::Map(1)])
            .decode()
            .is_none()
    );
    assert!(
        SavedValue(vec![SavedNode::Map(u32::MAX)])
            .decode()
            .is_none()
    );
}

#[test]
fn saves_with_too_deep_maps_are_rejected_and_up_to_limit_restore() {
    use crate::vm::save::{SavedNode, SavedValue};
    let nest = |depth: usize| {
        let mut nodes = vec![SavedNode::Int(0)];
        for _ in 0..depth {
            // Key 0 with value = the previous value
            nodes.insert(nodes.len() - 1, SavedNode::Int(0));
            nodes.push(SavedNode::Map(1));
        }
        SavedValue(nodes)
    };
    assert!(nest(60).decode().is_some());
    assert!(nest(100).decode().is_none());
}

pub(super) const FOR_SRC: &str = "save var total: int = 0\nlabel start {\n  for i in range(1, 4) {\n    for j in [10, 20] {\n      total += i * j\n      ${i}-${j} #p\n    }\n  }\n  合計 ${total}\n}\n";

#[test]
fn saving_and_reloading_at_stop_point_in_for_continues_identically() {
    let reg = registry();
    let prog = compile_ok(FOR_SRC);
    let full = {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        run_all(&mut vm)
    };
    assert_eq!(full.len(), 8);
    // Whichever stop point it is saved at, the continuation after reading it back is the same
    for stop in 0..6 {
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        let mut seen = Vec::new();
        for _ in 0..=stop {
            let y = vm.run().unwrap();
            seen.push(text_of(&y));
            if seen.len() <= stop {
                resume(&mut vm);
            }
        }
        let bytes = vm.save().unwrap();
        let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
        assert_ne!(report.resolved_by, ResolvedBy::LabelStart, "停止点 {stop}");
        let rest = run_all(&mut vm2);
        let mut all = seen.clone();
        all.extend(rest);
        assert_eq!(all, full, "停止点 {stop}");
    }
}

#[test]
fn for_iteration_state_restores_copied_list_position_and_loop_variable() {
    let src = "label start {\n  let xs = [1, 2, 3]\n  for x in xs {\n    xs.push(9)\n    ${x} #a\n  }\n  ${xs}\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), "2");
    let bytes = vm.save().unwrap();
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(var(&vm2, "x"), Value::Int(2));
    // It keeps iterating the copied list ([1, 2, 3] at the start), so values added to xs do not appear in the iteration
    assert_eq!(run_all(&mut vm2), ["3", "[1, 2, 3, 9, 9, 9]", "<end>"]);
}

#[test]
fn mid_for_save_also_goes_back_with_rollback() {
    let reg = registry();
    let prog = compile_ok(FOR_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    resume(&mut vm);
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    let first = run_all(&mut vm);
    vm.restore(&snap).unwrap();
    assert_eq!(run_all(&mut vm), first);
}

#[test]
fn for_can_be_saved_in_middle_of_call() {
    let src = "label start {\n  let r = call sum(3)\n  結果 ${r}\n}\n\nlabel sum(n: int) -> int {\n  let t = 0\n  for i in range(0, n) {\n    t += i\n    ${i} #a\n  }\n  return t\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    resume(&mut vm);
    assert_eq!(text_of(&vm.run().unwrap()), "1");
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_ne!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(run_all(&mut vm2), ["2", "結果 3", "<end>"]);
}

#[test]
fn changed_script_returns_to_label_start_when_hidden_for_locals_mismatch() {
    let reg: &'static Registry = Box::leak(Box::new(registry()));
    let old = "label start {\n  for x in [1, 2] {\n    ${x} #a\n  }\n}\n";
    // Adding a for changes the number of local variables
    let more = "label start {\n  for y in [1] {\n  }\n  for x in [1, 2] {\n    ${x} #a\n  }\n}\n";
    // The element type changes
    let typed = "label start {\n  for x in [\"a\", \"b\"] {\n    ${x} #a\n  }\n}\n";
    let old_prog = compile_ok(old);
    let mut vm = Vm::new(&old_prog, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    for new in [more, typed] {
        let new_prog: &'static Program = Box::leak(Box::new(compile_ok(new)));
        let (vm2, report) = Vm::load(
            new_prog,
            reg,
            VmConfig::default().load_policy(LoadPolicy::Lenient),
            &bytes,
        )
        .unwrap();
        assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
        assert!(!vm2.is_stopped());
        // The hidden name (`$for0.list`) does not appear in the report of local variables whose type changed
        assert!(
            report.type_changed_locals.iter().all(|n| !n.contains('$')),
            "{:?}",
            report.type_changed_locals
        );
    }
    // The same script can be restored
    let (_, report) = Vm::load(&old_prog, reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
}

#[test]
fn broken_saved_iteration_position_runs_without_panic() {
    let reg = registry();
    let prog = compile_ok(FOR_SRC);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0x55;
        // Rejected because the CRC does not match. Even with the CRC fixed, it does not panic
        let n = b.len() - 4;
        let crc = crate::vm::save::crc32(&b[..n]);
        b[n..].copy_from_slice(&crc.to_le_bytes());
        if let Ok((mut v, _)) = Vm::load(&prog, &reg, VmConfig::default(), &b) {
            let _ = run_all(&mut v);
        }
    }
}
