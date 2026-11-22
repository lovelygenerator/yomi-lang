//! Conversion from strings to numbers.

/// Reads as an integer: at most one leading `+` or `-`, then one or more ASCII digits (whitespace, separators and
/// full-width digits are not read). `None` outside the range of `int`.
pub(crate) fn parse_int(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

/// Reads as a decimal: at most one leading `+` or `-`, decimal digits and a decimal point (`1.5`, `.5`, `2.`), and an
/// exponent (`1e3`, `2.5E-2`). Names such as `inf` and `NaN`, and results that are not finite (`1e400`), are `None`.
/// `-0.0` becomes `0.0`.
pub(crate) fn parse_float(s: &str) -> Option<f64> {
    let ok = !s.is_empty()
        && s.bytes().any(|b| b.is_ascii_digit())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'));
    if !ok {
        return None;
    }
    let x = s.parse::<f64>().ok()?;
    if !x.is_finite() {
        return None;
    }
    Some(if x == 0.0 { 0.0 } else { x })
}
