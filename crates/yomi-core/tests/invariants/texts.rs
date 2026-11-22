//! Text replacement (localization)

use super::*;

/// A random translation. The `n` of `${n}` is 1 to `max` (not used if `max` is 0). Escapes are mixed in.
pub(super) fn random_translation(rng: &mut Rng, max: usize) -> String {
    let pieces = [
        "a", "あ", "「", "」", "\\「", "\\\\", "\\$", "$", "x y", "#", "\\#", "\\=",
    ];
    let mut out = String::new();
    for _ in 0..rng.below(5) {
        if max > 0 && rng.chance(1, 3) {
            write!(out, "${{{}}}", rng.range(1, max as i64)).unwrap();
        } else {
            out.push_str(rng.pick(&pieces));
        }
    }
    out
}

/// A random table for the script's keys (only some keys, and keys not in the script). `${n}` that do not fit the script
/// are fixed.
pub(super) fn random_table(prog: &Program, rng: &mut Rng) -> yomi_core::TextTable {
    let template = yomi_core::TextTable::template(prog, "xx");
    let mut table = yomi_core::TextTable::new("xx");
    for (key, _) in template.iter() {
        if rng.chance(2, 3) {
            table
                .insert(key, random_translation(rng, 3))
                .expect("生成した訳文は解析できる");
        }
    }
    for i in 0..rng.below(3) {
        table
            .insert(format!("nothing.nowhere#z{i}"), random_translation(rng, 3))
            .unwrap();
    }
    // Translations with number errors are replaced with translations without numbers
    for issue in table.check(prog) {
        if matches!(issue.kind, yomi_core::TextIssueKind::BadNumber { .. }) {
            table.insert(issue.key, "fixed").unwrap();
        }
    }
    table
}

/// The skeleton of a stop notification (command, ID and number of arguments), compared regardless of whether arguments
/// are without bodies (not replaced) or may change by replacement.
pub(super) fn skeleton(y: &Yield) -> String {
    match y {
        Yield::Stop { command, args, id } => {
            let lens: Vec<String> = args
                .iter()
                .map(|a| match a {
                    Value::List(items) => format!("list{}", items.len()),
                    _ => "v".to_string(),
                })
                .collect();
            format!("{command:?}/{id:?}/{lens:?}")
        }
        _ => "end".to_string(),
    }
}

/// A record of one stop point of the reference VM (notification, answer, save, variables, history).
pub(super) type Recorded = (
    Step,
    Option<Value>,
    Vec<u8>,
    Vec<yomi_core::Variable>,
    Vec<std::sync::Arc<yomi_core::StopRecord>>,
);

