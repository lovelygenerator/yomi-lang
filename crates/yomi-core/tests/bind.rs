//! Tests of the syntax that receives a command's return value in scripts.
//!
//! `let name = @command(...)` and `name = @command(...)`. Stop commands stop at a stop point, put the resume value
//! (`Input::Return`) into the variable, and proceed to the next statement. Effect commands put it into the variable right
//! away.

mod common;

use common::Counter;
use yomi_core::{
    DiagCode, Input, Program, Registry, ResolvedBy, Severity, Sugar, Value, Vm, VmConfig, Yield,
    compile, compile_with_warnings,
};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(String,), String>("game.ask").unwrap();
    r.stop::<(i64,), i64>("game.roll").unwrap();
    r.stop::<(String,), bool>("game.yes").unwrap();
    r.stop::<(f64,), f64>("game.half").unwrap();
    r.stop::<(String,), Vec<String>>("game.names").unwrap();
    r.stop::<(), ()>("game.pause").unwrap();
    r.package::<Counter>().unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

fn compile_ok(src: &str, reg: &Registry) -> Program {
    match compile(src, "main", reg) {
        Ok(p) => p,
        Err(d) => panic!("コンパイルできない: {d:?}\n{src}"),
    }
}

/// The list of codes of compile diagnostics (errors only).
fn errors(src: &str) -> Vec<DiagCode> {
    match compile_with_warnings(src, "main", &registry()) {
        Ok(_) => Vec::new(),
        Err(d) => d
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.code)
            .collect(),
    }
}

/// The return value decided by the number of the stop point (matching the command name and arguments).
fn answer(name: &str, args: &[Value], index: usize) -> Value {
    match name {
        "game.ask" => Value::Str(format!("答{index}").into()),
        "game.roll" => Value::Int(((index * 7) % 5 + 1) as i64),
        "game.yes" => Value::Bool(index.is_multiple_of(2)),
        "game.half" => match args.first() {
            Some(Value::Float(x)) => Value::Float(x / 2.0),
            _ => Value::Float(0.0),
        },
        "ui.choose" => Value::Int(0),
        _ => Value::Unit,
    }
}

/// Runs the script to the end and returns the sequence of stop points (`command(args) return value`) and the variables at
/// the end. At the `roundtrip_at`-th stop point, continues with a VM that was saved and loaded again.
fn play(src: &str, roundtrip_at: Option<usize>) -> (Vec<String>, Vec<String>) {
    let reg = registry();
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut log = Vec::new();
    let mut index = 0;
    loop {
        match vm.run().unwrap() {
            Yield::End => break,
            Yield::Stop { command, args, .. } => {
                if roundtrip_at == Some(index) {
                    let bytes = vm.save().unwrap();
                    let (loaded, report) =
                        Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
                    assert_ne!(report.resolved_by, ResolvedBy::LabelStart);
                    assert_eq!(loaded.current(), vm.current());
                    vm = loaded;
                }
                let name = reg.command(command).name.clone();
                let ret = answer(&name, &args, index);
                let shown: Vec<String> = args.iter().map(Value::to_text).collect();
                log.push(format!("{name}({}) -> {}", shown.join(", "), ret.to_text()));
                vm.resume(Input::Return(ret)).unwrap();
                index += 1;
            }
            _ => unreachable!(),
        }
    }
    let vars = vm
        .variables()
        .iter()
        .filter(|v| !v.local)
        .map(|v| format!("{}={}", v.name, v.value.to_text()))
        .collect();
    (log, vars)
}

fn var(vm: &Vm<'_>, name: &str) -> Value {
    vm.variables()
        .into_iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("変数 {name} がない"))
        .value
}

fn stop_text(y: &Yield) -> String {
    match y {
        Yield::Stop { args, .. } => args.first().map(Value::to_text).unwrap_or_default(),
        _ => "<end>".to_string(),
    }
}

const BASIC: &str = "save var total: int = 0\nlabel start {\n  let name = @game.ask(\"名前は？\") #name\n  こんにちは ${name} #hello\n  let n: int = @game.roll(3)\n  total = @game.roll(n)\n  合計 ${total}、${n} #sum\n}\n";

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

