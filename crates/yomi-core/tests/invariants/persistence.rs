//! persistent var, persistent data, rollback, random numbers, history

use super::*;

/// Runs the same script from the start up to just before the `n`-th (0-based) stop point.
pub(super) fn fork_at<'a>(
    prog: &'a Program,
    reg: &'a Registry,
    index: usize,
    cfg: VmConfig,
) -> Vm<'a> {
    let mut fork = Vm::new(prog, reg, cfg).unwrap();
    for _ in 0..=index {
        let s = step(&mut fork);
        let v = answer(reg, s.as_ref().unwrap());
        fork.resume(Input::Return(v)).unwrap();
    }
    fork
}

#[test]
fn generated_scripts_contain_persistent_var_and_random() {
    let n = case_count().max(100);
    let (mut persistents, mut randoms, mut interp_randoms) = (0, 0, 0);
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        persistents += usize::from(src.contains("persistent var "));
        randoms += usize::from(src.contains("random("));
        interp_randoms += usize::from(src.contains("${random("));
    }
    assert!(persistents * 2 >= n, "persistent var: {persistents}/{n}");
    assert!(randoms * 2 >= n, "random: {randoms}/{n}");
    let _ = interp_randoms;
}

#[test]
fn same_seed_gives_same_sequence_and_changing_seed_changes_it() {
    let reg = registry();
    let mut differ = 0usize;
    let mut total = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        if !src.contains("random(") {
            continue;
        }
        let prog = compile_ok(&src, &reg);
        let run = |s: u64| {
            let cfg = config().seed(s);
            match Vm::new(&prog, &reg, cfg) {
                Ok(mut vm) => trace(&mut vm, &reg, 2_000),
                Err(e) => vec![Err(e.kind.code().to_string())],
            }
        };
        assert_eq!(run(seed), run(seed), "seed={seed}: 同じ種で列が違う");
        total += 1;
        differ += usize::from(run(seed) != run(seed ^ 0xFFFF));
    }
    // A fair share of scripts have results affected by random numbers (only scripts where random numbers do not show in the
    // result give the same sequence with different seeds)
    assert!(
        total == 0 || differ * 10 >= total,
        "種で変わる台本: {differ}/{total}"
    );
}

#[test]
fn rollback_restores_only_save_state_and_continues_like_save_and_load() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0x4011_u64);
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let what = format!("seed={seed}");
        let r = catch_unwind(AssertUnwindSafe(|| {
            let total = trace_of(&prog, &reg, 2_000)
                .iter()
                .filter(|s| matches!(s, Ok(Yield::Stop { .. })))
                .count();
            let stride = (total / 12).max(1);
            let mut vm = Vm::new(&prog, &reg, config()).unwrap();
            let mut log = yomi_core::SnapshotLog::new(8);
            let mut index = 0usize;
            while let Ok(y @ Yield::Stop { .. }) = step(&mut vm) {
                log.push(vm.snapshot().unwrap());
                if index.is_multiple_of(stride) {
                    let mut probe = vm_at_stop(&prog, &reg, index);
                    check_rollback(&prog, &reg, &mut probe, &mut rng, index, &what);
                }
                vm.resume(Input::Return(answer(&reg, &y))).unwrap();
                index += 1;
                if index > 2_000 {
                    break;
                }
            }
            // The ring does not exceed its limit
            assert!(log.len() <= 8);
        }));
        if let Err(p) = r {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}

/// Makes a VM stopped at the `index`-th (0-based) stop point by running from the start.
pub(super) fn vm_at_stop<'a>(prog: &'a Program, reg: &'a Registry, index: usize) -> Vm<'a> {
    let mut vm = Vm::new(prog, reg, config()).unwrap();
    for _ in 0..index {
        let s = step(&mut vm);
        vm.resume(Input::Return(answer(reg, s.as_ref().unwrap())))
            .unwrap();
    }
    step(&mut vm).unwrap();
    vm
}

