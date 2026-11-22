//! Debugging by source line (`spec/host/09-debug.md`).

use std::sync::{Arc, Mutex};

use yomi_core::{
    DebugYield, Debugger, Input, Program, Registry, StepMode, Sugar, UnitKind, Value, Vm, VmConfig,
    Yield, compile,
};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

const SCRIPT: &str = "fn double(x: int) -> int {
    let y = x * 2
    return y
}
label start {
    let a = double(3)
    let b = a + 1
    一行目 #one
    二行目 #two
}
";

fn program(reg: &Registry) -> Program {
    compile(SCRIPT, "main", reg).unwrap()
}

fn say(vm: &mut Vm, dbg: &mut Debugger) -> DebugYield {
    vm.run_debug(dbg).unwrap()
}

fn resume(vm: &mut Vm) {
    vm.resume(Input::Return(Value::Unit)).unwrap();
}

#[test]
fn trace_reports_each_line_once_including_the_return_into_the_caller() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let mut dbg = Debugger::new();
    dbg.set_trace(Some(Box::new(move |e| {
        sink.lock()
            .unwrap()
            .push((e.unit.to_string(), e.line, e.depth));
    })));
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    resume(&mut vm);
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    resume(&mut vm);
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::End)
    ));
    let lines: Vec<(String, u32, usize)> = seen.lock().unwrap().clone();
    let expected = [
        ("start", 6, 0),
        ("double", 2, 1),
        ("double", 3, 1),
        ("start", 6, 0),
        ("start", 7, 0),
        ("start", 8, 0),
        ("start", 9, 0),
        // The implicit end of the label has the line of its header
        ("start", 5, 0),
    ];
    let expected: Vec<(String, u32, usize)> = expected
        .iter()
        .map(|(n, l, d)| (n.to_string(), *l, *d))
        .collect();
    assert_eq!(lines, expected);
}

#[test]
fn a_breakpoint_pauses_before_the_line_and_going_on_does_not_repeat_it() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    dbg.set_breakpoints("main", &[7]);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    assert!(vm.is_paused());
    let stack = vm.call_stack().unwrap();
    assert_eq!(stack.len(), 1);
    assert_eq!((stack[0].name.as_str(), stack[0].line), ("start", 7));
    let vars = vm.frame_variables(0).unwrap();
    let a = vars.iter().find(|v| v.name == "a").unwrap();
    assert_eq!(a.value, Value::Int(6));
    // `save`, `snapshot` and `resume` are for stop points only
    assert!(vm.save().is_err());
    assert!(vm.snapshot().is_err());
    assert!(vm.resume(Input::Return(Value::Unit)).is_err());
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
}

#[test]
fn step_in_enters_the_fn_and_step_out_returns_to_the_caller() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    dbg.set_breakpoints("main", &[6]);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    dbg.set_mode(StepMode::In);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    let stack = vm.call_stack().unwrap();
    assert_eq!(stack.len(), 2);
    assert_eq!(
        (stack[0].name.as_str(), stack[0].kind, stack[0].line),
        ("double", UnitKind::Fn, 2)
    );
    // The outer frame shows the line of the call
    assert_eq!((stack[1].name.as_str(), stack[1].line), ("start", 6));
    let x = vm.frame_variables(0).unwrap();
    assert_eq!(x[0].name, "x");
    assert_eq!(x[0].value, Value::Int(3));
    assert!(vm.frame_variables(9).unwrap().is_empty());
    dbg.set_mode(StepMode::Out);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    let stack = vm.call_stack().unwrap();
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].name, "start");
}

#[test]
fn step_over_does_not_enter_the_fn() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    dbg.set_breakpoints("main", &[6]);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    dbg.set_breakpoints("main", &[]);
    dbg.set_mode(StepMode::Over);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    let stack = vm.call_stack().unwrap();
    assert_eq!(stack.len(), 1);
    // Back in the caller, still on the line of the call (the assignment of the result)
    assert_eq!(stack[0].line, 6);
    dbg.set_mode(StepMode::Over);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    assert_eq!(vm.call_stack().unwrap()[0].line, 7);
}

#[test]
fn call_stack_works_at_a_stop_point_and_the_run_goes_on_afterwards() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    let stack = vm.call_stack().unwrap();
    assert_eq!((stack[0].name.as_str(), stack[0].line), ("start", 8));
    resume(&mut vm);
    // A plain `run` goes on too
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
}

#[test]
fn pausing_in_the_middle_and_going_on_with_run_finishes_the_script() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    dbg.set_breakpoints("main", &[2]);
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    assert!(!vm.is_paused());
}

#[test]
fn a_pause_request_pauses_at_the_next_line() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    dbg.pause_handle().request();
    assert_eq!(say(&mut vm, &mut dbg), DebugYield::Paused);
    assert_eq!(vm.call_stack().unwrap()[0].line, 6);
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
}

#[test]
fn breakable_lines_are_the_lines_with_instructions() {
    let reg = registry();
    let p = program(&reg);
    let lines = Debugger::breakable_lines(&p, "main");
    for l in [2, 6, 7, 8, 9] {
        assert!(lines.contains(&l), "{lines:?}");
    }
    assert!(!lines.contains(&4) && !lines.contains(&10), "{lines:?}");
    assert!(lines.windows(2).all(|w| w[0] < w[1]));
    assert!(Debugger::breakable_lines(&p, "nope").is_empty());
}

#[test]
fn the_step_limit_still_applies_in_a_debug_run() {
    let reg = registry();
    let p = compile(
        "label start {\n    while true {\n        let x = 1\n    }\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&p, &reg, VmConfig::default().max_steps_per_run(Some(1000))).unwrap();
    let mut dbg = Debugger::new();
    let err = vm.run_debug(&mut dbg).unwrap_err();
    assert_eq!(err.kind.code(), "step_limit_exceeded");
}

#[test]
fn restore_forgets_the_last_line() {
    let reg = registry();
    let p = program(&reg);
    let mut vm = Vm::new(&p, &reg, VmConfig::default()).unwrap();
    let mut dbg = Debugger::new();
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    vm.restore(&snap).unwrap();
    // After the restore, the line of the stop point is reported again
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    dbg.set_trace(Some(Box::new(move |e| sink.lock().unwrap().push(e.line))));
    resume(&mut vm);
    assert!(matches!(
        say(&mut vm, &mut dbg),
        DebugYield::Stop(Yield::Stop { .. })
    ));
    assert_eq!(seen.lock().unwrap().first(), Some(&9));
}
