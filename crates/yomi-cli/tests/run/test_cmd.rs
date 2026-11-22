//! yomi test (exploring the branches and golden files)

use super::*;

const BRANCHES: &str = "import std::input\nimport std::event\n\nsave var n: int = 0\n\nlabel start {\n  どうする？\n  choice {\n    「話す」 #talk => {\n      if n < 1 {\n        n += 1\n      }\n      話した\n    }\n    「聞く」 #ask => jump 聞く\n    「帰る」 #leave => end\n  }\n  jump start\n}\n\nlabel 聞く {\n  let ok = @input.confirm(\"本当に？\") #sure\n  if ok {\n    let r = @event.wait([\"approved\", \"rejected\"], Some(10.0)) #w\n    match r {\n      Arrived(name, data) => {\n        届いた ${name}:${data}\n      }\n      TimedOut => 期限切れ\n    }\n  }\n  end\n}\n";

/// A script in its own directory; returns the script path and the directory.
fn project(test: &str, source: &str) -> (String, PathBuf) {
    let dir = scratch(test);
    let _ = std::fs::remove_dir_all(dir.join("tests"));
    let path = write_file(&dir, "a.yomi", source);
    (path.to_str().unwrap().to_string(), dir)
}

fn test_cmd(script: &str, extra: &[&str]) -> Output {
    let mut args = vec!["--lang", "en", "test", script];
    args.extend_from_slice(extra);
    yomi(&args, "")
}

