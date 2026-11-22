//! Tests of invariants of packages' state slices and host-declared variables.
//!
//! Using a hand-written seeded random generator, makes random scripts that call package commands, and checks the
//! following.
//! - Whichever stop point it is saved and loaded at, the sequence of following stop points and the state at the end
//!   (slices, persistent slices, variables) are the same.
//! - Rollback restores state slices and save variables, and does not restore persistent slices and persistent variables.
//! - Breaking the bytes of saves and persistent data does not make loading panic (`Ok` or `Err`).
//! - The same script, input and seed give the same result (determinism).
//!
//! Environment variables: `YOMI_INVARIANT_CASES` (number of cases per test; default 200), `YOMI_INVARIANT_SEED` (seed).

mod common;
use common::crc32;
use common::rng::{self, Rng};

use common::{
    Counter, CounterPersistent, CounterState, Cued, Mirror, MirrorState, Other, cued_registry,
    full_registry,
};
use yomi_core::{
    Input, PackageItem, Program, Registry, RegistryError, Value, Vm, VmConfig, Yield, compile,
};

fn case_count() -> usize {
    rng::case_count_or(200)
}

fn base_seed() -> u64 {
    rng::base_seed_or(0x5EED_9A55)
}

/// Makes a script that calls package commands.
fn gen_script(rng: &mut Rng) -> String {
    let mut out = String::from("import counter\nimport mirror\n\nlabel start {\n");
    let mut next_id = 0;
    let lines = 4 + rng.below(14);
    gen_block(rng, &mut out, lines, 1, &mut next_id);
    out.push_str("}\n");
    out
}

fn gen_block(rng: &mut Rng, out: &mut String, lines: usize, depth: usize, next_id: &mut usize) {
    let pad = "  ".repeat(depth);
    for _ in 0..lines {
        let line = match rng.below(17) {
            0 => format!("@counter.inc({})", rng.range(-5, 9)),
            1 => format!("@counter.add_total({})", rng.range(0, 9)),
            2 => format!("@counter.roll({})", rng.range(1, 1000)),
            3 => format!("@counter.note(\"メモ{}\")", rng.below(100)),
            4 => "@mirror.copy()".to_string(),
            5 => format!("counter.step += {}", rng.range(0, 5)),
            6 => format!("counter.best += {}", rng.range(0, 5)),
            7 => format!("counter.title = \"題{}\"", rng.below(10)),
            8 | 9 => {
                *next_id += 1;
                format!(
                    "値 ${{counter.get()}} ${{mirror.double()}} ${{counter.total()}} ${{counter.step}} ${{counter.best}} ${{counter.title}} #id{next_id}"
                )
            }
            10 if depth < 3 => {
                let mut inner = String::new();
                let count = 1 + rng.below(3);
                gen_block(rng, &mut inner, count, depth + 1, next_id);
                format!(
                    "if counter.get() > {} {{\n{inner}{pad}}} else {{\n  {pad}@counter.inc(1)\n{pad}}}",
                    rng.range(-3, 20)
                )
            }
            11 if depth < 3 => {
                let mut inner = String::new();
                let count = 1 + rng.below(3);
                gen_block(rng, &mut inner, count, depth + 1, next_id);
                let n = 1 + rng.below(3);
                *next_id += 1;
                let v = format!("i{next_id}");
                format!("let {v} = 0\n{pad}while {v} < {n} {{\n{inner}{pad}  {v} += 1\n{pad}}}")
            }
            12 => format!("@counter.wait({})", rng.below(4)),
            // Receive the return values of effect commands. Put the received values into host-declared variables, a memo and the
            // state
            13 => {
                *next_id += 1;
                format!(
                    "let r{next_id} = @counter.roll({})\n{pad}counter.step = r{next_id}",
                    rng.range(1, 50)
                )
            }
            14 => format!("counter.step = @counter.roll({})", rng.range(1, 50)),
            15 => {
                *next_id += 1;
                format!(
                    "let q{next_id}: int = @counter.roll({})\n{pad}@counter.note(\"出目${{q{next_id}}}\")",
                    rng.range(1, 50)
                )
            }
            _ => format!("そのとき {}", rng.below(100)),
        };
        out.push_str(&pad);
        out.push_str(&line);
        out.push('\n');
    }
}

