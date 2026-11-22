//! Tests of planning for `assign-ids` (`plan_ids`, `apply_insertions`).

use yomi_core::{
    CompileOptions, IdRng, Identity, Input, Insertion, Registry, ResolvedBy, StopKind, Sugar,
    Value, Vm, VmConfig, Yield, apply_insertions, plan_ids,
};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(f64,), ()>("flow.wait").unwrap();
    r.stop::<(String,), String>("game.ask").unwrap();
    r.text_param("game.ask", 0).unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

fn run(src: &str) -> (Vec<Insertion>, String) {
    let reg = registry();
    let mut rng = IdRng::new(7);
    let ins = plan_ids(&[("main", src)], &reg, &CompileOptions::default(), &mut rng)
        .unwrap_or_else(|d| panic!("コンパイルできる: {d:?}"));
    let out = apply_insertions(src, &ins);
    (ins, out)
}

#[test]
fn assigns_ids_to_plain_quote_choice_and_stop_command_lines() {
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  おはよう\n  案内人「やあ」\n  @flow.wait(1.5)\n  let a = @game.ask(\"x\")\n  choice {\n    「はい」 => jump start\n    「いいえ」 if true => jump start\n  }\n}\n";
    let (ins, out) = run(src);
    assert_eq!(ins.len(), 6);
    let kinds: Vec<StopKind> = ins.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [
            StopKind::Plain,
            StopKind::Quote,
            StopKind::Command,
            StopKind::Command,
            StopKind::Choice,
            StopKind::Choice
        ]
    );
    // Planning again produces nothing
    let (again, out2) = run(&out);
    assert!(again.is_empty(), "{again:?}");
    assert_eq!(out, out2);
    println!("{out}");
}

/// Extracts the added IDs per line (replaces the 6 characters after `#` with `#ID` to make scripts easy to compare).
fn masked(out: &str) -> String {
    let mut s = String::new();
    let b: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '#'
            && i + 6 < b.len() + 1
            && b[i + 1..(i + 7).min(b.len())]
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && (i + 7 >= b.len() || !(b[i + 7].is_ascii_alphanumeric() || b[i + 7] == '_'))
            && i + 7 <= b.len()
            && i > 0
            && (b[i - 1] == ' ')
        {
            s.push_str("#ID");
            i += 7;
        } else {
            s.push(b[i]);
            i += 1;
        }
    }
    s
}

#[test]
fn only_ids_are_appended_and_rest_of_original_text_is_unchanged() {
    let src = "label start {\n  おはよう  \n  「やあ」\n  @flow.wait(1.0)   \n}\n";
    let (_, out) = run(src);
    assert_eq!(
        masked(&out),
        "label start {\n  おはよう #ID  \n  「やあ」 #ID\n  @flow.wait(1.0) #ID   \n}\n"
    );
}

#[test]
fn existing_ids_stay_and_only_missing_lines_get_ids() {
    let src = "label start {\n  おはよう #a\n  「やあ」\n  @flow.wait(1.0) #w\n  choice {\n    「はい」 #yes => jump start\n    「いいえ」 => jump start\n  }\n}\n";
    let (ins, out) = run(src);
    assert_eq!(ins.len(), 2);
    assert!(out.contains("おはよう #a\n"));
    assert!(out.contains("@flow.wait(1.0) #w\n"));
    assert!(out.contains("「はい」 #yes =>"));
}

#[test]
fn multiline_quote_ids_go_after_closing_bracket_of_last_line() {
    let src = "label start {\n  「あいう\n    えお」\n  「か」と言った。\n}\n";
    let (ins, out) = run(src);
    assert_eq!(ins.len(), 2);
    assert_eq!(
        masked(&out),
        "label start {\n  「あいう\n    えお」 #ID\n  「か」と言った。 #ID\n}\n"
    );
}

#[test]
fn option_ids_go_before_arrow_and_survive_after_once_and_if() {
    let src = "label start {\n  choice {\n    「は」=> jump start\n    「い」  if true  => jump start\n    「う」 once #u => jump start\n  }\n}\n";
    let (ins, out) = run(src);
    assert_eq!(ins.len(), 2);
    assert_eq!(
        masked(&out),
        "label start {\n  choice {\n    「は」 #ID => jump start\n    「い」  if true  #ID => jump start\n    「う」 once #u => jump start\n  }\n}\n"
    );
}

