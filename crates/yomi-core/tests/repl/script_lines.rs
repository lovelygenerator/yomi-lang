//! 7.4 Script lines and the flow of stops

use super::*;

#[test]
fn plain_line_is_stop_point_and_resume_ends_input() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    repl.submit("信頼度は${favor}").unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { command, args, id } => {
            assert_eq!(reg.command(command).name, "ui.say");
            assert_eq!(text_of(&args), "信頼度は3");
            assert_eq!(id, None);
        }
        other => panic!("{other:?}"),
    }
    // Calling run while stopped is an error (the input remains)
    assert_eq!(repl.run().unwrap_err().kind, VmErrorKind::AlreadyStopped);
    // submit cannot be called while stopped
    assert!(repl.submit("favor").is_err());
    repl.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(
        repl.run().unwrap(),
        ReplYield::Done {
            value: None,
            ended: false,
            ..
        }
    ));
    // Calling run after the end is an error
    assert_eq!(repl.run().unwrap_err().kind, VmErrorKind::Ended);
    assert_eq!(
        repl.resume(Input::Return(Value::Unit)).unwrap_err().kind,
        VmErrorKind::NotStopped
    );
    assert_eq!(eval(&mut repl, "favor"), "3");
}

#[test]
fn choice_is_stop_point_and_runs_chosen_arm() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    repl.submit("choice {\n  「買う」 => favor += 1\n  「帰る」 => favor -= 1\n}")
        .unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { command, args, .. } => {
            assert_eq!(reg.command(command).name, "ui.choose");
            let Value::List(opts) = &args[0] else {
                panic!("{args:?}")
            };
            assert_eq!(opts.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    // An out-of-range number is misuse (it stays stopped)
    assert!(matches!(
        repl.resume(Input::Return(Value::Int(5))).unwrap_err().kind,
        VmErrorKind::ChoiceOutOfRange { .. }
    ));
    repl.resume(Input::Return(Value::Int(0))).unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Done { .. }));
    assert_eq!(eval(&mut repl, "favor"), "4");
}

#[test]
fn stop_commands_with_return_values_can_be_received_by_let() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    repl.submit("let n = @game.roll(6)").unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { command, args, .. } => {
            assert_eq!(reg.command(command).name, "game.roll");
            assert_eq!(args, [Value::Int(6)]);
        }
        other => panic!("{other:?}"),
    }
    // A value of the wrong type is misuse
    assert!(matches!(
        repl.resume(Input::Return(Value::Str("x".into())))
            .unwrap_err()
            .kind,
        VmErrorKind::BadInput { .. }
    ));
    repl.resume(Input::Return(Value::Int(4))).unwrap();
    match repl.run().unwrap() {
        ReplYield::Done { declared, .. } => {
            assert!(
                matches!(&declared[0], yomi_core::Declared::Let { name, value: Value::Int(4), .. } if name == "n")
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(eval(&mut repl, "n + 1"), "5");
}

#[test]
fn call_calls_script_label_whose_stop_points_have_ids() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    repl.submit("call greet(7)").unwrap();
    match repl.run().unwrap() {
        ReplYield::Stop { args, id, .. } => {
            assert_eq!(text_of(&args), "「やあ7」");
            assert_eq!(id.as_deref(), Some("g1"));
        }
        other => panic!("{other:?}"),
    }
    repl.resume(Input::Return(Value::Unit)).unwrap();
    // On reaching the end of the label, it returns past the `call` (the end of that input)
    assert!(matches!(
        repl.run().unwrap(),
        ReplYield::Done { ended: false, .. }
    ));
    // A label with a return value can be received with `let`
    exec(&mut repl, "let s = call add(2, 3)");
    assert_eq!(eval(&mut repl, "s"), "5");
}

#[test]
fn end_inside_called_label_ends_with_ended_and_repl_continues() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "let before = 1");
    repl.submit("call bye()").unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Stop { .. }));
    repl.resume(Input::Return(Value::Unit)).unwrap();
    assert!(matches!(
        repl.run().unwrap(),
        ReplYield::Done {
            ended: true,
            value: None,
            ..
        }
    ));
    assert_eq!(eval(&mut repl, "before"), "1");
}

#[test]
fn call_changes_script_variables_visible_in_repl() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    // `shop` is a label without parameters. It can be `call`ed (choose, and change `favor`)
    repl.submit("call shop()").unwrap();
    assert!(matches!(repl.run().unwrap(), ReplYield::Stop { .. }));
    repl.resume(Input::Return(Value::Int(0))).unwrap();
    assert!(matches!(
        repl.run().unwrap(),
        ReplYield::Done { ended: false, .. }
    ));
    assert_eq!(eval(&mut repl, "favor"), "4");
}

#[test]
fn effect_and_query_commands_change_package_state_in_copy() {
    let reg = registry();
    let prog = program(&reg);
    let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
    exec(&mut repl, "@counter.inc(5)");
    assert_eq!(eval(&mut repl, "counter.get()"), "5");
}
