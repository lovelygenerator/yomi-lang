//! Errors of commands and of migrating state slices.

use super::*;

/// The kind of command failure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandErrorKind {
    /// A code decided by the package and named arguments (no message text; displaying it is the host's job).
    Failed {
        /// A stable English identifier in the form `package.error_name` (the core's own are `core.…`).
        code: String,
        /// Named arguments determined by each code.
        args: Vec<(&'static str, DiagArg)>,
    },
    /// Integer overflow (treated the same as `VmErrorKind::IntegerOverflow`).
    IntegerOverflow,
    /// A `float` result became `NaN` or `±∞` (`VmErrorKind::NonFiniteFloat`).
    NonFiniteFloat,
    /// A `float` does not fit in the range of `int` (`VmErrorKind::FloatOutOfRange`).
    FloatOutOfRange,
    /// Divided by 0 (`VmErrorKind::DivisionByZero`).
    DivisionByZero,
    /// The command stopped because an interrupt was requested (`VmErrorKind::Interrupted`).
    Interrupted,
}

/// A failure returned by a command function. When `Err` is returned, the VM returns it from `run` as a runtime error (the
/// VM adds the position).
///
/// The core has no error messages. It holds only codes and arguments; turning them into messages is the host's job (the
/// CLI uses FTL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub(super) kind: CommandErrorKind,
}

impl CommandError {
    /// Makes a failure with a code and named arguments. The code is `package.error_name` (such as
    /// `input.integer_out_of_range`).
    ///
    /// Argument values are [`DiagArg`]s (strings or integers; can be made with `.into()`). Pass decimals as strings.
    pub fn with_code<I>(code: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = (&'static str, DiagArg)>,
    {
        Self {
            kind: CommandErrorKind::Failed {
                code: code.into(),
                args: args.into_iter().collect(),
            },
        }
    }

    /// Adds one named argument (used instead of passing an array to `with_code` when value types are mixed).
    #[must_use]
    pub fn arg(mut self, name: &'static str, value: impl Into<DiagArg>) -> Self {
        if let CommandErrorKind::Failed { args, .. } = &mut self.kind {
            args.push((name, value.into()));
        }
        self
    }

    /// Integer overflow.
    pub fn integer_overflow() -> Self {
        Self {
            kind: CommandErrorKind::IntegerOverflow,
        }
    }

    /// A `float` result became `NaN` or `±∞`.
    pub fn non_finite_float() -> Self {
        Self {
            kind: CommandErrorKind::NonFiniteFloat,
        }
    }

    /// A `float` does not fit in the range of `int`.
    pub fn float_out_of_range() -> Self {
        Self {
            kind: CommandErrorKind::FloatOutOfRange,
        }
    }

    /// Divided by 0.
    pub fn division_by_zero() -> Self {
        Self {
            kind: CommandErrorKind::DivisionByZero,
        }
    }

    /// The command stopped because an interrupt was requested ([`Ctx::interrupted`]).
    pub fn interrupted() -> Self {
        Self {
            kind: CommandErrorKind::Interrupted,
        }
    }

    /// The kind of failure.
    pub fn kind(&self) -> &CommandErrorKind {
        &self.kind
    }

    /// The code. For the same kinds as the core's runtime errors, their English names (such as `integer_overflow`).
    pub fn code(&self) -> &str {
        match &self.kind {
            CommandErrorKind::Failed { code, .. } => code,
            CommandErrorKind::IntegerOverflow => "integer_overflow",
            CommandErrorKind::NonFiniteFloat => "non_finite_float",
            CommandErrorKind::FloatOutOfRange => "float_out_of_range",
            CommandErrorKind::DivisionByZero => "division_by_zero",
            CommandErrorKind::Interrupted => "interrupted",
        }
    }

    /// Named arguments. Empty for the same kinds as the core's runtime errors.
    pub fn args(&self) -> &[(&'static str, DiagArg)] {
        match &self.kind {
            CommandErrorKind::Failed { args, .. } => args,
            _ => &[],
        }
    }

    /// Turns it into the kind of a VM runtime error.
    pub(crate) fn into_vm_kind(self) -> VmErrorKind {
        match self.kind {
            CommandErrorKind::Failed { code, args } => VmErrorKind::CommandFailed { code, args },
            CommandErrorKind::IntegerOverflow => VmErrorKind::IntegerOverflow,
            CommandErrorKind::NonFiniteFloat => VmErrorKind::NonFiniteFloat,
            CommandErrorKind::FloatOutOfRange => VmErrorKind::FloatOutOfRange,
            CommandErrorKind::DivisionByZero => VmErrorKind::DivisionByZero,
            CommandErrorKind::Interrupted => VmErrorKind::Interrupted,
        }
    }
}

/// In the form `code (name=value, …)` (for logs; not a message for users).
impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())?;
        let args = self.args();
        if !args.is_empty() {
            write!(f, " (")?;
            for (i, (name, value)) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{name}={value}")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}

/// A failure of a slice's `migrate`. The load fails with `LoadErrorKind::PackageSlice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateError {
    pub(super) message: String,
}

impl MigrateError {
    /// Makes a failure with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// A slice of an unknown older version.
    pub fn unsupported_version(old_version: u32) -> Self {
        Self::new(format!("unsupported slice version {old_version}"))
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MigrateError {}

/// Turns the bytes of a slice (`postcard`) back into a type. Used by `migrate` to read bytes of an older version (the core
/// provides this so that packages need not depend on `postcard`).
pub fn decode_slice<T: DeserializeOwned>(raw: &[u8]) -> Result<T, MigrateError> {
    postcard::from_bytes(raw).map_err(|e| MigrateError::new(format!("decode: {e}")))
}

/// Turns a value into the bytes of a slice (`postcard`) (used in tests of `migrate` and so on).
pub fn encode_slice<T: Serialize>(value: &T) -> Result<Vec<u8>, MigrateError> {
    postcard::to_stdvec(value).map_err(|e| MigrateError::new(format!("encode: {e}")))
}
