//! The core of the yomi language.
//!
//! Has the parser, type checker, VM, saves and the package mechanism. Uses no `unsafe` (`forbid(unsafe_code)`).
//!
//! How to run a script (`spec/host/01-execution.md`):
//! 1. Register stop commands in a [`Registry`] and bind the sugar. Register packages ([`Package`]) with
//!    [`Registry::package`] (or [`Registry::package_set`] and [`packages!`] to register a set in any order): effect and
//!    query commands, state slices, and host-declared variables.
//! 2. Turn the source into a [`Program`] with [`compile()`](compile()) (diagnostics are [`Diagnostic`]). To receive
//!    warnings as well, use [`compile_with_warnings`]. For multiple modules (files that refer to each other with
//!    `import`), the host collects the sources and calls [`compile_modules`] (with warnings: [`compile_modules_with_warnings`]).
//! 3. Create a VM with [`Vm::new`] and repeat [`Vm::run`] and [`Vm::resume`].
//! 4. At a stop point, make a save with [`Vm::save`] and restore it with [`Vm::load`]. `persistent var`s and passed stop
//!    points are persistent data in a separate byte string ([`Vm::save_persistent`], [`Vm::load_persistent`]).
//! 5. For rollback, store [`Vm::snapshot`]s in a [`SnapshotLog`] at each stop point and go back with [`Vm::restore`].
//! 6. Host `struct`s and `enum`s get `#[derive(YomiType)]` and are registered with [`Registry::ty`]. A registry can be
//!    written to a declaration file ([`Registry::export_decls`], [`Registry::from_decls`]), so tools can check scripts
//!    without the host's Rust code.
//! 7. For localization, pass a [`TextTable`] (a table that replaces texts, keyed by stable IDs) with [`Vm::set_texts`].
//!    To switch languages while stopped, use [`Vm::refresh_current`]; to render the history in the current language, use
//!    [`Vm::render`] and [`Vm::history_text`]. Check a table with [`TextTable::check`], and require IDs with
//!    [`CompileOptions::require_text_ids`] (`spec/host/05-texts.md`).

#![forbid(unsafe_code)]

extern crate self as yomi_core;

mod analysis;
mod assign_ids;
mod backend;
mod codes;
mod compile;
mod decls;
mod diag;
mod explore;
mod format;
mod ident;
mod ir;
mod package;
mod pool;
mod program;
mod registry;
mod repl;
mod schema_file;
mod suggest;
mod syntax;
mod texts;
mod type_expand;
mod value;
mod vm;

pub use analysis::{Analysis, Reference, Span, Symbol, SymbolKind, analyze, is_name_char};
pub use assign_ids::{
    AUTO_ID_LEN, IdRng, Insertion, MissingId, apply as apply_insertions, plan as plan_ids,
    plan_program as plan_ids_program,
};
pub use codes::{CodeGroup, CodeInfo, ErrorCodeInfo, all_codes, core_command_error_codes};
pub use compile::{
    CompileOptions, LocalType, compile, compile_modules, compile_modules_with_options,
    compile_modules_with_warnings, compile_with_warnings, infer_local_types,
};
pub use decls::{DeclError, DeclErrorKind, MAX_DECLS_BYTES, MAX_JSON_DEPTH};
pub use diag::{DiagArg, DiagCode, Diagnostic, Severity};
pub use explore::{
    DEFAULT_MAX_STATES, ErrorPath, ExploreConfig, ExploreReport, ExploreStep, StuckPath, TestPath,
    explore,
};
pub use format::{FormatError, format_source};
pub use package::{
    CommandError, CommandErrorKind, Ctx, EffectFn, IntoReturn, MigrateError, Package, PackageReg,
    QueryFn, decode_slice, encode_slice,
};
pub use program::{Program, StopKind};
pub use registry::{
    CommandDecl, CommandId, CommandKind, FromArgs, FromValue, IntoValue, Kind, PackageDecl,
    PackageItem, Registry, RegistryError, StopDecl, Str, Sugar, VarDecl, VarHandle, YomiArgs,
    YomiDecl, YomiMap, YomiType,
};
pub use repl::{
    Applied, ApplyError, ApplyInvalid, Completeness, Declared, Def, InputKind, Name, Repl,
    ReplYield, Submitted, completeness,
};
pub use schema_file::{ExtractError, extract_schema, frozen_schema, migrate_template};
pub use syntax::keywords::{
    CONTEXTUAL_KEYWORDS, ContextualKeyword, ContextualPosition, KEYWORDS, Keyword, KeywordKind,
    is_reserved, keyword,
};
pub use texts::{TextError, TextErrorKind, TextIssue, TextIssueKind, TextTable};
pub use value::{
    EnumValue, FnValue, Identity, ListData, MAX_TYPE_DEPTH, MapData, MapEntries, StrValue,
    StructValue, Ty, TypeDecl, TypeKind, Value, VariantDecl, find_type,
};
pub use vm::{
    CueRef, DEFAULT_HISTORY_LIMIT, DEFAULT_SEED, DebugHook, DebugYield, Debugger, FrameInfo,
    HookAction, Input, InterruptHandle, LineEvent, LoadError, LoadErrorKind, LoadPolicy,
    LoadReport, PauseHandle, PendingStop, PersistentReport, ReloadError, ResolvedBy, Rng, Snapshot,
    SnapshotLog, StepMode, StopRecord, UnitKind, Variable, Vm, VmConfig, VmError, VmErrorKind,
    Yield, pending_stop,
};
pub use yomi_derive::YomiType;

/// The version of the core (the `version` in `Cargo.toml`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests;
