//! Tests of state, cues, saves, rollback and bulk registration for `audio`, `image` and `input` (the output of scripts is
//! in `golden.rs`, and checks of individual arguments are in the scripts of `tests/golden/`).

use yomi_core::{
    CommandKind, Input, Registry, RegistryError, Sugar, Value, Vm, VmConfig, VmErrorKind, Yield,
    compile,
};
use yomi_std::audio::{Audio, AudioCue, Channel};

use yomi_std::image::{Image, ImageCue, Item};
use yomi_std::input::Input as InputPkg;
/// The state where `town` is playing on the `"bgm"` channel at volume `volume`.
fn town(volume: f64) -> Vec<Channel> {
    vec![Channel {
        channel: "bgm".into(),
        name: "town".into(),
        volume,
    }]
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    yomi_std::register_all(&mut r).unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

fn stop(vm: &mut Vm<'_>) {
    match vm.run().expect("動く") {
        Yield::Stop { .. } => {}
        other => panic!("停止点のはず: {other:?}"),
    }
}

fn next(vm: &mut Vm<'_>) {
    vm.resume(Input::Return(Value::Unit)).expect("再開できる");
    stop(vm);
}

const SCRIPT: &str = "import std::audio\nimport std::image\nlabel start {\n  @audio.play(\"bgm\", \"town\", fade: 1.0)\n  @audio.volume(\"bgm\", 0.25)\n  @audio.se(\"door\")\n  @image.show(\"left\", \"guide\", 0.2, 0.4, 0.5)\n  一\n  @audio.voice(\"v1\")\n  @image.move(\"left\", 0.6, 0.4, 1.0)\n  二\n  三\n}\n";

#[test]
fn register_all_registers_nine_packages_regardless_of_dependencies() {
    let reg = registry();
    let mut names: Vec<String> = reg.packages().map(|p| p.name).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "audio", "event", "flow", "image", "input", "json", "math", "screen", "text"
        ]
    );
    assert_eq!(reg.command_kind("audio.play"), Some(CommandKind::Effect));
    assert_eq!(reg.command_kind("audio.playing"), Some(CommandKind::Query));
    assert_eq!(reg.command_kind("image.shown"), Some(CommandKind::Query));
    assert_eq!(reg.command_kind("input.text"), Some(CommandKind::Stop));
    assert_eq!(reg.command_kind("input.integer"), Some(CommandKind::Stop));
    assert_eq!(reg.command_kind("input.confirm"), Some(CommandKind::Stop));
}

#[test]
fn register_all_rejects_second_call_and_keeps_registry() {
    let mut reg = registry();
    let before = reg.packages().count();
    assert!(matches!(
        yomi_std::register_all(&mut reg),
        Err(RegistryError::DuplicatePackage(_))
    ));
    assert_eq!(reg.packages().count(), before);
}

#[test]
fn audio_and_image_have_state_and_input_has_none() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    assert!(vm.package_state::<Audio>().unwrap().channels.is_empty());
    assert!(vm.package_state::<Image>().unwrap().items.is_empty());
    assert_eq!(vm.package_state::<InputPkg>(), Some(&()));
}

#[test]
fn state_and_cues_are_readable_at_stop_point_and_only_cues_clear_on_next_resume() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    stop(&mut vm);
    let a = vm.package_state::<Audio>().unwrap().clone();
    assert_eq!(a.channels, town(0.25));
    assert_eq!(
        a.cues,
        [
            AudioCue::Play {
                channel: "bgm".into(),
                name: "town".into(),
                volume: 1.0,
                fade: 1.0
            },
            AudioCue::Volume {
                channel: "bgm".into(),
                volume: 0.25,
                fade: 0.0
            },
            AudioCue::Se {
                name: "door".into()
            },
        ]
    );
    let i = vm.package_state::<Image>().unwrap().clone();
    assert_eq!(
        i.cues,
        [ImageCue::Show {
            slot: "left".into(),
            fade: 0.5
        }]
    );
    next(&mut vm);
    let a = vm.package_state::<Audio>().unwrap();
    assert_eq!(a.cues, [AudioCue::Voice { name: "v1".into() }]);
    assert_eq!(a.channels, town(0.25), "永続的な部分は残る");
    let i = vm.package_state::<Image>().unwrap();
    assert_eq!(
        i.cues,
        [ImageCue::Move {
            slot: "left".into(),
            seconds: 1.0
        }]
    );
    assert_eq!(
        i.items,
        [Item {
            slot: "left".into(),
            name: "guide".into(),
            x: 0.6,
            y: 0.4,
            layer: 0,
            scale: 1.0,
            flip: false,
            alpha: 1.0
        }]
    );
    next(&mut vm);
    assert!(vm.package_state::<Audio>().unwrap().cues.is_empty());
    assert!(vm.package_state::<Image>().unwrap().cues.is_empty());
}

