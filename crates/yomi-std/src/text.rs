//! The `text` package: basic string functions (query commands only; `spec/packages/text.md`).
//!
//! # How "characters" are counted, and the limits
//!
//! `text.len` returns the number of grapheme clusters (characters as they appear). To avoid adding external dependencies,
//! this crate has its own **approximation** of the Unicode grapheme cluster rules (UAX #29). The following count as one
//! character.
//!
//! - `CR LF`
//! - A base character and the combining characters that follow it (the U+0300 block, U+1AB0 block, U+1DC0 block, U+20D0
//!   block, U+FE20 block, and the combining kana voiced and semi-voiced sound marks U+3099 and U+309A), variation
//!   selectors (U+FE00 to FE0F, U+E0100 to E01EF), skin tone modifiers (U+1F3FB to 1F3FF), tag characters (U+E0020 to
//!   E007F) and ZWNJ (U+200C)
//! - Sequences of emoji joined with ZWJ (U+200D) (when the character after the ZWJ is in the range used for emoji)
//! - Flags (two regional indicator symbols; from the third on, again two at a time)
//!
//! Since it is an approximation, the following are not counted correctly (it often counts more than the real rules).
//!
//! - Combining marks of Indic scripts, Thai, Arabic, Hebrew and others (those not in the ranges above)
//! - Sequences of initial, medial and final jamo (the Hangul U+1100 block) (precomposed modern syllables are one
//!   character)
//! - Emoji are detected by ranges. Symbols outside the ranges are not treated as joined even if connected by ZWJ
//!
//! The version of the counting is expressed by [`GRAPHEME_RULES_VERSION`]. Increase this value when the rules are fixed.
//! It is managed separately from [`Package::VERSION`] (the version of the state slice), which relates to the save format
//! (`text` has no state).
//!
//! So that the host (Rust) can use the same counting, [`grapheme_count`], [`grapheme_offsets`] and [`graphemes`] are
//! public. They give the same boundaries as the positions of `text.len`, `text.slice` and `text.find` in scripts. The host
//! can check the version of the counting with [`GRAPHEME_RULES_VERSION`].
//!
//! `contains` and `starts_with` compare as sequences of code points (no normalization, no check of cluster boundaries).
//! `trim` removes Unicode `White_Space` (including the full-width space U+3000) from both ends.
//!
//! # String operations
//!
//! `ends_with`, `replace`, `split`, `join`, `slice`, `find`, `repeat`, `pad_left`, `pad_right`, `upper`, `lower`.
//! Positions and widths are in grapheme clusters. `upper` and `lower` change only ASCII letters.
//! Negative arguments, empty separators, out-of-range positions and results exceeding the limit (16 MiB) are
//! `CommandError`s (runtime errors). Functions whose results can be large (`replace`, `join`, `repeat`, `pad_*`) estimate
//! the size before building and refuse.
//! [`GRAPHEME_RULES_VERSION`] is increased only when the counting is fixed (not when functions are added).
//!
//! `text`'s `Package::RULES_VERSION` (the version of the rules for the results of the whole package, recorded in saves) is
//! kept separately from the counting. Increase it when a fix changes the result for the same arguments, such as the
//! counting, the whitespace of `trim`, what `upper` and `lower` affect, or how `find` decides positions. When the counting
//! is fixed, increase both.

use yomi_core::{CommandError, Ctx, Package, PackageReg, Value};

/// The `text` package. It has no state.
pub struct Text;

mod grapheme;
mod ops;
mod parse;

pub use grapheme::*;
use ops::*;
pub(crate) use parse::*;

#[cfg(test)]
mod grapheme_tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod parse_tests;

