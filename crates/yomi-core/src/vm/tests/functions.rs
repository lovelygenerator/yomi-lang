//! Tests of `fn` and execution limits: syntax and type checking, forbidden statements, execution (recursion, compound
//! values, value semantics), limits (depth, instruction count, total memory), save, load and rollback.

use super::util::{self, *};
use crate::compile::{compile, compile_modules};
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Choice], |r| {
        r.stop::<(), ()>("ui.pause").unwrap();
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
    util::run_all(vm, 5000)
}

fn texts_with(src: &str, cfg: VmConfig) -> Vec<String> {
    util::texts_with(src, &registry(), cfg, 5000)
}

fn texts(src: &str) -> Vec<String> {
    texts_with(src, VmConfig::default())
}

/// The output of a script with one body line (excluding `<end>`). `fns` are declarations such as `fn` placed before the
/// label.
fn one(fns: &str, body: &str) -> String {
    let src = format!("{fns}\nlabel start {{\n{body}\n}}\n");
    let t = texts(&src);
    assert_eq!(t.last().map(String::as_str), Some("<end>"), "{t:?}");
    t[..t.len() - 1].join("|")
}

mod cross_module;
mod execution;
mod lambda;
mod limits;
mod save_load;
mod syntax_types;