#[test]
fn columns_stay_correct_after_full_width_characters() {
    let src = "label start {\n  choice {\n    「あ𠮷い」 if true => { @flow.wait(1.0); jump start }\n  }\n  こんにちは、世界\n}\n";
    let (_, out) = run(src);
    assert_eq!(
        masked(&out),
        "label start {\n  choice {\n    「あ𠮷い」 if true #ID => { @flow.wait(1.0) #ID; jump start }\n  }\n  こんにちは、世界 #ID\n}\n"
    );
    let (again, _) = run(&out);
    assert!(again.is_empty());
}

#[test]
fn keeps_line_endings_and_bom() {
    let src = "\u{feff}label start {\r\n  おはよう\r\n  案内人「x」\r\n  @flow.wait(1.0)\r\n}\r\n";
    let reg = registry();
    let mut rng = IdRng::new(1);
    // 案内人 is not an identity, so it is a plain line
    let ins = plan_ids(&[("main", src)], &reg, &CompileOptions::default(), &mut rng).unwrap();
    let out = apply_insertions(src, &ins);
    assert!(out.starts_with('\u{feff}'));
    assert_eq!(out.matches("\r\n").count(), 5);
    assert_eq!(out.matches('\n').count(), 5);
    assert_eq!(ins.len(), 3);
    assert_eq!(
        masked(&out),
        "\u{feff}label start {\r\n  おはよう #ID\r\n  案内人「x」 #ID\r\n  @flow.wait(1.0) #ID\r\n}\r\n"
    );
    let ins2 = plan_ids(
        &[("main", &out)],
        &reg,
        &CompileOptions::default(),
        &mut rng,
    )
    .unwrap();
    assert!(ins2.is_empty());
}

#[test]
fn can_attach_to_plain_lines_with_whitespace_and_special_starts() {
    let src = "label start {\n  \\=ほげ\n  a \\# b\n  \u{3000}字下げ\n}\n";
    let (ins, out) = run(src);
    assert_eq!(ins.len(), 3);
    let (again, _) = run(&out);
    assert!(again.is_empty(), "{out}");
}

#[test]
fn plans_multiple_modules_at_once_unique_per_label() {
    let reg = registry();
    let a = "import b\nlabel start {\n  あ\n  い\n  jump b.other\n}\n";
    let b = "label other {\n  う\n  え\n}\nlabel start2 {\n  お #x\n}\n";
    let mut rng = IdRng::new(3);
    let ins = plan_ids(
        &[("a", a), ("b", b)],
        &reg,
        &CompileOptions::default(),
        &mut rng,
    )
    .unwrap();
    assert_eq!(ins.len(), 4);
    let mods: Vec<&str> = ins.iter().map(|i| i.module.as_str()).collect();
    assert_eq!(mods, ["a", "a", "b", "b"]);
    let mut seen = std::collections::HashSet::new();
    for i in &ins {
        assert!(seen.insert((i.module.clone(), i.label.clone(), i.id.clone())));
        assert_eq!(i.id.len(), 6);
    }
}

#[test]
fn same_seed_gives_same_ids_unique_within_label() {
    let src = "label start {\n  あ\n  い\n  う\n  え\n  お\n  か\n  き\n  く\n}\n";
    let (a, _) = run(src);
    let (b, _) = run(src);
    assert_eq!(a, b);
    let ids: std::collections::HashSet<&str> = a.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), a.len());
    for i in &a {
        assert!(
            i.id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
    }
}

#[test]
fn id_generation_avoids_used_ids_and_is_seedable() {
    let mut r1 = IdRng::new(42);
    let mut r2 = IdRng::new(42);
    assert_eq!(r1.next_id(), r2.next_id());
    assert_ne!(IdRng::new(1).next_id(), IdRng::new(2).next_id());
    // If the next ID is marked as used, a different ID is produced
    let next = IdRng::new(9).next_id();
    let mut used = std::collections::HashSet::new();
    used.insert(next.clone());
    let got = IdRng::new(9).unique_id(&mut used);
    assert_ne!(got, next);
    assert!(used.contains(&got));
    // It does not get stuck even with a seed of 0
    let mut z = IdRng::new(0);
    assert_ne!(z.next_id(), z.next_id());
}