/// The state of packages at some point (a bundle for comparison).
#[derive(Debug, Clone, PartialEq)]
struct Observed {
    counter: CounterState,
    counter_persistent: CounterPersistent,
    mirror: MirrorState,
    vars: Vec<(String, Value)>,
}

fn observe(vm: &Vm<'_>, reg: &Registry) -> Observed {
    Observed {
        counter: vm.package_state::<Counter>().unwrap().clone(),
        counter_persistent: vm.package_persistent::<Counter>().unwrap().clone(),
        mirror: vm.package_state::<Mirror>().unwrap().clone(),
        vars: reg
            .vars()
            .iter()
            .map(|v| (v.name.clone(), vm.get_var(&v.name).unwrap()))
            .collect(),
    }
}

/// Runs to the end and lists the stop notifications (runtime errors become codes).
fn play(vm: &mut Vm<'_>, limit: usize) -> Vec<Result<Yield, String>> {
    let mut out = Vec::new();
    for _ in 0..limit {
        let step = vm.run().map_err(|e| e.kind.code().to_string());
        let done = match &step {
            Ok(Yield::End) | Err(_) => true,
            Ok(_) => {
                vm.resume(Input::Return(Value::Unit)).expect("再開できる");
                false
            }
        };
        out.push(step);
        if done {
            break;
        }
    }
    out
}

fn config(seed: u64) -> VmConfig {
    VmConfig::default()
        .max_steps_per_run(Some(100_000))
        .seed(seed)
        .start("main.start")
}

fn compile_script(src: &str, reg: &Registry) -> Program {
    match yomi_core::compile_modules(&[("main", src)], reg) {
        Ok(p) => p,
        Err(d) => panic!("コンパイルできない: {d:?}\n{src}"),
    }
}

/// At each stop point, collects (save, persistent data, the state at that time). Also returns the sequence of stop points
/// and the state at the end after running to the end.
struct Recorded {
    stops: Vec<Result<Yield, String>>,
    saves: Vec<(Vec<u8>, Vec<u8>, Observed)>,
    last: Observed,
}

fn record(prog: &Program, reg: &Registry, seed: u64) -> Recorded {
    let mut vm = Vm::new(prog, reg, config(seed)).unwrap();
    let mut stops = Vec::new();
    let mut saves = Vec::new();
    for _ in 0..500 {
        let step = vm.run().map_err(|e| e.kind.code().to_string());
        match &step {
            Ok(Yield::Stop { .. }) => {
                saves.push((vm.save().unwrap(), vm.save_persistent(), observe(&vm, reg)));
                vm.resume(Input::Return(Value::Unit)).unwrap();
                stops.push(step);
            }
            _ => {
                stops.push(step);
                break;
            }
        }
    }
    Recorded {
        stops,
        saves,
        last: observe(&vm, reg),
    }
}

#[test]
fn saving_and_loading_at_any_stop_point_gives_same_continuation_and_final_state() {
    let reg = full_registry();
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() + i as u64);
        let src = gen_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let seed = rng.next();
        let whole = record(&prog, &reg, seed);
        for (k, (save, persistent, at_stop)) in whole.saves.iter().enumerate() {
            let cfg = config(seed).persistent_data(persistent.clone());
            let (mut vm, _) = Vm::load(&prog, &reg, cfg, save)
                .unwrap_or_else(|e| panic!("読み込めない（停止点 {k}）: {e}\n{src}"));
            assert!(vm.is_stopped(), "停止点 {k} で止まっている\n{src}");
            assert_eq!(
                &observe(&vm, &reg),
                at_stop,
                "読み込み直後の状態が、保存した時点と同じ（停止点 {k}）\n{src}"
            );
            // Continuation: the stop notifications from stop point k onward are the same as the continuation of an uninterrupted run
            vm.resume(Input::Return(Value::Unit)).unwrap();
            let rest = play(&mut vm, 500);
            assert_eq!(
                rest,
                whole.stops[k + 1..],
                "停止点 {k} からの続きが同じ\n{src}"
            );
            assert_eq!(
                observe(&vm, &reg),
                whole.last,
                "終わりの状態が同じ（停止点 {k}）\n{src}"
            );
        }
    }
}

