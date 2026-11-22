//! Showing errors and aborting (section 7.7 of `spec/host/07-repl.md`)

use super::*;

#[test]
fn runtime_errors_print_to_stderr_in_repl_form_and_discard_input_and_continue() {
    // The example of 7.7
    let o = repl(GAME, "let xs = [1, 2]\nxs[5]\nxs\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "xs = [1, 2]\n[1, 2]\n");
    assert_eq!(
        o.stderr,
        "<repl>: error: the list index 5 is out of range (the list has 2 items)\n"
    );
}

#[test]
fn all_changes_of_input_with_runtime_error_are_discarded() {
    let o = repl(
        GAME,
        "let xs = [1]\nif true {\n  favor = 50\n  let y = xs[3]\n}\nfavor\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "xs = [1]\n3\n");
    // For multi-line input, attach the line within the input
    assert_eq!(
        o.stderr,
        "<repl>: error: the list index 3 is out of range (the list has 1 items) (line 3)\n"
    );
}

#[test]
fn runtime_error_inside_called_label_has_label_and_line() {
    let o = repl(GAME, "call boom()\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(
        o.stderr,
        "<repl>: error: the list index 3 is out of range (the list has 1 items) (label repl_game.boom, line 63)\n"
    );
}

#[test]
fn endless_work_stops_at_instruction_limit_and_repl_continues() {
    let o = repl(GAME, "while true {\n  favor += 1\n}\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    // Attach the position where the limit was reached (the line of the loop). The position is decided by the instructions
    // the backend counts
    assert_eq!(
        o.stderr,
        "<repl>: error: the step limit for one run was exceeded (line 1)\n"
    );
}
