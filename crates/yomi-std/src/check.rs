//! Checks of numeric arguments (shared by `audio`, `image` and `screen`).

use yomi_core::CommandError;

/// Must be a finite number (not `NaN` or `±∞`). Script `float`s are always finite, but command functions check it too.
pub(crate) fn finite(command: &'static str, arg: &'static str, x: f64) -> Result<(), CommandError> {
    if x.is_finite() {
        Ok(())
    } else {
        Err(CommandError::with_code(
            format!("{}.not_finite", package_of(command)),
            [("command", command.into()), ("arg", arg.into())],
        ))
    }
}

/// The package name part of a command name (`audio.play`) (the first half of error codes).
fn package_of(command: &str) -> &str {
    command.split_once('.').map_or(command, |(p, _)| p)
}

/// Must be finite and 0 or more (`fade`, `seconds`).
pub(crate) fn non_negative(
    command: &'static str,
    arg: &'static str,
    x: f64,
) -> Result<(), CommandError> {
    finite(command, arg, x)?;
    if x < 0.0 {
        Err(CommandError::with_code(
            format!("{}.negative", package_of(command)),
            [("command", command.into()), ("arg", arg.into())],
        ))
    } else {
        Ok(())
    }
}

/// Must be finite and greater than 0 (`scale`).
pub(crate) fn positive(
    command: &'static str,
    arg: &'static str,
    x: f64,
) -> Result<(), CommandError> {
    finite(command, arg, x)?;
    if x > 0.0 {
        Ok(())
    } else {
        Err(CommandError::with_code(
            format!("{}.not_positive", package_of(command)),
            [("command", command.into()), ("arg", arg.into())],
        ))
    }
}

/// Must be finite and in the range 0.0 to 1.0 (`volume`, `alpha`).
pub(crate) fn unit_range(
    command: &'static str,
    arg: &'static str,
    x: f64,
) -> Result<(), CommandError> {
    finite(command, arg, x)?;
    if (0.0..=1.0).contains(&x) {
        Ok(())
    } else {
        Err(CommandError::with_code(
            format!("{}.out_of_unit_range", package_of(command)),
            [("command", command.into()), ("arg", arg.into())],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_numbers_are_rejected() {
        assert!(finite("c", "a", 1.5).is_ok());
        assert!(finite("c", "a", f64::NAN).is_err());
        assert!(finite("c", "a", f64::INFINITY).is_err());
        assert!(finite("c", "a", f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn seconds_must_be_finite_and_non_negative() {
        assert!(non_negative("c", "a", 0.0).is_ok());
        assert!(non_negative("c", "a", -0.1).is_err());
        assert!(non_negative("c", "a", f64::NAN).is_err());
        assert!(non_negative("c", "a", f64::INFINITY).is_err());
    }

    #[test]
    fn volume_must_be_between_0_and_1() {
        assert!(unit_range("c", "a", 0.0).is_ok());
        assert!(unit_range("c", "a", 1.0).is_ok());
        assert!(unit_range("c", "a", 1.01).is_err());
        assert!(unit_range("c", "a", -0.01).is_err());
        assert!(unit_range("c", "a", f64::NAN).is_err());
    }
}
