//! Public types of the REPL (names, completeness of input, results of submit and run, declarations, results of apply).

use super::*;

/// The name of a variable (module variables and host-declared variables include the module (namespace)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    /// The module (the namespace for host-declared variables). `None` for local variables.
    pub module: Option<String>,
    /// The name of the variable (without the module).
    pub name: String,
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.module {
            Some(m) => write!(f, "{m}.{}", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

/// The completion state of input, decided by syntax alone (section 7.5 of `spec/host/07-repl.md`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Completeness {
    /// Can be read as one input.
    Complete,
    /// Cannot be read because the end of input was reached (a `{ }` block or the `「` of a quote line is not closed).
    Incomplete,
    /// There is an error in the middle of the input that writing more will not fix. Diagnostics are in input position order.
    Invalid(Vec<Diagnostic>),
}

/// Decides whether input is complete by syntax alone (independent of the environment; 7.5).
///
/// Each time a line is added, the host calls this on the whole input so far (lines joined with newlines, including empty
/// lines). If the only causes are `unterminated_block` and `unterminated_quote`, `Incomplete`. Lines starting with `:`
/// (commands) and strings of only whitespace are `Complete`. Input starting with `save var`, `persistent var` or `label`
/// is `Invalid` with `repl_not_allowed`. Since the environment is unknown and it cannot tell whether a name is an
/// identity, a line starting with `name「` whose `「` is not closed is treated as a quote line.
pub fn completeness(input: &str) -> Completeness {
    let mut idents = IdentMap::new();
    idents.insert(ANY_IDENTITY.to_string(), HashSet::new());
    match classify(input, &idents, false) {
        Ok(_) => Completeness::Complete,
        Err(diags) => {
            let total = input.split('\n').count();
            let open = |d: &Diagnostic| match d.code {
                DiagCode::UnterminatedBlock => true,
                // Up to 256 continuation lines. A quote line still not closed beyond that cannot be fixed by writing more (invalid)
                DiagCode::UnterminatedQuote => total.saturating_sub(d.line as usize) <= 256,
                _ => false,
            };
            if !diags.is_empty() && diags.iter().all(open) {
                Completeness::Incomplete
            } else {
                Completeness::Invalid(diags)
            }
        }
    }
}

/// The result of `submit` accepting input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Submitted {
    /// The kind of input.
    pub kind: InputKind,
    /// Warnings (in position order).
    pub warnings: Vec<Diagnostic>,
}

/// The kind of input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum InputKind {
    /// An expression (with its type).
    Expression(Ty),
    /// Statements or script lines (including empty input).
    Statement,
    /// Declarations.
    Declaration,
}

/// The result of `run`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ReplYield {
    /// Stopped at a stop command (same as `Yield::Stop`; `id` is `Some` only for lines inside a `call`ed label).
    Stop {
        /// The command called.
        command: CommandId,
        /// The evaluated arguments.
        args: Vec<Value>,
        /// The stable ID at the end of the line.
        id: Option<String>,
    },
    /// Running the input finished.
    Done {
        /// The value of the expression (only for expression input).
        value: Option<(Value, Ty)>,
        /// What this input declared (including the values of `let`).
        declared: Vec<Declared>,
        /// Reached `end` inside a `call`ed label.
        ended: bool,
    },
}

/// Something declared by input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Declared {
    /// `let` (the declared variable and its value).
    Let {
        /// The name of the variable.
        name: String,
        /// The value.
        value: Value,
        /// The type.
        ty: Ty,
    },
    /// `fn`.
    Fn {
        /// The name.
        name: String,
    },
    /// `struct` or `enum`.
    Type {
        /// The name.
        name: String,
    },
    /// `import`.
    Import {
        /// The name of the module (package).
        name: String,
    },
    /// `identity`.
    Identity {
        /// The name.
        name: String,
    },
}

/// Something declared in the REPL (`defs`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Def {
    /// `fn` (with parameter and return types).
    Fn {
        /// The name.
        name: String,
        /// Parameters (names and types).
        params: Vec<(String, Ty)>,
        /// The return type.
        ret: Option<Ty>,
    },
    /// `struct`.
    Struct {
        /// The name.
        name: String,
    },
    /// `enum`.
    Enum {
        /// The name.
        name: String,
    },
    /// `import`.
    Import {
        /// The name of the module (package).
        name: String,
    },
    /// `identity`.
    Identity {
        /// The name.
        name: String,
    },
}

/// The one-line form of `:defs` (`fn bonus(n: int) -> int`, `struct Prize`, `enum Mood`, `import std::math`,
/// `identity 案内人`).
impl std::fmt::Display for Def {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Def::Fn { name, params, ret } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|(n, t)| format!("{n}: {}", t.name()))
                    .collect();
                write!(f, "fn {name}({})", ps.join(", "))?;
                if let Some(r) = ret {
                    write!(f, " -> {}", r.name())?;
                }
                Ok(())
            }
            Def::Struct { name } => write!(f, "struct {name}"),
            Def::Enum { name } => write!(f, "enum {name}"),
            Def::Import { name } => write!(f, "import {name}"),
            Def::Identity { name } => write!(f, "identity {name}"),
        }
    }
}

/// The result of `apply`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The variables applied (values after applying; same form and order as `vm.variables()`).
    pub variables: Vec<Variable>,
    /// Variables whose values differed but were not applied (`persistent var`, `Kind::Persistent`).
    pub skipped: Vec<Name>,
}

/// A failure of `apply`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyError {
    /// There is no script (a `Repl` made with `from_program` or `empty`). Also returned while input is running.
    NotAttached,
    /// Since the stop point at `attach`, `vm` advanced or was reloaded (`resume`, `restore`, `load`), or it is another `Vm`.
    VmChanged,
    /// A value to apply failed a check of type, limits or saveable shape.
    Invalid {
        /// The variable that failed.
        name: Name,
        /// The reason.
        reason: ApplyInvalid,
    },
    /// The arguments of the stop point could not be rebuilt with the applied values (runtime error).
    StopArgs(VmError),
}

/// The reason a check of `apply` failed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyInvalid {
    /// The value does not match the declared type of the variable (including values of types declared in the REPL).
    Type,
    /// The size of one value exceeds `max_value_bytes`.
    ValueTooLarge,
    /// The total of values held in variables exceeds `max_total_bytes`.
    MemoryLimit,
    /// A shape that cannot be saved (nesting too deep, and so on).
    NotSavable,
}
