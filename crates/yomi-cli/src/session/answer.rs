//! The kinds of answers awaited at stop points (`Expect`), and interpreting one line of input.

use super::*;
use yomi_core::IntoValue;
use yomi_std::event::Outcome;

/// The name of the stop command that waits for a number of seconds (`flow` of `yomi-std`).
pub(super) const WAIT: &str = "flow.wait";

/// The name of the stop command that waits for input (`flow` of `yomi-std`).
pub(super) const WAIT_INPUT: &str = "flow.wait_input";

/// The name of the stop command that asks for string input (`input` of `yomi-std`).
pub(super) const INPUT_TEXT: &str = "input.text";

/// The name of the stop command that asks for integer input (`input` of `yomi-std`).
pub(super) const INPUT_INT: &str = "input.integer";

/// The name of the stop command that asks yes or no (`input` of `yomi-std`).
pub(super) const INPUT_CONFIRM: &str = "input.confirm";

/// The name of the stop command that waits for an event (`event` of `yomi-std`).
pub(super) const EVENT_WAIT: &str = "event.wait";

/// What kind of input a stop point waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expect {
    /// Bodies and the like. Enter goes to the next.
    Next,
    /// A choice. The stable IDs of the options (in list order; `None` for options without one; the length is the number of
    /// options).
    Choice(Vec<Option<String>>),
    /// A string. An empty line (and EOF) gives the default.
    Text(String),
    /// An integer. An empty line (and EOF) gives the default.
    Int(i64),
    /// Yes or no. An empty line (and EOF) means "yes".
    Confirm,
    /// An event. `names` are the events waited for; with a time limit (`timed`), an empty line (and EOF) means that it ran out.
    Event { names: Vec<String>, timed: bool },
}

/// Takes the stable ID of the `i`-th option from the arguments of a choice stop point (the list of option IDs).
pub(super) fn choice_id(ids: &Value, i: usize) -> Option<&str> {
    match ids {
        Value::List(l) => match l.get(i) {
            Some(Value::Option(Some(v))) => match &**v {
                Value::Str(s) => Some(&**s),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// The interpreted result of one line of answer at a stop point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Answer {
    /// Go on (an empty line at a body or wait).
    Advance,
    /// An option number (0-based) was chosen.
    Pick(usize),
    /// The value of `input.*`.
    Return(Value),
    /// The input ended where there is no answer to fall back on (an event wait without a time limit).
    EndOfInput,
    /// A line starting with `:` (an interactive command).
    Command,
    /// Input that is not an answer (at a body or wait, neither an empty line nor a line starting with `:`).
    Other,
}

/// Input that was not an answer (holds the input string).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AnswerError {
    /// Not an integer.
    BadInt(String),
    /// Not yes or no.
    BadConfirm(String),
    /// The answer to a choice is out of range, not a number, or an empty line.
    Invalid(String),
    /// An ID not in the list of options (still with `#`).
    UnknownId(String),
    /// The first word is not one of the events waited for.
    UnknownEvent(String),
    /// An empty line at an event wait without a time limit.
    EmptyEvent,
}

/// Interprets one line at a stop point (without the newline) as an answer (section 6.3.2 of `spec/host/06-cli.md`).
pub(crate) fn parse_answer(expect: &Expect, raw: &str) -> Result<Answer, AnswerError> {
    let line = raw.trim();
    // Lines starting with `:` are interactive commands at any stop point
    if line.starts_with(':') {
        return Ok(Answer::Command);
    }
    match expect {
        Expect::Text(default) => Ok(Answer::Return(Value::Str(if raw.is_empty() {
            default.clone().into()
        } else {
            raw.to_string().into()
        }))),
        Expect::Int(default) => {
            if line.is_empty() {
                return Ok(Answer::Return(Value::Int(*default)));
            }
            line.parse::<i64>()
                .map(|n| Answer::Return(Value::Int(n)))
                .map_err(|_| AnswerError::BadInt(line.to_string()))
        }
        Expect::Confirm => match line.to_lowercase().as_str() {
            "" | "y" | "yes" | "はい" => Ok(Answer::Return(Value::Bool(true))),
            "n" | "no" | "いいえ" => Ok(Answer::Return(Value::Bool(false))),
            _ => Err(AnswerError::BadConfirm(line.to_string())),
        },
        Expect::Choice(ids) => {
            // Answer with a number or the stable ID of an option (with `#`)
            match line.strip_prefix('#') {
                Some(id) => ids
                    .iter()
                    .position(|i| i.as_deref() == Some(id))
                    .map(Answer::Pick)
                    .ok_or_else(|| AnswerError::UnknownId(line.to_string())),
                None => match line.parse::<usize>() {
                    Ok(n) if (1..=ids.len()).contains(&n) => Ok(Answer::Pick(n - 1)),
                    _ => Err(AnswerError::Invalid(line.to_string())),
                },
            }
        }
        Expect::Event { names, timed } => {
            if raw.is_empty() {
                return if *timed {
                    Ok(Answer::Return(Outcome::TimedOut.into_value()))
                } else {
                    Err(AnswerError::EmptyEvent)
                };
            }
            // The first word is the name; the rest after the first whitespace is the data (kept as it is)
            let (name, data) = raw.split_once(char::is_whitespace).unwrap_or((raw, ""));
            if names.iter().any(|n| n == name) {
                Ok(Answer::Return(
                    Outcome::Arrived(name.to_string(), data.to_string()).into_value(),
                ))
            } else {
                Err(AnswerError::UnknownEvent(name.to_string()))
            }
        }
        Expect::Next if line.is_empty() => Ok(Answer::Advance),
        Expect::Next => Ok(Answer::Other),
    }
}

/// The names of the events waited for, joined for messages (empty if the stop point is not an event wait).
pub(crate) fn event_names(expect: &Expect) -> String {
    match expect {
        Expect::Event { names, .. } => names.join(", "),
        _ => String::new(),
    }
}

/// The default answer at the end of standard input (EOF).
pub(crate) fn eof_answer(expect: &Expect) -> Answer {
    match expect {
        Expect::Choice(_) => Answer::Pick(0),
        Expect::Next => Answer::Advance,
        Expect::Text(default) => Answer::Return(Value::Str(default.clone().into())),
        Expect::Int(default) => Answer::Return(Value::Int(*default)),
        Expect::Confirm => Answer::Return(Value::Bool(true)),
        Expect::Event { timed: true, .. } => Answer::Return(Outcome::TimedOut.into_value()),
        Expect::Event { timed: false, .. } => Answer::EndOfInput,
    }
}

/// The kind of input a stop point waits for.
pub(crate) fn expectation(reg: &Registry, command: CommandId, args: &[Value]) -> Expect {
    match (reg.command(command).name.as_str(), args) {
        (INPUT_TEXT, [_, Value::Str(default), _]) => Expect::Text(default.to_string()),
        (INPUT_INT, [_, Value::Int(default), _, _]) => Expect::Int(*default),
        (INPUT_CONFIRM, _) => Expect::Confirm,
        (EVENT_WAIT, [Value::List(names), timeout]) => Expect::Event {
            names: names.iter().map(Value::to_text).collect(),
            timed: matches!(timeout, Value::Option(Some(_))),
        },
        (_, [Value::List(items), ids]) => Expect::Choice(
            (0..items.len())
                .map(|i| choice_id(ids, i).map(str::to_string))
                .collect(),
        ),
        _ => Expect::Next,
    }
}