fn golden_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("tests/golden"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn without_a_golden_directory_it_explores_and_says_nothing_was_compared() {
    let (script, _) = project("test_nogolden", BRANCHES);
    let o = test_cmd(&script, &[]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("States examined: 15"), "{}", o.stdout);
    assert!(
        o.stdout.contains("Paths that reach the end: 11"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("nothing was compared"), "{}", o.stdout);
}

#[test]
fn update_writes_a_golden_per_path_and_then_the_comparison_passes() {
    let (script, dir) = project("test_update", BRANCHES);
    let o = test_cmd(&script, &["--update"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("11 written, 0 removed"), "{}", o.stdout);
    let names = golden_names(&dir);
    assert_eq!(names.len(), 11);
    assert!(names.contains(&"leave.golden".to_string()));
    let leave = std::fs::read_to_string(dir.join("tests/golden/leave.golden")).unwrap();
    assert_eq!(
        leave,
        "yomi-golden 1\nseed 0\nanswer #leave\n---\nどうする？\n1: 「話す」 #talk\n2: 「聞く」 #ask\n3: 「帰る」 #leave\n(end)\n"
    );
    // Answers of input and event stops are lines too
    let timed = std::fs::read_to_string(dir.join("tests/golden/ask~4.golden")).unwrap();
    assert!(
        timed.starts_with("yomi-golden 1\nseed 0\nanswer #ask\nanswer y\nanswer \n---\n"),
        "{timed}"
    );

    let o = test_cmd(&script, &[]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(
        !o.stdout.contains("mismatch") && !o.stdout.contains("Stale"),
        "{}",
        o.stdout
    );
    // A second update changes nothing
    let o = test_cmd(&script, &["--update"]);
    assert!(o.stdout.contains("0 written, 0 removed"), "{}", o.stdout);
}

#[test]
fn a_changed_output_is_a_mismatch_with_the_differing_lines() {
    let (script, dir) = project("test_mismatch", BRANCHES);
    test_cmd(&script, &["--update"]);
    let path = write_file(&dir, "a.yomi", &BRANCHES.replace("話した\n", "話したよ\n"));
    let o = test_cmd(path.to_str().unwrap(), &[]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(o.stdout.contains("Golden mismatch:"), "{}", o.stdout);
    assert!(
        o.stdout.contains("  - 話した\n  + 話したよ\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn stale_new_and_unexplored_goldens_are_told_apart() {
    let (script, dir) = project("test_kinds", BRANCHES);
    test_cmd(&script, &["--update"]);
    let g = dir.join("tests/golden");
    std::fs::remove_file(g.join("talk-leave.golden")).unwrap();
    let text = std::fs::read_to_string(g.join("leave.golden")).unwrap();
    std::fs::write(g.join("zzz.golden"), &text).unwrap();
    std::fs::write(
        g.join("leave.golden"),
        text.replace("answer #leave", "answer #nope"),
    )
    .unwrap();
    let o = test_cmd(&script, &[]);
    assert_eq!(o.code, 1);
    assert!(
        o.stdout
            .contains("Stale golden (its recorded answers no longer work): leave"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("New path without a golden file: talk-leave"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("Golden file of a path that was not explored: zzz"),
        "{}",
        o.stdout
    );
    // Only the new path and the unexplored golden do not fail
    std::fs::write(g.join("leave.golden"), &text).unwrap();
    assert_eq!(test_cmd(&script, &[]).code, 0);
    // --update brings it back: writes the new one, removes the unexplored one
    let o = test_cmd(&script, &["--update"]);
    assert!(o.stdout.contains("1 written, 1 removed"), "{}", o.stdout);
    assert!(!g.join("zzz.golden").exists() && g.join("talk-leave.golden").exists());
}

#[test]
fn a_path_with_a_runtime_error_fails_and_is_reported_with_its_answers() {
    let src = "label start {\n  choice {\n    「割る」 #div => {\n      let x = 1 / 0\n      ${x}\n    }\n    「終わる」 #stop => end\n  }\n}\n";
    let (script, _) = project("test_error", src);
    let o = test_cmd(&script, &[]);
    assert_eq!(o.code, 1);
    assert!(
        o.stdout.contains("Error path (seed 0, answers: #div)"),
        "{}",
        o.stdout
    );
    let o = test_cmd(&script, &["--json"]);
    assert_eq!(o.code, 1);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["errors"][0]["code"], "division_by_zero");
    assert_eq!(v["errors"][0]["answers"][0], "#div");
    assert_eq!(v["paths"][0]["name"], "stop");
    assert_eq!(v["truncated"], false);
}

#[test]
fn require_coverage_fails_on_labels_and_stops_never_reached() {
    let src = "label start {\n  始まり\n}\nlabel never {\n  届かない #gone\n}\n";
    let (script, _) = project("test_coverage", src);
    let o = test_cmd(&script, &[]);
    assert_eq!(o.code, 0);
    assert!(
        o.stdout.contains("Labels never entered:\n  a.never\n"),
        "{}",
        o.stdout
    );
    let o = test_cmd(&script, &["--require-coverage"]);
    assert_eq!(o.code, 1);
}

#[test]
fn an_unbounded_state_is_cut_off_at_max_states() {
    let src = "save var n: int = 0\nlabel start {\n  n += 1\n  数える\n  jump start\n}\n";
    let (script, _) = project("test_truncated", src);
    let o = test_cmd(&script, &["--max-states", "40"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("States examined: 40"), "{}", o.stdout);
    assert!(
        o.stdout.contains("stopped at the limit of 40 states"),
        "{}",
        o.stdout
    );
    assert_eq!(
        test_cmd(&script, &["--max-states", "40", "--require-coverage"]).code,
        1
    );
}

#[test]
fn seeds_prefix_the_path_names_and_are_recorded_in_the_goldens() {
    let src = "label start {\n  let x = random(1, 2)\n  if x == 1 {\n    一\n  } else {\n    二\n  }\n}\n";
    let (script, dir) = project("test_seeds", src);
    let o = test_cmd(&script, &["--seeds", "3", "--update"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        golden_names(&dir),
        ["s0-main.golden", "s1-main.golden", "s2-main.golden"]
    );
    let g = std::fs::read_to_string(dir.join("tests/golden/s1-main.golden")).unwrap();
    assert!(g.starts_with("yomi-golden 1\nseed 1\n---\n"), "{g}");
    assert_eq!(test_cmd(&script, &["--seeds", "3"]).code, 0);
}

#[test]
fn input_text_does_not_try_the_empty_string_when_the_default_is_not_empty() {
    let src = "import std::input\nlabel start {\n  let s = @input.text(\"名前\", \"ゆみ\", 2) #name\n  ${s}\n}\n";
    let (script, _) = project("test_text", src);
    let o = test_cmd(&script, &["--json"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    let answers: Vec<&str> = v["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["answers"][0].as_str().unwrap())
        .collect();
    assert_eq!(answers, ["ゆみ", "aa"]);
}

#[test]
fn the_golden_directory_and_search_root_can_be_given() {
    let (script, dir) = project("test_golden_opt", BRANCHES);
    let other = dir.join("elsewhere");
    let o = test_cmd(&script, &["--update", "--golden", other.to_str().unwrap()]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(other.join("leave.golden").exists());
    assert!(!dir.join("tests").exists());
    assert_eq!(
        test_cmd(&script, &["--golden", other.to_str().unwrap()]).code,
        0
    );
}

#[test]
fn compile_errors_and_bad_arguments_are_failures() {
    let (script, _) = project("test_bad", "label start {\n  let x: int = \"a\"\n}\n");
    assert_eq!(test_cmd(&script, &[]).code, 1);
    let (ok, _) = project("test_bad_args", "label start {\n  行\n}\n");
    assert_eq!(test_cmd(&ok, &["--seeds", "0"]).code, 2);
    let o = test_cmd(&ok, &["--start", "nowhere"]);
    assert_eq!(o.code, 1);
}
