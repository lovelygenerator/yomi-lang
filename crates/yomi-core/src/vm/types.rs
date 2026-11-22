//! Types exchanged with the host (stop points, input, variables, records of stop points).

use super::*;

/// The stop notification returned by `run`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Yield {
    /// Stopped at a stop command.
    Stop {
        /// The command called.
        command: CommandId,
        /// The evaluated arguments.
        args: Vec<Value>,
        /// The stable ID at the end of the line.
        id: Option<String>,
    },
    /// Reached `end`.
    End,
}

/// The input passed to `resume`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The return value of a stop command.
    Return(Value),
}

/// How a variable looks (for inspection).
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    /// For a module variable, the name of the module that declared it (`None` for local variables).
    pub module: Option<String>,
    /// The name (without the module name).
    pub name: String,
    /// The value.
    pub value: Value,
    /// Whether it is a local variable (not a module variable).
    pub local: bool,
    /// Whether it is a `persistent var` (a variable that goes into persistent data, not saves).
    pub persistent: bool,
}

/// A record of a stop point (one history entry; [`Vm::history`]).
///
/// Holds the original and the evaluated values. Does not hold translations after replacement (so the history can be
/// re-rendered in the current language even after switching languages).
#[derive(Debug, Clone, PartialEq)]
pub struct StopRecord {
    /// The label of the stop point (`module.label`).
    pub label: String,
    /// The kind of stop point (a sugar, or an `@` command). Used to re-render the body in the current language
    /// ([`Vm::render`]).
    pub kind: StopKind,
    /// `module.label#id` (`None` for stop points without a stable ID and for choices).
    pub key: Option<String>,
    /// The name of the command it stopped at (the name in the registry).
    pub command: String,
    /// The arguments of the stop notification (the evaluated original, before replacement).
    pub args: Vec<Value>,
    /// The evaluated values of `${expr}` (strings) for each body. Bodies are in the order evaluated. A plain or quote line has
    /// one body, a choice has one per option included in the list, and a stop command has one per string literal argument
    /// (empty if there is no `${expr}`).
    pub values: Vec<Vec<String>>,
    /// The return value passed on resume ([`Vm::resume`]). `None` for stop points not yet resumed and for those whose return
    /// value is `()`. For a choice, the number in the given list (`Int`). Included in saves and undone by rollback.
    pub ret: Option<Value>,
}
