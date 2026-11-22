//! The `event` package: `event.wait` runs as a stop command, and the host returns an `event.Outcome`.

use yomi_core::{
    Input, IntoValue, Registry, Sugar, Value, Vm, VmConfig, VmErrorKind, Yield, compile,
};
use yomi_std::event::Outcome;

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    yomi_std::register_all(&mut r).unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

const SCRIPT: &str = "import std::event\n\nlabel start {\n  let r = @event.wait([\"ok\", \"no\"], Some(60.0)) #w\n  match r {\n    Arrived(name, data) => 届いた ${name} ${data}\n    TimedOut => 期限切れ\n  }\n}\n";

fn names(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::Str((*s).into())).collect())
}

/// Starts the script and returns the VM stopped at `event.wait`.
fn stopped_at_wait<'a>(prog: &'a yomi_core::Program, reg: &'a Registry) -> (Vm<'a>, Vec<Value>) {
    let mut vm = Vm::new(prog, reg, VmConfig::default()).unwrap();
    let Yield::Stop { command, args, id } = vm.run().unwrap() else {
        panic!("止まるはず")
    };
    assert_eq!(Some(command), reg.command_id("event.wait"));
    assert_eq!(id.as_deref(), Some("w"));
    (vm, args)
}

fn say(vm: &mut Vm<'_>) -> String {
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => match &args[0] {
            Value::Str(s) => s.to_string(),
            other => panic!("{other:?}"),
        },
        Yield::End => "<end>".to_string(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn wait_stops_with_names_and_timeout_and_returns_the_outcome() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let (mut vm, args) = stopped_at_wait(&prog, &reg);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], names(&["ok", "no"]));
    assert_eq!(args[1], Value::Option(Some(Box::new(Value::Float(60.0)))));
    let outcome = Outcome::Arrived("ok".into(), "{\"by\":1}".into()).into_value();
    vm.resume(Input::Return(outcome)).unwrap();
    assert_eq!(say(&mut vm), "届いた ok {\"by\":1}");
}

#[test]
fn timed_out_is_returned_when_a_timeout_was_given() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let (mut vm, _) = stopped_at_wait(&prog, &reg);
    vm.resume(Input::Return(Outcome::TimedOut.into_value()))
        .unwrap();
    assert_eq!(say(&mut vm), "期限切れ");
}

#[test]
fn timeout_defaults_to_none() {
    let reg = registry();
    let src = "import std::event\nlabel start {\n  let r = @event.wait([\"a\"])\n  ${r}\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!()
    };
    assert_eq!(args[1], Value::Option(None));
    // Without a time limit, `TimedOut` cannot come back
    let e = vm
        .resume(Input::Return(Outcome::TimedOut.into_value()))
        .unwrap_err();
    let (code, args) = e.kind.command_error().expect("コマンドの失敗");
    assert_eq!(code, "event.unexpected_timeout");
    assert!(args.is_empty());
}

#[test]
fn an_event_outside_the_names_is_a_runtime_error() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let (mut vm, _) = stopped_at_wait(&prog, &reg);
    let e = vm
        .resume(Input::Return(
            Outcome::Arrived("other".into(), String::new()).into_value(),
        ))
        .unwrap_err();
    let (code, args) = e.kind.command_error().expect("コマンドの失敗");
    assert_eq!(code, "event.unknown_event");
    assert_eq!(args.len(), 1);
    // After the error the VM is poisoned, like for other runtime errors
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
}

#[test]
fn bad_arguments_are_runtime_errors_before_stopping() {
    let reg = registry();
    for (call, want) in [
        ("[]", "event.no_names"),
        ("[\"a\", \"\"]", "event.empty_name"),
        ("[\"a\", \"b\", \"a\"]", "event.duplicate_name"),
    ] {
        let src = format!("import std::event\nlabel start {{\n  let r = @event.wait({call})\n}}\n");
        let prog = compile(&src, "main", &reg).unwrap();
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        let e = vm.run().unwrap_err();
        assert_eq!(e.kind.command_error().expect("コマンドの失敗").0, want);
    }
    let src = "import std::event\nlabel start {\n  let r = @event.wait([\"a\"], Some(-1.0))\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.run().unwrap_err();
    let (code, args) = e.kind.command_error().expect("コマンドの失敗");
    assert_eq!(code, "event.negative_timeout");
    assert_eq!(args.len(), 1);
    // Zero is allowed
    let src = "import std::event\nlabel start {\n  let r = @event.wait([\"a\"], Some(0.0))\n}\n";
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
}

#[test]
fn a_waiting_save_can_be_read_without_the_vm_and_resumed_later() {
    let reg = registry();
    let prog = compile(SCRIPT, "main", &reg).unwrap();
    let (vm, _) = stopped_at_wait(&prog, &reg);
    let bytes = vm.save().unwrap();
    // The waiting condition is read from the bytes alone
    let pending = yomi_core::pending_stop(&bytes)
        .unwrap()
        .expect("履歴がある");
    assert_eq!(pending.command, "event.wait");
    assert_eq!(pending.key.as_deref(), Some("main.start#w"));
    assert_eq!(pending.label, "main.start");
    assert_eq!(pending.args[0], names(&["ok", "no"]));
    assert_eq!(
        pending.args[1],
        Value::Option(Some(Box::new(Value::Float(60.0))))
    );
    // Later: load and return the event
    let (mut vm, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    // The loaded VM is stopped at the same wait
    let Some(Yield::Stop { command, args, .. }) = vm.current().cloned() else {
        panic!("同じ待ちで止まっているはず")
    };
    assert_eq!(Some(command), reg.command_id("event.wait"));
    assert_eq!(args[0], names(&["ok", "no"]));
    vm.resume(Input::Return(
        Outcome::Arrived("no".into(), "x".into()).into_value(),
    ))
    .unwrap();
    assert_eq!(say(&mut vm), "届いた no x");
}