#[test]
fn no_plan_for_scripts_with_compile_errors() {
    let reg = registry();
    let mut rng = IdRng::new(1);
    let err = plan_ids(
        &[("main", "label start {\n  あ\n  jump nowhere\n}\n")],
        &reg,
        &CompileOptions::default(),
        &mut rng,
    )
    .unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn require_text_ids_passes_after_adding() {
    let reg = registry();
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  あ\n  案内人「い」\n  choice {\n    「は」 => jump start\n    「ひ」 => jump start\n  }\n}\n";
    let strict = CompileOptions::default().require_text_ids(true);
    assert!(compile_modules_strict(src, &reg, &strict).is_err());
    let (_, out) = run(src);
    assert!(compile_modules_strict(&out, &reg, &strict).is_ok());
}

fn compile_modules_strict(
    src: &str,
    reg: &Registry,
    o: &CompileOptions,
) -> Result<(), Vec<yomi_core::Diagnostic>> {
    yomi_core::compile_modules_with_options(&[("main", src)], reg, o).map(|_| ())
}

#[test]
fn can_assign_with_require_text_ids_even_with_sugar_ignoring_registry() {
    let mut reg = Registry::new();
    reg.stop::<(String,), ()>("ui.say").unwrap();
    reg.ignore(Sugar::Plain).unwrap();
    reg.ignore(Sugar::Quote).unwrap();
    reg.ignore(Sugar::Choice).unwrap();
    let strict = CompileOptions::default().require_text_ids(true);
    let src = "label start {\n  あ\n  「い」\n  choice {\n    「は」 => jump start\n  }\n}\n";
    assert!(compile_modules_strict(src, &reg, &strict).is_err());
    let mut rng = IdRng::new(5);
    let ins = plan_ids(&[("main", src)], &reg, &CompileOptions::default(), &mut rng).unwrap();
    assert_eq!(ins.len(), 3);
    let out = apply_insertions(src, &ins);
    assert!(compile_modules_strict(&out, &reg, &strict).is_ok());
}

#[test]
fn saves_from_before_adding_load_with_new_script_and_once_and_seen_unchanged() {
    let reg = registry();
    let src = "label start {\n  あ #first\n  い\n  choice {\n    「u」 once #u => jump start\n    「v」 => jump start\n  }\n}\n";
    let before = yomi_core::compile(src, "main", &reg).unwrap();
    let (_, out) = run(src);
    let after = yomi_core::compile(&out, "main", &reg).unwrap();
    let cfg = || VmConfig::default().start("main.start");
    let mut vm = Vm::new(&before, &reg, cfg()).unwrap();
    let mut saves = Vec::new();
    // Advance to the second choice after choosing "u"
    for _ in 0..5 {
        match vm.run().unwrap() {
            Yield::Stop { args, .. } => {
                saves.push(vm.save().unwrap());
                let pick = 0;
                let ret = if args.len() == 2 {
                    Value::Int(pick)
                } else {
                    Value::Unit
                };
                vm.resume(Input::Return(ret)).unwrap();
            }
            Yield::End => break,
            _ => break,
        }
    }
    assert!(saves.len() >= 4);
    let persistent = vm.save_persistent();
    for (n, bytes) in saves.iter().enumerate() {
        let (vm_old, r_old) = Vm::load(
            &before,
            &reg,
            cfg().persistent_data(persistent.clone()),
            bytes,
        )
        .unwrap();
        let (vm_new, r_new) = Vm::load(
            &after,
            &reg,
            cfg().persistent_data(persistent.clone()),
            bytes,
        )
        .unwrap();
        assert_ne!(r_new.resolved_by, ResolvedBy::LabelStart, "{n}");
        // The list of options (whether removed by once) is the same
        let norm = |y: &Yield| match y.clone() {
            Yield::Stop { args, .. } => args.into_iter().take(1).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        assert_eq!(
            norm(vm_old.current().unwrap()),
            norm(vm_new.current().unwrap()),
            "{n}"
        );
        let _ = r_old;
    }
}

// ---------------------------------------------------------------------------
// A plan can be made even when a `once` option has no ID (`once_without_id`)
// ---------------------------------------------------------------------------

/// Makes a plan and applies it to one module (on error, the list of diagnostic codes).
fn run_codes(src: &str, options: &CompileOptions) -> Result<(Vec<Insertion>, String), Vec<String>> {
    let reg = registry();
    let mut rng = IdRng::new(11);
    match plan_ids(&[("main", src)], &reg, options, &mut rng) {
        Ok(ins) => {
            let out = apply_insertions(src, &ins);
            Ok((ins, out))
        }
        Err(d) => Err(d.iter().map(|d| d.code.as_str().to_string()).collect()),
    }
}

#[test]
fn missing_once_ids_can_be_added_and_script_compiles() {
    let reg = registry();
    let src = "label start {\n  choice {\n    「は」 once => jump start\n    「い」 once if true => jump start\n    「う」 => jump start\n  }\n}\n";
    // Before adding, it cannot compile
    assert!(yomi_core::compile(src, "main", &reg).is_err());
    let (ins, out) = run_codes(src, &CompileOptions::default()).unwrap();
    assert_eq!(ins.len(), 3);
    assert_eq!(
        masked(&out),
        "label start {\n  choice {\n    「は」 once #ID => jump start\n    「い」 once if true #ID => jump start\n    「う」 #ID => jump start\n  }\n}\n"
    );
    assert!(yomi_core::compile(&out, "main", &reg).is_ok());
    // Idempotent
    let (again, out2) = run_codes(&out, &CompileOptions::default()).unwrap();
    assert!(again.is_empty());
    assert_eq!(out, out2);
}

#[test]
fn missing_once_ids_are_added_with_other_missing_ids_and_unique() {
    let src = "label start {\n  あ\n  choice {\n    「は」 once #a1b2c3 => jump start\n    「い」 once => jump start\n    「う」 once => jump start\n  }\n  い\n}\n";
    let (ins, out) = run_codes(src, &CompileOptions::default()).unwrap();
    assert_eq!(ins.len(), 4);
    let kinds: Vec<StopKind> = ins.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [
            StopKind::Plain,
            StopKind::Choice,
            StopKind::Choice,
            StopKind::Plain
        ]
    );
    let ids: std::collections::HashSet<&str> = ins.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), 4);
    assert!(!ids.contains("a1b2c3"));
    assert!(out.contains("「は」 once #a1b2c3 =>"));
}

