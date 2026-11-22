//! The registry, and how to run scripts

use super::*;

pub(super) fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(String, i64), ()>("game.give").unwrap();
    r.stop::<(i64,), i64>("game.roll").unwrap();
    r.stop::<(String,), String>("game.ask").unwrap();
    r.stop::<(String,), bool>("game.yes").unwrap();
    r.stop::<(f64,), f64>("game.half").unwrap();
    r.stop::<(), ()>("game.pause").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

pub(super) fn config() -> VmConfig {
    VmConfig::default()
        .max_steps_per_run(Some(100_000))
        .start("main.start")
}

/// The value to return at a stop point (a deterministic value decided by the arguments).
pub(super) fn answer(reg: &Registry, y: &Yield) -> Value {
    if let Yield::Stop { command, args, .. } = y {
        let decl = reg.command(*command);
        if decl.name == "game.roll"
            && let Some(Value::Int(n)) = args.first()
        {
            return Value::Int(n.wrapping_add(1));
        }
        // Stop commands whose return values are received in the script (`let`, assignment)
        if decl.name == "game.ask"
            && let Some(Value::Str(t)) = args.first()
        {
            return Value::Str(format!("答{}", t.chars().count()).into());
        }
        if decl.name == "game.yes"
            && let Some(Value::Str(t)) = args.first()
        {
            return Value::Bool(t.len() % 2 == 0);
        }
        if decl.name == "game.half"
            && let Some(Value::Float(x)) = args.first()
        {
            return Value::Float(x / 2.0);
        }
        // For choices, returns the number decided by the text of the options (the number in the given list)
        if decl.name == "ui.choose"
            && let Some(Value::List(items)) = args.first()
        {
            let sum: usize = items.iter().map(|v| v.to_text().chars().count()).sum();
            return Value::Int((sum % items.len().max(1)) as i64);
        }
    }
    Value::Unit
}

/// The result of one `run` (errors become the code of their kind).
pub(super) type Step = Result<Yield, String>;

pub(super) fn step(vm: &mut Vm<'_>) -> Step {
    vm.run().map_err(|e| e.kind.code().to_string())
}

/// Runs to the end (or to an error) and lists the results of `run`. At most `limit` times.
pub(super) fn trace(vm: &mut Vm<'_>, reg: &Registry, limit: usize) -> Vec<Step> {
    let mut out = Vec::new();
    for _ in 0..limit {
        let s = step(vm);
        let done = match &s {
            Ok(Yield::End) | Err(_) => true,
            Ok(y) => {
                let v = answer(reg, y);
                vm.resume(Input::Return(v)).expect("再開できる");
                false
            }
        };
        out.push(s);
        if done {
            break;
        }
    }
    out
}

pub(super) fn trace_of(prog: &Program, reg: &Registry, limit: usize) -> Vec<Step> {
    match Vm::new(prog, reg, config()) {
        Ok(mut vm) => trace(&mut vm, reg, limit),
        Err(e) => vec![Err(e.kind.code().to_string())],
    }
}