/// With a stopped `vm`, takes a snapshot, advances a few steps, goes back, and checks. `vm` is run to the end and
/// discarded.
pub(super) fn check_rollback(
    prog: &Program,
    reg: &Registry,
    vm: &mut Vm<'_>,
    rng: &mut Rng,
    index: usize,
    what: &str,
) {
    let snap = vm.snapshot().unwrap();
    let save_then = vm.save().unwrap();
    let persistent_then = vm.save_persistent();
    let current_then = vm.current().cloned();
    let history_then = vm.history().to_vec();
    let variables_then = vm.variables();
    // Advance a few steps (it may go to the end or an error)
    for _ in 0..1 + rng.below(4) {
        if let Some(y) = vm.current().cloned() {
            vm.resume(Input::Return(answer(reg, &y))).unwrap();
        }
        match step(vm) {
            Ok(Yield::End) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let persistent_after = vm.save_persistent();
    vm.restore(&snap)
        .unwrap_or_else(|e| panic!("{what}: 停止点 {index}: 戻せない {e}"));
    // What does not go back: persistent var and passed stop points
    assert_eq!(
        vm.save_persistent(),
        persistent_after,
        "{what}: 停止点 {index}: 永続データが戻ってしまった"
    );
    // What goes back
    assert_eq!(
        vm.current().cloned(),
        current_then,
        "{what}: 停止点 {index}"
    );
    assert_eq!(vm.history(), &history_then[..], "{what}: 停止点 {index}");
    let save_vars =
        |v: Vec<yomi_core::Variable>| v.into_iter().filter(|x| !x.persistent).collect::<Vec<_>>();
    assert_eq!(
        save_vars(vm.variables()),
        save_vars(variables_then),
        "{what}: 停止点 {index}: save var とローカル変数が戻っていない"
    );
    // If the persistent data is also put back to what it was when saved, the save is the same as when taken (all other state
    // went back). The arguments are rebuilt with the persistent data put back, and become the same.
    // When the persistent data did not change while advancing, compare the continuation as is without putting it back (to
    // directly check that the random state went back)
    if persistent_after != persistent_then {
        vm.load_persistent(&persistent_then)
            .unwrap_or_else(|e| panic!("{what}: 停止点 {index}: 永続データを戻せない {e}"));
    }
    assert_eq!(vm.save().unwrap(), save_then, "{what}: 停止点 {index}");
    assert_eq!(
        vm.current().cloned(),
        current_then,
        "{what}: 停止点 {index}"
    );
    // The continuation is the same as a VM run from the start (at the same stop point)
    let mut fork = fork_at(prog, reg, index, config());
    let a = trace(&mut fork, reg, 2_000);
    let b = continue_trace(vm, reg, 2_000);
    assert_eq!(a, b, "{what}: 停止点 {index}: 戻したあとの続きが違う");
    assert_eq!(
        fork.variables(),
        vm.variables(),
        "{what}: 停止点 {index}: 戻したあとの最終の変数が違う"
    );
    assert_eq!(fork.history(), vm.history(), "{what}: 停止点 {index}");
}

#[test]
fn persistent_data_round_trips_and_loads_wherever_saved() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        for i in 0..200 {
            // It can be saved even when not stopped
            let bytes = vm.save_persistent();
            assert_eq!(bytes, vm.save_persistent(), "seed={seed} 手順 {i}");
            let mut fresh = Vm::new(&prog, &reg, config().persistent_data(bytes.clone()))
                .unwrap_or_else(|e| panic!("seed={seed}: {e}"));
            assert_eq!(fresh.save_persistent(), bytes, "seed={seed} 手順 {i}");
            assert_eq!(fresh.seen_points(), vm.seen_points());
            let persistent_vars = |v: &Vm<'_>| {
                v.variables()
                    .into_iter()
                    .filter(|x| x.persistent)
                    .collect::<Vec<_>>()
            };
            assert_eq!(persistent_vars(&fresh), persistent_vars(&vm));
            // The same even when read later
            let mut late = Vm::new(&prog, &reg, config()).unwrap();
            let report = late.load_persistent(&bytes).unwrap();
            let mut expected = yomi_core::PersistentReport::default();
            expected.from_schema_version = 1;
            expected.to_schema_version = 1;
            assert_eq!(report, expected);
            assert_eq!(late.save_persistent(), bytes);
            fresh.load_persistent(&bytes).unwrap();
            match step(&mut vm) {
                Ok(y @ Yield::Stop { .. }) => vm.resume(Input::Return(answer(&reg, &y))).unwrap(),
                _ => break,
            }
        }
    }
}