#[test]
fn same_script_and_seed_give_same_stop_points_and_state() {
    let reg = full_registry();
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() ^ (0xABCD + i as u64));
        let src = gen_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let seed = rng.next();
        let a = record(&prog, &reg, seed);
        let b = record(&prog, &reg, seed);
        assert_eq!(a.stops, b.stops, "{src}");
        assert_eq!(a.last, b.last, "{src}");
        for (x, y) in a.saves.iter().zip(&b.saves) {
            assert_eq!(x.0, y.0, "セーブのバイト列も同じ\n{src}");
            assert_eq!(x.1, y.1, "永続データのバイト列も同じ\n{src}");
        }
    }
}

#[test]
fn rollback_restores_state_slices_and_variables_but_not_persistent_ones() {
    let reg = full_registry();
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() ^ (0x1357 + i as u64));
        let src = gen_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let seed = rng.next();
        // Take a snapshot at the k-th stop point. After running to the end, go back
        let probe = record(&prog, &reg, seed);
        if probe.saves.is_empty() {
            continue;
        }
        let k = rng.below(probe.saves.len());
        let mut vm = Vm::new(&prog, &reg, config(seed)).unwrap();
        let mut snapshot = None;
        let mut at_snapshot = None;
        let mut n = 0;
        while let Yield::Stop { .. } = vm.run().unwrap() {
            if n == k {
                snapshot = Some(vm.snapshot().unwrap());
                at_snapshot = Some(observe(&vm, &reg));
            }
            n += 1;
            vm.resume(Input::Return(Value::Unit)).unwrap();
        }
        let (snapshot, at_snapshot) = (snapshot.unwrap(), at_snapshot.unwrap());
        let before = observe(&vm, &reg);
        vm.restore(&snapshot).unwrap();
        let after = observe(&vm, &reg);
        assert_eq!(after.counter, at_snapshot.counter, "状態が戻る\n{src}");
        assert_eq!(after.mirror, at_snapshot.mirror, "状態が戻る\n{src}");
        assert_eq!(
            after.counter_persistent, before.counter_persistent,
            "永続スライスは戻らない\n{src}"
        );
        for ((name, v_after), ((_, v_before), (_, v_snap))) in after
            .vars
            .iter()
            .zip(before.vars.iter().zip(&at_snapshot.vars))
        {
            let persistent = reg
                .vars()
                .iter()
                .any(|d| d.name == *name && d.kind == yomi_core::Kind::Persistent);
            if persistent {
                assert_eq!(v_after, v_before, "永続の変数 {name} は戻らない\n{src}");
            } else {
                assert_eq!(v_after, v_snap, "変数 {name} は戻る\n{src}");
            }
        }
        // It can continue running after going back. Persistent variables do not go back, so even the text may differ; compare
        // only the number of stops and how it ended
        vm.resume(Input::Return(Value::Unit)).unwrap();
        let rest = play(&mut vm, 500);
        assert_eq!(
            rest.len(),
            probe.stops[k + 1..].len(),
            "戻した続きの停止の数が同じ\n{src}"
        );
        assert!(
            matches!(rest.last(), Some(Ok(Yield::End))),
            "最後まで動く\n{src}"
        );
    }
}

