//! Tests: call, arguments, return values, multiple modules

use super::*;

pub(super) fn as_refs(sources: &[(String, String)]) -> Vec<(&str, &str)> {
    sources
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect()
}

/// Whether the only warnings are `label_falls_through` (labels not called from anywhere; possible in generated scripts).
pub(super) fn only_falls_through(warnings: &[Diagnostic]) -> bool {
    warnings.iter().all(|d| {
        matches!(
            d.code,
            yomi_core::DiagCode::LabelFallsThrough
                | yomi_core::DiagCode::UnusedImport
                | yomi_core::DiagCode::UnusedVariable
        )
    })
}

pub(super) fn compile_modules_ok(sources: &[(String, String)], reg: &Registry) -> Program {
    match compile_modules_with_warnings(&as_refs(sources), reg) {
        Ok((p, warnings)) => {
            assert!(only_falls_through(&warnings), "{warnings:?}");
            p
        }
        Err(d) => {
            let all: String = sources
                .iter()
                .map(|(n, s)| format!("--- {n}\n{s}"))
                .collect();
            panic!(
                "生成したモジュールがコンパイルできない: {}\n{all}",
                show_diags(&d)
            )
        }
    }
}

pub(super) fn all_sources(sources: &[(String, String)]) -> String {
    sources
        .iter()
        .map(|(n, s)| format!("--- {n}\n{s}"))
        .collect()
}

#[test]
fn generated_scripts_contain_call_args_and_return_values() {
    let n = case_count().max(100);
    let (mut calls, mut args, mut rets, mut values, mut early) = (0, 0, 0, 0, 0);
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        calls += usize::from(src.contains("call "));
        args += usize::from(src.contains("(a0: "));
        rets += usize::from(src.contains(") -> "));
        values += usize::from(src.contains("= call "));
        early += usize::from(src.contains("  return"));
    }
    assert!(calls * 2 >= n, "call: {calls}/{n}");
    assert!(args * 4 >= n, "引数: {args}/{n}");
    assert!(rets * 4 >= n, "戻り値: {rets}/{n}");
    assert!(values * 4 >= n, "戻り値を受ける call: {values}/{n}");
    assert!(early * 2 >= n, "return: {early}/{n}");
}

#[test]
fn generated_scripts_have_stop_points_mid_call() {
    let reg = registry();
    let n = case_count().max(100);
    let (mut deep, mut max_depth) = (0, 0);
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        let mut found = false;
        for _ in 0..2_000 {
            let Ok(y @ Yield::Stop { .. }) = step(&mut vm) else {
                break;
            };
            max_depth = max_depth.max(vm.call_depth());
            found |= vm.call_depth() > 0;
            vm.resume(Input::Return(answer(&reg, &y))).unwrap();
        }
        deep += usize::from(found);
    }
    assert!(deep * 4 >= n, "call の途中の停止点がある台本: {deep}/{n}");
    assert!(max_depth >= 2, "深さ {max_depth}");
}

#[test]
fn multi_module_scripts_continue_identically_after_save_restore() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + 0x1000 + case as u64;
        let mut rng = Rng::new(seed);
        let n = 2 + rng.below(2);
        let sources = gen_program(&mut rng, n);
        let prog = compile_modules_ok(&sources, &reg);
        let what = format!("seed={seed} ({n} モジュール)");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{}", all_sources(&sources));
            std::panic::resume_unwind(p);
        }
    }
}

#[test]
fn multi_module_scripts_are_deterministic_and_run_across_modules() {
    let reg = registry();
    let n = case_count();
    let (mut crossed, mut ended) = (0, 0);
    for case in 0..n {
        let seed = base_seed() + 0x2000 + case as u64;
        let mut rng = Rng::new(seed);
        let n_modules = 2 + rng.below(2);
        let sources = gen_program(&mut rng, n_modules);
        let a = trace_of(&compile_modules_ok(&sources, &reg), &reg, 3_000);
        let b = trace_of(&compile_modules_ok(&sources, &reg), &reg, 3_000);
        assert_eq!(a, b, "seed={seed}\n{}", all_sources(&sources));
        // A fair share of scripts contain references across modules (`module.name` and `call module.`)
        let text: String = sources.iter().map(|(_, s)| s.as_str()).collect();
        crossed += usize::from(text.contains("call alpha.") || text.contains("call ベータ."));
        ended += usize::from(matches!(a.last(), Some(Ok(Yield::End))));
    }
    if n >= 100 {
        assert!(crossed * 4 >= n, "モジュールをまたぐ call: {crossed}/{n}");
        assert!(ended * 10 >= n * 9, "最後まで動いたのは {ended}/{n}");
    }
}

/// Makes some saves at stop points. Gives priority to the middle of calls (depth 1 or more).
pub(super) fn deep_saves(
    prog: &Program,
    reg: &Registry,
    rng: &mut Rng,
    max: usize,
) -> Vec<Vec<u8>> {
    let mut vm = Vm::new(prog, reg, config()).unwrap();
    let mut out = Vec::new();
    for _ in 0..300 {
        match step(&mut vm) {
            Ok(y @ Yield::Stop { .. }) => {
                let hit = if vm.call_depth() > 0 {
                    rng.chance(1, 2)
                } else {
                    rng.chance(1, 8)
                };
                if hit {
                    out.push(vm.save().unwrap());
                    if out.len() >= max {
                        break;
                    }
                }
                vm.resume(Input::Return(answer(reg, &y))).unwrap();
            }
            _ => break,
        }
    }
    out
}