#[test]
fn stop_let_and_assignment_store_resume_value_and_go_to_next_statement() {
    let reg = registry();
    let prog = compile_ok(BASIC, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    assert_eq!(stop_text(&y), "名前は？");
    // Before receiving, the default value
    assert_eq!(var(&vm, "name"), Value::Str("".into()));
    vm.resume(Input::Return(Value::Str("たろう".into())))
        .unwrap();
    let y = vm.run().unwrap();
    assert_eq!(stop_text(&y), "こんにちは たろう");
    assert_eq!(var(&vm, "name"), Value::Str("たろう".into()));
    vm.resume(Input::Return(Value::Unit)).unwrap();
    // let n = @game.roll(3)
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm.resume(Input::Return(Value::Int(2))).unwrap();
    // total = @game.roll(n) (the argument is the variable that received)
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => assert_eq!(args, vec![Value::Int(2)]),
        other => panic!("{other:?}"),
    }
    vm.resume(Input::Return(Value::Int(9))).unwrap();
    assert_eq!(stop_text(&vm.run().unwrap()), "合計 9、2");
    assert_eq!(var(&vm, "total"), Value::Int(9));
}

#[test]
fn resume_with_mismatched_received_type_is_bad_input_and_variable_unchanged() {
    let reg = registry();
    let prog = compile_ok(BASIC, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let e = vm.resume(Input::Return(Value::Int(1))).unwrap_err();
    assert_eq!(e.kind.code(), "bad_input");
    assert!(vm.is_stopped());
    vm.resume(Input::Return(Value::Str("ok".into()))).unwrap();
    vm.run().unwrap();
    assert_eq!(var(&vm, "name"), Value::Str("ok".into()));
}

#[test]
fn bool_and_float_can_be_received() {
    let src = "label start {\n  let y = @game.yes(\"はい？\")\n  let h = @game.half(5.0)\n  ${y} ${h}\n}\n";
    let (log, _) = play(src, None);
    assert_eq!(
        log,
        vec![
            "game.yes(はい？) -> true",
            "game.half(5.0) -> 2.5",
            "ui.say(true 2.5) -> "
        ]
    );
}

#[test]
fn effect_return_values_are_stored_immediately() {
    let reg = registry();
    let src = "import counter\nsave var seen_roll: int = 0\nlabel start {\n  let v = @counter.roll(6)\n  seen_roll = @counter.roll(6)\n  counter.step = @counter.roll(6)\n  値 ${v} ${seen_roll} ${counter.step} 状態 ${counter.get()}\n}\n";
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    let text = stop_text(&y);
    let nums: Vec<i64> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|t| t.parse().ok())
        .collect();
    assert_eq!(nums.len(), 4, "{text}");
    // The last value drawn is in both the state and the host-declared variable
    assert_eq!(nums[2], nums[3]);
    for n in &nums {
        assert!((1..=6).contains(n));
    }
}

#[test]
fn can_receive_in_blocks_label_parameters_and_called_labels() {
    let src = "label start {\n  let i = 0\n  while i < 2 {\n    let w = @game.ask(\"言葉${i}\")\n    ${i}:${w}\n    i += 1\n  }\n  if i == 2 {\n    let k = @game.roll(i)\n    choice {\n      「はい」 => {\n        let who = @game.ask(\"誰？\")\n        ${who}と${k}\n      }\n      「いいえ」 => {\n        end\n      }\n    }\n  }\n  let t = call tail(\"甲\")\n  結果 ${t}\n}\nlabel tail(who: str) -> int {\n  let r = @game.roll(5)\n  ${who}:${r}\n  if r > 2 {\n    let extra = @game.roll(r)\n    return r + extra\n  }\n  return r\n}\n";
    let (base, vars) = play(src, None);
    assert!(base.len() >= 10, "{base:?}");
    // The same whichever stop point it is saved and loaded at
    for k in 0..base.len() {
        let (log, v) = play(src, Some(k));
        assert_eq!(log, base, "k={k}");
        assert_eq!(v, vars, "k={k}");
    }
    // The received value is used
    assert!(base.iter().any(|l| l.starts_with("ui.say(答")), "{base:?}");
}

// ---------------------------------------------------------------------------
// Save and load
// ---------------------------------------------------------------------------

