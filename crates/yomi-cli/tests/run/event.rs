//! yomi run with the event package (`event.wait`)

use super::*;

const TIMED: &str = "import std::event\n\nlabel start {\n    let r = @event.wait([\"approved\", \"rejected\"], Some(60.0)) #w\n    match r {\n        Arrived(name, data) => {\n            あり ${name}:${data}\n        }\n        TimedOut => {\n            期限切れ\n        }\n    }\n    end\n}\n";

const UNTIMED: &str = "import std::event\n\nlabel start {\n    let r = @event.wait([\"approved\"], None) #w\n    match r {\n        Arrived(name, data) => {\n            あり ${name}\n        }\n        TimedOut => {\n            期限切れ\n        }\n    }\n    end\n}\n";

fn script(test: &str, text: &str) -> String {
    let dir = scratch(test);
    write_file(&dir, "a.yomi", text)
        .to_str()
        .unwrap()
        .to_string()
}

#[test]
fn event_wait_arrives_with_name_and_data() {
    let p = script("event_arrive", TIMED);
    let o = yomi(&["--lang", "ja", "run", &p, "--auto"], "approved ok data\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "[event] 待ち: approved, rejected（期限 60.0 秒）\nあり approved:ok data\n（おわり）\n"
    );
}

#[test]
fn event_wait_empty_answer_times_out_when_timed() {
    let p = script("event_timeout", TIMED);
    let o = yomi(&["--lang", "ja", "run", &p, "--auto"], "\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("期限切れ"), "{}", o.stdout);
}

#[test]
fn event_wait_auto_rejects_unknown_event_name() {
    let p = script("event_unknown", TIMED);
    let o = yomi(&["--lang", "ja", "run", &p, "--auto"], "nope\n");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("nope"), "{}", o.stderr);
}

#[test]
fn event_wait_without_time_limit_rejects_empty_answer_and_eof() {
    let p = script("event_untimed", UNTIMED);
    let o = yomi(&["--lang", "ja", "run", &p, "--auto"], "\n");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("期限がありません"), "{}", o.stderr);
    let o = yomi(&["--lang", "ja", "run", &p, "--auto"], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("入力が終わりました"), "{}", o.stderr);
}