/// Breaks bytes randomly (keeping the first 4 bytes `YOMS` / `YOMP`, the version and the checksum, and breaking the
/// body).
fn corrupt(bytes: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let body = 8..out.len().saturating_sub(4);
    if body.len() > 1 {
        for _ in 0..1 + rng.below(3) {
            let at = body.start + rng.below(body.len());
            match rng.below(3) {
                0 => out[at] = rng.next() as u8,
                1 => out[at] ^= 1 << rng.below(8),
                _ => {
                    out.truncate(at.max(8));
                    out.extend_from_slice(&[0, 0, 0, 0]);
                    break;
                }
            }
        }
    }
    // Recompute the checksum (so it gets as far as validating the body)
    if out.len() >= 12 {
        let n = out.len() - 4;
        let crc = crc32(&out[..n]);
        out[n..].copy_from_slice(&crc.to_le_bytes());
    }
    out
}

#[test]
fn loading_broken_saves_or_persistent_data_does_not_panic() {
    let reg = full_registry();
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() ^ (0x2468 + i as u64));
        let src = gen_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let seed = rng.next();
        let whole = record(&prog, &reg, seed);
        if whole.saves.is_empty() {
            continue;
        }
        let (save, persistent, _) = &whole.saves[rng.below(whole.saves.len())];
        for _ in 0..8 {
            let bad_save = corrupt(save, &mut rng);
            let bad_persistent = corrupt(persistent, &mut rng);
            // A broken save and valid persistent data
            let _ = Vm::load(
                &prog,
                &reg,
                config(seed).persistent_data(persistent.clone()),
                &bad_save,
            );
            // A valid save and broken persistent data
            let _ = Vm::load(
                &prog,
                &reg,
                config(seed).persistent_data(bad_persistent.clone()),
                save,
            );
            // Both broken, and `load_persistent`
            let _ = Vm::load(
                &prog,
                &reg,
                config(seed).persistent_data(bad_persistent.clone()),
                &bad_save,
            );
            if let Ok(mut vm) = Vm::new(&prog, &reg, config(seed)) {
                let _ = vm.run();
                let _ = vm.load_persistent(&bad_persistent);
                // Even if it could be read, it can keep running (does not panic)
                let _ = play(&mut vm, 50);
            }
        }
    }
}

#[test]
fn generated_scripts_call_package_commands_and_stop_at_checked_stop_commands() {
    let reg = full_registry();
    let mut saw_wait = 0;
    let mut saw_stop = 0;
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() + i as u64);
        let src = gen_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let rec = record(&prog, &reg, 1);
        for s in &rec.stops {
            if let Ok(Yield::Stop { command, .. }) = s {
                saw_stop += 1;
                if reg.command(*command).name == "counter.wait" {
                    saw_wait += 1;
                }
            }
        }
        assert!(
            rec.stops
                .last()
                .is_some_and(|s| matches!(s, Ok(Yield::End))),
            "最後まで動く\n{src}"
        );
    }
    assert!(saw_stop > 0 && saw_wait > 0, "{saw_stop} {saw_wait}");
    let _ = compile("", "main", &reg);
}

// ---------------------------------------------------------------------------
// Cues and bulk registration
// ---------------------------------------------------------------------------

/// Makes a script with only statements that add cues and stop points, and also returns the cues that should be visible at
/// each stop point (those added after the previous stop point).
fn gen_cued_script(rng: &mut Rng) -> (String, Vec<Vec<String>>) {
    let mut src = String::from("import cued\n\nlabel start {\n");
    let mut expected = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for k in 0..(3 + rng.below(12)) {
        for _ in 0..rng.below(4) {
            let name = format!("c{}", rng.below(50));
            src.push_str(&format!("  @cued.cue(\"{name}\")\n"));
            pending.push(name);
        }
        src.push_str(&format!("  行{k} #s{k}\n"));
        expected.push(std::mem::take(&mut pending));
    }
    src.push_str("}\n");
    (src, expected)
}

fn cued_of(vm: &Vm<'_>) -> common::CuedState {
    vm.package_state::<Cued>().unwrap().clone()
}

