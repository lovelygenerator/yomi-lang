//! `:repl` of yomi run (7.1, 7.3.4, 7.3.6)

use super::*;

#[test]
fn repl_prints_entry_line_and_exit_returns_to_same_stop_point() {
    let o = run(GAME, &["--start", "talk"], ":repl\nfavor\n:exit\n\n2\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\n(entering the REPL on a copy of this stop point: :exit returns here, :help lists the commands)\n3\n1: 「買う」\n2: 「帰る」\n(end)\n"
    );
}

#[test]
fn changes_in_repl_do_not_change_script_without_apply() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nfavor = 9\nlet x = 1\n:exit\n:vars\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("favor = 3\n"), "{}", o.stdout);
    assert!(!o.stdout.contains("favor = 9\nitems"), "{}", o.stdout);
    // The stop point is not shown again
    assert_eq!(o.stdout.matches("リオン「信頼度は").count(), 1);
}

#[test]
fn apply_writes_variables_and_shows_rebuilt_stop_point() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nfavor += 4\ngold = 1\n:apply\n:exit\n:vars\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\n(entering the REPL on a copy of this stop point: :exit returns here, :help lists the commands)\nfavor = 7\ngold = 1\nリオン「信頼度は7」\nfavor = 7\nitems = [\"お面\"]\nbest = 0\ngold = 1\n1: 「買う」\n2: 「帰る」\n(end)\n"
    );
}

#[test]
fn applied_values_show_in_following_stop_points_and_vars() {
    // Change it to 7 and choose "買う", and it becomes 8 and ends
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nfavor = 7\n:apply\n:exit\n\n1\n:vars\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with("(end)\n"), "{}", o.stdout);
    // After the end, :vars is not read, so check with another input
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nfavor = 7\n:apply\n:exit\n\n:vars\n1\n",
    );
    assert!(o.stdout.contains("favor = 7\nitems"), "{}", o.stdout);
}

#[test]
fn persistent_var_is_not_applied_but_reported() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nbest = 5\nfavor = 4\n:apply\n:vars\n:exit\n:vars\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "not applied (persistent): repl_game.best\n");
    // The script's `best` stays 0
    let vars: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("best = "))
        .collect();
    assert_eq!(vars, ["best = 5", "best = 0"]);
}

#[test]
fn reports_when_there_are_no_variables_to_apply() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\nfavor += 1\nfavor -= 1\n:apply\n:exit\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("no variables to apply\n"), "{}", o.stdout);
    // Nothing was applied, so the stop point is not shown again
    assert_eq!(o.stdout.matches("リオン「信頼度は").count(), 1);
}

#[test]
fn apply_leaves_everything_and_stays_in_repl_when_stop_point_cannot_be_rebuilt() {
    let o = run(
        GAME,
        &["--start", "calc"],
        ":repl\nd = 0\nfavor = 9\n:apply\n:exit\n:vars\n:repl\nd = 4\nfavor = 9\n:apply\n:exit\n",
    );
    assert_eq!(o.code, 0, "{}", o.stdout);
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    assert!(
        o.stderr
            .starts_with("cannot rebuild the stop point with the new values: "),
        "{}",
        o.stderr
    );
    // A failed `:apply` changes nothing (`favor` stays 3), and the copy remains. The second time, both are applied
    assert!(
        o.stdout.contains(
            "favor = 3
"
        ),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout.contains(
            "favor = 9
d = 4
"
        ),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("リオン「割ると25」"), "{}", o.stdout);
}

#[test]
fn apply_writes_save_vars_and_stop_point_locals_together() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl
let bonus = 1
favor = 8
items = [\"a\"]
:apply
:vars
:exit
",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    // `bonus` (a REPL variable) is not subject to applying
    assert!(
        o.stdout.contains("favor = 8\nitems = [\"a\"]\n"),
        "{}",
        o.stdout
    );
    assert_eq!(o.stdout.matches("bonus = 1\n").count(), 2, "{}", o.stdout);
}

#[test]
fn back_after_apply_returns_to_state_before_apply() {
    // The first stop point (talk) → proceed by choosing, and go back. `:apply` retakes the snapshot of the current stop point
    let o = run(
        GAME,
        &["--start", "talk"],
        "\n:repl\nfavor = 20\n:apply\n:exit\n:vars\n:back\n:vars\n1\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let vars: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("favor = "))
        .collect();
    // Applied (20), `:vars` (20), after going back (3)
    assert_eq!(
        vars,
        ["favor = 20", "favor = 20", "favor = 3"],
        "{}",
        o.stdout
    );
}

#[test]
fn exit_is_unknown_command_in_yomi_run() {
    let o = run(GAME, &[], ":exit\n\n1\n");
    assert_eq!(o.code, 0);
    assert!(
        o.stderr.contains(
            "unknown command: :exit (available: :save :load :back :vars :texts :repl :reload :quit)"
        ),
        "{}",
        o.stderr
    );
}

#[test]
fn save_and_others_cannot_be_used_in_repl_only_after_returning() {
    let dir = scratch("save_in_repl");
    let file = dir.join("s.bin");
    let f = file.to_str().unwrap();
    let o = run(
        GAME,
        &["--start", "talk"],
        &format!(":repl\n:save {f}\n:exit\n:save {f}\n\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stderr.contains(":save cannot be used in the REPL"),
        "{}",
        o.stderr
    );
    assert!(file.exists());
    assert_eq!(o.stdout.matches("saved to").count(), 1, "{}", o.stdout);
}

#[test]
fn quit_in_repl_stops_whole_run() {
    let o = run(GAME, &["--start", "talk"], ":repl\n:quit\n\n1\n");
    assert_eq!(o.code, 0);
    assert!(!o.stdout.contains("1: 「買う」"), "{}", o.stdout);
}

#[test]
fn eof_in_repl_ends_only_repl_and_returns_to_stop_point() {
    let o = run(GAME, &["--start", "talk"], ":repl\n");
    assert_eq!(o.code, 0);
    // Bodies proceed at EOF, and the choice chooses number 1 and ends
    assert!(o.stdout.ends_with("(end)\n"), "{}", o.stdout);
}

#[test]
fn script_lines_typed_in_repl_read_input_at_stop_points() {
    let o = run(
        GAME,
        &["--start", "talk"],
        ":repl\n案内人「やあ」\n\nchoice {\n  「a」 => favor = 1\n}\n1\nfavor\n:exit\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout.contains("リオン「やあ」\n1: 「a」\n1\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn auto_allows_repl_at_choices_and_repl_stop_points_read_input() {
    let o = run(
        GAME,
        &["--start", "talk", "--auto"],
        ":repl\n案内人「やあ」\n\nfavor\n:exit\n#nope\n",
    );
    // Bodies proceed without reading input, and at the choice (without IDs) it enters :repl. Bodies inside the REPL read input
    assert!(o.stdout.contains("リオン「やあ」\n3\n"), "{}", o.stdout);
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert!(o.stderr.contains("(--auto)"));
}

#[test]
fn legacy_yomi_run_output_is_unchanged() {
    let o = run(GAME, &["--start", "talk"], "\n1\n");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\n1: 「買う」\n2: 「帰る」\n(end)\n"
    );
    assert_eq!(o.stderr, "");
}
