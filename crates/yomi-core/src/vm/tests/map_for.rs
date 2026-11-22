//! Tests of `map`, `for`, `range` and `Identity`: syntax and type checking, execution, value semantics, save, load and
//! rollback, limits.

use super::util::{self, *};
use crate::program::Program;
use crate::registry::{Sugar, YomiMap};
use crate::value::Identity;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Choice], |r| {
        r.stop::<(), YomiMap<String, i64>>("game.scores").unwrap();
        r.stop::<(), Identity>("game.who").unwrap();
        r.stop::<(YomiMap<String, i64>,), ()>("game.show").unwrap();
    })
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

fn compile_codes(src: &str) -> Vec<&'static str> {
    util::error_codes(src, &registry())
}

fn warning_codes(src: &str) -> Vec<&'static str> {
    util::diag_codes(src, &registry())
}

fn run_all(vm: &mut Vm<'_>) -> Vec<String> {
    util::run_all(vm, 2000)
}

fn texts(src: &str) -> Vec<String> {
    util::texts_with(src, &registry(), VmConfig::default(), 2000)
}

/// The output of a script with one body line (excluding `<end>`).
fn one(body: &str) -> String {
    let src = format!("label start {{\n{body}\n}}\n");
    let t = texts(&src);
    assert_eq!(t.last().map(String::as_str), Some("<end>"), "{t:?}");
    t[..t.len() - 1].join("|")
}

mod command_returns;
mod for_pairs;
mod for_range;
mod identity;
mod limits;
mod map_exec;
mod save_load;
mod syntax_types;
mod value_semantics;