#[test]
fn no_plan_if_errors_other_than_missing_once_ids() {
    // A type error and once_without_id together
    let src =
        "label start {\n  let n: int = \"a\"\n  choice {\n    「は」 once => jump start\n  }\n}\n";
    let codes = run_codes(src, &CompileOptions::default()).unwrap_err();
    assert!(codes.contains(&"once_without_id".to_string()), "{codes:?}");
    assert!(codes.contains(&"type_mismatch".to_string()), "{codes:?}");
    // Together with an error of duplicated IDs
    let src =
        "label start {\n  あ #x\n  い #x\n  choice {\n    「は」 once => jump start\n  }\n}\n";
    let codes = run_codes(src, &CompileOptions::default()).unwrap_err();
    assert!(
        codes.contains(&"duplicate_stable_id".to_string()),
        "{codes:?}"
    );
    // Even for an error of only a once without a line, no plan is made if a syntax error is present too
    let src = "label start {\n  choice {\n    「は」 once => jump nowhere\n  }\n}\n";
    let codes = run_codes(src, &CompileOptions::default()).unwrap_err();
    assert!(codes.contains(&"unknown_label".to_string()), "{codes:?}");
}

#[test]
fn script_with_added_once_ids_has_same_stop_points_and_once_behavior() {
    let reg = registry();
    let with_ids = "label start {\n  あ\n  choice {\n    「は」 once #u1 => jump start\n    「い」 => 抜けた\n  }\n  い\n}\n";
    let without = "label start {\n  あ\n  choice {\n    「は」 once => jump start\n    「い」 => 抜けた\n  }\n  い\n}\n";
    let (_, assigned) = run_codes(without, &CompileOptions::default()).unwrap();
    let a = yomi_core::compile(with_ids, "main", &reg).unwrap();
    let b = yomi_core::compile(&assigned, "main", &reg).unwrap();
    // With the same choices (choose `once` once, and it cannot be chosen next), the sequence of stop points is the same
    let trace = |p: &yomi_core::Program| {
        let mut vm = Vm::new(p, &reg, VmConfig::default().start("main.start")).unwrap();
        let mut out = Vec::new();
        for _ in 0..20 {
            match vm.run().unwrap() {
                Yield::Stop { args, .. } => {
                    let ret = if args.len() == 2 {
                        out.push(format!("choice{}", args[0].to_text()));
                        Value::Int(0)
                    } else {
                        out.push(args.last().map(Value::to_text).unwrap_or_default());
                        Value::Unit
                    };
                    vm.resume(Input::Return(ret)).unwrap();
                }
                _ => break,
            }
        }
        out
    };
    let (ta, tb) = (trace(&a), trace(&b));
    assert_eq!(ta, tb);
    // In the second round, the only option is the one without `once`
    assert!(
        ta.iter().any(|t| t.contains("choice[「は」, 「い」]")),
        "{ta:?}"
    );
    assert!(ta.iter().any(|t| t == "choice[「い」]"), "{ta:?}");
}

