//! Runs the support-ticket flow (`examples/ticket/`) with a small host that answers each wait with an event or a time-out.
//!
//! If the script is missing (a public repository exported with only the crate), it finishes without checking anything.

use yomi_core::{Input, IntoValue, Registry, Sugar, Value, Vm, VmConfig, Yield, compile};
use yomi_std::event::Outcome;

fn script() -> Option<String> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ticket/ticket.yomi");
    std::fs::read_to_string(path).ok()
}

/// What happens at each wait: an event arrives (name, data), or the time limit passes.
enum Step {
    Event(&'static str, &'static str),
    Timeout,
}

/// Returns the lines the script showed, and the time limits (seconds) of the waits it went through.
fn run(steps: &[Step]) -> Option<(Vec<String>, Vec<Option<f64>>)> {
    let source = script()?;
    let mut reg = Registry::new();
    reg.stop::<(String,), ()>("ui.say").unwrap();
    yomi_std::register_all(&mut reg).unwrap();
    reg.bind(Sugar::Plain, "ui.say").unwrap();
    let program = compile(&source, "ticket", &reg).unwrap();
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    let (mut shown, mut limits) = (Vec::new(), Vec::new());
    let mut steps = steps.iter();
    loop {
        match vm.run().unwrap() {
            Yield::End => break,
            Yield::Stop { command, args, .. } => match reg.command(command).name.as_str() {
                "ui.say" => {
                    if let Some(Value::Str(text)) = args.first() {
                        shown.push(text.to_string());
                    }
                    vm.resume(Input::Return(Value::Unit)).unwrap();
                }
                "event.wait" => {
                    limits.push(match args.get(1) {
                        Some(Value::Option(Some(inner))) => match **inner {
                            Value::Float(f) => Some(f),
                            _ => None,
                        },
                        _ => None,
                    });
                    let Some(step) = steps.next() else {
                        break;
                    };
                    let outcome = match step {
                        Step::Event(name, data) => {
                            Outcome::Arrived(name.to_string(), data.to_string())
                        }
                        Step::Timeout => Outcome::TimedOut,
                    };
                    vm.resume(Input::Return(outcome.into_value())).unwrap();
                }
                other => panic!("unknown command {other}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }
    Some((shown, limits))
}

#[test]
fn an_urgent_subject_is_accepted_as_urgent() {
    let Some((out, _)) = run(&[
        Step::Event("opened", r#"{"subject":"ログイン障害"}"#),
        Step::Event("replied", "{}"),
    ]) else {
        return;
    };
    assert_eq!(
        out[1],
        "「ログイン障害」を緊急として受け付けました。担当者に知らせます。"
    );
    assert_eq!(
        out.last().unwrap(),
        "お客様から返信がありました。担当者に回します。"
    );
}

#[test]
fn an_ordinary_subject_is_accepted_normally() {
    let Some((out, _)) = run(&[
        Step::Event("opened", r#"{"subject":"使い方の質問"}"#),
        Step::Event("resolved", "{}"),
    ]) else {
        return;
    };
    assert_eq!(out[1], "「使い方の質問」を受け付けました。");
    assert_eq!(out.last().unwrap(), "解決済みとして閉じました。");
}

#[test]
fn a_ticket_without_a_subject_is_refused() {
    let Some((out, _)) = run(&[Step::Event("opened", "{}")]) else {
        return;
    };
    assert_eq!(
        out.last().unwrap(),
        "件名のない問い合わせは受け付けられません。"
    );
}

#[test]
fn silence_is_reminded_twice_and_then_closed() {
    let Some((out, limits)) = run(&[
        Step::Event("opened", r#"{"subject":"使い方の質問"}"#),
        Step::Timeout,
        Step::Timeout,
        Step::Timeout,
    ]) else {
        return;
    };
    assert_eq!(out[2], "返信をお願いする連絡を送りました（1 回目）。");
    assert_eq!(out[3], "返信をお願いする連絡を送りました（2 回目）。");
    assert_eq!(out.last().unwrap(), "返信がないため、自動で閉じます。");
    assert_eq!(
        limits,
        [None, Some(259200.0), Some(259200.0), Some(259200.0)]
    );
}

#[test]
fn a_reply_after_a_reminder_still_reaches_the_staff() {
    let Some((out, _)) = run(&[
        Step::Event("opened", r#"{"subject":"使い方の質問"}"#),
        Step::Timeout,
        Step::Event("replied", "{}"),
    ]) else {
        return;
    };
    assert_eq!(
        out.last().unwrap(),
        "お客様から返信がありました。担当者に回します。"
    );
}