/// Checks that having a text table, switching it, and `refresh_current` do not affect positions, variables, saves, the
/// sequence of stop points or the history.
pub(super) fn check_texts_invariants(prog: &Program, reg: &Registry, rng: &mut Rng, what: &str) {
    // Reference: run without a table, and record the stop notifications and answers, and the save, variables and history at
    // each stop point
    let mut base = Vm::new(prog, reg, config()).expect("作れる");
    let mut script: Vec<Recorded> = Vec::new();
    for _ in 0..400 {
        let s = step(&mut base);
        match &s {
            Ok(Yield::End) | Err(_) => {
                script.push((s, None, Vec::new(), Vec::new(), Vec::new()));
                break;
            }
            Ok(y) => {
                let v = answer(reg, y);
                let saved = base.save().expect("保存できる");
                script.push((
                    s.clone(),
                    Some(v.clone()),
                    saved,
                    base.variables(),
                    base.history().to_vec(),
                ));
                base.resume(Input::Return(v)).expect("再開できる");
            }
        }
    }
    let identity = yomi_core::TextTable::template(prog, "id");
    // 1. A table of the originals as they are (a template) is the same as no table in every respect
    // 2. Random tables and switching do not change the skeleton, saves, variables or history, and with `None` it matches the
    // original
    for mode in 0..2 {
        let mut vm = Vm::new(prog, reg, config()).expect("作れる");
        let mut table: Option<yomi_core::TextTable> = None;
        if mode == 0 {
            vm.set_texts(Some(identity.clone()))
                .expect("ひな形は設定できる");
            table = Some(identity.clone());
        }
        for (i, (expected, ans, saved, vars, history)) in script.iter().enumerate() {
            if mode == 1 && rng.chance(1, 2) {
                // Switch the table when not stopped (before `run`)
                table = match rng.below(3) {
                    0 => None,
                    1 => Some(random_table(prog, rng)),
                    _ => Some(identity.clone()),
                };
                vm.set_texts(table.clone()).expect("設定できる");
            }
            let got = step(&mut vm);
            let ctx = format!("{what} mode={mode} 停止点 {i}");
            match (&got, expected) {
                (Ok(g), Ok(e)) => {
                    assert_eq!(skeleton(g), skeleton(e), "{ctx}");
                    if table.is_none() || mode == 0 {
                        assert_eq!(g, e, "{ctx}: 原文（ひな形）と同じ知らせ");
                    }
                }
                (Err(g), Err(e)) => assert_eq!(g, e, "{ctx}"),
                _ => panic!("{ctx}: 知らせの種類が違う {got:?} {expected:?}"),
            }
            let Some(ans) = ans else { break };
            // Operations while stopped: switching the table and `refresh_current`
            if rng.chance(1, 2) {
                if mode == 1 && rng.chance(1, 2) {
                    table = if rng.chance(1, 3) {
                        None
                    } else {
                        Some(random_table(prog, rng))
                    };
                    vm.set_texts(table.clone()).expect("設定できる");
                }
                let refreshed = vm.refresh_current().expect("停止中は呼べる");
                assert_eq!(
                    skeleton(&refreshed),
                    skeleton(expected.as_ref().unwrap()),
                    "{ctx}"
                );
                if table.is_none() {
                    assert_eq!(&refreshed, expected.as_ref().unwrap(), "{ctx}: 原文に戻る");
                }
                assert_eq!(vm.current(), Some(&refreshed), "{ctx}");
                // With the same table, rebuilding any number of times gives the same result
                assert_eq!(vm.refresh_current().unwrap(), refreshed, "{ctx}");
            }
            // Positions, variables, saves and history are not affected by the table
            assert_eq!(&vm.save().expect("保存できる"), saved, "{ctx}: セーブ");
            assert_eq!(&vm.variables(), vars, "{ctx}: 変数");
            assert_eq!(vm.history(), history.as_slice(), "{ctx}: 履歴");
            // Re-rendering the history does not change the number of entries, and with a table of originals gives the originals
            for r in vm.history() {
                assert_eq!(vm.render(r).len(), r.args.len(), "{ctx}");
                if table.is_none() || mode == 0 {
                    assert_eq!(vm.render(r), r.args, "{ctx}: 履歴の描画");
                }
            }
            // Now and then, save, load it into a VM without a table, and go back to the same stop point
            if rng.chance(1, 8) {
                let (mut loaded, _) = Vm::load(
                    prog,
                    reg,
                    config().persistent_data(vm.save_persistent()),
                    &vm.save().unwrap(),
                )
                .expect("読める");
                let cur = loaded.current().cloned();
                assert_eq!(
                    cur.as_ref().map(skeleton),
                    expected.as_ref().ok().map(skeleton),
                    "{ctx}: ロード"
                );
                if let Some(t) = &table {
                    loaded.set_texts(Some(t.clone())).unwrap();
                    let again = loaded.refresh_current().unwrap();
                    assert_eq!(
                        again,
                        vm.current().cloned().unwrap(),
                        "{ctx}: ロード後の作り直し"
                    );
                }
            }
            vm.resume(Input::Return(ans.clone())).expect("再開できる");
        }
        // Even after the end, the history is the same as without a table
        assert_eq!(
            vm.history(),
            base.history(),
            "{what} mode={mode}: 最後の履歴"
        );
    }
}

#[test]
fn text_table_presence_and_switching_do_not_affect_position_variables_saves_or_stop_points() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + 0x4000 + case as u64;
        let mut rng = Rng::new(seed);
        let n = 1 + rng.below(3);
        let sources = gen_program(&mut rng, n);
        let prog = compile_modules_ok(&sources, &reg);
        let what = format!("seed={seed} ({n} モジュール)");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| {
            check_texts_invariants(&prog, &reg, &mut rng, &what);
        })) {
            eprintln!("--- 失敗した台本（{what}）\n{}", all_sources(&sources));
            std::panic::resume_unwind(p);
        }
    }
}

#[test]
fn generated_scripts_contain_replaceable_text() {
    let reg = registry();
    let n = case_count();
    let mut with_keys = 0;
    for case in 0..n {
        let mut rng = Rng::new(base_seed() + 0x4000 + case as u64);
        let k = 1 + rng.below(3);
        let sources = gen_program(&mut rng, k);
        let prog = compile_modules_ok(&sources, &reg);
        let t = yomi_core::TextTable::template(&prog, "xx");
        // The template passes the check as is
        assert!(t.check(&prog).is_empty(), "seed={case}");
        // It can be written to a file and read back
        assert_eq!(yomi_core::TextTable::from_file(&t.to_file()).unwrap(), t);
        with_keys += usize::from(!t.is_empty());
    }
    if n >= 100 {
        assert!(
            with_keys * 2 >= n,
            "ID つきの本文を持つ台本: {with_keys}/{n}"
        );
    }
}

#[test]
fn reading_random_table_files_does_not_panic() {
    let mut rng = Rng::new(base_seed() ^ 0x7e57);
    let pieces = [
        "@lang en",
        "a\tb",
        "#c",
        "",
        "\t",
        "k\t${1}",
        "k\t\\n",
        "k\t\\",
        "x\t${",
        "@",
        "\r",
        "$",
        "\\\\",
        "日本語\tあ",
    ];
    for _ in 0..case_count() * 10 {
        let mut src = String::new();
        for _ in 0..rng.below(8) {
            src.push_str(rng.pick(&pieces));
            src.push(if rng.chance(1, 5) { '\r' } else { '\n' });
        }
        // If it could be read, it can be written and read back
        if let Ok(t) = yomi_core::TextTable::from_file(&src) {
            assert_eq!(
                yomi_core::TextTable::from_file(&t.to_file()).unwrap(),
                t,
                "{src:?}"
            );
        }
    }
}