#[test]
fn with_require_text_ids_translatable_prompt_lines_get_ids_and_pass() {
    let strict = CompileOptions::default().require_text_ids(true);
    let src = "label start {\n  あ #a\n  let n = @game.ask(\"名前は？\")\n  @flow.wait(1.0)\n}\n";
    let reg = registry();
    let err = yomi_core::compile_modules_with_options(&[("main", src)], &reg, &strict)
        .map(|_| ())
        .unwrap_err();
    let kinds: Vec<(&str, &str)> = err
        .iter()
        .map(|d| (d.code.as_str(), d.arg_str("kind").unwrap_or("")))
        .collect();
    // Only the prompt line is an error (`@flow.wait` without `text_param` is not required)
    assert_eq!(kinds, vec![("missing_text_id", "command")]);
    let (ins, out) = run_codes(src, &strict).unwrap();
    // `assign-ids` adds IDs to all stop command lines (even those not covered by `require_text_ids`)
    assert_eq!(ins.len(), 2);
    assert!(
        yomi_core::compile_modules_with_options(&[("main", &out)], &reg, &strict).is_ok(),
        "{out}"
    );
}

#[test]
fn missing_once_and_body_ids_can_be_added_together_with_require_text_ids() {
    let strict = CompileOptions::default().require_text_ids(true);
    let src = "label start {\n  あ\n  choice {\n    「は」 once => jump start\n    「い」 => jump start\n  }\n}\n";
    // Before adding, both diagnostics are emitted
    let reg = registry();
    let err = yomi_core::compile_modules_with_options(&[("main", src)], &reg, &strict)
        .map(|_| ())
        .unwrap_err();
    let codes: Vec<&str> = err.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"once_without_id") && codes.contains(&"missing_text_id"));
    let (ins, out) = run_codes(src, &strict).unwrap();
    assert_eq!(ins.len(), 3);
    // After adding, `require_text_ids` also passes
    assert!(
        yomi_core::compile_modules_with_options(&[("main", &out)], &reg, &strict).is_ok(),
        "{out}"
    );
    // If `require_text_ids` is enabled and there are other errors, no plan is made
    let bad = "label start {\n  あ\n  jump nowhere\n}\n";
    assert!(run_codes(bad, &strict).is_err());
}

#[test]
fn once_ids_can_be_assigned_even_with_sugar_ignoring_registry() {
    let mut reg = Registry::new();
    reg.stop::<(String,), ()>("ui.say").unwrap();
    reg.ignore(Sugar::Plain).unwrap();
    reg.ignore(Sugar::Quote).unwrap();
    reg.ignore(Sugar::Choice).unwrap();
    let src = "label start {\n  choice {\n    「は」 once => jump start\n  }\n}\n";
    let mut rng = IdRng::new(5);
    assert!(yomi_core::compile(src, "main", &reg).is_err());
    let ins = plan_ids(&[("main", src)], &reg, &CompileOptions::default(), &mut rng).unwrap();
    assert_eq!(ins.len(), 1);
    let out = apply_insertions(src, &ins);
    assert!(yomi_core::compile(&out, "main", &reg).is_ok());
}

#[test]
fn plans_missing_once_ids_across_modules_at_once() {
    let reg = registry();
    let a = "import b\nlabel start {\n  choice {\n    「は」 once => jump b.other\n  }\n}\n";
    let b = "label other {\n  choice {\n    「へ」 once => jump other\n    「ほ」 once => jump other\n  }\n}\n";
    let mut rng = IdRng::new(3);
    let ins = plan_ids(
        &[("a", a), ("b", b)],
        &reg,
        &CompileOptions::default(),
        &mut rng,
    )
    .unwrap();
    let mods: Vec<&str> = ins.iter().map(|i| i.module.as_str()).collect();
    assert_eq!(mods, ["a", "b", "b"]);
    let out_a = apply_insertions(a, &ins[..1]);
    let out_b = apply_insertions(b, &ins[1..]);
    assert!(yomi_core::compile_modules(&[("a", &out_a), ("b", &out_b)], &reg).is_ok());
}
