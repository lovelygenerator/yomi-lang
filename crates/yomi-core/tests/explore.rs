//! Exploring the branches of a script (`spec/host/08-test.md`).

use yomi_core::{ExploreConfig, ExploreReport, Registry, Sugar, Value, Yield, compile, explore};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(i64,), i64>("game.roll").unwrap();
    r.stop::<(String,), String>("game.ask").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

fn run(src: &str, config: &ExploreConfig) -> ExploreReport {
    run_with(src, config, &mut |_| None)
}

fn run_with(
    src: &str,
    config: &ExploreConfig,
    answers: &mut dyn FnMut(&Yield) -> Option<Vec<Value>>,
) -> ExploreReport {
    let reg = registry();
    let program = match compile(src, "main", &reg) {
        Ok(p) => p,
        Err(d) => panic!("compile failed: {d:?}\n{src}"),
    };
    explore(&program, &reg, config, answers).unwrap()
}

fn names(r: &ExploreReport) -> Vec<&str> {
    r.paths.iter().map(|p| p.name.as_str()).collect()
}

const LOOP: &str = "save var n: int = 0

label start {
  こんにちは
  choice {
    「話す」 #talk => {
      if n < 2 {
        n += 1
      }
      話した
    }
    「調べる」 once #look => 調べた
    「帰る」 if n > 0 #leave => jump ending
  }
  続き
  jump start
}

label ending {
  さようなら
  end
}
";

#[test]
fn straight_script_has_one_path_named_main() {
    let r = run(
        "label start {\n  一行目\n  二行目\n}\n",
        &ExploreConfig::default(),
    );
    assert_eq!(names(&r), ["main"]);
    assert_eq!(r.paths[0].steps.len(), 2);
    assert!(r.paths[0].steps.iter().all(|s| s.answer == Value::Unit));
    assert!(r.errors.is_empty() && r.unanswered.is_empty());
    assert!(r.unreached_stops.is_empty() && r.unreached_labels.is_empty() && !r.truncated);
}

#[test]
fn every_option_of_a_choice_is_a_path_named_by_its_id() {
    let src = "label start {\n  choice {\n    「はい」 #yes => 返事\n    「いいえ」 => 別の返事\n  }\n  end\n}\n";
    let r = run(src, &ExploreConfig::default());
    assert_eq!(names(&r), ["yes", "c2"]);
    assert_eq!(r.paths[0].steps[0].answer, Value::Int(0));
    assert_eq!(r.paths[1].steps[0].answer, Value::Int(1));
    assert!(r.unchosen_options.is_empty());
}

#[test]
fn a_loop_is_explored_once_per_state_and_every_option_gets_a_path() {
    let r = run(LOOP, &ExploreConfig::default());
    assert!(!r.truncated, "{r:?}");
    assert!(r.errors.is_empty() && r.unanswered.is_empty());
    assert!(r.unchosen_options.is_empty(), "{:?}", r.unchosen_options);
    assert!(r.unreached_labels.is_empty() && r.unreached_stops.is_empty());
    // The path that goes home needs a talk first
    assert!(names(&r).contains(&"talk-leave"), "{:?}", names(&r));
    // Every path ends at the end, so the last choice of each one is the way out
    assert!(
        r.paths.iter().all(|p| p.name.ends_with("leave")),
        "{:?}",
        names(&r)
    );
}

#[test]
fn a_runtime_error_is_reported_once_with_the_answers_that_lead_to_it() {
    let src = "label start {\n  choice {\n    「割る」 #div => {\n      let x = 1 / 0\n      ${x}\n    }\n    「終わる」 #stop => end\n  }\n}\n";
    let r = run(src, &ExploreConfig::default());
    assert_eq!(names(&r), ["stop"]);
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].error.kind.code(), "division_by_zero");
    assert_eq!(r.errors[0].steps.len(), 1);
    assert_eq!(r.errors[0].steps[0].answer, Value::Int(0));
}

