//! Tests: quote lines, choices, once, seen, visited

use super::*;

#[test]
fn generated_scripts_contain_quote_lines_choices_once_seen_and_visited() {
    let n = case_count().max(100);
    let (mut quotes, mut multi, mut choices, mut onces, mut seens, mut visits) = (0, 0, 0, 0, 0, 0);
    for case in 0..n {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        quotes += usize::from(src.contains("「") && src.contains("identity "));
        multi += usize::from(src.contains("\n\t") || src.contains("\n  ") && src.contains("」 #"));
        choices += usize::from(src.contains("choice {"));
        onces += usize::from(src.contains(" once "));
        seens += usize::from(src.contains("seen(\""));
        visits += usize::from(src.contains("visited(\""));
    }
    assert!(quotes * 2 >= n, "引用行: {quotes}/{n}");
    assert!(multi * 2 >= n, "複数行の引用の候補: {multi}/{n}");
    assert!(choices * 5 >= n, "choice: {choices}/{n}");
    assert!(onces * 10 >= n, "once: {onces}/{n}");
    assert!(seens * 20 >= n, "seen: {seens}/{n}");
    assert!(visits * 10 >= n, "visited: {visits}/{n}");
}

#[test]
fn generated_choices_stop_at_choice_stop_point_when_run() {
    let reg = registry();
    let mut choices = 0usize;
    for case in 0..case_count() {
        let src = gen_script(&mut Rng::new(base_seed() + case as u64));
        for s in trace_of(&compile_ok(&src, &reg), &reg, 2_000) {
            if let Ok(Yield::Stop { command, args, .. }) = s
                && reg.command(command).name == "ui.choose"
            {
                // The list is not empty and has the same length as the list of stable IDs
                let [Value::List(items), Value::List(ids)] = args.as_slice() else {
                    panic!("選択の引数の形が違う: {args:?}");
                };
                assert!(!items.is_empty());
                assert_eq!(items.len(), ids.len());
                choices += 1;
            }
        }
    }
    assert!(choices > 0 || case_count() < 20, "選択の停止点がない");
}

#[test]
fn saving_at_choice_restores_chosen_once_and_visited_labels_as_before_choosing() {
    let reg = registry();
    let mut checked = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        let mut last_once: Vec<String> = Vec::new();
        for _ in 0..1_000 {
            let Ok(y) = step(&mut vm) else { break };
            // once options already chosen do not decrease while running
            let once = vm.chosen_once();
            assert!(
                last_once.iter().all(|k| once.contains(k)),
                "seed={seed}: once が減った {last_once:?} -> {once:?}"
            );
            last_once = once.clone();
            let Yield::Stop { command, .. } = &y else {
                break;
            };
            if reg.command(*command).name == "ui.choose" {
                let bytes = vm.save().unwrap();
                let cfg = config().seen_points(vm.seen_points());
                let (vm2, _) = Vm::load(&prog, &reg, cfg, &bytes).unwrap();
                assert_eq!(vm2.chosen_once(), once, "seed={seed}: once の状態");
                assert_eq!(
                    vm2.visited_labels(),
                    vm.visited_labels(),
                    "seed={seed}: 通ったラベル"
                );
                checked += 1;
            }
            vm.resume(Input::Return(answer(&reg, &y))).unwrap();
        }
    }
    assert!(checked > 0 || case_count() < 20, "確かめた選択がない");
}

#[test]
fn chosen_once_becomes_selectable_after_going_back_before_save() {
    // Save at choice stop point A → choose → go on → load the save from A, and the state of once goes back to that at A.
    let reg = registry();
    let src = "\
identity 案内人 = \"案内人\"

label start {
  choice {
    「見る」 once #look => 見た
    「待つ」 #wait => 待った
  }
  案内人「次」
  end
}
";
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, config()).unwrap();
    let y = step(&mut vm).unwrap();
    let bytes = vm.save().unwrap();
    assert!(vm.chosen_once().is_empty());
    // Choose "見る" (number 0 in the given list)
    assert!(
        matches!(&y, Yield::Stop { args, .. } if matches!(&args[0], Value::List(l) if l.len() == 2))
    );
    vm.resume(Input::Return(Value::Int(0))).unwrap();
    let after = step(&mut vm).unwrap();
    assert!(matches!(&after, Yield::Stop { args, .. } if args == &[Value::Str("見た".into())]));
    assert_eq!(vm.chosen_once(), vec!["main.start#look".to_string()]);
    // Go back
    let (vm2, _) = Vm::load(&prog, &reg, config(), &bytes).unwrap();
    assert!(vm2.chosen_once().is_empty());
    let Some(Yield::Stop { args, .. }) = vm2.current() else {
        panic!("停止点に戻る");
    };
    assert!(matches!(&args[0], Value::List(l) if l.len() == 2));
    // Going back from a save in the already-chosen state (the "見た" stop point), the list is only "待つ"
    let bytes2 = vm.save().unwrap();
    let mut vm3 = Vm::new(&prog, &reg, config()).unwrap();
    let _ = step(&mut vm3).unwrap();
    vm3.resume(Input::Return(Value::Int(0))).unwrap();
    let _ = step(&mut vm3).unwrap();
    assert_eq!(vm3.chosen_once(), vm.chosen_once());
    let (vm4, _) = Vm::load(&prog, &reg, config(), &bytes2).unwrap();
    assert_eq!(vm4.chosen_once(), vm.chosen_once());
}
