//! The `input` package: input of values from the player (`spec/packages/input.md`).
//!
//! All commands are stop commands that return values. Showing input screens and collecting values is the host's job, and
//! the values are passed with `Input::Return`. It has neither state nor a persistent slice. Arguments that break the
//! constraints are runtime errors before stopping. When a value returned by the host does not satisfy the constraints
//! (`max_len`, range), `resume` returns a runtime error (`PackageReg::stop_validated`).

use yomi_core::{CommandError, Package, PackageReg};

use crate::text::grapheme_count;

/// The `input` package. It has no state.
///
/// - `input.text(prompt, default, max_len) -> str`: a string. `max_len` is the maximum number of grapheme clusters
///   (counted the same as `text.len`).
/// - `input.integer(prompt, default, min, max) -> int`: an integer.
/// - `input.confirm(prompt) -> bool`: yes or no.
pub struct Input;

impl Package for Input {
    const NAME: &'static str = "input";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.stop_validated::<(String, String, i64), String, _, _>(
            "input.text",
            |(_, _, max_len)| {
                if *max_len <= 0 {
                    Err(CommandError::with_code(
                        "input.max_len_not_positive",
                        [("max_len", (*max_len).into())],
                    ))
                } else {
                    Ok(())
                }
            },
            |(_, _, max_len), value| {
                if i64::try_from(grapheme_count(value)).is_ok_and(|n| n <= *max_len) {
                    Ok(())
                } else {
                    Err(CommandError::with_code(
                        "input.text_too_long",
                        [
                            ("max_len", (*max_len).into()),
                            ("len", grapheme_count(value).into()),
                        ],
                    ))
                }
            },
        )
        .doc("文字列を入力させる（max_len は書記素の上限）")
        .param_names(&["prompt", "default", "max_len"])
        .text_param(0);
        reg.stop_validated::<(String, i64, i64, i64), i64, _, _>(
            "input.integer",
            |(_, default, min, max)| {
                if min > max {
                    Err(CommandError::with_code(
                        "input.range_reversed",
                        [("min", (*min).into()), ("max", (*max).into())],
                    ))
                } else if default < min || default > max {
                    Err(CommandError::with_code(
                        "input.default_out_of_range",
                        [
                            ("default", (*default).into()),
                            ("min", (*min).into()),
                            ("max", (*max).into()),
                        ],
                    ))
                } else {
                    Ok(())
                }
            },
            |(_, _, min, max), value| {
                if (*min..=*max).contains(value) {
                    Ok(())
                } else {
                    Err(CommandError::with_code(
                        "input.integer_out_of_range",
                        [
                            ("value", (*value).into()),
                            ("min", (*min).into()),
                            ("max", (*max).into()),
                        ],
                    ))
                }
            },
        )
        .doc("整数を入力させる（min 以上 max 以下。default は範囲の中）")
        .param_names(&["prompt", "default", "min", "max"])
        .text_param(0);
        reg.stop::<(String,), bool>("input.confirm")
            .doc("はい・いいえを尋ねる")
            .param_names(&["prompt"])
            .text_param(0);
    }
}
