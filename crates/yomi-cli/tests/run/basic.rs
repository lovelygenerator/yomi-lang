//! Basics

use super::*;

#[test]
fn without_input_runs_to_end_and_finishes() {
    let o = run("ja", "hello.yomi", &[], "");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "おはよう\nこんにちは 1\nさようなら 2\n（おわり）\n"
    );
    assert_eq!(o.stderr, "");
}

#[test]
fn can_display_in_english() {
    let o = run("en", "hello.yomi", &[], "");
    assert_eq!(o.code, 0);
    assert!(o.stdout.ends_with("(end)\n"), "{}", o.stdout);
}

#[test]
fn empty_line_advances_to_next_stop_point() {
    // `:quit` while stopped at line 1, and line 2 does not appear.
    let o = run("ja", "hello.yomi", &[], ":quit\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "おはよう\n");
    // One empty line advances to line 2, and `:quit` ends.
    let o = run("ja", "hello.yomi", &[], "\n:quit\n");
    assert_eq!(o.stdout, "おはよう\nこんにちは 1\n");
}

#[test]
fn start_label_can_be_specified_and_jump_between_labels_works() {
    let o = run("ja", "labels.yomi", &[], "");
    assert_eq!(o.stdout, "最初\n別のラベル\n（おわり）\n");
    let o = run("ja", "labels.yomi", &["--start", "other"], "");
    assert_eq!(o.stdout, "別のラベル\n（おわり）\n");
}

#[test]
fn missing_start_label_ends_with_runtime_error() {
    let o = run("ja", "labels.yomi", &["--start", "nowhere"], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(
        o.stderr
            .contains("エラー: 開始のラベル 'labels.nowhere' がありません"),
        "{}",
        o.stderr
    );
}

#[test]
fn vars_prints_variables_with_names_and_values() {
    let o = run("ja", "hello.yomi", &[], "\n:vars\n:quit\n");
    assert_eq!(o.stdout, "おはよう\nこんにちは 1\nn = 1\n");
}

#[test]
fn vars_prints_compound_variables_in_fixed_notation() {
    let o = run("ja", "composite.yomi", &[], ":vars\n\n:vars\n:quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        concat!(
            "はじまり 7\n",
            "c = Chara { name: \"案内人\", hp: 7, items: [\"薬\"] }\n",
            "mode = Mode.Idle\n",
            "best = None\n",
            "n = 0\n",
            "2歩\n",
            "c = Chara { name: \"案内人\", hp: 7, items: [\"薬\"] }\n",
            "mode = Mode.Run(2)\n",
            "best = Some(7)\n",
            "n = 2\n",
        )
    );
}

#[test]
fn compound_variables_restore_with_save_and_load() {
    let dir = scratch("composite-save");
    let save = dir.join("c.sav");
    let save = save.to_str().unwrap();
    // Save at the second stop point (after `mode` and `best` changed)
    let o = run(
        "ja",
        "composite.yomi",
        &[],
        &format!("\n:save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = run(
        "ja",
        "composite.yomi",
        &[],
        &format!(":load {save}\n:vars\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains(concat!(
            "c = Chara { name: \"案内人\", hp: 7, items: [\"薬\"] }\n",
            "mode = Mode.Run(2)\n",
            "best = Some(7)\n",
        )),
        "{}",
        o.stdout
    );
}

#[test]
fn save_writes_and_load_continues_from_stop_point() {
    let dir = scratch("save-load");
    let save = dir.join("a.sav");
    let save = save.to_str().unwrap();

    // Save at the second stop point (n = 1) and end.
    let o = run("ja", "hello.yomi", &[], &format!("\n:save {save}\n:quit\n"));
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        format!("おはよう\nこんにちは 1\n{save} に保存しました\n")
    );
    assert!(std::fs::metadata(save).unwrap().len() > 0);

    // Loading at the first stop point of a new run continues from the saved stop point.
    let o = run("ja", "hello.yomi", &[], &format!(":load {save}\n:vars\n\n"));
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        format!(
            "おはよう\n{save} から読み込みました\nこんにちは 1\nn = 1\nさようなら 2\n（おわり）\n"
        )
    );
}

#[test]
fn loading_broken_save_ends_with_error() {
    let dir = scratch("bad-load");
    let bad = dir.join("bad.sav");
    std::fs::write(&bad, b"not a save file at all").unwrap();
    let o = run(
        "en",
        "hello.yomi",
        &[],
        &format!(":load {}\n", bad.display()),
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains(": error: "), "{}", o.stderr);
    assert!(
        o.stderr.contains("this is not a yomi save file") || o.stderr.contains("damaged"),
        "{}",
        o.stderr
    );
}

