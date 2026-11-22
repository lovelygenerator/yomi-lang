//! The answers the exploration (`yomi_core::explore`) tries at the stop commands of the standard packages
//! (`spec/host/08-test.md`, 8.4).

use yomi_core::{IntoValue, Registry, Value, Yield};

use crate::event::Outcome;

/// The longest string tried for `input.text`, even if `max_len` is larger.
const MAX_TEXT: i64 = 100_000;

/// The candidate answers for a stop at a command of the standard packages (`None` for any other command).
///
/// Pass it, with the commands of the host added, as the `answers` of [`yomi_core::explore`]. It does not decide the commands
/// that return `()`; the exploration answers those.
pub fn test_answers(registry: &Registry, stop: &Yield) -> Option<Vec<Value>> {
    test_answers_with_events(registry, stop, &[])
}

/// Like [`test_answers`], with the data to try for the events of `event.wait`.
///
/// `events` are pairs of an event name and data. A name that has data is tried with each of them (in the order given)
/// instead of with empty data; a name without data is tried with empty data. Names that the stop does not wait for are
/// not used.
pub fn test_answers_with_events(
    registry: &Registry,
    stop: &Yield,
    events: &[(String, String)],
) -> Option<Vec<Value>> {
    let Yield::Stop { command, args, .. } = stop else {
        return None;
    };
    let mut out: Vec<Value> = Vec::new();
    match (registry.command(*command).name.as_str(), args.as_slice()) {
        ("input.text", [_, Value::Str(default), Value::Int(max_len)]) => {
            out.push(Value::Str(default.to_string().into()));
            out.push(Value::Str(String::new().into()));
            let n = (*max_len).clamp(0, MAX_TEXT) as usize;
            out.push(Value::Str("a".repeat(n).into()));
        }
        ("input.integer", [_, Value::Int(default), Value::Int(min), Value::Int(max)]) => {
            out.extend([*default, *min, *max].map(Value::Int));
        }
        ("input.confirm", _) => out.extend([Value::Bool(true), Value::Bool(false)]),
        ("event.wait", [Value::List(names), timeout]) => {
            for name in names.iter() {
                let name = name.to_text();
                let data: Vec<&str> = events
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .map(|(_, d)| d.as_str())
                    .collect();
                let data = if data.is_empty() { vec![""] } else { data };
                for d in data {
                    out.push(Outcome::Arrived(name.clone(), d.to_string()).into_value());
                }
            }
            if matches!(timeout, Value::Option(Some(_))) {
                out.push(Outcome::TimedOut.into_value());
            }
        }
        _ => return None,
    }
    let mut unique: Vec<Value> = Vec::new();
    for v in out {
        if !unique.contains(&v) {
            unique.push(v);
        }
    }
    Some(unique)
}
