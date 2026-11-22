//! Runtime errors of the VM.

use super::*;

/// The kind of runtime error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmErrorKind {
    /// Exceeded the limit on the number of instructions.
    StepLimitExceeded,
    /// An interrupt was requested through the token ([`crate::vm::InterruptHandle`]).
    Interrupted,
    /// The depth of calls (nesting of `call`) exceeded the limit.
    CallDepthExceeded,
    /// A value (a string, `list`, `struct`, and so on) exceeded the size limit.
    ValueTooLarge,
    /// The total size of values held in variables exceeded the limit (`VmConfig::max_total_bytes`).
    MemoryLimitExceeded,
    /// An index into a `list` is out of range.
    IndexOutOfRange {
        /// The index given.
        index: i64,
        /// The length of the `list`.
        len: i64,
    },
    /// Read a key missing from a `map` by index (`m[k]`), or tried to update it (`m[k] += 1`, `m[k].x = 1`).
    /// `m[k] = v` adds the key if missing, so it does not cause this.
    KeyNotFound {
        /// The key given (as the string written for `${}` interpolation).
        key: String,
    },
    /// Integer overflow.
    IntegerOverflow,
    /// Divided by 0 (for both `int` and `float`).
    DivisionByZero,
    /// The result of a `float` operation became `NaN` or `±∞` (which never appear as values).
    NonFiniteFloat,
    /// The result of `to_int` does not fit in the range of `int`.
    FloatOutOfRange,
    /// Called `resume` or `save` while not stopped.
    NotStopped,
    /// Called `run` while stopped.
    AlreadyStopped,
    /// Called `run` after the end.
    Ended,
    /// None of the options of a `choice` are in the list (`if` was false, or already chosen with `once`).
    NoChoiceAvailable,
    /// The number of the choice passed to `resume` is out of the range of the given list.
    ChoiceOutOfRange {
        /// The number given.
        index: i64,
        /// The length of the list.
        count: i64,
    },
    /// Tried to continue after a runtime error.
    Poisoned,
    /// The persistent data in `VmConfig::persistent` could not be read (`Vm::new`).
    PersistentData(LoadErrorKind),
    /// `min > max` in `random(min, max)`.
    InvalidRandomRange {
        /// The value given as the minimum.
        min: i64,
        /// The value given as the maximum.
        max: i64,
    },
    /// A snapshot made with another program was passed to `restore`.
    SnapshotMismatch,
    /// The type of the input to `resume` does not match.
    BadInput {
        /// The expected type.
        expected: Ty,
        /// The type given.
        found: Ty,
    },
    /// There is no start label.
    StartLabelNotFound(String),
    /// A command or variable used by the program is not in the registry.
    CommandMissing(String),
    /// A command function returned `Err`. Has a code decided by the package (`package.error_name`) and named arguments.
    /// No message text (displaying it is the host's job).
    CommandFailed {
        /// The error code of the package (those returned by the core are `core.…`).
        code: String,
        /// Named arguments determined by each code.
        args: Vec<(&'static str, DiagArg)>,
    },
    /// The variable named in `get_var` / `set_var` is not registered.
    UnknownVariable(String),
    /// A package's state slice could not be serialized (a bug in the implementation of `Package::State`).
    SliceSerialize(String),
    /// Tried to create a VM with a registry made from a declaration file (it has no functions to run commands).
    DeclOnlyRegistry,
}

impl VmErrorKind {
    /// The stable English name.
    pub fn code(&self) -> &'static str {
        match self {
            VmErrorKind::StepLimitExceeded => "step_limit_exceeded",
            VmErrorKind::Interrupted => "interrupted",
            VmErrorKind::CallDepthExceeded => "call_depth_exceeded",
            VmErrorKind::ValueTooLarge => "value_too_large",
            VmErrorKind::MemoryLimitExceeded => "memory_limit_exceeded",
            VmErrorKind::IndexOutOfRange { .. } => "index_out_of_range",
            VmErrorKind::KeyNotFound { .. } => "key_not_found",
            VmErrorKind::IntegerOverflow => "integer_overflow",
            VmErrorKind::DivisionByZero => "division_by_zero",
            VmErrorKind::NonFiniteFloat => "non_finite_float",
            VmErrorKind::FloatOutOfRange => "float_out_of_range",
            VmErrorKind::NotStopped => "not_stopped",
            VmErrorKind::AlreadyStopped => "already_stopped",
            VmErrorKind::Ended => "ended",
            VmErrorKind::NoChoiceAvailable => "no_choice_available",
            VmErrorKind::ChoiceOutOfRange { .. } => "choice_out_of_range",
            VmErrorKind::Poisoned => "poisoned",
            VmErrorKind::PersistentData(_) => "persistent_data",
            VmErrorKind::InvalidRandomRange { .. } => "invalid_random_range",
            VmErrorKind::SnapshotMismatch => "snapshot_mismatch",
            VmErrorKind::BadInput { .. } => "bad_input",
            VmErrorKind::StartLabelNotFound(_) => "start_label_not_found",
            VmErrorKind::CommandMissing(_) => "command_missing",
            VmErrorKind::CommandFailed { .. } => "command_failed",
            VmErrorKind::UnknownVariable(_) => "unknown_variable",
            VmErrorKind::SliceSerialize(_) => "slice_serialize",
            VmErrorKind::DeclOnlyRegistry => "decl_only_registry",
        }
    }

    /// For a command failure ([`VmErrorKind::CommandFailed`]), the package's error code and named arguments.
    pub fn command_error(&self) -> Option<(&str, &[(&'static str, DiagArg)])> {
        match self {
            VmErrorKind::CommandFailed { code, args } => Some((code, args)),
            _ => None,
        }
    }
}

/// A runtime error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmError {
    /// Kind.
    pub kind: VmErrorKind,
    /// The name of the module (when known).
    pub module: Option<String>,
    /// The label (when known).
    pub label: Option<String>,
    /// The line (when known).
    pub line: Option<u32>,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind.code())?;
        if let Some(l) = &self.label {
            write!(f, " (label ")?;
            if let Some(m) = &self.module {
                write!(f, "{m}.")?;
            }
            write!(f, "{l}")?;
            if let Some(n) = self.line {
                write!(f, ", line {n}")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl std::error::Error for VmError {}
