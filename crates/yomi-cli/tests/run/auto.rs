//! yomi run --auto and --show-ids (automatic play; answering with the stable IDs of options)

use super::*;

/// The output when answering the 3 choices of `choices.yomi` with IDs (`--auto`).
pub(super) const AUTO_CHOICES_OUT: &str = "案内人「どうする？」\n「ひとりごと」\n1: 「話す」 #talk\n2: 「調べる」 #look\n話した\n続き\n案内人「どうする？」\n「ひとりごと」\n1: 「話す」 #talk\n2: 「調べる」 #look\n3: 「帰る」 #leave\n調べた\n続き\n案内人「どうする？」\n「ひとりごと」\n1: 「話す」 #talk\n2: 「帰る」 #leave\nさようなら\n（おわり）\n";

#[test]
fn auto_skips_bodies_and_answers_choices_by_stable_id() {
    let o = run("ja", "choices.yomi", &["--auto"], "#talk\n#look\n#leave\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, AUTO_CHOICES_OUT);
    assert_eq!(o.stderr, "");
    // Empty lines for bodies are not needed (if present, they are read as answers to choices and it stops)
    let o = run("ja", "choices.yomi", &["--auto"], "\n#talk\n");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("答え"), "{}", o.stderr);
}

#[test]
fn auto_answers_choices_by_number_and_mixes_numbers_and_ids() {
    // 1st: a number, 2nd: an ID, 3rd: a number (go home)
    let o = run("ja", "choices.yomi", &["--auto"], "1\n#look\n2\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, AUTO_CHOICES_OUT);
}

#[test]
fn auto_shows_option_ids_and_none_for_options_without_ids() {
    let dir = scratch("auto-noid");
    let path = write_file(
        &dir,
        "a.yomi",
        "label start {\n  choice {\n    「はい」 #yes => 返事\n    「いいえ」 => 返事\n  }\n}\n",
    );
    let o = yomi(
        &["--lang", "ja", "run", path.to_str().unwrap(), "--auto"],
        "#yes\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "1: 「はい」 #yes\n2: 「いいえ」\n返事\n（おわり）\n"
    );
}

#[test]
fn auto_exits_1_on_id_not_in_list() {
    let o = run("ja", "choices.yomi", &["--auto"], "#talk\n#nope\n#look\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("（--auto）答え #nope は、一覧のどの選択肢の ID でもありません"),
        "{}",
        o.stderr
    );
    // Input after an ID not in the list is not read, and it does not go further (after the first choice, it stops by the
    // second list; not checked)
    assert!(o.stdout.ends_with("3: 「帰る」 #leave\n"), "{}", o.stdout);
    assert!(!o.stdout.contains("調べた"));
    let o = run("en", "choices.yomi", &["--auto"], "#zzz\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("(--auto) the answer #zzz is not the ID of any option in the list"),
        "{}",
        o.stderr
    );
}

#[test]
fn auto_exits_1_on_input_that_is_not_an_answer() {
    for bad in ["0\n", "9\n", "abc\n", "\n"] {
        let o = run("ja", "choices.yomi", &["--auto"], bad);
        assert_eq!(o.code, 1, "{bad:?}");
        assert!(
            o.stderr.contains("選択になりません"),
            "{bad:?}: {}",
            o.stderr
        );
    }
}

#[test]
fn auto_exits_1_when_input_ends_before_choice() {
    let o = run("ja", "choices.yomi", &["--auto"], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("（--auto）選択の答えを読む前に、入力が終わりました"),
        "{}",
        o.stderr
    );
    // The list of options is shown before it stops
    assert_eq!(
        o.stdout,
        "案内人「どうする？」\n「ひとりごと」\n1: 「話す」 #talk\n2: 「調べる」 #look\n"
    );
    let o = run("en", "choices.yomi", &["--auto"], "#talk\n");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr
            .contains("(--auto) the input ended before the answer to a choice"),
        "{}",
        o.stderr
    );
    // Without `--auto`, it chooses number 1 as before (unchanged)
    let o = run("ja", "eof_choice.yomi", &[], "");
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("返事した"));
}

#[test]
fn auto_skips_input_for_flow_waits_and_reads_input_stop_points() {
    let o = run("ja", "std_flow.yomi", &["--auto"], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "はじまり\n（1.5 秒の待ち: Enter で進みます）\n（入力待ち: Enter で進みます）\nおわり\n（おわり）\n"
    );
    // `input.*` reads one line at a time. When input runs out, the default value as before
    let o = run("ja", "std_input.yomi", &["--auto"], "たろう\n30\nn\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with("おわり\n（おわり）\n"), "{}", o.stdout);
    let o = run("ja", "std_input.yomi", &["--auto"], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
}

#[test]
fn auto_allows_interactive_commands() {
    let dir = scratch("auto-commands");
    let save = dir.join("a.sav");
    let save = save.to_str().unwrap();
    // At a choice stop point, variables can be shown, saved, gone back and loaded
    let input =
        format!(":vars\n:save {save}\n#talk\n:back\n#look\n:load {save}\n#talk\n#talk\n#leave\n");
    let o = run("ja", "choices.yomi", &["--auto"], &input);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains("回数 = 0") || o.stdout.contains("main.回数 = 0"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("保存しました"), "{}", o.stdout);
    assert!(o.stdout.contains("から読み込みました"), "{}", o.stdout);
    assert!(
        o.stdout.ends_with("さようなら\n（おわり）\n"),
        "{}",
        o.stdout
    );
    // "調べる" was chosen only once (loading goes back to the state before choosing)
    assert_eq!(o.stdout.matches("調べた").count(), 1, "{}", o.stdout);
}

#[test]
fn show_ids_shows_ids_without_auto_and_accepts_ids() {
    // The display changes (with IDs). Answers can be numbers or IDs
    let o = run(
        "ja",
        "choices.yomi",
        &["--show-ids"],
        "\n\n#talk\n\n\n\n\n#look\n\n\n\n\n#leave\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout
            .contains("1: 「話す」 #talk\n2: 「調べる」 #look\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("調べた\n"), "{}", o.stdout);
    assert!(
        o.stdout.ends_with("さようなら\n（おわり）\n"),
        "{}",
        o.stdout
    );
    // Without the option, IDs are not shown (the output stays as before)
    let o = run("ja", "choices.yomi", &[], "\n\n1\n:quit\n");
    assert!(
        o.stdout.contains("1: 「話す」\n2: 「調べる」\n"),
        "{}",
        o.stdout
    );
    assert!(!o.stdout.contains('#'));
}

#[test]
fn without_auto_unknown_id_asks_again() {
    let o = run(
        "en",
        "choices.yomi",
        &[],
        "\n\n#nope\n#talk\n\n\n\n\n#leave\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stderr
            .matches("no option with the ID #nope in this list")
            .count(),
        1,
        "{}",
        o.stderr
    );
    assert!(o.stdout.contains("話した"), "{}", o.stdout);
}