#[test]
fn a_stop_that_cannot_be_answered_is_reported_and_not_passed() {
    let src = "label start {\n  let a = @game.ask(\"名前\") #ask\n  ${a}\n}\n";
    let r = run(src, &ExploreConfig::default());
    assert!(r.paths.is_empty());
    assert_eq!(r.unanswered.len(), 1);
    assert_eq!(r.unanswered[0].stop, "main.start#ask");
    assert_eq!(r.unanswered[0].command, "game.ask");
    assert!(
        r.unreached_stops
            .iter()
            .any(|s| s.starts_with("main.start:3"))
    );
}

#[test]
fn answers_from_the_host_are_tried_in_order_without_repeats() {
    let src = "label start {\n  let a = @game.roll(6) #roll\n  ${a}\n}\n";
    let mut asked = 0;
    let r = run_with(src, &ExploreConfig::default(), &mut |_| {
        asked += 1;
        Some(vec![Value::Int(2), Value::Int(5), Value::Int(2)])
    });
    assert_eq!(asked, 1);
    assert_eq!(names(&r), ["main", "main~2"]);
    assert_eq!(r.paths[0].steps[0].answer, Value::Int(2));
    assert_eq!(r.paths[1].steps[0].answer, Value::Int(5));
}

#[test]
fn a_wrong_type_of_answer_is_an_error_path() {
    let src = "label start {\n  let a = @game.roll(6) #roll\n  ${a}\n}\n";
    let r = run_with(src, &ExploreConfig::default(), &mut |_| {
        Some(vec![Value::Bool(true)])
    });
    assert!(r.paths.is_empty());
    assert_eq!(r.errors.len(), 1);
}

#[test]
fn unbounded_state_stops_at_max_states() {
    let src = "save var n: int = 0\nlabel start {\n  n += 1\n  数える\n  jump start\n}\n";
    let mut cfg = ExploreConfig::default();
    cfg.max_states = 50;
    let r = run(src, &cfg);
    assert!(r.truncated);
    assert_eq!(r.states, 50);
    assert!(r.paths.is_empty());
}

#[test]
fn seeds_prefix_the_names_and_change_random_branches() {
    let src = "label start {\n  let x = random(1, 2)\n  if x == 1 {\n    一\n  } else {\n    二\n  }\n}\n";
    let mut cfg = ExploreConfig::default();
    cfg.seeds = vec![0, 1, 2, 3];
    let r = run(src, &cfg);
    assert_eq!(r.paths.len(), 4);
    assert!(r.paths.iter().all(|p| p.name.starts_with('s')));
    assert_eq!(r.paths[2].seed, 2);
}

#[test]
fn exploration_is_deterministic() {
    let a = format!("{:?}", run(LOOP, &ExploreConfig::default()));
    let b = format!("{:?}", run(LOOP, &ExploreConfig::default()));
    assert_eq!(a, b);
}

#[test]
fn unreached_labels_and_stops_are_listed() {
    let src = "label start {\n  始まり\n}\nlabel never {\n  届かない #gone\n}\n";
    let r = run(src, &ExploreConfig::default());
    assert_eq!(r.unreached_labels, ["main.never"]);
    assert_eq!(r.unreached_stops, ["main.never#gone"]);
}

#[test]
fn a_missing_start_label_is_an_error() {
    let reg = registry();
    let program = compile("label other {\n  x\n}\n", "main", &reg).unwrap();
    let r = explore(&program, &reg, &ExploreConfig::default(), &mut |_| None);
    assert!(r.is_err());
}

#[test]
fn a_loop_with_an_unbounded_counter_still_covers_every_branch() {
    let src = LOOP.replace(
        "      if n < 2 {\n        n += 1\n      }\n",
        "      n += 1\n",
    );
    let mut cfg = ExploreConfig::default();
    cfg.max_states = 300;
    let r = run(&src, &cfg);
    // The state space is infinite, so the search is cut off, but breadth first reaches the end and every option early
    assert!(r.truncated);
    assert!(r.unchosen_options.is_empty(), "{:?}", r.unchosen_options);
    assert!(r.unreached_labels.is_empty() && r.unreached_stops.is_empty());
    assert_eq!(names(&r).first().copied(), Some("talk-leave"));
}