#[test]
fn save_and_load_at_stop_point_store_resume_value_in_same_variable() {
    let reg = registry();
    let prog = compile_ok(BASIC, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut loaded, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert!(loaded.is_stopped());
    loaded
        .resume(Input::Return(Value::Str("じろう".into())))
        .unwrap();
    assert_eq!(stop_text(&loaded.run().unwrap()), "こんにちは じろう");
    assert_eq!(var(&loaded, "name"), Value::Str("じろう".into()));
}

#[test]
fn can_receive_without_stable_id_found_by_hash() {
    let src = "label start {\n  あ\n  let a = @game.ask(\"一\")\n  let b = @game.ask(\"二\")\n  ${a}${b}\n}\n";
    let reg = registry();
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Str("A".into()))).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut loaded, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Hash);
    loaded
        .resume(Input::Return(Value::Str("B".into())))
        .unwrap();
    assert_eq!(stop_text(&loaded.run().unwrap()), "AB");
}

#[test]
fn load_without_position_reruns_that_let_from_label_start() {
    let old =
        "label start {\n  let a = @game.ask(\"一\")\n  let b = @game.ask(\"二\")\n  ${a}${b}\n}\n";
    // The local variables increased, so the position of the stop point cannot be trusted
    let new = "label start {\n  let z = 0\n  let a = @game.ask(\"一\")\n  let b = @game.ask(\"二\")\n  ${a}${b}${z}\n}\n";
    let reg = registry();
    let prog_old = compile_ok(old, &reg);
    let prog_new = compile_ok(new, &reg);
    let mut vm = Vm::new(&prog_old, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Str("A".into()))).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut loaded, report) = Vm::load(&prog_new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert!(!loaded.is_stopped());
    // Start over from the first let
    match loaded.run().unwrap() {
        Yield::Stop { args, .. } => assert_eq!(args, vec![Value::Str("一".into())]),
        other => panic!("{other:?}"),
    }
    loaded
        .resume(Input::Return(Value::Str("X".into())))
        .unwrap();
    loaded.run().unwrap();
    assert_eq!(var(&loaded, "a"), Value::Str("X".into()));
    loaded
        .resume(Input::Return(Value::Str("Y".into())))
        .unwrap();
    assert_eq!(stop_text(&loaded.run().unwrap()), "XY0");
}

#[test]
fn save_version_is_12() {
    let reg = registry();
    let prog = compile_ok(BASIC, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let mut bytes = vm.save().unwrap();
    assert_eq!(&bytes[4..8], &12u32.to_le_bytes());
    // Version 11 is not read
    bytes[4..8].copy_from_slice(&11u32.to_le_bytes());
    let e = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert_eq!(e.kind.code(), "unsupported_version");
}

// ---------------------------------------------------------------------------
// Rollback and history
// ---------------------------------------------------------------------------

#[test]
fn rollback_restores_received_variable_to_value_before_stop_point() {
    let src = "save var g: int = 0\nlabel start {\n  let a = @game.roll(1)\n  あ ${a}\n  let b = @game.roll(1)\n  g = @game.roll(1)\n  い ${a} ${b} ${g}\n}\n";
    let reg = registry();
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Int(1))).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap(); // stop point of b
    let snap = vm.snapshot().unwrap();
    vm.resume(Input::Return(Value::Int(5))).unwrap();
    vm.run().unwrap(); // stop point of g
    vm.resume(Input::Return(Value::Int(7))).unwrap();
    assert_eq!(stop_text(&vm.run().unwrap()), "い 1 5 7");
    assert_eq!(var(&vm, "g"), Value::Int(7));
    vm.restore(&snap).unwrap();
    assert_eq!(var(&vm, "b"), Value::Int(0));
    assert_eq!(var(&vm, "g"), Value::Int(0));
    assert_eq!(var(&vm, "a"), Value::Int(1));
    vm.resume(Input::Return(Value::Int(2))).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Int(3))).unwrap();
    assert_eq!(stop_text(&vm.run().unwrap()), "い 1 2 3");
}

