//! 7.6, 7.8 type_of, variables, defs, reset

use super::*;

#[test]
fn type_of_returns_type_as_written_in_scripts_without_evaluating() {
    let reg = registry();
    let prog = program(&reg);
    let repl = Repl::from_program(&prog, &reg, config()).unwrap();
    assert_eq!(repl.type_of("favor").unwrap(), Ty::Int);
    assert_eq!(repl.type_of("items").unwrap().name(), "list<str>");
    assert_eq!(repl.type_of("mood_of(favor)").unwrap().name(), "main.Mood");
    assert_eq!(repl.type_of("Some(1)").unwrap().name(), "Option<int>");
    // Input that is not an expression, and expressions with diagnostics, are errors
    assert!(repl.type_of("favor += 1").is_err());
    assert!(repl.type_of("let x = 1").is_err());
    assert_eq!(codes(&repl.type_of("gld").unwrap_err()), ["unknown_name"]);
    // Does not evaluate (neither the random generator nor the state advances)
    let mut a = Repl::from_program(&prog, &reg, config()).unwrap();
    let mut b = Repl::from_program(&prog, &reg, config()).unwrap();
    a.type_of("random(1, 1000000)").unwrap();
    a.type_of("counter.get()").unwrap();
    assert_eq!(
        eval(&mut a, "random(1, 1000000)"),
        eval(&mut b, "random(1, 1000000)")
    );
}

#[test]
fn variables_returns_script_variables_stop_point_locals_and_repl_lets() {
    let reg = registry();
    let prog = program(&reg);
    let mut vm = Vm::new(&prog, &reg, config().start("talk")).unwrap();
    vm.run().unwrap();
    let mut repl = Repl::attach(&vm).unwrap();
    exec(&mut repl, "let bonus = 25");
    let vars = repl.variables();
    let find = |name: &str| vars.iter().find(|v| v.name == name).unwrap().clone();
    assert_eq!(find("favor").module.as_deref(), Some("main"));
    assert!(!find("favor").local);
    assert!(find("best").persistent);
    let gold = find("gold");
    assert!(gold.local && gold.module.is_none());
    assert_eq!(gold.value, Value::Int(100));
    let bonus = find("bonus");
    assert_eq!(bonus.module.as_deref(), Some("<repl>"));
    assert!(!bonus.local);
    assert_eq!(bonus.value, Value::Int(25));
    // Script variables first, REPL variables after
    let pos = |n: &str| vars.iter().position(|v| v.name == n).unwrap();
    assert!(pos("favor") < pos("gold") && pos("gold") < pos("bonus"));
    // Only one variable with the same name
    assert_eq!(vars.iter().filter(|v| v.name == "gold").count(), 1);
}

#[test]
fn reset_restores_declarations_and_variables_to_start_state() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let a = 1");
    exec(&mut repl, "fn f() -> int {\n  return 1\n}");
    exec(&mut repl, "favor = 99");
    exec(&mut repl, "@counter.inc(9)");
    repl.reset().unwrap();
    assert_eq!(eval(&mut repl, "favor"), "3");
    assert!(repl.defs().is_empty());
    assert_eq!(eval(&mut repl, "counter.get()"), "0");
    assert_eq!(codes(&submit_err(&mut repl, "a")), ["unknown_name"]);
    // Input that is running is discarded too
    repl.submit("\\ここで止まる").unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Stop { .. }));
    repl.reset().unwrap();
    exec(&mut repl, "let a = 2");
}

#[test]
fn expressions_and_declarations_work_in_empty_environment_without_script() {
    let reg = registry();
    let mut repl = Repl::empty(&reg, config()).unwrap();
    assert_eq!(eval(&mut repl, "1 + 2"), "3");
    exec(&mut repl, "let x = 10");
    exec(&mut repl, "fn f(n: int) -> int {\n  return n * 2\n}");
    assert_eq!(eval(&mut repl, "f(x)"), "20");
}
