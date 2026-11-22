//! Tests: generated valid scripts

use super::*;

#[test]
fn save_and_restore_continue_identically() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let what = format!("seed={seed}");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}

#[test]
fn same_script_and_input_give_same_stop_points() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let a = trace_of(&compile_ok(&src, &reg), &reg, 2_000);
        let b = trace_of(&compile_ok(&src, &reg), &reg, 2_000);
        assert_eq!(a, b, "seed={seed}\n{src}");
        assert!(!a.is_empty());
    }
}

#[test]
fn generated_scripts_run_to_end_and_rarely_hit_runtime_errors() {
    // Soundness of the generator: most scripts run to the end (`End`).
    let reg = registry();
    let (mut ended, mut stops) = (0, 0);
    let n = case_count();
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        let t = trace_of(&compile_ok(&src, &reg), &reg, 5_000);
        stops += t
            .iter()
            .filter(|s| matches!(s, Ok(Yield::Stop { .. })))
            .count();
        if matches!(t.last(), Some(Ok(Yield::End))) {
            ended += 1;
        }
    }
    assert!(ended * 10 >= n * 9, "最後まで動いたのは {ended}/{n}");
    assert!(stops >= n, "停止点が少なすぎる: {stops}");
}

#[test]
fn stable_ids_of_generated_stop_points_take_priority_on_restore() {
    // Targeting only stop points with IDs, it goes back to the stop point with the same ID even after the script's bodies are
    // rewritten.
    let reg = registry();
    let mut checked = 0;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        let mut found = None;
        for _ in 0..500 {
            match step(&mut vm) {
                Ok(y @ Yield::Stop { .. }) => {
                    if matches!(&y, Yield::Stop { id: Some(_), .. }) {
                        found = Some(y);
                        break;
                    }
                    vm.resume(Input::Return(answer(&reg, &y))).unwrap();
                }
                _ => break,
            }
        }
        let Some(Yield::Stop { id: Some(id), .. }) = found else {
            continue;
        };
        let bytes = vm.save().unwrap();
        // A script with changed bodies (the same IDs). Just replacing `おはよう` with `OHAYO` only changes the hashes of lines
        // without IDs and does not affect lines with IDs.
        let src2 = src.replace("おはよう", "ほげ");
        let Ok(prog2) = compile(&src2, "main", &reg) else {
            continue;
        };
        if let Ok((vm2, report)) = Vm::load(&prog2, &reg, config(), &bytes)
            && let Some(Yield::Stop { id: Some(id2), .. }) = vm2.current()
            && report.resolved_by == ResolvedBy::Id
        {
            assert_eq!(*id2, id, "seed={seed}");
            checked += 1;
        }
    }
    assert!(checked > 0 || case_count() < 20, "確かめた停止点がない");
}

#[test]
fn generated_scripts_contain_stop_return_syntax_and_save_restore_follows_received_values() {
    // Soundness of the generator: `let name = @command(...)` and `name = @command(...)` appear.
    // Saving and loading again at a receiving stop point (`= @game.`) gives the same continuation.
    let reg = registry();
    let n = case_count().max(100);
    let (mut lets, mut assigns, mut checked) = (0, 0, 0);
    for case in 0..n {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        for l in src.lines().map(str::trim_start) {
            if l.contains("= @game.") {
                if l.starts_with("let ") {
                    lets += 1;
                } else {
                    assigns += 1;
                }
            }
        }
        if !src.contains("= @game.") {
            continue;
        }
        let prog = compile_ok(&src, &reg);
        let base = trace_of(&prog, &reg, 2_000);
        // For each receiving stop point, continue with a VM saved and loaded again at that stop point and compare the sequences
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        let mut got: Vec<Step> = Vec::new();
        for _ in 0..2_000 {
            let s = step(&mut vm);
            match &s {
                Ok(Yield::End) | Err(_) => {
                    got.push(s);
                    break;
                }
                Ok(y) => {
                    let binds = matches!(
                        y,
                        Yield::Stop { command, .. }
                            if reg.command(*command).name.starts_with("game.")
                                && reg.command(*command).name != "game.give"
                                && reg.command(*command).name != "game.pause"
                    );
                    if binds && let Ok(bytes) = vm.save() {
                        let cfg = config().persistent_data(vm.save_persistent());
                        let (loaded, _) = Vm::load(&prog, &reg, cfg, &bytes).unwrap();
                        vm = loaded;
                        checked += 1;
                    }
                    let v = answer(&reg, y);
                    got.push(s.clone());
                    vm.resume(Input::Return(v)).expect("再開できる");
                }
            }
        }
        assert_eq!(got, base, "seed={seed}\n{src}");
    }
    assert!(lets > 0 && assigns > 0, "let={lets} 代入={assigns}");
    assert!(checked > 0);
}

