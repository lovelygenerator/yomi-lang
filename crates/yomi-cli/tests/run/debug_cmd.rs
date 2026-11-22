//! `yomi debug` as an actual command.

use super::*;

const SCRIPT: &str = "fn twice(n: int) -> int {
    let m = n * 2
    return m
}

label start {
    let x = 1
    let y = twice(x)
    あいさつ
    let z = x + y
    z
}
";

fn debug(test: &str, stdin: &str) -> Output {
    let dir = scratch(test);
    let script = dir.join("a.yomi");
    std::fs::write(&script, SCRIPT).unwrap();
    yomi(&["--lang", "en", "debug", script.to_str().unwrap()], stdin)
}

#[test]
fn stops_at_the_first_line_and_steps_over_a_call() {
    let o = debug("debug_next", "next\nnext\nquit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.starts_with("-> a:7 (start)\n7 |     let x = 1\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("-> a:8 (start)\n"));
    assert!(o.stdout.contains("-> a:9 (start)\n"));
    assert!(!o.stdout.contains("(twice)"));
}

#[test]
fn step_enters_the_call_and_bt_and_vars_show_it() {
    let o = debug("debug_step", "n\ns\nbt\nvars\nup\np x\ndown\nfin\nq\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("-> a:2 (twice)\n"), "{}", o.stdout);
    assert!(
        o.stdout.contains("*#0 a:2 (twice)\n #1 a:8 (start)\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("n = 1\n"));
    assert!(o.stdout.contains("x = 1\n"));
}

#[test]
fn breakpoints_can_be_set_listed_moved_and_deleted() {
    let o = debug(
        "debug_break",
        "b 2\nb 4\nb 99\nbreaks\nc\nd 1\nbreaks\nd\nbreaks\nc\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("breakpoint 1 at a:2\n"), "{}", o.stdout);
    assert!(
        o.stdout.contains("breakpoint 2 at a:6 (line 4 has no code"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("1 a:2\n"));
    assert!(o.stdout.contains("-> a:2 (twice)\n"));
    assert!(o.stderr.contains("99"), "{}", o.stderr);
    assert!(o.stdout.contains("no breakpoints\n"));
    assert!(o.stdout.trim_end().ends_with("(end)") || o.stdout.contains("(end)"));
}

#[test]
fn list_marks_the_current_line_and_breakpoints() {
    let o = debug("debug_list", "b 9\nlist 2\nq\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("=> 7 |     let x = 1\n"), "{}", o.stdout);
    assert!(o.stdout.contains("*  9 |"), "{}", o.stdout);
}

#[test]
fn a_stop_point_takes_an_answer_and_colon_commands() {
    let o = debug("debug_stop", "b 9\nc\n:vars\n:continue\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("-> a:9 (start)\n"), "{}", o.stdout);
    assert!(o.stdout.contains("あいさつ\n"));
    assert!(o.stdout.contains("x = 1\n"));
    assert!(o.stdout.trim_end().ends_with("(end)"), "{}", o.stdout);
}

#[test]
fn an_empty_line_repeats_the_last_motion_and_eof_quits() {
    let o = debug("debug_repeat", "s\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("-> a:8 (start)\n"));
    assert!(o.stdout.contains("-> a:2 (twice)\n"));
}

#[test]
fn unknown_commands_are_reported_and_ignored() {
    let o = debug("debug_unknown", "frobnicate\nframe 9\nup\nq\n");
    assert_eq!(o.code, 0);
    assert!(
        o.stderr.contains("unknown command: frobnicate"),
        "{}",
        o.stderr
    );
    assert!(o.stderr.contains("no frame 9"), "{}", o.stderr);
    assert!(o.stderr.contains("outermost"), "{}", o.stderr);
}

#[test]
fn a_runtime_error_exits_with_one() {
    let path = fixture("divzero.yomi");
    let o = yomi(&["--lang", "en", "debug", path.to_str().unwrap()], "c\n");
    assert_eq!(o.code, 1);
    assert!(!o.stderr.is_empty());
}

#[test]
fn missing_script_is_an_argument_error() {
    let o = yomi(&["debug"], "");
    assert_eq!(o.code, 2);
}
