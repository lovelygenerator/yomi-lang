//! 7.7 Aborting and limits

use super::*;

#[test]
fn input_with_runtime_error_is_discarded_restoring_variables_and_declarations() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let xs = [1, 2]");
    repl.submit("xs[5]").unwrap();
    let e = repl.run().unwrap_err();
    assert!(matches!(
        e.kind,
        VmErrorKind::IndexOutOfRange { index: 5, len: 2 }
    ));
    assert_eq!(eval(&mut repl, "xs"), "[1, 2]");
    // Changes made partway through execution are also undone
    repl.submit("favor = 99\nlet ys = 5\n@counter.inc(3)\nxs[7] = 1")
        .unwrap();
    assert!(repl.run().is_err());
    assert_eq!(eval(&mut repl, "favor"), "3");
    assert_eq!(eval(&mut repl, "counter.get()"), "0");
    assert_eq!(codes(&submit_err(&mut repl, "ys")), ["unknown_name"]);
    // It can still be used after aborting (it does not become `poisoned`)
    exec(&mut repl, "favor += 1");
    assert_eq!(eval(&mut repl, "favor"), "4");
}

#[test]
fn after_runtime_error_declared_fn_and_let_type_changes_are_reverted() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let v = 1");
    // An error after replacing `let v` with a string → `v` stays an int
    repl.submit("let v = \"あ\"\nlet e = [1][3]").unwrap();
    assert!(repl.run().is_err());
    assert_eq!(repl.type_of("v").unwrap(), Ty::Int);
    assert_eq!(eval(&mut repl, "v"), "1");
}

#[test]
fn abort_discards_running_input_and_restores_state_except_shown_stop_points() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let a = 1");
    repl.submit("favor = 50\nlet b = 2\n\\ここで止まる\nfavor = 60")
        .unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Stop { .. }));
    repl.abort();
    assert_eq!(eval(&mut repl, "favor"), "3");
    assert_eq!(codes(&submit_err(&mut repl, "b")), ["unknown_name"]);
    assert_eq!(eval(&mut repl, "a"), "1");
    // Aborting when nothing is running does nothing
    repl.abort();
    assert_eq!(eval(&mut repl, "a"), "1");
    // Input that has not been run (before waiting for resume at a stop point) can also be aborted
    repl.submit("favor = 70").unwrap();
    repl.abort();
    assert_eq!(eval(&mut repl, "favor"), "3");
}

#[test]
fn instruction_limit_is_recounted_per_input_and_endless_loops_are_aborted() {
    let reg = registry();
    let prog = program(&reg);
    let cfg = VmConfig::default().max_steps_per_run(Some(2_000));
    let mut repl = Repl::from_program(&prog, &reg, cfg).unwrap();
    for _ in 0..5 {
        // Each input is within the limit (the total exceeds the limit)
        exec(&mut repl, "let i = 0\nwhile i < 150 {\n  i += 1\n}");
    }
    exec(&mut repl, "favor = 5");
    repl.submit("while true {\n  favor += 1\n}").unwrap();
    assert_eq!(repl.run().unwrap_err().kind, VmErrorKind::StepLimitExceeded);
    assert_eq!(eval(&mut repl, "favor"), "5");
}

#[test]
fn value_size_limit_applies_as_in_scripts() {
    let reg = registry();
    let prog = program(&reg);
    let cfg = config().max_value_bytes(1000);
    let mut repl = Repl::from_program(&prog, &reg, cfg).unwrap();
    exec(&mut repl, "let s = \"あああああああ\"");
    exec(&mut repl, "let xs: list<str> = []");
    repl.submit("while true {\n  xs.push(s)\n}").unwrap();
    assert_eq!(repl.run().unwrap_err().kind, VmErrorKind::ValueTooLarge);
    assert_eq!(eval(&mut repl, "s"), "\"あああああああ\"");
}

#[test]
fn repl_variables_count_toward_total_variable_size() {
    let reg = registry();
    let prog = program(&reg);
    let cfg = config().max_total_bytes(Some(4000));
    let mut repl = Repl::from_program(&prog, &reg, cfg).unwrap();
    exec(
        &mut repl,
        "let a = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
    );
    repl.submit("let xs: list<str> = []\nlet i = 0\nwhile i < 100 {\n  xs.push(a)\n  i += 1\n}")
        .unwrap();
    assert_eq!(
        repl.run().unwrap_err().kind,
        VmErrorKind::MemoryLimitExceeded
    );
}