/// Keeps running a restored VM (if stopped, resumes first).
pub(super) fn continue_trace(vm: &mut Vm<'_>, reg: &Registry, limit: usize) -> Vec<Step> {
    if let Some(y) = vm.current().cloned() {
        vm.resume(Input::Return(answer(reg, &y))).unwrap();
    }
    trace(vm, reg, limit)
}

/// Changes one module of the script (mutation, deleting a `call` line, swapping lines).
pub(super) fn edit_sources(sources: &[(String, String)], rng: &mut Rng) -> Vec<(String, String)> {
    let mut edited = sources.to_vec();
    let which = rng.below(edited.len());
    let src = &edited[which].1;
    edited[which].1 = match rng.below(4) {
        0 | 1 => mutate(src, rng),
        2 => {
            // Delete the first `call` line
            let mut done = false;
            src.lines()
                .filter(|l| {
                    let hit = !done && l.contains("call ");
                    done |= hit;
                    !hit
                })
                .map(|l| format!("{l}\n"))
                .collect()
        }
        _ => {
            // Swap two adjacent lines
            let mut lines: Vec<&str> = src.lines().collect();
            if lines.len() > 2 {
                let i = rng.below(lines.len() - 1);
                lines.swap(i, i + 1);
            }
            lines.iter().map(|l| format!("{l}\n")).collect()
        }
    };
    edited
}

#[test]
fn save_from_changed_script_does_not_panic_and_returns_to_label_start_or_found_position() {
    let reg = registry();
    let (mut loaded, mut label_start, mut with_frames) = (0usize, 0usize, 0usize);
    for case in 0..case_count() {
        let seed = base_seed() + 0x3000 + case as u64;
        let mut rng = Rng::new(seed);
        let n_modules = 1 + rng.below(3);
        let sources = gen_program(&mut rng, n_modules);
        let prog = compile_modules_ok(&sources, &reg);
        for bytes in deep_saves(&prog, &reg, &mut rng, 2) {
            for _ in 0..4 {
                let edited = edit_sources(&sources, &mut rng);
                let Ok(prog2) = compile_modules(&as_refs(&edited), &reg) else {
                    continue;
                };
                let r = catch_unwind(AssertUnwindSafe(|| {
                    let Ok((mut vm, report)) = Vm::load(&prog2, &reg, config(), &bytes) else {
                        return None;
                    };
                    if report.resolved_by == ResolvedBy::LabelStart {
                        // When it went back to the start of the label, it is not stopped and the call stack is empty
                        assert!(vm.current().is_none() && vm.call_depth() == 0);
                    } else {
                        assert!(vm.current().is_some());
                    }
                    let depth = vm.call_depth();
                    let _ = vm.variables();
                    // Continuing to run (even with the call stack restored) does not panic
                    let t = continue_trace(&mut vm, &reg, 400);
                    assert!(!t.is_empty());
                    Some((report.resolved_by == ResolvedBy::LabelStart, depth))
                }));
                match r {
                    Ok(Some((start, depth))) => {
                        loaded += 1;
                        label_start += usize::from(start);
                        with_frames += usize::from(depth > 0);
                    }
                    Ok(None) => {}
                    Err(p) => {
                        eprintln!(
                            "--- パニックした（seed={seed}）\n変える前:\n{}\n変えたあと:\n{}",
                            all_sources(&sources),
                            all_sources(&edited)
                        );
                        std::panic::resume_unwind(p);
                    }
                }
            }
        }
    }
    if case_count() >= 100 {
        assert!(loaded > 0, "読み込めたセーブがない");
        assert!(label_start > 0, "ラベルの先頭へ戻った例がない");
        assert!(with_frames > 0, "コールスタックごと戻った例がない");
    }
}

#[test]
fn running_and_saving_mutated_multi_module_scripts_does_not_panic() {
    let reg = registry();
    let (mut ok, n) = (0usize, case_count());
    for case in 0..n {
        let seed = base_seed() + 0x4000 + case as u64;
        let mut rng = Rng::new(seed);
        let n_modules = 2 + rng.below(2);
        let sources = gen_program(&mut rng, n_modules);
        for _ in 0..3 {
            let edited = edit_sources(&sources, &mut rng);
            let r = catch_unwind(AssertUnwindSafe(|| {
                let Ok(prog) = compile_modules(&as_refs(&edited), &reg) else {
                    return false;
                };
                let _ = trace_of(&prog, &reg, 600);
                check_save_restore(&prog, &reg, "変形したモジュール");
                true
            }));
            match r {
                Ok(done) => ok += usize::from(done),
                Err(p) => {
                    eprintln!("--- パニックした（seed={seed}）\n{}", all_sources(&edited));
                    std::panic::resume_unwind(p);
                }
            }
        }
    }
    if n >= 100 {
        assert!(ok > 0, "コンパイルできた入力が 1 つもない");
    }
}

#[test]
fn recursion_stops_at_call_depth_and_instruction_limits() {
    // Even random scripts containing recursive `call`s do not exhaust Rust's stack and stop at the limit
    let reg = registry();
    let src = "label start {\n  call f(0)\n}\n\nlabel f(n: int) {\n  call f(n + 1)\n  call g(n)\n}\n\nlabel g(n: int) {\n  call f(n)\n}\n";
    let prog = compile_ok(src, &reg);
    for depth in [1, 2, 50, 256, 100_000] {
        let cfg = config()
            .max_call_depth(depth)
            .max_steps_per_run(Some(10_000_000));
        let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
        assert_eq!(
            vm.run().unwrap_err().kind,
            VmErrorKind::CallDepthExceeded,
            "深さ {depth}"
        );
        assert_eq!(vm.call_depth(), depth as usize);
    }
}
