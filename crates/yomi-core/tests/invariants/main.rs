//! Invariant tests.
//!
//! Using a hand-written seeded random generator, makes random scripts and checks the following.
//! - Saving and restoring gives the same continuation (including stop points in the middle of `call` and multiple
//!   modules).
//! - A save from a changed script does not panic, and goes back to the start of the label or to the position found.
//! - The same input gives the same output (determinism). The bytes of saves are the same too.
//! - A broken save does not panic and becomes `Err`.
//! - `compile` does not panic on any input.
//! - A script that compiled does not panic when run within the instruction limit.
//!
//! Environment variables:
//! - `YOMI_INVARIANT_CASES`: the number of cases per test (default 300).
//! - `YOMI_INVARIANT_SEED`: the random seed (the default is a fixed value; used to reproduce failures).

#[path = "../common/mod.rs"]
mod common;
use common::crc32;
use common::rng::{self, Rng};

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

use yomi_core::{
    CompileOptions, Diagnostic, Identity, Input, Program, Registry, ResolvedBy, Severity, Sugar,
    Value, Vm, VmConfig, VmErrorKind, Yield, compile_modules_with_options,
};

fn compile_modules_with_warnings(
    modules: &[(&str, &str)],
    registry: &Registry,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_with_options(modules, registry, &CompileOptions::default())
}

fn compile_modules(
    modules: &[(&str, &str)],
    registry: &Registry,
) -> Result<Program, Vec<Diagnostic>> {
    compile_modules_with_warnings(modules, registry).map(|(p, _)| p)
}

fn compile_with_warnings(
    source: &str,
    module: &str,
    registry: &Registry,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_with_warnings(&[(module, source)], registry)
}

fn compile(source: &str, module: &str, registry: &Registry) -> Result<Program, Vec<Diagnostic>> {
    compile_with_warnings(source, module, registry).map(|(p, _)| p)
}

mod api_misuse;
mod assign_ids;
mod choices;
mod composite;
mod functions;
mod fuzz;
mod generate;
mod harness;
mod map_for;
mod map_index;
mod multi_module;
mod persistence;
mod rand;
mod save_bytes;
mod save_check;
mod texts;
mod valid_scripts;

use fuzz::*;
use generate::*;
use harness::*;
use map_for::*;
use multi_module::*;
use persistence::*;
use rand::*;
use save_bytes::*;
use save_check::*;
