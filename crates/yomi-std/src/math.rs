//! The `math` package: basic numeric functions (query commands only; `spec/packages/math.md`).

use yomi_core::{CommandError, Ctx, Package, PackageReg};

/// The `math` package. It has no state.
pub struct Math;

/// Converts a `float` that fits in the range of `int` to an `int` (out of range is a runtime error).
fn to_int(x: f64) -> Result<i64, CommandError> {
    // 2^63 is `i64::MAX + 1` and is exactly representable as `f64`. `-2^63` is `i64::MIN`
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if (-LIMIT..LIMIT).contains(&x) {
        Ok(x as i64)
    } else {
        Err(CommandError::float_out_of_range())
    }
}

/// Checks that the result of a `float` operation is finite.
fn finite(x: f64) -> Result<f64, CommandError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(CommandError::non_finite_float())
    }
}

/// Integer power. `exp < 0` and overflow are runtime errors.
fn pow_int(base: i64, exp: i64) -> Result<i64, CommandError> {
    if exp < 0 {
        return Err(CommandError::with_code(
            "math.pow_negative_exp",
            [("exp", exp.into())],
        ));
    }
    match u32::try_from(exp) {
        Ok(e) => base
            .checked_pow(e)
            .ok_or_else(CommandError::integer_overflow),
        // When the exponent does not fit in `u32`, the result fits only when the base is 0, 1 or -1
        Err(_) => match base {
            0 | 1 => Ok(base),
            -1 => Ok(if exp % 2 == 0 { 1 } else { -1 }),
            _ => Err(CommandError::integer_overflow()),
        },
    }
}

