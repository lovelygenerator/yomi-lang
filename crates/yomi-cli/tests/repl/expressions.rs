//! Expressions, statements, declarations, multiple lines (7.2, 7.5)

use super::*;

#[test]
fn expression_values_print_to_stdout_in_display_form() {
    let o = repl(
        GAME,
        "favor\nitems\nmood_of(favor)\nPrize { name: \"お面\", hp: 3 }\n1 + 2\n\"a\\\"b\"\nSome(3)\nmap { \"a\": 1 }\n2.0\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "3\n[\"お面\"]\nMood.Normal\nPrize { name: \"お面\", hp: 3 }\n3\n\"a\\\"b\"\nSome(3)\nmap { \"a\": 1 }\n2.0\n"
    );
}

#[test]
fn let_prints_name_and_value_and_can_be_used_later() {
    let o = repl(GAME, "let bonus = 25\nbonus + favor\nbonus = 1\nbonus\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "bonus = 25\n28\n1\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn multiline_declarations_and_blocks_with_empty_lines_can_be_pasted_as_is() {
    // The example of 7.5.2
    let o = repl(
        GAME,
        "fn bonus(n: int) -> int {\n  return n * 2\n}\nbonus(favor)\nif favor >= 3 {\n  favor = 0\n\n  favor += 1\n}\nfavor\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "6\n1\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn cancel_discards_partial_input_without_running() {
    // The last example of 7.5.2
    let o = repl(
        GAME,
        "if favor >= 3 {\n  favor = 99\n:cancel\nfavor\n:cancel\nfavor\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n3\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn lines_starting_with_colon_mid_input_are_part_of_input() {
    // Inside a multi-line quote line, lines starting with `:` (other than `:cancel`) are part of the body
    let o = repl(GAME, "案内人「一行目\n:vars\n三行目」\n\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "リオン「一行目:vars三行目」\n");
}

#[test]
fn empty_and_comment_lines_are_ignored_and_print_nothing() {
    let o = repl(GAME, "\n   \n// コメント\nfavor\n\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn diagnostics_print_to_stderr_with_repl_position_and_discard_input_and_continue() {
    let o = repl(GAME, "let gold = 100\ngld\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "gold = 100\n3\n");
    assert_eq!(
        o.stderr,
        "<repl>:1:1: error: unknown name 'gld' (did you mean: gold?)\n"
    );
}

#[test]
fn invalid_input_is_reported_and_discarded_and_next_line_starts_new_input() {
    // A line with only `}` has no matching opening (invalid)
    let o = repl(GAME, "}\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert!(o.stderr.starts_with("<repl>:1:1: error: "), "{}", o.stderr);
    assert_eq!(o.stderr.lines().count(), 1);
}

#[test]
fn things_that_cannot_be_written_report_repl_not_allowed() {
    let o = repl(GAME, "save var x: int = 1\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert!(
        o.stderr.contains("cannot be used in a REPL input"),
        "{}",
        o.stderr
    );
}

#[test]
fn incomplete_input_at_eof_is_reported_and_discarded_with_exit_0() {
    let o = repl(GAME, "favor\nfn f() {\n  let a = 1\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(o.stderr, "<repl>:1:4: error: a block is not closed\n");
}

#[test]
fn expressions_and_declarations_work_in_empty_environment_without_script() {
    let o = repl_empty("1 + 2\nlet x = 10\nx * 2\nimport std::math\nmath.abs(-3)\n:vars\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\nx = 10\n20\n3\n-- REPL variables --\nx = 10\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn script_names_are_undeclared_in_repl_without_script() {
    let o = repl_empty("favor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "");
    assert!(
        o.stderr
            .starts_with("<repl>:1:1: error: unknown name 'favor'")
    );
}