#[test]
fn save_and_load_restore_shown_items_and_empty_cues() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let cfg = || VmConfig::default().start("main.start");
    let mut vm = Vm::new(&prog, &reg, cfg()).unwrap();
    stop(&mut vm);
    next(&mut vm);
    let bytes = vm.save().unwrap();
    let (loaded, report) = Vm::load(&prog, &reg, cfg(), &bytes).unwrap();
    // flow, math, text and input, which have no state, do not go into saves (they do not appear in the report of default
    // values either)
    assert!(
        report.defaulted_slices.is_empty(),
        "{:?}",
        report.defaulted_slices
    );
    let a = loaded.package_state::<Audio>().unwrap();
    assert_eq!(a.channels, town(0.25));
    assert!(a.cues.is_empty());
    let i = loaded.package_state::<Image>().unwrap();
    assert_eq!(
        i.items,
        [Item {
            slot: "left".into(),
            name: "guide".into(),
            x: 0.6,
            y: 0.4,
            layer: 0,
            scale: 1.0,
            flip: false,
            alpha: 1.0
        }]
    );
    assert!(i.cues.is_empty());
}

#[test]
fn rollback_restores_sound_and_image_state_and_empties_cues() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    stop(&mut vm);
    let snap = vm.snapshot().unwrap();
    next(&mut vm);
    next(&mut vm);
    vm.restore(&snap).unwrap();
    let a = vm.package_state::<Audio>().unwrap();
    assert_eq!(a.channels, town(0.25));
    assert!(a.cues.is_empty());
    let i = vm.package_state::<Image>().unwrap();
    assert_eq!(
        i.items,
        [Item {
            slot: "left".into(),
            name: "guide".into(),
            x: 0.2,
            y: 0.4,
            layer: 0,
            scale: 1.0,
            flip: false,
            alpha: 1.0
        }]
    );
    assert!(i.cues.is_empty());
    // Advancing after going back gives the same cues again
    next(&mut vm);
    assert_eq!(
        vm.package_state::<Audio>().unwrap().cues,
        [AudioCue::Voice { name: "v1".into() }]
    );
}

#[test]
fn resume_turns_input_return_check_failures_into_runtime_errors() {
    let reg = registry();
    let prog = compile(
        "import std::input\nlabel start {\n  @input.text(\"n\", \"\", 3)\n  @input.integer(\"i\", 1, 1, 9)\n  @input.confirm(\"c\")\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let cfg = || VmConfig::default().start("main.start");
    // Values within range pass
    let mut vm = Vm::new(&prog, &reg, cfg()).unwrap();
    stop(&mut vm);
    vm.resume(Input::Return(Value::Str("あいう".into())))
        .unwrap();
    stop(&mut vm);
    vm.resume(Input::Return(Value::Int(9))).unwrap();
    stop(&mut vm);
    vm.resume(Input::Return(Value::Bool(false))).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::End));
    // Exceeding the character count or the range is a runtime error (the VM code is command_failed, with the package's code
    // and arguments)
    for (bad, case, want_code) in [
        (Value::Str("あいうえ".into()), 0, "input.text_too_long"),
        (Value::Int(0), 1, "input.integer_out_of_range"),
        (Value::Int(10), 1, "input.integer_out_of_range"),
    ] {
        let mut vm = Vm::new(&prog, &reg, cfg()).unwrap();
        stop(&mut vm);
        if case == 1 {
            vm.resume(Input::Return(Value::Str("a".into()))).unwrap();
            stop(&mut vm);
        }
        let e = vm.resume(Input::Return(bad.clone())).unwrap_err();
        assert_eq!(e.kind.code(), "command_failed", "{bad:?}");
        let (code, args) = e.kind.command_error().expect("コマンドの失敗");
        assert_eq!(code, want_code, "{bad:?}");
        assert!(!args.is_empty());
        assert_eq!(e.label.as_deref(), Some("start"));
    }
    // Type errors are bad_input, as before
    let mut vm = Vm::new(&prog, &reg, cfg()).unwrap();
    stop(&mut vm);
    let e = vm.resume(Input::Return(Value::Int(1))).unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
}

