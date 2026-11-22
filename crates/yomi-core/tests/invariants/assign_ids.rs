//! assign-ids (automatic IDs)

use super::*;

/// Removes stable IDs (the `id` of lines and the `ids` argument of choices) from a sequence of stop points.
pub(super) fn without_ids(reg: &Registry, steps: Vec<Step>) -> Vec<Step> {
    steps
        .into_iter()
        .map(|s| {
            s.map(|y| match y {
                Yield::Stop {
                    command, mut args, ..
                } => {
                    if reg.command(command).name == "ui.choose"
                        && let Some(Value::List(ids)) = args.get_mut(1)
                    {
                        *ids = vec![Value::Option(None); ids.len()].into();
                    }
                    Yield::Stop {
                        command,
                        args,
                        id: None,
                    }
                }
                other => other,
            })
        })
        .collect()
}

/// Makes a plan and applies it to all modules.
pub(super) fn assign_all(
    sources: &[(String, String)],
    reg: &Registry,
    rng: &mut yomi_core::IdRng,
) -> (Vec<(String, String)>, usize) {
    let refs = as_refs(sources);
    let plan = yomi_core::plan_ids(&refs, reg, &Default::default(), rng)
        .unwrap_or_else(|d| panic!("生成した台本がコンパイルできない: {}", show_diags(&d)));
    let out = sources
        .iter()
        .map(|(m, s)| {
            let mine: Vec<_> = plan.iter().filter(|i| &i.module == m).cloned().collect();
            (m.clone(), yomi_core::apply_insertions(s, &mine))
        })
        .collect();
    (out, plan.len())
}

#[test]
fn assign_ids_on_random_scripts_keeps_stop_points_and_resolution_and_is_idempotent() {
    let reg = registry();
    let mut added = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let n = 1 + rng.below(3);
        let before = gen_program(&mut rng, n);
        let what = format!("seed={seed}");
        let mut ids = yomi_core::IdRng::new(seed);
        let (after, count) = assign_all(&before, &reg, &mut ids);
        added += count;
        // (a) Compilation passes
        let prog_before = compile_modules_ok(&before, &reg);
        let prog_after = compile_modules_ok(&after, &reg);
        // (b) The sequence of stop points (without IDs) is the same
        let t0 = without_ids(&reg, trace_of(&prog_before, &reg, 2_000));
        let t1 = without_ids(&reg, trace_of(&prog_after, &reg, 2_000));
        assert_eq!(
            t0,
            t1,
            "{what}: 停止点の列が変わった\n{}",
            all_sources(&after)
        );
        // (c) Idempotent: the second time adds nothing
        let (again, count2) = assign_all(&after, &reg, &mut yomi_core::IdRng::new(seed + 1));
        assert_eq!(count2, 0, "{what}: 2 回目に付いた");
        assert_eq!(again, after, "{what}: 冪等でない");
        // No lines remain without anything that were there before adding
        assert!(prog_after.missing_ids().is_empty(), "{what}");
        // (d) A save from before adding can be read with the script after adding (stop points without IDs are found by hash)
        let mut vrng = Rng::new(seed ^ 0xABCD);
        for bytes in some_saves(&prog_before, &reg, &mut vrng, 6) {
            let (_, report) = Vm::load(&prog_after, &reg, config(), &bytes)
                .unwrap_or_else(|e| panic!("{what}: 付与後の台本で復元できない: {e}"));
            assert_ne!(
                report.resolved_by,
                ResolvedBy::LabelStart,
                "{what}: 位置を見失った"
            );
        }
    }
    assert!(added > 0, "ID を付ける行が 1 つも生成されなかった");
}

/// Removes the IDs (` #i<number>`) of `once` options. Also returns how many were removed.
pub(super) fn strip_once_ids(src: &str) -> (String, usize) {
    let mut out = String::with_capacity(src.len());
    let mut stripped = 0;
    for line in src.split_inclusive('\n') {
        let head_end = line.find('」').map_or(0, |i| i + '」'.len_utf8());
        let rest = &line[head_end..];
        if rest.starts_with(" once")
            && let Some(at) = rest.find(" #i")
        {
            let digits = rest[at + 3..]
                .chars()
                .take_while(char::is_ascii_digit)
                .count();
            if digits > 0 {
                out.push_str(&line[..head_end]);
                out.push_str(&rest[..at]);
                out.push_str(&rest[at + 3 + digits..]);
                stripped += 1;
                continue;
            }
        }
        out.push_str(line);
    }
    (out, stripped)
}

#[test]
fn assign_ids_on_random_scripts_without_once_ids_compiles_keeps_stop_points_and_is_idempotent() {
    let reg = registry();
    let mut stripped_total = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let n = 1 + rng.below(3);
        let original = gen_program(&mut rng, n);
        let what = format!("seed={seed}");
        let mut stripped: Vec<(String, String)> = Vec::new();
        let mut count = 0;
        for (m, src) in &original {
            let (s, k) = strip_once_ids(src);
            count += k;
            stripped.push((m.clone(), s));
        }
        if count == 0 {
            continue;
        }
        // Removing them gives a compile error of only `once_without_id`
        let refs = as_refs(&stripped);
        let diags = yomi_core::compile_modules(&refs, &reg).unwrap_err();
        // Scripts that name the removed IDs in `seen` and the like change meaning when IDs are replaced (`unknown_position`), so
        // they are excluded
        if diags.iter().any(|d| d.code.as_str() == "unknown_position") {
            continue;
        }
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| d.severity == yomi_core::Severity::Error)
            .collect();
        assert!(
            errors.iter().all(|d| d.code.as_str() == "once_without_id"),
            "{what}: {}",
            show_diags(&errors)
        );
        assert_eq!(errors.len(), count, "{what}");
        stripped_total += count;
        // (a) Adding them makes compilation pass
        let (after, added) = assign_all(&stripped, &reg, &mut yomi_core::IdRng::new(seed));
        assert!(added >= count, "{what}");
        let prog_original = compile_modules_ok(&original, &reg);
        let prog_after = compile_modules_ok(&after, &reg);
        // (b) The sequence of stop points (without IDs) is the same as the script before removing
        let t0 = without_ids(&reg, trace_of(&prog_original, &reg, 2_000));
        let t1 = without_ids(&reg, trace_of(&prog_after, &reg, 2_000));
        assert_eq!(
            t0,
            t1,
            "{what}: 停止点の列が変わった\n{}",
            all_sources(&after)
        );
        // (c) Idempotent
        let (again, count2) = assign_all(&after, &reg, &mut yomi_core::IdRng::new(seed + 1));
        assert_eq!(count2, 0, "{what}: 2 回目に付いた");
        assert_eq!(again, after, "{what}: 冪等でない");
        assert!(prog_after.missing_ids().is_empty(), "{what}");
    }
    assert!(
        stripped_total > 0,
        "once の ID を持つ行が 1 つも生成されなかった"
    );
}