#[test]
fn generated_scripts_contain_floats_while_and_break() {
    // Soundness of the generator: the new syntax appears at a reasonable rate.
    let n = case_count().max(100);
    let (mut floats, mut whiles, mut breaks, mut conv) = (0, 0, 0, 0);
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        floats += usize::from(src.contains("var ratio") || src.contains("var 速度"));
        whiles += usize::from(src.contains("while lp"));
        breaks += usize::from(src.contains("break") || src.contains("continue"));
        conv += usize::from(src.contains("to_int(") && src.contains("to_float("));
    }
    assert!(floats * 2 >= n, "小数の変数: {floats}/{n}");
    assert!(whiles * 4 >= n, "while: {whiles}/{n}");
    assert!(breaks * 10 >= n, "break/continue: {breaks}/{n}");
    assert!(conv * 20 >= n, "to_int と to_float: {conv}/{n}");
}

#[test]
fn float_variables_stay_finite_and_never_negative_zero_while_running() {
    let reg = registry();
    let mut seen_floats = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        for _ in 0..2_000 {
            let Ok(y) = step(&mut vm) else { break };
            for v in vm.variables() {
                if let Value::Float(x) = v.value {
                    seen_floats += 1;
                    assert!(x.is_finite(), "seed={seed}: {} = {x}", v.name);
                    assert!(
                        x.to_bits() != (-0.0f64).to_bits(),
                        "seed={seed}: {} が負のゼロ",
                        v.name
                    );
                }
            }
            if matches!(y, Yield::End) {
                break;
            }
            vm.resume(Input::Return(answer(&reg, &y))).unwrap();
        }
    }
    assert!(seen_floats > 0);
}

#[test]
fn compile_returns_same_program_regardless_of_warnings() {
    let reg = registry();
    for case in 0..case_count() {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        let a = compile(&src, "main", &reg).unwrap();
        let (b, warnings) = compile_with_warnings(&src, "main", &reg).unwrap();
        assert_eq!(a, b);
        // Generated scripts do not make variables with the same names as built-in functions. They may make labels that are never
        // called
        assert!(only_falls_through(&warnings), "{warnings:?}");
    }
}

#[test]
fn diagnostics_are_position_ordered_and_err_only_on_errors() {
    let reg = registry();
    let mut rng = Rng::new(base_seed() ^ 0x5151);
    for case in 0..case_count() * 2 {
        let base = gen_script(&mut Rng::new(base_seed() + case as u64));
        let src = if case % 2 == 0 {
            mutate(&base, &mut rng)
        } else {
            random_text(&mut rng)
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            compile_with_warnings(&src, "main", &reg)
        }));
        let Ok(result) = result else {
            panic!("パニック:\n{src}");
        };
        let diags = match &result {
            Ok((_, warnings)) => warnings,
            Err(diags) => {
                assert!(
                    diags
                        .iter()
                        .any(|d| d.severity == yomi_core::Severity::Error),
                    "Err なのにエラーがない:\n{src}"
                );
                diags
            }
        };
        if result.is_ok() {
            assert!(
                diags
                    .iter()
                    .all(|d| d.severity == yomi_core::Severity::Warning)
            );
        }
        let keys: Vec<(u32, u32)> = diags.iter().map(|d| (d.line, d.col)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "位置の順でない:\n{src}");
        assert!(diags.iter().all(|d| d.line >= 1 && d.col >= 1));
    }
}