impl Package for Text {
    const NAME: &'static str = "text";
    const STD: bool = true;
    /// The version of the rules for the results of all of `text` (separate from the counting version
    /// [`GRAPHEME_RULES_VERSION`]). Recorded in saves; if it differs on load, it appears in `LoadReport::changed_rules`.
    /// Increase it also when the counting is fixed.
    const RULES_VERSION: u32 = 1;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.query("text.to_int", |_: &Ctx<Text>, s: String| -> Option<i64> {
            parse_int(&s)
        })
        .doc("文字列を整数として読む（符号と ASCII の数字だけ。読めなければ None）")
        .param_names(&["s"]);
        reg.query("text.to_float", |_: &Ctx<Text>, s: String| -> Option<f64> {
            parse_float(&s)
        })
        .doc("文字列を小数として読む（10 進の小数・指数の形。有限でなければ None）")
        .param_names(&["s"]);
        reg.query("text.len", |_: &Ctx<Text>, s: String| {
            grapheme_count(&s) as i64
        })
        .doc("文字列の長さ（書記素クラスタの数）")
        .param_names(&["s"]);
        reg.query("text.contains", |_: &Ctx<Text>, s: String, part: String| {
            s.contains(part.as_str())
        })
        .doc("文字列に part が含まれるか")
        .param_names(&["s", "part"]);
        reg.query(
            "text.starts_with",
            |_: &Ctx<Text>, s: String, prefix: String| s.starts_with(prefix.as_str()),
        )
        .doc("文字列が prefix で始まるか")
        .param_names(&["s", "prefix"]);
        reg.query("text.trim", |_: &Ctx<Text>, s: String| s.trim().to_string())
            .doc("文字列の前後の空白を取り除く")
            .param_names(&["s"]);
        reg.query(
            "text.ends_with",
            |_: &Ctx<Text>, s: String, suffix: String| ends_with(&s, &suffix),
        )
        .doc("文字列が suffix で終わるか")
        .param_names(&["s", "suffix"]);
        reg.query(
            "text.replace",
            |_: &Ctx<Text>, s: String, from: String, to: String| replace(&s, &from, &to),
        )
        .doc("from の一致をすべて to に置き換える（from が空なら実行時エラー）")
        .param_names(&["s", "from", "to"]);
        reg.query("text.split", |_: &Ctx<Text>, s: String, sep: String| {
            split(&s, &sep)
        })
        .doc("sep で区切って list にする（sep が空なら実行時エラー）")
        .param_names(&["s", "sep"]);
        reg.query(
            "text.join",
            |_: &Ctx<Text>, parts: Vec<String>, sep: String| join(&parts, &sep),
        )
        .doc("parts を sep でつなぐ")
        .param_names(&["parts", "sep"])
        .defaults(&[("sep", Value::Str("".into()))]);
        reg.query(
            "text.slice",
            |_: &Ctx<Text>, s: String, start: i64, end: i64| slice(&s, start, end),
        )
        .doc("書記素の位置 start から end の手前までを取り出す（範囲外は実行時エラー）")
        .param_names(&["s", "start", "to"]);
        reg.query("text.find", |_: &Ctx<Text>, s: String, part: String| {
            find(&s, &part)
        })
        .doc("part が最初に現れる書記素の位置（なければ None）")
        .param_names(&["s", "part"]);
        reg.query("text.repeat", |_: &Ctx<Text>, s: String, n: i64| {
            repeat(&s, n)
        })
        .doc("s を n 回くり返す（n が負、または結果が大きすぎるなら実行時エラー）")
        .param_names(&["s", "n"]);
        reg.query(
            "text.pad_left",
            |_: &Ctx<Text>, s: String, width: i64, fill: String| {
                pad("text.pad_left", &s, width, &fill, true)
            },
        )
        .doc("書記素 width 個になるまで、左に fill をくり返して埋める")
        .param_names(&["s", "width", "fill"])
        .defaults(&[("fill", Value::Str(" ".into()))]);
        reg.query(
            "text.pad_right",
            |_: &Ctx<Text>, s: String, width: i64, fill: String| {
                pad("text.pad_right", &s, width, &fill, false)
            },
        )
        .doc("書記素 width 個になるまで、右に fill をくり返して埋める")
        .param_names(&["s", "width", "fill"])
        .defaults(&[("fill", Value::Str(" ".into()))]);
        reg.query("text.upper", |_: &Ctx<Text>, s: String| {
            s.to_ascii_uppercase()
        })
        .doc("ASCII の英字だけを大文字にする")
        .param_names(&["s"]);
        reg.query("text.lower", |_: &Ctx<Text>, s: String| {
            s.to_ascii_lowercase()
        })
        .doc("ASCII の英字だけを小文字にする")
        .param_names(&["s"]);
    }
}
