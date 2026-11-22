//! Script lines and the flow of stops (section 7.4 of `spec/host/07-repl.md`)

use super::*;

#[test]
fn script_lines_and_choice_are_stop_points_shown_like_yomi_run() {
    // The example of 7.4
    let o = repl(
        GAME,
        "案内人「信頼度は${favor}」\n\nchoice {\n  「買う」 => favor += 1\n  「帰る」 => favor -= 1\n}\n1\nfavor\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\n1: 「買う」\n2: 「帰る」\n4\n"
    );
    assert_eq!(o.stderr, "");
}

#[test]
fn choice_errors_reprompt_at_same_stop_point() {
    let o = repl(
        GAME,
        "choice {\n  「買う」 => favor += 1\n  「帰る」 => favor -= 1\n}\n9\nx\n#no\n\n2\nfavor\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "1: 「買う」\n2: 「帰る」\n2\n");
    assert_eq!(o.stderr.lines().count(), 4, "{}", o.stderr);
    assert!(o.stderr.contains("enter a number from 1 to 2"));
    assert!(o.stderr.contains("no option with the ID #no"));
}

#[test]
fn stop_point_answer_at_eof_has_same_default_as_yomi_run() {
    // The choice is number 1, bodies proceed
    let o = repl(
        GAME,
        "choice {\n  「買う」 => favor += 1\n  「帰る」 => favor -= 1\n}\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "1: 「買う」\n2: 「帰る」\n");
}

#[test]
fn choice_inside_called_label_accepts_number_or_id() {
    let o = repl(GAME, "call pick()\n#no\nfavor\ncall pick()\n1\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "1: 「はい」\n2: 「いいえ」\n23\n1: 「はい」\n2: 「いいえ」\n33\n"
    );
}

#[test]
fn ids_shown_only_in_repl_inside_yomi_run_with_show_ids() {
    let path = fixture(GAME);
    let p = path.to_str().unwrap();
    let o = yomi(
        &["--lang", "en", "run", p, "--start", "pick", "--show-ids"],
        ":repl\ncall pick()\n1\n:exit\n1\n",
    );
    assert_eq!(o.code, 0);
    // A stop point (with an ID), and a stop point inside the REPL (with an ID)
    assert_eq!(o.stdout.matches("#yes").count(), 2, "{}", o.stdout);
}

#[test]
fn call_reaching_end_shows_end_and_continues() {
    let o = repl(GAME, "call bye()\n\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "さよなら\n(end)\n3\n");
}

#[test]
fn call_with_jump_to_another_label_works_as_scripted() {
    let o = repl(GAME, "call talk()\n\n1\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\n1: 「買う」\n2: 「帰る」\n4\n"
    );
}

#[test]
fn vars_help_abort_and_quit_available_at_stop_points() {
    let o = repl(GAME, "call talk()\n:vars\n:abort\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "リオン「信頼度は3」\nfavor = 3\nitems = [\"お面\"]\nbest = 0\n3\n"
    );
    assert_eq!(o.stderr, "");
    // :abort goes back to the state before the input (output already shown cannot be undone)
    let o = repl(GAME, "favor = 7\ncall pick()\n:abort\nfavor\n:vars\n");
    assert_eq!(o.stdout.lines().filter(|l| *l == "7").count(), 1);
    // At a stop point, :apply, :exit and the like come after finishing the input
    let o = repl(GAME, "call bye()\n:exit\n\nfavor\n");
    assert!(
        o.stderr
            .contains(":exit cannot be used while an input is stopped")
    );
    assert_eq!(o.stdout, "さよなら\n(end)\n3\n");
    let o = repl(GAME, "call bye()\n:quit\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "さよなら\n");
}

#[test]
fn stop_command_input_errors_report_and_discard_input_without_exiting() {
    let o = repl(
        GAME,
        "call ask()\nx\nfavor\ncall ask()\n99\nfavor\ncall ask()\n7\n\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "[input] 数 (integer from 0 to 10; Enter for the default: 5)\n3\n[input] 数 (integer from 0 to 10; Enter for the default: 5)\n3\n[input] 数 (integer from 0 to 10; Enter for the default: 5)\nリオン「7」\n"
    );
    assert_eq!(o.stderr.lines().count(), 2, "{}", o.stderr);
    let mut lines = o.stderr.lines();
    assert_eq!(lines.next().unwrap(), "<repl>: error: not an integer: x");
    assert!(lines.next().unwrap().starts_with("<repl>: error: "));
}
