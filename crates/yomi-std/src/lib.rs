//! yomi's standard packages (`flow`, `math`, `text`, `json`, `audio`, `image`, `screen`, `input`, `event`).
//!
//! The only dependencies are `yomi-core`, `serde` and `serde_json` (`spec/packages/`). The host either registers all 9 together with
//! [`register_all`], or chooses among [`flow::Flow`], [`math::Math`], [`text::Text`], [`json::Json`], [`audio::Audio`],
//! [`image::Image`], [`screen::Screen`], [`input::Input`] and [`event::Event`] and registers them with
//! [`yomi_core::Registry::package`] (to register several at once, [`yomi_core::packages!`]).
//!
//! Sound (`audio`), images (`image`) and screen effects (`screen`) have state and cues (`cues`). The host reads them from
//! `Vm::package_state` at stop points to play sounds and draw pictures. Input (`input`) consists of stop commands that
//! return values.

#![forbid(unsafe_code)]

pub mod audio;
mod check;
pub mod event;
pub mod flow;
pub mod image;
pub mod input;
pub mod json;
pub mod math;
pub mod screen;
mod test_answers;
pub mod text;

pub use test_answers::{test_answers, test_answers_with_events};
use yomi_core::{ErrorCodeInfo, Registry, RegistryError, packages};

/// Registers `flow`, `math`, `text`, `json`, `audio`, `image`, `screen`, `input` and `event` together (bulk registration in any order).
///
/// If registration fails (for example when the same name is already registered), the registry is unchanged.
pub fn register_all(reg: &mut Registry) -> Result<(), RegistryError> {
    reg.package_set(&packages![
        flow::Flow,
        math::Math,
        text::Text,
        json::Json,
        audio::Audio,
        image::Image,
        screen::Screen,
        input::Input,
        event::Event
    ])
}

/// The list of error codes that the standard packages return as `CommandError` (in package order, then name order within
/// a package).
///
/// Codes are `package.error_name`. Arguments are named values decided by each code, with no message text (displaying it
/// is the host's job; checked mechanically against the "error codes" tables in `spec/packages/*.md` by tests).
pub fn error_codes() -> &'static [ErrorCodeInfo] {
    const CODES: &[ErrorCodeInfo] = &[
        ErrorCodeInfo::new("flow.negative_seconds", &["seconds"]),
        ErrorCodeInfo::new("math.bounds_reversed", &["command", "lo", "hi"]),
        ErrorCodeInfo::new("math.pow_negative_exp", &["exp"]),
        ErrorCodeInfo::new("text.empty_arg", &["command", "arg"]),
        ErrorCodeInfo::new("text.fill_not_grapheme", &["command"]),
        ErrorCodeInfo::new("text.negative", &["command", "arg"]),
        ErrorCodeInfo::new("text.result_too_large", &["command", "limit"]),
        ErrorCodeInfo::new("text.slice_range", &["start", "end", "len"]),
        ErrorCodeInfo::new("audio.channel_empty", &["command", "channel"]),
        ErrorCodeInfo::new("audio.negative", &["command", "arg"]),
        ErrorCodeInfo::new("audio.not_finite", &["command", "arg"]),
        ErrorCodeInfo::new("audio.out_of_unit_range", &["command", "arg"]),
        ErrorCodeInfo::new("image.negative", &["command", "arg"]),
        ErrorCodeInfo::new("image.not_finite", &["command", "arg"]),
        ErrorCodeInfo::new("image.not_positive", &["command", "arg"]),
        ErrorCodeInfo::new("image.out_of_unit_range", &["command", "arg"]),
        ErrorCodeInfo::new("image.slot_empty", &["command", "slot"]),
        ErrorCodeInfo::new("screen.negative", &["command", "arg"]),
        ErrorCodeInfo::new("screen.not_finite", &["command", "arg"]),
        ErrorCodeInfo::new("input.default_out_of_range", &["default", "min", "max"]),
        ErrorCodeInfo::new("input.integer_out_of_range", &["value", "min", "max"]),
        ErrorCodeInfo::new("input.max_len_not_positive", &["max_len"]),
        ErrorCodeInfo::new("input.range_reversed", &["min", "max"]),
        ErrorCodeInfo::new("input.text_too_long", &["max_len", "len"]),
        ErrorCodeInfo::new("event.duplicate_name", &["name"]),
        ErrorCodeInfo::new("event.empty_name", &["index"]),
        ErrorCodeInfo::new("event.negative_timeout", &["timeout"]),
        ErrorCodeInfo::new("event.no_names", &[]),
        ErrorCodeInfo::new("event.unexpected_timeout", &[]),
        ErrorCodeInfo::new("event.unknown_event", &["name"]),
    ];
    CODES
}
