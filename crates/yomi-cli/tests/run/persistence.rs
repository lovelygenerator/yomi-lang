//! Persistent data (--persistent), rollback with persistent data, random numbers

use super::*;

/// The path for the persistent data file (not created yet).
pub(super) fn persistent_path(test: &str) -> PathBuf {
    let path = scratch(test).join("data.yomp");
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn persistent_writes_data_and_carries_it_to_next_run() {
    let path = persistent_path("persist-carry");
    let p = path.to_str().unwrap();
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "はじめまして\nスコア10 最高10\n（おわり）\n");
    assert_eq!(o.stderr, "");
    assert!(path.exists(), "永続データのファイルができる");
    // Second round: passed stop points (seen) and persistent vars are carried over
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "また来たね 2\nスコア20 最高20\n（おわり）\n");
    // Third round
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.stdout, "また来たね 3\nスコア30 最高30\n（おわり）\n");
    // No temporary files are left
    assert!(!path.with_extension("yomp.tmp").exists());
}

#[test]
fn without_persistent_nothing_is_carried_over() {
    for _ in 0..2 {
        let o = run("ja", "persist.yomi", &[], "");
        assert_eq!(o.stdout, "はじめまして\nスコア10 最高10\n（おわり）\n");
    }
}

#[test]
fn persistent_keeps_data_up_to_quit_midway() {
    let path = persistent_path("persist-quit");
    let p = path.to_str().unwrap();
    // End at the first stop point (seen and plays are recorded by then)
    let o = run("ja", "persist.yomi", &["--persistent", p], ":quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "はじめまして\n");
    let o = run("ja", "persist.yomi", &["--persistent", p], ":vars\n:quit\n");
    assert_eq!(o.stdout, "また来たね 2\nplays = 2\nbest = 0\nscore = 20\n");
}

#[test]
fn vars_also_prints_persistent_var() {
    let o = run("ja", "persist.yomi", &[], "\n:vars\n:quit\n");
    assert_eq!(
        o.stdout,
        "はじめまして\nスコア10 最高10\nplays = 1\nbest = 10\nscore = 10\n"
    );
}

#[test]
fn back_does_not_restore_persistent_var() {
    let o = run(
        "ja",
        "rollback.yomi",
        &[],
        "\n\n:vars\n:back\n:vars\n:back\n:vars\n:quit\n",
    );
    assert_eq!(
        o.stdout,
        concat!(
            "一\n二\n三\n",
            "best = 3\nn = 3\n",
            // Back to 二: n goes back, best does not
            "二\n",
            "best = 3\nn = 2\n",
            "一\n",
            "best = 3\nn = 1\n",
        )
    );
}

#[test]
fn passed_stop_points_do_not_go_back_after_back() {
    let path = persistent_path("back-seen");
    let p = path.to_str().unwrap();
    // Advance 2, go back 1, and run to the end. Going back does not erase seen, so both remain in the persistent data
    let o = run("ja", "persist.yomi", &["--persistent", p], "\n:back\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "はじめまして\nスコア10 最高10\nはじめまして\nスコア10 最高10\n（おわり）\n"
    );
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert!(o.stdout.starts_with("また来たね "), "{}", o.stdout);
}

#[test]
fn load_reads_only_save_and_uses_current_runs_persistent_data() {
    let dir = scratch("load-persistent");
    let save = dir.join("p.sav");
    let save = save.to_str().unwrap();
    // Save at the first stop point
    let o = run(
        "ja",
        "rollback.yomi",
        &[],
        &format!(":save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    // In another run, advance 2 and then load: n is as when saved (1), best stays as advanced (3)
    let o = run(
        "ja",
        "rollback.yomi",
        &[],
        &format!("\n\n:load {save}\n:vars\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        format!("一\n二\n三\n{save} から読み込みました\n一\nbest = 3\nn = 1\n")
    );
}

#[test]
fn broken_persistent_data_file_ends_with_error() {
    let path = persistent_path("persist-broken");
    std::fs::write(&path, b"YOMP\x05\x00\x00\x00xxxx").unwrap();
    let p = path.to_str().unwrap();
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(
        o.stderr.contains("永続データ") && o.stderr.contains("壊れています"),
        "{}",
        o.stderr
    );
    let o = run("en", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("cannot read the persistent data"),
        "{}",
        o.stderr
    );
    // A save file is not persistent data
    std::fs::write(&path, b"YOMS\x04\x00\x00\x00").unwrap();
    let o = run("ja", "persist.yomi", &["--persistent", p], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("セーブファイルではありません") || o.stderr.contains("yomi のセーブ"),
        "{}",
        o.stderr
    );
}

#[test]
fn failing_to_write_persistent_data_ends_with_error() {
    let dir = scratch("persist-unwritable");
    // A path whose parent directory does not exist
    let path = dir.join("no-such-dir").join("data.yomp");
    let o = run(
        "ja",
        "persist.yomi",
        &["--persistent", path.to_str().unwrap()],
        "",
    );
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("永続データ") && o.stderr.contains("書けません"),
        "{}",
        o.stderr
    );
}

#[test]
fn persistent_data_for_changed_script_reports_added_removed_and_type_changes_and_continues() {
    let dir = scratch("persist-compat");
    let path = persistent_path("persist-compat");
    let p = path.to_str().unwrap();
    // Rewrite and run a script with the same module name (game)
    let script = dir.join("game.yomi");
    let v1 = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/persist.yomi"),
    )
    .unwrap();
    std::fs::write(&script, v1).unwrap();
    let script_arg = script.to_str().unwrap().to_string();
    let args = || {
        [
            "--lang",
            "ja",
            "run",
            script_arg.as_str(),
            "--persistent",
            p,
        ]
    };
    let o = yomi(&args(), "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    // A type change fails under the default `Strict` policy and leaves the persistent data as it was
    std::fs::write(
        &script,
        "persistent var plays: str = \"なし\"\npersistent var best: int = 0\n\nlabel start {\n  plays = \"あり\"\n  ${plays} #result\n}\n",
    )
    .unwrap();
    let o = yomi(&args(), "");
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert!(o.stderr.contains("game.plays"), "{}", o.stderr);
    // With `--lenient`, the changed variable gets its initial value and the run continues
    let copy = path.with_extension("lenient");
    std::fs::copy(&path, &copy).unwrap();
    let copy_arg = copy.to_str().unwrap().to_string();
    let o = yomi(
        &[
            "--lang",
            "ja",
            "run",
            script_arg.as_str(),
            "--persistent",
            copy_arg.as_str(),
            "--lenient",
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stderr.contains("型が変わった変数: game.plays"),
        "{}",
        o.stderr
    );
    // The new script: level is added, plays and best stay. Passed stop points remain
    std::fs::write(
        &script,
        "persistent var plays: int = 0\npersistent var best: int = 0\npersistent var level: int = 5\n\nlabel start {\n  レベル${level} ${plays} #result\n}\n",
    )
    .unwrap();
    let o = yomi(&args(), "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "レベル5 1\n（おわり）\n");
    assert!(
        o.stderr.contains("既定値で補った変数: game.level"),
        "{}",
        o.stderr
    );
    assert!(!o.stderr.contains("捨てた"), "{}", o.stderr);
    // Once more, make it a script without `best`
    std::fs::write(
        &script,
        "persistent var level: int = 5\n\nlabel start {\n  レベル${level} #result\n}\n",
    )
    .unwrap();
    let o = yomi(&args(), "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stderr
            .contains("台本から消えていたので捨てた変数: game.plays, game.best"),
        "{}",
        o.stderr
    );
}

#[test]
fn same_seed_gives_same_random_sequence_and_different_seed_differs() {
    let a = run("ja", "random.yomi", &["--seed", "42"], "");
    let b = run("ja", "random.yomi", &["--seed", "42"], "");
    let c = run("ja", "random.yomi", &["--seed", "43"], "");
    assert_eq!(a.code, 0, "{}", a.stderr);
    assert_eq!(a.stdout, b.stdout);
    assert_ne!(a.stdout, c.stdout);
    assert_eq!(a.stdout.lines().count(), 4, "{}", a.stdout);
}

#[test]
fn random_gives_same_value_when_advancing_again_after_back() {
    let o = run("ja", "random.yomi", &["--seed", "7"], "\n:back\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    // 1, 2, (going back) 1 printed again… the second one and the second one after going back have the same value
    assert_eq!(lines[0], lines[2], "{}", o.stdout);
    assert_eq!(lines[1], lines[3], "{}", o.stdout);
}

#[test]
fn random_min_greater_than_max_ends_with_runtime_error() {
    let o = run("ja", "random_bad.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "先に\n");
    assert!(
        o.stderr
            .contains("random の最小（3）が最大（2）より大きいです"),
        "{}",
        o.stderr
    );
    let o = run("en", "random_bad.yomi", &[], "");
    assert!(
        o.stderr
            .contains("the minimum of random (3) is greater than the maximum (2)"),
        "{}",
        o.stderr
    );
}

#[test]
fn flow_waits_do_not_wait_but_show_stop_point_and_advance_on_enter() {
    let o = run("ja", "std_flow.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "はじまり\n（1.5 秒の待ち: Enter で進みます）\n（入力待ち: Enter で進みます）\nおわり\n（おわり）\n"
    );
    let o = run("en", "std_flow.yomi", &[], "\n:quit\n");
    assert_eq!(
        o.stdout,
        "はじまり\n(wait 1.5 seconds: press Enter to continue)\n"
    );
}

#[test]
fn math_and_text_functions_can_be_called_from_scripts() {
    let o = run("ja", "std_math_text.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "文字数 2\n最大 5 小数 3\n（おわり）\n");
}

#[test]
fn text_string_operations_can_be_called_from_scripts() {
    let o = run("ja", "std_text_ops.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "ゆみ・ひろ・さく 5\n007 OK いう\nSome(2) true ====\n（おわり）\n"
    );
}

#[test]
fn huge_text_repeat_ends_with_runtime_error_without_using_memory() {
    let o = run("ja", "std_text_repeat_huge.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "前\n");
    assert!(
        o.stderr.contains("text.repeat の結果が大きすぎます") && o.stderr.contains("5 行目"),
        "{}",
        o.stderr
    );
}

#[test]
fn negative_seconds_to_flow_wait_end_with_runtime_error() {
    let o = run("ja", "std_negative_wait.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "前\n");
    assert!(
        o.stderr.contains("flow.wait の秒数は 0 以上") && o.stderr.contains("5 行目"),
        "{}",
        o.stderr
    );
}

#[test]
fn unimported_standard_package_stops_with_diagnostic() {
    let o = run("ja", "std_not_imported.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("math"), "{}", o.stderr);
}

#[test]
fn audio_and_image_cues_print_one_line_each_before_stop_point_ja() {
    let o = run("ja", "std_audio_image.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "はじまり\n\
         [audio] bgm = town 音量 1（フェード 1.5 秒）\n\
         [audio] se door（効果音）\n\
         [image] show bg = street (0, 0)（フェード 0 秒）\n\
         [image] show left = guide (0.25, 0.5)（フェード 0.5 秒）\n\
         音と絵\n\
         [audio] bgm 音量 0.5（フェード 2 秒）\n\
         [audio] voice v001（ボイス）\n\
         [image] move left → (0.4, 0.5)（1 秒）\n\
         動く\n\
         [audio] bgm 停止（フェード 1 秒）\n\
         [image] hide left（フェード 0.5 秒）\n\
         [image] clear（フェード 0 秒）\n\
         おわり\n\
         （おわり）\n"
    );
}

#[test]
fn loop_image_transform_and_screen_cues_print_one_line_each_before_stop_point() {
    let o = run("ja", "std_loop_screen.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "[audio] rain = rain_soft 音量 0.5（フェード 2 秒）\n\
         [image] show left = guide (0.2, 0.5)（フェード 0 秒）\n\
         [image] layer left = 2（重なり）\n\
         [image] scale left = 1.5（0.5 秒）\n\
         [image] flip left = true（左右反転）\n\
         [image] alpha left = 0.5（1 秒）\n\
         [screen] fade out black（1 秒）\n\
         暗いまま\n\
         [audio] rain 音量 0.2（フェード 1 秒）\n\
         [audio] se 停止（効果音）\n\
         [audio] voice 停止（ボイス）\n\
         [screen] fade in（1 秒）\n\
         [screen] shake 0.5（0.3 秒）\n\
         [screen] flash white（0.2 秒）\n\
         明るくなった\n\
         [audio] rain 停止（フェード 1 秒）\n\
         （おわり）\n"
    );
    let en = run("en", "std_loop_screen.yomi", &[], "");
    assert_eq!(en.code, 0, "{}", en.stderr);
    assert!(
        en.stdout.contains("[screen] fade out to black (1 s)\n"),
        "{}",
        en.stdout
    );
    assert!(
        en.stdout
            .contains("[audio] rain = rain_soft volume 0.5 (fade 2 s)\n")
    );
}

#[test]
fn audio_and_image_cues_print_in_english_too() {
    let o = run("en", "std_audio_image.yomi", &[], "\n:quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "はじまり\n\
         [audio] bgm = town volume 1 (fade 1.5 s)\n\
         [audio] se door\n\
         [image] show bg = street at (0, 0) (fade 0 s)\n\
         [image] show left = guide at (0.25, 0.5) (fade 0.5 s)\n\
         音と絵\n"
    );
}

#[test]
fn back_does_not_replay_cues() {
    // `:back` at the second stop point (sound and picture) goes back to the first stop point (the beginning). Right after
    // going back, the cues are empty
    let o = run("ja", "std_audio_image.yomi", &[], "\n:back\n:quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    let first = lines.iter().position(|l| *l == "はじまり").unwrap();
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("[audio] bgm = town"))
            .count(),
        1
    );
    assert_eq!(
        lines[first..].iter().filter(|l| **l == "はじまり").count(),
        2
    );
    assert_eq!(lines.last(), Some(&"はじまり"), "{:?}", o.stdout);
}

#[test]
fn load_does_not_replay_cues() {
    let dir = scratch("load_cues");
    let save = dir.join("a.sav");
    let save = save.to_str().unwrap();
    // Save at the second stop point, go further, and load
    let o = run(
        "ja",
        "std_audio_image.yomi",
        &[],
        &format!("\n:save {save}\n\n:load {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    // Before the stop point after loading (sound and picture), no cues are printed
    let after = o.stdout.rsplit_once("から読み込みました").unwrap().1;
    assert_eq!(after.trim(), "音と絵", "{}", o.stdout);
}

#[test]
fn input_reads_values_from_stdin_and_empty_or_eof_gives_default() {
    let o = run("ja", "std_input.yomi", &[], "たろう\n42\nn\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "[input] 名前は？（文字列。4 文字まで。Enter で既定値: ゆみ）\n\
         [input] 年齢は？（整数 0〜120。Enter で既定値: 10）\n\
         [input] よろしいですか？（y/n。Enter で「はい」）\n\
         おわり\n\
         （おわり）\n"
    );
    // An empty line and the end of input give the default value
    let o = run("en", "std_input.yomi", &[], "\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with("おわり\n(end)\n"), "{}", o.stdout);
    let o = run("en", "std_input.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout
            .contains("[input] 名前は？ (text, up to 4 characters; Enter for the default: ゆみ)")
    );
}

#[test]
fn unreadable_integer_and_yes_no_input_ends_with_error() {
    let o = run("ja", "std_input.yomi", &[], "\nabc\n");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("整数ではありません: abc"), "{}", o.stderr);
    let o = run("en", "std_input.yomi", &[], "\n\nmaybe\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("answer with y or n: maybe"),
        "{}",
        o.stderr
    );
}

#[test]
fn input_values_breaking_constraints_end_with_core_runtime_error() {
    // Exceeding the character count (max_len 2, 3 characters)
    let o = run("ja", "std_input_over.yomi", &[], "abc\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("input.text の入力が max_len を超えています"),
        "{}",
        o.stderr
    );
    assert!(!o.stdout.contains("ここには来ない"));
    // Outside the integer range (200 for 0 to 120)
    let o = run("ja", "std_input.yomi", &[], "\n200\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("input.integer の入力が min から max の範囲の外です"),
        "{}",
        o.stderr
    );
}

#[test]
fn standard_package_runtime_errors_follow_display_language() {
    // English (--lang en). No Japanese text mixed in
    let o = run("en", "std_input_over.yomi", &[], "abc\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("the input to input.text is longer than max_len"),
        "{}",
        o.stderr
    );
    let o = run("en", "std_input.yomi", &[], "\n200\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("the input to input.integer is outside the range from min to max"),
        "{}",
        o.stderr
    );
    let o = run("en", "std_negative_wait.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("the seconds of flow.wait must be 0 or more"),
        "{}",
        o.stderr
    );
    // Japanese is the same text as before
    let o = run("ja", "std_negative_wait.yomi", &[], "");
    assert!(
        o.stderr
            .contains("flow.wait の秒数は 0 以上でなければなりません"),
        "{}",
        o.stderr
    );
}

#[test]
fn old_option_and_interactive_command_names_are_unavailable() {
    let script = path_of("texts.yomi");
    // The old name --language is an "unknown option" of clap
    let o = yomi(
        &[
            "--lang",
            "en",
            "texts",
            "template",
            &script,
            "--language",
            "en",
        ],
        "",
    );
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("--language"), "{}", o.stderr);
    assert!(o.stdout.is_empty(), "{}", o.stdout);
    // The old name :lang is an unknown command. Points to :texts
    let table = path_of("texts_en.texts");
    for (lang, hint) in [("en", ":texts <file|off>"), ("ja", ":texts <ファイル|off>")] {
        let o = run(lang, "texts.yomi", &[], &format!(":lang {table}\n:quit\n"));
        assert_eq!(o.code, 0, "{}", o.stderr);
        assert!(o.stderr.contains(":lang"), "{}", o.stderr);
        assert!(o.stderr.contains(hint), "{lang}: {}", o.stderr);
        // The table is not set and it stays the original
        assert_eq!(o.stdout, "朝 3 日目\n", "{lang}");
    }
    // For other unknown commands, the list of available commands includes :texts
    let o = run("en", "texts.yomi", &[], ":nope\n:quit\n");
    assert!(o.stderr.contains(":texts"), "{}", o.stderr);
    assert!(!o.stderr.contains(":lang"), "{}", o.stderr);
}

#[test]
fn texts_help_prints_descriptions_containing_n_fully() {
    for lang in ["en", "ja"] {
        for args in [&["texts", "--help"][..], &["texts", "check", "--help"][..]] {
            let mut a = vec!["--lang", lang];
            a.extend_from_slice(args);
            let o = yomi(&a, "");
            assert_eq!(o.code, 0, "{lang} {args:?}: {}", o.stderr);
            assert!(o.stdout.contains("${n}"), "{lang} {args:?}: {}", o.stdout);
            assert!(!o.stdout.contains('\u{200B}'), "{lang} {args:?}");
        }
    }
    // The description continues after ${n}
    let o = yomi(&["--lang", "en", "texts", "check", "--help"], "");
    assert!(o.stdout.contains("wrong ${n} numbers)"), "{}", o.stdout);
    let o = yomi(&["--lang", "ja", "texts", "check", "--help"], "");
    assert!(o.stdout.contains("${n} の番号の誤り"), "{}", o.stdout);
}

#[test]
fn input_line_starting_with_colon_is_interactive_command() {
    let o = run("ja", "std_input.yomi", &[], ":quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "[input] 名前は？（文字列。4 文字まで。Enter で既定値: ゆみ）\n"
    );
}

#[test]
fn input_return_values_received_by_let_and_assignment_can_be_shown() {
    let o = run("ja", "std_input_bind.yomi", &[], "たろう\n42\nn\n\n7\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "[input] 名前は？（文字列。8 文字まで。Enter で既定値: ゆみ）\n\
         [input] 年齢は？（整数 0〜120。Enter で既定値: 10）\n\
         [input] よろしいですか？（y/n。Enter で「はい」）\n\
         たろうさん（42歳）\n\
         [input] 合計は？（整数 0〜100。Enter で既定値: 0）\n\
         false、7\n\
         （おわり）\n"
    );
    // An empty line gives the default value, and the default goes into the variable
    let o = run("en", "std_input_bind.yomi", &[], "\n\n\n\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains("ゆみさん（10歳）\n") && o.stdout.contains("true、0\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn input_variables_show_in_vars_go_back_with_back_and_survive_save_load() {
    let o = run(
        "ja",
        "std_input_bind.yomi",
        &[],
        "たろう\n42\ny\n:vars\n:quit\n",
    );
    assert!(
        o.stdout
            .ends_with("total = 0\nname = \"たろう\"\nage = 42\nok = true\n"),
        "{}",
        o.stdout
    );
    // After entering the age, go back one and enter it again (the name stays as received before the stop point gone back to)
    let o = run(
        "ja",
        "std_input_bind.yomi",
        &[],
        "たろう\n42\n:back\n30\ny\n\n7\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("たろうさん（30歳）\n"), "{}", o.stdout);
    assert!(!o.stdout.contains("42歳"), "{}", o.stdout);
    // Save at the age stop point, and in another run, load and then answer. The name is the one received when saved
    let dir = scratch("input-bind-load");
    let save = dir.join("b.sav");
    let save = save.to_str().unwrap();
    let o = run(
        "ja",
        "std_input_bind.yomi",
        &[],
        &format!("たろう\n:save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = run(
        "ja",
        "std_input_bind.yomi",
        &[],
        &format!(":load {save}\n42\ny\n\n7\n\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains("たろうさん（42歳）\n") && o.stdout.contains("true、7\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn vars_prints_maps_in_insertion_order_and_hides_for_variables() {
    let o = run("ja", "map_for.yomi", &[], "\n\n\n:vars\n:quit\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains("登録 map { 案内人: 3, 村長: 5 }\n"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("scores = map { \"村長\": 5 }\ntotal = 8\n"),
        "{}",
        o.stdout
    );
    assert!(!o.stdout.contains("$for"), "{}", o.stdout);
}

#[test]
fn saving_mid_for_continues_identically_after_load() {
    let dir = scratch("for-save");
    let save = dir.join("f.sav");
    let save = save.to_str().unwrap();
    // Save at the third stop point (inside the body of the second round of for)
    let o = run(
        "ja",
        "map_for.yomi",
        &[],
        &format!("\n\n:save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = run(
        "ja",
        "map_for.yomi",
        &[],
        &format!(":load {save}\n:vars\n\n\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout
            .contains("村長 8\nscores = map { \"案内人\": 3, \"村長\": 5 }\ntotal = 8\n"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout.ends_with("おわり map { 村長: 5 } 8\n（おわり）\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn updating_missing_map_key_ends_with_runtime_error() {
    let o = run("ja", "map_missing.yomi", &[], "\n\n");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "はじまり\n");
    assert!(
        o.stderr
            .contains("エラー: map にキー 案内人 がありません（ラベル map_missing.start、5 行目）"),
        "{}",
        o.stderr
    );
    let o = run("en", "map_missing.yomi", &[], "\n\n");
    assert!(
        o.stderr.contains("error: the map has no key 案内人"),
        "{}",
        o.stderr
    );
}

#[test]
fn fn_calls_in_expressions_continue_identically_after_save_and_load() {
    let o = run("ja", "fn_calc.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "階乗 120\n合計 12\nおわり 479001600\n（おわり）\n"
    );
    let dir = scratch("fn-calc-save");
    let save = dir.join("f.sav");
    let save = save.to_str().unwrap();
    let o = run(
        "ja",
        "fn_calc.yomi",
        &[],
        &format!("\n:save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = run(
        "ja",
        "fn_calc.yomi",
        &[],
        &format!(":load {save}\n:vars\n\n\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("total = 12\n"), "{}", o.stdout);
    assert!(
        o.stdout.ends_with("おわり 479001600\n（おわり）\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn too_deep_fn_recursion_stops_at_call_depth_limit() {
    let o = run("ja", "fn_depth.yomi", &[], "\n\n");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "はじまり\n");
    assert!(
        o.stderr
            .contains("エラー: 呼び出し（call・fn）の入れ子が、深さの上限を超えました（ラベル fn_depth.deep、2 行目）"),
        "{}",
        o.stderr
    );
    let o = run("en", "fn_depth.yomi", &[], "\n\n");
    assert!(
        o.stderr
            .contains("calls (call or fn) are nested deeper than the call depth limit"),
        "{}",
        o.stderr
    );
}

#[test]
fn statements_and_writes_not_allowed_in_fn_are_compile_errors() {
    let o = run("ja", "fn_bad.yomi", &[], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("fn_bad.yomi:4:3"), "{}", o.stderr);
    assert!(
        o.stderr.contains("fn の中から変数 'score' へ書けません"),
        "{}",
        o.stderr
    );
    assert!(o.stderr.contains("fn_bad.yomi:5:3"), "{}", o.stderr);
    assert!(
        o.stderr.contains("fn の中では 'jump' を使えません"),
        "{}",
        o.stderr
    );
    let o = run("en", "fn_bad.yomi", &[], "");
    assert!(
        o.stderr
            .contains("a fn cannot write to the variable 'score'"),
        "{}",
        o.stderr
    );
}