#[test]
fn loading_unreadable_file_can_continue() {
    let o = run("en", "hello.yomi", &[], ":load /nonexistent/x.sav\n:quit\n");
    assert_eq!(o.code, 0);
    assert!(o.stderr.contains("/nonexistent/x.sav"), "{}", o.stderr);
}

#[test]
fn back_returns_to_previous_stop_point_with_variables() {
    let o = run(
        "ja",
        "hello.yomi",
        &[],
        "\n\n:vars\n:back\n:vars\n:back\n:vars\n:quit\n",
    );
    assert_eq!(
        o.stdout,
        concat!(
            "おはよう\nこんにちは 1\nさようなら 2\n",
            "n = 2\n",
            "こんにちは 1\n",
            "n = 1\n",
            "おはよう\n",
            "n = 0\n",
        )
    );
}

#[test]
fn advancing_after_back_passes_same_stop_point_again() {
    let o = run("ja", "hello.yomi", &[], "\n\n:back\n\n");
    assert_eq!(
        o.stdout,
        "おはよう\nこんにちは 1\nさようなら 2\nこんにちは 1\nさようなら 2\n（おわり）\n"
    );
}

#[test]
fn reports_and_stays_when_there_is_no_stop_point_to_go_back_to() {
    let o = run("ja", "hello.yomi", &[], ":back\n:quit\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "おはよう\n戻れる停止点がありません\n");
}

#[test]
fn unknown_interactive_commands_print_to_stderr_and_continue() {
    let o = run("en", "hello.yomi", &[], ":nope\n:quit\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "おはよう\n");
    assert!(o.stderr.contains("unknown command: :nope"), "{}", o.stderr);
}

#[test]
fn diagnostics_print_file_line_column_severity_message_and_exit_1_japanese() {
    let o = run("ja", "broken.yomi", &[], "");
    let path = fixture("broken.yomi");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert_eq!(
        o.stderr,
        format!(
            "{}:3:3: エラー: 存在しないラベルです: 'nowhere'\n",
            path.display()
        )
    );
}

#[test]
fn diagnostics_print_file_line_column_severity_message_and_exit_1_english() {
    let o = run("en", "broken.yomi", &[], "");
    let path = fixture("broken.yomi");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert_eq!(
        o.stderr,
        format!("{}:3:3: error: unknown label 'nowhere'\n", path.display())
    );
}

#[test]
fn runtime_errors_print_to_stderr_with_location_and_exit_1() {
    let o = run("ja", "divzero.yomi", &[], "");
    let path = fixture("divzero.yomi");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "あ\n");
    assert_eq!(
        o.stderr,
        format!(
            "{}: エラー: 0 で割りました（ラベル divzero.start、4 行目）\n",
            path.display()
        )
    );
}

#[test]
fn missing_script_file_ends_with_error() {
    let o = yomi(&["--lang", "en", "run", "/nonexistent/none.yomi"], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("cannot read /nonexistent/none.yomi"),
        "{}",
        o.stderr
    );
}

#[test]
fn warnings_print_to_stderr_and_script_runs() {
    let o = run("en", "warn.yomi", &[], "");
    let path = fixture("warn.yomi");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "9\n(end)\n");
    assert_eq!(
        o.stderr,
        format!(
            "{}:1:10: warning: the variable 'to_int' has the same name as a built-in function (the built-in function can no longer be used)\n",
            path.display()
        )
    );
    let ja = run("ja", "warn.yomi", &[], "");
    assert!(
        ja.stderr
            .contains(":1:10: 警告: 組み込み関数と同じ名前 'to_int'"),
        "{}",
        ja.stderr
    );
}

#[test]
fn diagnostics_get_did_you_mean_for_close_names() {
    let o = run("ja", "typo.yomi", &[], "");
    let path = fixture("typo.yomi");
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        format!(
            "{}:4:3: エラー: 宣言されていない名前です: 'gld'（もしかして: gold）\n",
            path.display()
        )
    );
}

#[test]
fn scripts_using_floats_and_while_run() {
    let o = run("en", "whilefloat.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "1.0 after 4 steps\n3\n(end)\n");
}

#[test]
fn float_runtime_errors_show_module_label_and_line() {
    let o = run("en", "floatdiv.yomi", &[], "");
    let path = fixture("floatdiv.yomi");
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        format!(
            "{}: error: division by zero (label floatdiv.start, line 3)\n",
            path.display()
        )
    );
}