#[test]
fn history_keeps_resume_values_in_saves_and_goes_back_with_rollback() {
    let reg = registry();
    let prog = compile_ok(BASIC, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    assert_eq!(vm.history()[0].ret, None);
    let first = vm.snapshot().unwrap();
    vm.resume(Input::Return(Value::Str("たろう".into())))
        .unwrap();
    assert_eq!(vm.history()[0].ret, Some(Value::Str("たろう".into())));
    vm.run().unwrap(); // hello (the return value is `()`)
    vm.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(vm.history()[1].ret, None);
    vm.run().unwrap(); // n
    let bytes = vm.save().unwrap();
    let (loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(loaded.history(), vm.history());
    assert_eq!(loaded.history()[0].ret, Some(Value::Str("たろう".into())));
    // The return value of a choice is the number passed
    vm.restore(&first).unwrap();
    assert_eq!(vm.history().len(), 1);
    assert_eq!(vm.history()[0].ret, None);
    vm.resume(Input::Return(Value::Str("じろう".into())))
        .unwrap();
    assert_eq!(vm.history()[0].ret, Some(Value::Str("じろう".into())));
}

#[test]
fn choice_return_value_in_history_is_number_passed() {
    let src = "label start {\n  choice {\n    「はい」 => {\n      あ\n    }\n    「いいえ」 => {\n      い\n    }\n  }\n}\n";
    let reg = registry();
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Int(1))).unwrap();
    assert_eq!(vm.history()[0].ret, Some(Value::Int(1)));
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn at_mid_expression_is_invalid_command_use() {
    for src in [
        "label start {\n  let a = 1 + @game.roll(2)\n}\n",
        "label start {\n  if @game.yes(\"?\") {\n  }\n}\n",
        "label start {\n  @game.roll(@game.roll(1))\n}\n",
        "label start {\n  let a = (@game.roll(1))\n}\n",
    ] {
        let e = errors(src);
        assert!(!e.is_empty(), "{src}");
        assert!(
            e.iter().all(|c| *c == DiagCode::InvalidCommandUse),
            "{src}: {e:?}"
        );
    }
}

#[test]
fn unit_returning_commands_and_queries_cannot_be_received() {
    for src in [
        "label start {\n  let a = @game.pause()\n}\n",
        "label start {\n  let a = @ui.say(\"x\")\n}\n",
        "import counter\nlabel start {\n  let a = @counter.inc(1)\n}\n",
        "import counter\nlabel start {\n  let a = @counter.get()\n}\n",
        "import counter\nlabel start {\n  let a = 0\n  a = @counter.get()\n}\n",
    ] {
        assert_eq!(errors(src), vec![DiagCode::InvalidCommandUse], "{src}");
    }
}

#[test]
fn type_mismatch_when_receiving_variable_and_return_types_differ() {
    assert_eq!(
        errors("label start {\n  let a: int = @game.ask(\"x\")\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("label start {\n  let a = 0\n  a = @game.ask(\"x\")\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("save var n: bool = false\nlabel start {\n  n = @game.roll(1)\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
}

#[test]
fn cannot_be_written_on_compound_assignment_rhs() {
    assert_eq!(
        errors("label start {\n  let a = 0\n  a += @game.roll(1)\n}\n"),
        vec![DiagCode::InvalidCommandUse]
    );
}

#[test]
fn list_return_values_can_be_received_in_variables() {
    assert_eq!(
        errors("label start {\n  let a = @game.names(\"x\")\n  let n: int = a.len()\n}\n"),
        vec![]
    );
    // The type is the command's return type (`list<str>`)
    assert_eq!(
        errors("label start {\n  let a: list<int> = @game.names(\"x\")\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
}

#[test]
fn argument_errors_and_unregistered_commands_are_diagnosed() {
    assert_eq!(
        errors("label start {\n  let a = @game.roll(\"x\")\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("label start {\n  let a = @game.nothing(1)\n}\n"),
        vec![DiagCode::UnknownCommand]
    );
}

#[test]
fn received_variable_cannot_be_used_before_declaration() {
    assert_eq!(
        errors("label start {\n  let a = @game.roll(a)\n}\n"),
        vec![DiagCode::UnknownName]
    );
}

#[test]
fn deeply_nested_at_calls_are_rejected_at_depth_limit_without_exhausting_stack() {
    let n = 20_000;
    let src = format!(
        "label start {{\n  let a = @game.roll({}1{})\n}}\n",
        "@game.roll(".repeat(n),
        ")".repeat(n)
    );
    let r = compile_with_warnings(&src, "main", &registry());
    let d = r.expect_err("コンパイルできない");
    assert!(d.iter().any(|d| d.code == DiagCode::TooDeep), "{d:?}");
}
