use super::*;

#[test]
fn integers_read_only_sign_and_ascii_digits() {
    assert_eq!(parse_int("42"), Some(42));
    assert_eq!(parse_int("-7"), Some(-7));
    assert_eq!(parse_int("+3"), Some(3));
    assert_eq!(parse_int("9223372036854775807"), Some(i64::MAX));
    assert_eq!(parse_int("-9223372036854775808"), Some(i64::MIN));
    for bad in [
        "",
        "+",
        "-",
        " 1",
        "1 ",
        "1.0",
        "1e3",
        "１２",
        "1_000",
        "9223372036854775808",
        "0x10",
    ] {
        assert_eq!(parse_int(bad), None, "{bad:?}");
    }
}

#[test]
fn floats_read_decimal_and_exponent_and_reject_non_finite() {
    assert_eq!(parse_float("1.5"), Some(1.5));
    assert_eq!(parse_float("-2"), Some(-2.0));
    assert_eq!(parse_float(".5"), Some(0.5));
    assert_eq!(parse_float("2."), Some(2.0));
    assert_eq!(parse_float("2.5E-2"), Some(0.025));
    assert_eq!(
        parse_float("-0.0").map(f64::to_bits),
        Some(0.0f64.to_bits())
    );
    for bad in [
        "", ".", "e3", "inf", "-inf", "NaN", "infinity", "1e400", " 1", "1,5", "１.５", "1.2.3",
    ] {
        assert_eq!(parse_float(bad), None, "{bad:?}");
    }
}
