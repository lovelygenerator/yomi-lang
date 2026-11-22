//! The host of the approval flow (`examples/approval/approval.yomi`): runs the script, saves it while it waits for an
//! event, and resumes it from the save when the event arrives.
//!
//! `cargo run -p yomi-std --example approval`

use yomi_core::{
    Input, IntoValue, Registry, Sugar, Value, Vm, VmConfig, Yield, compile, pending_stop,
};
use yomi_std::event::Outcome;

/// The registry: the standard packages, and one command that shows a line of text.
fn registry() -> Result<Registry, Box<dyn std::error::Error>> {
    let mut reg = Registry::new();
    reg.stop::<(String,), ()>("ui.say")?;
    yomi_std::register_all(&mut reg)?;
    reg.bind(Sugar::Plain, "ui.say")?;
    Ok(reg)
}

/// Runs the script. `events` are the events that arrive, in order, as (name, data). Returns the lines the script showed
/// and the wait conditions the host found in the saves.
pub fn run(
    source: &str,
    events: &[(&str, &str)],
) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let reg = registry()?;
    let program = compile(source, "approval", &reg).map_err(|d| format!("{d:?}"))?;
    let config = VmConfig::default().max_steps_per_run(Some(100_000));
    let mut vm = Vm::new(&program, &reg, config.clone())?;
    let mut events = events.iter();
    let (mut shown, mut waits) = (Vec::new(), Vec::new());
    loop {
        match vm.run()? {
            Yield::End => break,
            Yield::Stop { command, args, .. } => match reg.command(command).name.as_str() {
                "ui.say" => {
                    if let Some(Value::Str(text)) = args.first() {
                        shown.push(text.to_string());
                    }
                    vm.resume(Input::Return(Value::Unit))?;
                }
                "event.wait" => {
                    // The script is waiting. Save it and read the wait condition from the save, as a host does
                    // for a script that sleeps for a long time.
                    let bytes = vm.save()?;
                    let wait = pending_stop(&bytes)?.expect("the save is at a stop point");
                    waits.push(format!("{} {:?}", wait.command, wait.args));
                    // ... time passes, the process may exit. Later, the event arrives and the host resumes from the save.
                    let Some((name, data)) = events.next() else {
                        break;
                    };
                    let (loaded, _report) = Vm::load(&program, &reg, config.clone(), &bytes)?;
                    vm = loaded;
                    let outcome = Outcome::Arrived(name.to_string(), data.to_string());
                    vm.resume(Input::Return(outcome.into_value()))?;
                }
                other => return Err(format!("unknown command {other}").into()),
            },
            other => return Err(format!("unexpected {other:?}").into()),
        }
    }
    Ok((shown, waits))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/approval/approval.yomi");
    let source = std::fs::read_to_string(path)?;
    let events = [
        ("submitted", r#"{"title":"モニター","amount":120000}"#),
        ("approved", "{}"),
    ];
    let (shown, waits) = run(&source, &events)?;
    for line in &shown {
        println!("{line}");
    }
    for wait in &waits {
        println!("[wait] {wait}");
    }
    Ok(())
}
