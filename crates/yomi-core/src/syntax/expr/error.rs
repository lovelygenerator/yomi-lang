//! Syntax parsing failures, and helpers for common errors.

use super::*;

/// A parse failure (with a position; the form before becoming a diagnostic).
#[derive(Debug, Clone)]
pub(crate) struct PErr {
    pub code: DiagCode,
    pub args: Vec<(&'static str, DiagArg)>,
    pub pos: Pos,
}

/// The result of parsing an expression.
pub(crate) type PResult<T> = Result<T, PErr>;

/// Makes a syntax error.
pub(crate) fn err<T>(code: DiagCode, args: Vec<(&'static str, DiagArg)>, pos: Pos) -> PResult<T> {
    Err(PErr { code, args, pos })
}

/// Makes an unexpected token error.
pub(crate) fn unexpected<T>(found: &str, pos: Pos) -> PResult<T> {
    err(
        DiagCode::UnexpectedToken,
        vec![("found", DiagArg::Str(found.to_string()))],
        pos,
    )
}

/// The line ended where more was expected.
pub(crate) fn unexpected_eol<T>(pos: Pos) -> PResult<T> {
    err(DiagCode::UnexpectedEol, vec![], pos)
}

/// The next character (`None` at the end of the line) must not come here.
pub(crate) fn unexpected_char<T>(c: Option<char>, pos: Pos) -> PResult<T> {
    match c {
        Some(c) => unexpected(&c.to_string(), pos),
        None => unexpected_eol(pos),
    }
}

/// Makes an error for syntax that cannot be used yet.
pub(crate) fn not_yet<T>(what: &str, pos: Pos) -> PResult<T> {
    err(
        DiagCode::NotYetSupported,
        vec![("what", DiagArg::Str(what.to_string()))],
        pos,
    )
}
