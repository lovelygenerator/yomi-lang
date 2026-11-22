//! choice, once, seen, visited

use super::*;

pub(super) fn full_registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<crate::Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.bind(crate::registry::Sugar::Plain, "ui.say").unwrap();
    r.bind(crate::registry::Sugar::Quote, "ui.say_as").unwrap();
    r.bind(crate::registry::Sugar::Choice, "ui.choose").unwrap();
    r
}

pub(super) fn options_of(y: &Yield) -> Vec<String> {
    match y {
        Yield::Stop { args, .. } => match &args[0] {
            Value::List(items) => items.iter().map(Value::to_text).collect(),
            other => panic!("{other:?}"),
        },
        Yield::End => panic!("終わっている"),
    }
}

pub(super) const CHOICE_SRC: &str = "\
save var n: int = 0

label start {
  choice {
    「a」 #a => n += 1
    「b」 if n >= 1 => n += 10
    「c」 once #c => n += 100
  }
  続き ${n}
  jump start
}
";

#[test]
fn choice_hides_false_if_options_and_maps_number_back_to_original() {
    let reg = full_registry();
    let prog = compile(CHOICE_SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    // b's n >= 1 is false, so the list is a and c
    assert_eq!(options_of(&y), vec!["「a」", "「c」"]);
    // Choose number 1 in the list (c) → enters the body of c (n += 100)
    vm.resume(Input::Return(Value::Int(1))).unwrap();
    let y = vm.run().unwrap();
    assert!(matches!(&y, Yield::Stop { args, .. } if args[0] == Value::Str("続き 100".into())));
    assert_eq!(vm.chosen_once(), vec!["main.start#c".to_string()]);
    vm.resume(Input::Return(Value::Unit)).unwrap();
    // Second round: c has been chosen and drops out, and b appears
    let y = vm.run().unwrap();
    assert_eq!(options_of(&y), vec!["「a」", "「b」"]);
}

#[test]
fn out_of_range_choice_number_errors_and_state_is_unchanged() {
    let reg = full_registry();
    let prog = compile(CHOICE_SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    for bad in [-1, 2, 99] {
        let e = vm.resume(Input::Return(Value::Int(bad))).unwrap_err();
        assert!(
            matches!(e.kind, VmErrorKind::ChoiceOutOfRange { count: 2, .. }),
            "{e:?}"
        );
    }
}

#[test]
fn all_options_filtered_out_is_runtime_error() {
    let reg = full_registry();
    let src = "label start {\n  choice {\n    「a」 if false => end\n  }\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::NoChoiceAvailable);
}

#[test]
fn loading_choice_save_restores_once_state_at_save_time() {
    let reg = full_registry();
    let prog = compile(CHOICE_SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let before = vm.save().unwrap();
    vm.resume(Input::Return(Value::Int(1))).unwrap(); // c
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    let second = vm.run().unwrap();
    assert_eq!(options_of(&second), vec!["「a」", "「b」"]);
    let after = vm.save().unwrap();
    // A save before choosing: c is in the list again
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &before).unwrap();
    assert!(vm2.chosen_once().is_empty());
    assert_eq!(options_of(vm2.current().unwrap()), vec!["「a」", "「c」"]);
    // A save after choosing: c stays out
    let (vm3, _) = Vm::load(&prog, &reg, VmConfig::default(), &after).unwrap();
    assert_eq!(vm3.chosen_once(), vec!["main.start#c".to_string()]);
    assert_eq!(options_of(vm3.current().unwrap()), vec!["「a」", "「b」"]);
}

#[test]
fn choice_saves_restore_without_panic_when_choices_change() {
    let reg = full_registry();
    let prog = compile(CHOICE_SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // Change the text of an option (the hash changes). Either it is found by position (ordinal), or it goes back to the start
    // of the label
    let edited = CHOICE_SRC.replace("「a」", "「あ」");
    let prog2 = compile(&edited, "main", &reg).unwrap();
    let (vm2, report) = Vm::load(&prog2, &reg, VmConfig::default(), &bytes).unwrap();
    assert!(matches!(
        report.resolved_by,
        ResolvedBy::Ordinal | ResolvedBy::LabelStart
    ));
    assert!(vm2.current().is_some() || report.resolved_by == ResolvedBy::LabelStart);
    // A script with the option removed
    let removed = "label start {\n  終わり\n}\n";
    let prog3 = compile(removed, "main", &reg).unwrap();
    Vm::load(&prog3, &reg, VmConfig::default(), &bytes).unwrap();
}

#[test]
fn quote_line_stops_with_identity_and_body() {
    let reg = full_registry();
    let src = "identity 案内人 = \"あんないにん\"\nlabel start {\n  案内人「やあ ${1 + 1}」 #hi\n  「ひとり」\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let Yield::Stop { args, id, .. } = vm.run().unwrap() else {
        panic!("止まる");
    };
    assert_eq!(id.as_deref(), Some("hi"));
    assert_eq!(
        args,
        vec![
            Value::Option(Some(Box::new(Value::Identity(std::sync::Arc::new(
                crate::Identity {
                    id: "案内人".into(),
                    name: "あんないにん".into()
                }
            ))))),
            Value::Str("「やあ 2」".into())
        ]
    );
    vm.resume(Input::Return(Value::Unit)).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!("止まる");
    };
    assert_eq!(args[0], Value::Option(None));
}

const SEEN_SRC: &str = "\
label start {
  最初 #first
  一回目 ${seen(\"start#first\")} ${visited(\"start\")} ${visited(\"other\")}
  jump other
}

label other {
  次 #second
  終わり ${seen(\"main.start#first\")} ${visited(\"main.other\")} ${seen(\"other#second\")}
}
";

pub(super) fn run_texts(vm: &mut Vm<'_>) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::Stop { args, .. } => {
                out.push(args.last().map(Value::to_text).unwrap_or_default());
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            Yield::End => return out,
        }
    }
}

#[test]
fn seen_becomes_true_after_entering_line_and_visited_after_entering_label() {
    let reg = full_registry();
    let prog = compile(SEEN_SRC, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let out = run_texts(&mut vm);
    assert_eq!(
        out,
        vec![
            "最初",
            "一回目 true true false",
            "次",
            "終わり true true true"
        ]
    );
    assert_eq!(
        vm.seen_points(),
        vec![
            "main.other#second".to_string(),
            "main.start#first".to_string()
        ]
    );
    assert_eq!(
        vm.visited_labels(),
        vec!["main.start".to_string(), "main.other".to_string()]
    );
}

#[test]
fn passed_seen_is_true_from_start_not_saved_and_visited_is_restored() {
    let reg = full_registry();
    let prog = compile(SEEN_SRC, "main", &reg).unwrap();
    // Pass the seen held by the host
    let cfg = VmConfig::default().seen_points(["main.other#second"]);
    let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
    let out = run_texts(&mut vm);
    assert_eq!(out[3], "終わり true true true");
    // If not passed, `other#second` has not been passed yet, so false
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // In the loaded VM, visited (which goes into saves) is restored, and seen is only the current stop point unless passed
    let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(vm2.visited_labels(), vec!["main.start".to_string()]);
    assert!(vm2.seen_points().contains(&"main.start#first".to_string()));
    vm2.set_seen_points(["x.y#z"]);
    assert_eq!(vm2.seen_points(), vec!["x.y#z".to_string()]);
}