#[test]
fn cues_hold_only_previous_interval_per_stop_point_and_empty_after_resume_load_and_rollback() {
    let reg = cued_registry();
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() ^ (0xC0E5 + i as u64));
        let (src, expected) = gen_cued_script(&mut rng);
        let prog = compile_script(&src, &reg);
        let seed = rng.next();
        let mut vm = Vm::new(&prog, &reg, config(seed)).unwrap();
        let mut total = 0;
        let mut snaps = Vec::new();
        for (k, want) in expected.iter().enumerate() {
            let y = vm.run().unwrap();
            assert!(matches!(y, Yield::Stop { .. }), "{src}");
            let at = cued_of(&vm);
            assert_eq!(&at.cues, want, "停止点 {k} の合図\n{src}");
            total += want.len() as i64;
            assert_eq!(at.count, total, "足した回数は空にならない\n{src}");
            // Load: only the cues become empty; the rest is the same
            let (loaded, _) = Vm::load(&prog, &reg, config(seed), &vm.save().unwrap()).unwrap();
            let l = cued_of(&loaded);
            assert!(l.cues.is_empty(), "ロード直後は空（停止点 {k}）\n{src}");
            assert_eq!(l.count, at.count, "{src}");
            snaps.push((vm.snapshot().unwrap(), at.count));
            // Empty right after resume
            vm.resume(Input::Return(Value::Unit)).unwrap();
            let after = cued_of(&vm);
            assert!(after.cues.is_empty(), "resume 直後は空\n{src}");
            assert_eq!(after.count, at.count, "{src}");
        }
        assert!(matches!(vm.run().unwrap(), Yield::End), "{src}");
        // Rollback: whichever stop point it goes back to, the cues are empty and the count of additions goes back
        for (k, (snap, count)) in snaps.iter().enumerate() {
            vm.restore(snap).unwrap();
            let r = cued_of(&vm);
            assert!(
                r.cues.is_empty(),
                "ロールバック直後は空（停止点 {k}）\n{src}"
            );
            assert_eq!(r.count, *count, "足した回数が戻る\n{src}");
        }
    }
}

#[test]
fn package_set_puts_dependencies_first_in_any_order_and_unsatisfiable_set_keeps_registry() {
    // 0: counter, 1: other, 2: mirror (depends on counter), 3: cued
    let items = [
        PackageItem::of::<Counter>(),
        PackageItem::of::<Other>(),
        PackageItem::of::<Mirror>(),
        PackageItem::of::<Cued>(),
    ];
    for i in 0..case_count() {
        let mut rng = Rng::new(base_seed() ^ (0xBA7C + i as u64));
        // Whether counter is registered beforehand
        let pre_counter = rng.below(3) == 0;
        let mut reg = Registry::new();
        if pre_counter {
            reg.package::<Counter>().unwrap();
        }
        // Arrange a subset without duplicates in random order
        let mut picks: Vec<usize> = (0..4)
            .filter(|&k| !(pre_counter && k == 0) && rng.below(4) != 0)
            .collect();
        for k in (1..picks.len()).rev() {
            picks.swap(k, rng.below(k + 1));
        }
        let before: Vec<String> = reg.packages().map(|p| p.name).collect();
        let set: Vec<PackageItem> = picks.iter().map(|&k| items[k]).collect();
        let has_counter = pre_counter || picks.contains(&0);
        let result = reg.package_set(&set);
        let after: Vec<String> = reg.packages().map(|p| p.name).collect();
        if picks.contains(&2) && !has_counter {
            assert!(
                matches!(result, Err(RegistryError::MissingDependency { .. })),
                "{picks:?} {result:?}"
            );
            assert_eq!(after, before, "失敗したら変わらない {picks:?}");
        } else {
            result.unwrap_or_else(|e| panic!("{picks:?}: {e:?}"));
            assert_eq!(after.len(), before.len() + picks.len(), "{picks:?}");
            // Dependencies come before the packages that depend on them
            if let Some(m) = after.iter().position(|n| n == "mirror") {
                let c = after.iter().position(|n| n == "counter").unwrap();
                assert!(c < m, "{after:?}");
            }
            // The registered commands can be looked up
            if picks.contains(&2) {
                assert!(reg.command_kind("mirror.copy").is_some());
            }
        }
    }
}