#[test]
fn input_text_limit_counts_graphemes_like_text_len() {
    let reg = registry();
    // A string containing combining characters, ZWJ emoji and flags. It passes with a `max_len` equal to the count of
    // `text.len`, and is refused with one less
    let samples = [
        "か\u{3099}e\u{301}",
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}x",
        "\u{1F1EF}\u{1F1F5}\u{1F1FA}\u{1F1F8}",
        "\u{2764}\u{FE0F}x",
        "",
    ];
    for s in samples {
        let n = {
            let prog = compile(
                &format!("import std::text\nlabel start {{\n  ${{text.len(\"{s}\")}}\n}}\n"),
                "main",
                &reg,
            )
            .unwrap();
            let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
            match vm.run().unwrap() {
                Yield::Stop { args, .. } => match &args[0] {
                    Value::Str(t) => t.parse::<i64>().unwrap(),
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            }
        };
        for (max, ok) in [(n, true), (n - 1, false)] {
            if max <= 0 {
                continue; // `max_len <= 0` is a runtime error before stopping
            }
            let prog = compile(
                &format!(
                    "import std::input\nlabel start {{\n  @input.text(\"p\", \"\", {max})\n}}\n"
                ),
                "main",
                &reg,
            )
            .unwrap();
            let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
            stop(&mut vm);
            let r = vm.resume(Input::Return(Value::Str(s.into())));
            assert_eq!(r.is_ok(), ok, "{s:?} max_len={max}");
        }
    }
}

// Cues are listed in the order written in the script, across packages
#[test]
fn cue_order_follows_written_order_across_packages_and_empties_on_next_resume() {
    let reg = registry();
    let src = "import std::audio\nimport std::image\nimport std::screen\nlabel start {\n  @audio.se(\"a\")\n  @image.show(\"left\", \"x\", 0.0, 0.0, 0.0)\n  @screen.shake(0.5, 0.3)\n  @audio.voice(\"v\")\n  @image.move(\"left\", 0.1, 0.1, 0.0)\n  一\n  @audio.se(\"b\")\n  二\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    stop(&mut vm);
    let order: Vec<(&str, usize)> = vm.cue_order().map(|c| (c.package, c.index)).collect();
    // audio's cues are 0 and 1 in the order `se`, `voice`. image and screen are each one's number 0, and image's number 1
    // (move)
    assert_eq!(
        order,
        [
            ("audio", 0),
            ("image", 0),
            ("screen", 0),
            ("audio", 1),
            ("image", 1)
        ]
    );
    // The cues of each package can be taken from the entries of the table
    let audio = vm.package_state::<Audio>().unwrap();
    assert!(matches!(audio.cues[0], AudioCue::Se { .. }));
    assert!(matches!(audio.cues[1], AudioCue::Voice { .. }));
    next(&mut vm);
    let order: Vec<(&str, usize)> = vm.cue_order().map(|c| (c.package, c.index)).collect();
    assert_eq!(order, [("audio", 0)], "前の区間の分は空になる");
}

#[test]
fn json_commands_run_in_a_script() {
    let reg = registry();
    let src = "import std::json\nlabel start {\n  let d = json.set(\"{}\", \"a.b\", \"1\")\n  let e = json.set(\"{\\\"a\\\":{\\\"b\\\":[7,8]}}\", \"a.b[1]\", json.quote(\"x\"))\n  match e {\n    Some(t) => @ui.say(t)\n    None => @ui.say(\"none\")\n  }\n  match json.get_int(\"{\\\"n\\\":5}\", \"n\") {\n    Some(n) => @ui.say(\"${n}\")\n    None => @ui.say(\"none\")\n  }\n  @ui.say(\"${json.len(\"[1,2,3]\", \"\")}\")\n  @ui.say(\"${json.keys(\"{\\\"b\\\":1,\\\"a\\\":2}\", \"\")}\")\n}\n";
    let program = compile(src, "main", &reg).expect("コンパイルできる");
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    let mut shown: Vec<String> = Vec::new();
    loop {
        match vm.run().expect("動く") {
            Yield::Stop { args, .. } => {
                if let Some(Value::Str(s)) = args.first() {
                    shown.push(s.to_string());
                }
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            Yield::End => break,
            _ => unreachable!(),
        }
    }
    assert_eq!(
        shown,
        [r#"{"a":{"b":[7,"x"]}}"#, "5", "Some(3)", "Some([a, b])"]
    );
}