impl Package for Math {
    const NAME: &'static str = "math";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.query("math.abs", |_: &Ctx<Math>, x: i64| {
            x.checked_abs().ok_or_else(CommandError::integer_overflow)
        })
        .doc("整数の絶対値")
        .param_names(&["x"]);
        reg.query("math.wrapping_add", |_: &Ctx<Math>, a: i64, b: i64| {
            a.wrapping_add(b)
        })
        .doc("整数の足し算（範囲を超えたら、64 ビットの 2 の補数で折り返す）")
        .param_names(&["a", "b"]);
        reg.query("math.wrapping_sub", |_: &Ctx<Math>, a: i64, b: i64| {
            a.wrapping_sub(b)
        })
        .doc("整数の引き算（範囲を超えたら、64 ビットの 2 の補数で折り返す）")
        .param_names(&["a", "b"]);
        reg.query("math.wrapping_mul", |_: &Ctx<Math>, a: i64, b: i64| {
            a.wrapping_mul(b)
        })
        .doc("整数の掛け算（範囲を超えたら、64 ビットの 2 の補数で折り返す）")
        .param_names(&["a", "b"]);
        reg.query("math.abs_f", |_: &Ctx<Math>, x: f64| x.abs())
            .doc("小数の絶対値")
            .param_names(&["x"]);
        reg.query("math.min", |_: &Ctx<Math>, a: i64, b: i64| a.min(b))
            .doc("整数の小さい方")
            .param_names(&["a", "b"]);
        reg.query("math.min_f", |_: &Ctx<Math>, a: f64, b: f64| a.min(b))
            .doc("小数の小さい方")
            .param_names(&["a", "b"]);
        reg.query("math.max", |_: &Ctx<Math>, a: i64, b: i64| a.max(b))
            .doc("整数の大きい方")
            .param_names(&["a", "b"]);
        reg.query("math.max_f", |_: &Ctx<Math>, a: f64, b: f64| a.max(b))
            .doc("小数の大きい方")
            .param_names(&["a", "b"]);
        reg.query("math.clamp", |_: &Ctx<Math>, x: i64, lo: i64, hi: i64| {
            if lo > hi {
                Err(CommandError::with_code(
                    "math.bounds_reversed",
                    [
                        ("command", "math.clamp".into()),
                        ("lo", lo.into()),
                        ("hi", hi.into()),
                    ],
                ))
            } else {
                Ok(x.clamp(lo, hi))
            }
        })
        .doc("整数を lo 以上 hi 以下に収める（lo が hi より大きいと実行時エラー）")
        .param_names(&["x", "lo", "hi"]);
        reg.query("math.clamp_f", |_: &Ctx<Math>, x: f64, lo: f64, hi: f64| {
            if lo > hi {
                Err(CommandError::with_code(
                    "math.bounds_reversed",
                    [
                        ("command", "math.clamp_f".into()),
                        ("lo", lo.to_string().into()),
                        ("hi", hi.to_string().into()),
                    ],
                ))
            } else {
                Ok(x.clamp(lo, hi))
            }
        })
        .doc("小数を lo 以上 hi 以下に収める（lo が hi より大きいと実行時エラー）")
        .param_names(&["x", "lo", "hi"]);
        reg.query("math.pow", |_: &Ctx<Math>, base: i64, exp: i64| {
            pow_int(base, exp)
        })
        .doc("整数のべき乗（exp は 0 以上。あふれたら実行時エラー）")
        .param_names(&["base", "exp"]);
        reg.query("math.pow_f", |_: &Ctx<Math>, base: f64, exp: f64| {
            finite(base.powf(exp))
        })
        .doc("小数のべき乗（結果が有限でなければ実行時エラー）")
        .param_names(&["base", "exp"]);
        reg.query("math.floor", |_: &Ctx<Math>, x: f64| to_int(x.floor()))
            .doc("小数を切り下げて整数にする")
            .param_names(&["x"]);
        reg.query("math.ceil", |_: &Ctx<Math>, x: f64| to_int(x.ceil()))
            .doc("小数を切り上げて整数にする")
            .param_names(&["x"]);
        // `f64::round` rounds halves away from 0
        reg.query("math.round", |_: &Ctx<Math>, x: f64| to_int(x.round()))
            .doc("小数を四捨五入して整数にする（半分は 0 から遠い方へ）")
            .param_names(&["x"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_distinguishes_range_edges_and_outside() {
        assert_eq!(to_int(-9_223_372_036_854_775_808.0), Ok(i64::MIN));
        assert_eq!(
            to_int(9_223_372_036_854_774_784.0),
            Ok(9_223_372_036_854_774_784)
        );
        assert_eq!(
            to_int(9_223_372_036_854_775_808.0),
            Err(CommandError::float_out_of_range())
        );
        assert_eq!(to_int(-1e30), Err(CommandError::float_out_of_range()));
    }

    #[test]
    fn integer_pow_makes_negative_exponents_and_overflow_runtime_errors() {
        assert_eq!(pow_int(2, 10), Ok(1024));
        assert_eq!(pow_int(-2, 3), Ok(-8));
        assert_eq!(pow_int(7, 0), Ok(1));
        assert_eq!(pow_int(0, 0), Ok(1));
        assert_eq!(pow_int(2, 63), Err(CommandError::integer_overflow()));
        assert_eq!(pow_int(2, 62), Ok(1 << 62));
        assert!(matches!(pow_int(2, -1), Err(e) if e != CommandError::integer_overflow()));
    }

    #[test]
    fn exponent_beyond_u32_computes_for_bases_0_1_and_minus_1() {
        assert_eq!(pow_int(1, i64::MAX), Ok(1));
        assert_eq!(pow_int(0, i64::MAX), Ok(0));
        assert_eq!(pow_int(-1, i64::MAX), Ok(-1));
        assert_eq!(pow_int(-1, i64::MAX - 1), Ok(1));
        assert_eq!(pow_int(2, i64::MAX), Err(CommandError::integer_overflow()));
    }

    #[test]
    fn non_finite_float_results_are_runtime_errors() {
        assert_eq!(finite(1.5), Ok(1.5));
        assert_eq!(finite(f64::INFINITY), Err(CommandError::non_finite_float()));
        assert_eq!(finite(f64::NAN), Err(CommandError::non_finite_float()));
    }
}
