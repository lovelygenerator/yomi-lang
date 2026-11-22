//! Interactive commands (section 7.6 of `spec/host/07-repl.md`)

use super::*;

#[test]
fn vars_prints_script_variables_separator_and_repl_variables_in_order() {
    let o = repl(GAME, "let gold = 100\nfavor = 5\n:vars\n");
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "gold = 100\nfavor = 5\nitems = [\"お面\"]\nbest = 0\n-- REPL variables --\ngold = 100\n"
    );
}

#[test]
fn defs_lists_repl_declarations_in_order() {
    let o = repl(
        GAME,
        "import std::math\nstruct P {\n  a: int\n}\nenum E { A, B }\nfn f(n: int) -> int {\n  return n\n}\nidentity 村長 = \"たろう\"\n:defs\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "import std::math\nstruct P\nenum E\nfn f(n: int) -> int\nidentity 村長\n"
    );
    assert_eq!(o.stderr, "");
}

#[test]
fn type_command_prints_type_as_written_in_scripts_without_evaluating() {
    let o = repl(
        GAME,
        "let n = 0\n:type items\n:type mood_of(favor)\n:type [1, 2]\n:type favor = 1\n:type\n:type n = 5\nn\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "n = 0\nlist<str>\nMood\nlist<int>\n0\n");
    // Input that is not an expression, and commands without arguments, go to standard error
    assert_eq!(o.stderr.lines().count(), 3, "{}", o.stderr);
    assert!(o.stderr.contains(":type needs an argument"));
}

#[test]
fn reset_discards_declarations_and_variable_changes() {
    let o = repl(
        GAME,
        "let a = 1\nfavor = 99\nfn f() -> int {\n  return 1\n}\n:reset\nfavor\n:defs\na\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "a = 1\n3\n");
    assert_eq!(o.stderr.lines().count(), 1);
    assert!(o.stderr.contains("unknown name 'a'"));
}

#[test]
fn help_lists_one_command_per_line() {
    let o = repl(GAME, ":help\n");
    assert_eq!(o.code, 0);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 10);
    for name in [
        ":help", ":vars", ":defs", ":type", ":apply", ":reset", ":exit", ":quit", ":cancel",
        ":abort",
    ] {
        assert!(lines.iter().any(|l| l.starts_with(name)), "{name}");
    }
}

#[test]
fn unknown_commands_report_with_list_and_wait_for_next_input() {
    let o = repl(GAME, ":foo\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(
        o.stderr,
        "unknown command: :foo (available: :help :vars :defs :type :apply :reset :exit :quit :cancel :abort)\n"
    );
}

#[test]
fn save_load_back_texts_unavailable_in_repl() {
    let o = repl(GAME, ":save x\n:load x\n:back\n:texts off\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(o.stderr.lines().count(), 4);
    for (line, name) in o.stderr.lines().zip([":save", ":load", ":back", ":texts"]) {
        assert!(
            line.starts_with(&format!("{name} cannot be used in the REPL")),
            "{line}"
        );
    }
}

#[test]
fn apply_is_unavailable_in_yomi_repl_and_abort_outside_stop_points() {
    let o = repl(GAME, ":apply\n:abort\n:repl\nfavor\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "3\n");
    assert_eq!(o.stderr.lines().count(), 3, "{}", o.stderr);
    assert!(o.stderr.contains(":apply can only be used"));
    assert!(o.stderr.contains(":abort can only be used"));
    assert!(o.stderr.contains("already in the REPL"));
}

#[test]
fn exit_quit_and_eof_end_with_exit_code_0() {
    for stdin in [":exit\nfavor\n", ":quit\nfavor\n", ""] {
        let o = repl(GAME, stdin);
        assert_eq!(o.code, 0, "{stdin:?}");
        assert_eq!(o.stdout, "", "{stdin:?}");
    }
}
