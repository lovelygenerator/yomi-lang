//! The answers tried by the exploration for the standard packages (`spec/host/08-test.md`, 8.4).

use yomi_core::{ExploreConfig, Registry, Value, Yield, compile, explore};
use yomi_std::event::Outcome;

fn registry() -> Registry {
    let mut r = Registry::new();
    yomi_std::register_all(&mut r).unwrap();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(yomi_core::Sugar::Plain, "ui.say").unwrap();
    r
}

/// The candidates `test_answers` gave at each stop, in order of first asking.
fn candidates(src: &str) -> Vec<Vec<Value>> {
    candidates_with(src, &[])
}

fn candidates_with(src: &str, events: &[(String, String)]) -> Vec<Vec<Value>> {
    let reg = registry();
    let program = compile(src, "main", &reg).unwrap();
    let mut asked: Vec<Vec<Value>> = Vec::new();
    let report = explore(
        &program,
        &reg,
        &ExploreConfig::default(),
        &mut |y: &Yield| {
            let c = yomi_std::test_answers_with_events(&reg, y, events);
            if let Some(c) = &c
                && !asked.contains(c)
            {
                asked.push(c.clone());
            }
            c
        },
    )
    .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.unanswered.is_empty(), "{:?}", report.unanswered);
    asked
}

fn s(text: &str) -> Value {
    Value::Str(text.into())
}

#[test]
fn input_text_tries_the_default_empty_and_the_longest() {
    let asked =
        candidates("import std::input\nlabel start {\n  @input.text(\"名前\", \"ゆみ\", 3)\n}\n");
    assert_eq!(asked, [vec![s("ゆみ"), s(""), s("aaa")]]);
}

#[test]
fn input_text_does_not_repeat_candidates() {
    let asked =
        candidates("import std::input\nlabel start {\n  @input.text(\"名前\", \"\", 1)\n}\n");
    assert_eq!(asked, [vec![s(""), s("a")]]);
}

#[test]
fn input_integer_tries_the_default_min_and_max() {
    let asked =
        candidates("import std::input\nlabel start {\n  @input.integer(\"年\", 10, 0, 120)\n}\n");
    assert_eq!(
        asked,
        [vec![Value::Int(10), Value::Int(0), Value::Int(120)]]
    );
}

#[test]
fn input_confirm_tries_both() {
    let asked = candidates("import std::input\nlabel start {\n  @input.confirm(\"よい？\")\n}\n");
    assert_eq!(asked, [vec![Value::Bool(true), Value::Bool(false)]]);
}

#[test]
fn event_wait_tries_each_name_and_the_time_out_only_when_timed() {
    use yomi_core::IntoValue;
    let arrived = |n: &str| Outcome::Arrived(n.to_string(), String::new()).into_value();
    let timed = candidates(
        "import std::event\nlabel start {\n  let r = @event.wait([\"a\", \"b\"], Some(5.0))\n  ${r}\n}\n",
    );
    assert_eq!(
        timed,
        [vec![
            arrived("a"),
            arrived("b"),
            Outcome::TimedOut.into_value()
        ]]
    );
    let untimed =
        candidates("import std::event\nlabel start {\n  let r = @event.wait([\"a\"])\n  ${r}\n}\n");
    assert_eq!(untimed, [vec![arrived("a")]]);
}

#[test]
fn other_commands_get_no_candidates() {
    let reg = registry();
    let program = compile("label start {\n  行\n}\n", "main", &reg).unwrap();
    let mut vm = yomi_core::Vm::new(&program, &reg, yomi_core::VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    assert_eq!(yomi_std::test_answers(&reg, &y), None);
    assert_eq!(yomi_std::test_answers(&reg, &Yield::End), None);
}

#[test]
fn explore_runs_every_branch_of_an_input_script() {
    let reg = registry();
    let src = "import std::input\nlabel start {\n  let ok = @input.confirm(\"よい？\") #ok\n  if ok {\n    はい\n  } else {\n    いいえ\n  }\n}\n";
    let program = compile(src, "main", &reg).unwrap();
    let r = explore(
        &program,
        &reg,
        &ExploreConfig::default(),
        &mut |y: &Yield| yomi_std::test_answers(&reg, y),
    )
    .unwrap();
    assert_eq!(r.paths.len(), 2);
    assert!(r.unreached_stops.is_empty());
}

#[test]
fn event_data_replaces_the_empty_data_of_its_name_only() {
    use yomi_core::IntoValue;
    let arrived = |n: &str, d: &str| Outcome::Arrived(n.to_string(), d.to_string()).into_value();
    let pair = |n: &str, d: &str| (n.to_string(), d.to_string());
    let src = "import std::event\nlabel start {\n  let r = @event.wait([\"a\", \"b\"], Some(5.0))\n  ${r}\n}\n";
    let got = candidates_with(
        src,
        &[
            pair("a", "{\"n\":1}"),
            pair("a", "x=y"),
            pair("zzz", "unused"),
        ],
    );
    assert_eq!(
        got,
        [vec![
            arrived("a", "{\"n\":1}"),
            arrived("a", "x=y"),
            arrived("b", ""),
            Outcome::TimedOut.into_value()
        ]]
    );
    // An empty data is tried only when asked for
    let got = candidates_with(src, &[pair("a", ""), pair("a", "z")]);
    assert_eq!(
        got[0][..3],
        [arrived("a", ""), arrived("a", "z"), arrived("b", "")]
    );
}