#[test]
fn broken_persistent_data_does_not_panic_and_cannot_be_loaded() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0xBEEF);
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        for _ in 0..rng.below(6) {
            match step(&mut vm) {
                Ok(y @ Yield::Stop { .. }) => vm.resume(Input::Return(answer(&reg, &y))).unwrap(),
                _ => break,
            }
        }
        let bytes = vm.save_persistent();
        let before = vm.save_persistent();
        // A 1-bit flip is always caught (CRC-32)
        for _ in 0..8 {
            let mut broken = bytes.clone();
            let pos = rng.below(broken.len());
            broken[pos] ^= 1 << rng.below(8);
            assert!(
                vm.load_persistent(&broken).is_err(),
                "seed={seed} 位置 {pos}"
            );
            assert!(Vm::new(&prog, &reg, config().persistent_data(broken)).is_err());
        }
        for len in [0, 1, 4, 8, 11, bytes.len() - 1] {
            assert!(
                vm.load_persistent(&bytes[..len]).is_err(),
                "seed={seed} 長さ {len}"
            );
        }
        // Break the body and recompute the checksum: an error is fine, but no panic. On failure, the VM does not change
        for _ in 0..8 {
            let mut body = bytes[..bytes.len() - 4].to_vec();
            let pos = 8 + rng.below(body.len() - 8);
            body[pos] ^= 1 << rng.below(8);
            let crc = crc32(&body);
            body.extend_from_slice(&crc.to_le_bytes());
            let r = catch_unwind(AssertUnwindSafe(|| {
                let mut other = Vm::new(&prog, &reg, config()).unwrap();
                let ok = other.load_persistent(&body).is_ok();
                if !ok {
                    assert_eq!(
                        other.save_persistent(),
                        Vm::new(&prog, &reg, config()).unwrap().save_persistent()
                    );
                }
                let _ = trace(&mut other, &reg, 300);
                let _ = Vm::new(&prog, &reg, config().persistent_data(body.clone()));
            }));
            assert!(
                r.is_ok(),
                "seed={seed}: 壊れた永続データでパニック（位置 {pos}）"
            );
        }
        // A failed read does not change the VM
        assert_eq!(vm.save_persistent(), before);
    }
}

#[test]
fn loading_random_persistent_data_does_not_panic() {
    let reg = registry();
    let prog = compile_ok(
        "persistent var a: int = 1\npersistent var b: float = 1.5\nlabel start {\n  あ #a\n}\n",
        &reg,
    );
    let mut rng = Rng::new(base_seed() ^ 0x99);
    for _ in 0..case_count() * 4 {
        let len = rng.below(64);
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
        if rng.chance(1, 2) && bytes.len() >= 8 {
            bytes[..4].copy_from_slice(b"YOMP");
            bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        }
        if rng.chance(1, 2) && bytes.len() >= 12 {
            let n = bytes.len() - 4;
            let crc = crc32(&bytes[..n]);
            bytes[n..].copy_from_slice(&crc.to_le_bytes());
        }
        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut vm = Vm::new(&prog, &reg, config()).unwrap();
            let _ = vm.load_persistent(&bytes);
            let _ = vm.save_persistent();
            let _ = Vm::new(&prog, &reg, config().persistent_data(bytes.clone()));
        }));
        assert!(r.is_ok(), "{bytes:?}");
    }
}

#[test]
fn history_grows_per_stop_point_within_limit_and_survives_save_restore() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let limit = 1 + (seed as usize % 7);
        let mut vm = Vm::new(&prog, &reg, config().history_limit(limit)).unwrap();
        let mut stops = 0usize;
        while let Ok(y @ Yield::Stop { .. }) = step(&mut vm) {
            stops += 1;
            assert_eq!(vm.history().len(), stops.min(limit), "seed={seed}");
            // The last record is the current stop point
            let last = vm.history().last().unwrap();
            if let Yield::Stop { args, .. } = &y {
                assert_eq!(&last.args, args, "seed={seed}");
            }
            if stops % 5 == 1 {
                let bytes = vm.save().unwrap();
                let (vm2, _) = Vm::load(
                    &prog,
                    &reg,
                    config()
                        .history_limit(limit)
                        .persistent_data(vm.save_persistent()),
                    &bytes,
                )
                .unwrap();
                assert_eq!(vm2.history(), vm.history(), "seed={seed}");
            }
            vm.resume(Input::Return(answer(&reg, &y))).unwrap();
            if stops > 2_000 {
                break;
            }
        }
    }
}
