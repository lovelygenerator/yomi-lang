use super::*;
use crate::program::Program;
use crate::registry::Sugar;

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(String, i64), ()>("game.give").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

/// Diagnostics (including warnings). `label_falls_through` is emitted for labels other than `start` regardless of other
/// checks, so it is excluded (that warning is checked by dedicated tests).
fn diags(src: &str) -> Vec<Diagnostic> {
    let all = match compile_with_warnings(src, "m", &registry()) {
        Ok((_, warnings)) => warnings,
        Err(d) => d,
    };
    all.into_iter()
        .filter(|d| d.code != DiagCode::LabelFallsThrough)
        .collect()
}

fn errors(src: &str) -> Vec<DiagCode> {
    diags(src)
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

mod checks;
mod choices;
mod nesting;

use choices::*;
