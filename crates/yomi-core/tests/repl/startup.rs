//! 7.1 The startup environment

use super::*;

#[test]
fn from_program_starts_at_start_label_with_no_locals() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    assert!(repl.variables().iter().all(|v| !v.local));
    assert_eq!(codes(&submit_err(&mut repl, "gold")), ["unknown_name"]);
    // The start label may be missing
    let cfg = config().start("nothing");
    let mut repl = Repl::from_program(&prog, &reg, cfg).unwrap();
    assert_eq!(eval(&mut repl, "favor"), "3");
}

#[test]
fn from_program_reads_initial_values_of_declared_persistent_vars() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = Vm::new(&prog, &reg, config()).unwrap();
    vm.run().unwrap();
    // Pass persistent data (best stays 0; it can be read)
    let bytes = vm.save_persistent();
    let mut repl = Repl::from_program(&prog, &reg, config().persistent_data(bytes)).unwrap();
    assert_eq!(eval(&mut repl, "best"), "0");
}
