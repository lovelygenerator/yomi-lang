//! The `event` package: waiting for events from outside (`spec/packages/event.md`).
//!
//! `event.wait` is a stop command. The host waits for an event to arrive and passes the result with `Input::Return`. It has
//! neither state nor a persistent slice. The time limit is measured by the host. Arguments that break the constraints are
//! runtime errors before stopping, and a value returned by the host that does not fit the arguments is a runtime error from
//! `resume` (`PackageReg::stop_validated`).

use yomi_core::{CommandError, Package, PackageReg, Value, YomiType};

/// The result of `event.wait` (the type `event.Outcome` in scripts).
#[derive(Debug, Clone, PartialEq, YomiType)]
#[yomi(namespace = "event")]
pub enum Outcome {
    /// An event arrived: its name (one of the names waited for) and its data (the form is up to the host).
    Arrived(String, String),
    /// The time limit passed.
    TimedOut,
}

/// The `event` package. It has no state.
///
/// - `event.wait(names, timeout = None) -> event.Outcome`: waits until one of the events named in `names` arrives. If
///   `timeout` is `Some(seconds)`, the wait ends with `TimedOut` after that many seconds (measured by the host).
pub struct Event;

impl Package for Event {
    const NAME: &'static str = "event";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.ty::<Outcome>()
            .doc("event.wait の結果（届いたイベントの名前とデータ、または期限切れ）");
        reg.stop_validated::<(Vec<String>, Option<f64>), Outcome, _, _>(
            "event.wait",
            |(names, timeout)| check_args(names, *timeout),
            |(names, timeout), outcome| check_outcome(names, *timeout, outcome),
        )
        .doc("名前のどれかのイベントが届くまで待つ（timeout 秒で期限切れ）")
        .param_names(&["names", "timeout"])
        .defaults(&[("timeout", Value::Option(None))]);
    }
}

fn check_args(names: &[String], timeout: Option<f64>) -> Result<(), CommandError> {
    if names.is_empty() {
        return Err(CommandError::with_code("event.no_names", []));
    }
    for (i, name) in names.iter().enumerate() {
        if name.is_empty() {
            return Err(CommandError::with_code(
                "event.empty_name",
                [("index", i.into())],
            ));
        }
        if names[..i].contains(name) {
            return Err(CommandError::with_code(
                "event.duplicate_name",
                [("name", name.as_str().into())],
            ));
        }
    }
    if let Some(t) = timeout
        && t < 0.0
    {
        return Err(CommandError::with_code(
            "event.negative_timeout",
            [("timeout", t.to_string().into())],
        ));
    }
    Ok(())
}

fn check_outcome(
    names: &[String],
    timeout: Option<f64>,
    outcome: &Outcome,
) -> Result<(), CommandError> {
    match outcome {
        Outcome::Arrived(name, _) if !names.contains(name) => Err(CommandError::with_code(
            "event.unknown_event",
            [("name", name.as_str().into())],
        )),
        Outcome::TimedOut if timeout.is_none() => {
            Err(CommandError::with_code("event.unexpected_timeout", []))
        }
        _ => Ok(()),
    }
}
