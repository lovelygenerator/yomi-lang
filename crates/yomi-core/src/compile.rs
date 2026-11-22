//! Compilation: turns the syntax tree into bytecode while checking types.

use std::collections::{HashMap, HashSet};

use crate::backend::Backend as _;
use crate::backend::register::{MAX_REGS, RegBackend, TooManyRegisters};
use crate::diag::{DiagArg, DiagCode, Diagnostic, Severity};
use crate::ir::{
    Body, Builtin, Choice, ChoiceOpt as IrChoiceOpt, ExprId, ExprKind, ForKind, IfArm, Ir,
    MatchArm as IrArm, Method, Operand, PatTest, StmtId, StmtKind, Stop, StopArg, Tail, TextPiece,
};
use crate::program::{
    Action, ArithOp, CallSite, ChoiceInfo, ChoiceOptInfo, FnCode, GlobalInfo, IdSite, IdentityInfo,
    LabelCode, LocalInfo, PathStep, PlaceRef, Program, RegCode, StopInfo, StopKind, TextArg,
    TextPart, fnv1a,
};
use crate::registry::{Binding, CommandKind, Registry};
use crate::suggest::suggest;
use crate::syntax::ast::{
    AssignOp, BinOp, ChoiceOpt, CommandCall, Expr, FieldInit, IdentRef, IdentityDecl, LabelCall,
    LabelDecl, LabelRef, MatchArm, Module, PathElem, Pattern, Pos, StableId, Stmt, StrPart, TyName,
    TypeDeclKind, TypeRef, UnOp, VarDecl,
};
use crate::syntax::keywords::is_reserved;
use crate::syntax::parser::{IdentMap, parse_with, scan_identities};
use crate::value::{
    Identity, MAX_TYPE_DEPTH, Ty, TypeDecl, TypeKind, Value, VariantDecl, find_type, finite_float,
};

/// Compiles the source of one module. Diagnostics are returned in position order. If there is any error, `Err`.
///
/// Warnings are dropped. To receive warnings as well, use [`compile_with_warnings`]. For multiple modules, use
/// [`compile_modules`].
pub fn compile(
    source: &str,
    module: &str,
    registry: &Registry,
) -> Result<Program, Vec<Diagnostic>> {
    compile_with_warnings(source, module, registry).map(|(program, _)| program)
}

/// Compiles the source of one module. On success, returns the program and the warnings (in position order).
///
/// If there is any error, `Err`. In that case the diagnostics include the warnings too (in position order).
pub fn compile_with_warnings(
    source: &str,
    module: &str,
    registry: &Registry,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_with_warnings(&[(module, source)], registry)
}

/// Compiles multiple modules (`(module name, source)`) into one program.
///
/// Module names are identifiers and must not collide. Reading files is done by the host (the core does not read files).
/// Cyclic `import`s can be written, because they are resolved after all modules are read.
/// Diagnostics are returned in the order the modules were given, and in position order within each. Warnings are dropped.
/// To receive warnings as well, use [`compile_modules_with_warnings`].
pub fn compile_modules(
    modules: &[(&str, &str)],
    registry: &Registry,
) -> Result<Program, Vec<Diagnostic>> {
    compile_modules_with_warnings(modules, registry).map(|(program, _)| program)
}

/// Compile options. Built from `Default` with methods.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CompileOptions {
    /// Labels that the host starts from outside (`module.label`; if there is only one module, just the label name also works).
    /// They are excluded from the warning `label_falls_through` (spec 4.5). When empty, labels named `start` are excluded as
    /// start labels (the default of `VmConfig::start`).
    pub entry_labels: Vec<String>,
    /// Makes plain lines, quote lines and option texts without a stable ID a compile error (`missing_text_id`). Text
    /// replacement (localization; `spec/host/05-texts.md`) is keyed by stable IDs, so enable this for scripts that will be
    /// translated. Disabled by default. It is checked as a writing rule even with a registry that ignores the sugar.
    pub require_text_ids: bool,
    /// The content of `migrations.yomi` (`spec/language/07-migrate.md`).
    pub migrations: Option<String>,
    /// Frozen schema files (path such as `schema/v2.yomi`, content).
    pub frozen_schemas: Vec<(String, String)>,
}

impl CompileOptions {
    /// Sets whether text lines without a stable ID are errors ([`CompileOptions::require_text_ids`]).
    pub fn require_text_ids(mut self, on: bool) -> Self {
        self.require_text_ids = on;
        self
    }

    /// Sets the content of `migrations.yomi`. It can be given once.
    pub fn migrations(mut self, source: &str) -> Self {
        self.migrations = Some(source.to_string());
        self
    }

    /// Adds a frozen schema file. `path` is relative to the project root, like `schema/v2.yomi`.
    pub fn frozen_schema_file(mut self, path: &str, source: &str) -> Self {
        self.frozen_schemas
            .push((path.to_string(), source.to_string()));
        self
    }

    /// Adds one start label (`module.label`).
    pub fn entry_label(mut self, label: &str) -> Self {
        self.entry_labels.push(label.to_string());
        self
    }
}

/// The type of a local variable (bound by `let`, `for` or `match`) as decided by the compiler. For editors and tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalType {
    /// Module name.
    pub module: String,
    /// The line of the name (1-based).
    pub line: u32,
    /// The column of the name (1-based, in characters).
    pub col: u32,
    /// The type (`int`, `list<str>`, `module.TypeName`).
    pub ty: String,
}

/// Finds the types of local variables. Even if compilation fails, returns those whose types were decided.
pub fn infer_local_types(modules: &[(&str, &str)], registry: &Registry) -> Vec<LocalType> {
    let mut types = Vec::new();
    let _ = compile_modules_core(
        modules,
        registry,
        &CompileOptions::default(),
        &mut Vec::new(),
        &mut types,
        None,
        &mut None,
    );
    types
}

/// A version of [`compile_modules`] that also returns warnings. When it fails with errors, the diagnostics include the
/// warnings too.
///
/// Uses the default options ([`CompileOptions::default`]). With options, use [`compile_modules_with_options`].
pub fn compile_modules_with_warnings(
    modules: &[(&str, &str)],
    registry: &Registry,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_with_options(modules, registry, &CompileOptions::default())
}

/// A version of [`compile_modules_with_warnings`] with options.
pub fn compile_modules_with_options(
    modules: &[(&str, &str)],
    registry: &Registry,
    options: &CompileOptions,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_impl(modules, registry, options, &mut Vec::new())
}

/// The result of compilation, and the lines that can have stable IDs when it failed.
pub(crate) type SitedResult = (
    Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>>,
    Vec<IdSite>,
);

/// In addition to [`compile_modules_with_options`], also returns the lines that can have stable IDs when compilation
/// fails (used by `assign-ids` to find where to add IDs when the only errors are caused by missing IDs). Empty on success.
pub(crate) fn compile_modules_with_sites(
    modules: &[(&str, &str)],
    registry: &Registry,
    options: &CompileOptions,
) -> SitedResult {
    let mut failed_sites = Vec::new();
    let result = compile_modules_impl(modules, registry, options, &mut failed_sites);
    (result, failed_sites)
}

/// The body that compiles multiple modules in order. On success, the program and warnings. On failure, the diagnostics
/// (errors and warnings). When compilation fails, adds the lines that can have stable IDs to `failed_sites` (for
/// `assign-ids`).
fn compile_modules_impl(
    modules: &[(&str, &str)],
    registry: &Registry,
    options: &CompileOptions,
    failed_sites: &mut Vec<IdSite>,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_core(
        modules,
        registry,
        options,
        failed_sites,
        &mut Vec::new(),
        None,
        &mut None,
    )
}

impl Compiler<'_> {
    /// The table of declarations of the module being read.
    fn md(&self) -> &ModDecl {
        &self.modules[self.cur_mod]
    }

    /// Returns the declarations of the current module in a mutable form.
    fn md_mut(&mut self) -> &mut ModDecl {
        &mut self.modules[self.cur_mod]
    }

    /// Adds a diagnostic (error or warning) with its position, code and arguments.
    fn diag(
        &mut self,
        severity: Severity,
        pos: Pos,
        code: DiagCode,
        args: Vec<(&'static str, DiagArg)>,
    ) {
        self.diags.push(Diagnostic {
            module: self.modules[self.cur_mod].name.clone(),
            line: pos.line,
            col: pos.col,
            severity,
            code,
            args,
        });
    }

    /// Adds an error diagnostic.
    fn error(&mut self, pos: Pos, code: DiagCode, args: Vec<(&'static str, DiagArg)>) {
        self.diag(Severity::Error, pos, code, args);
    }

    /// A type mismatch error (`expected` and `found` are type names).
    fn type_mismatch(&mut self, pos: Pos, expected: &str, found: &str) {
        self.error(
            pos,
            DiagCode::TypeMismatch,
            vec![("expected", s(expected)), ("found", s(found))],
        );
    }

    /// An error for a wrong number of arguments.
    fn arg_count_error(&mut self, name: &str, expected: usize, found: usize, pos: Pos) {
        self.error(
            pos,
            DiagCode::ArgCount,
            vec![
                ("name", s(name)),
                ("expected", DiagArg::Int(expected as i64)),
                ("found", DiagArg::Int(found as i64)),
            ],
        );
    }

    /// An error whose only argument is a name (`name`).
    fn name_error(&mut self, pos: Pos, code: DiagCode, name: &str) {
        self.error(pos, code, vec![("name", s(name))]);
    }

    /// Compares the type of an argument with the expected type. Does nothing if either is unknown (the error is already
    /// reported).
    fn check_arg_type(&mut self, arg: &Expr, want: Option<&Ty>, got: Option<&Ty>) {
        if let (Some(w), Some(g)) = (want, got)
            && g != w
        {
            self.type_mismatch(arg.pos(), &w.name(), &g.name());
        }
    }

    /// Adds a warning diagnostic. Warnings about unreachable patterns at the same position are not repeated.
    fn warn(&mut self, pos: Pos, code: DiagCode, args: Vec<(&'static str, DiagArg)>) {
        // Even if a pattern is unreachable for two reasons (already exhaustive, or a repeated pattern), there is only one warning
        if code == DiagCode::UnreachablePattern
            && self.diags.iter().any(|d| {
                d.code == code
                    && d.line == pos.line
                    && d.col == pos.col
                    && d.module == self.modules[self.cur_mod].name
            })
        {
            return;
        }
        self.diag(Severity::Warning, pos, code, args);
    }

    /// The warning for declaring a variable with the same name as a built-in function.
    fn warn_if_builtin(&mut self, name: &str, pos: Pos) {
        if BUILTIN_NAMES.contains(&name) {
            self.warn(pos, DiagCode::ShadowedBuiltin, vec![("name", s(name))]);
        }
    }

    /// Returns the state of the label being compiled (not called outside a label).
    fn ctx(&mut self) -> &mut LabelCtx {
        self.cur.as_mut().expect("ラベルの中")
    }

    /// Makes an IR expression.
    fn node(&mut self, kind: ExprKind, line: u32) -> ExprId {
        self.ctx().ir.expr(kind, line)
    }

    /// An expression that produced a diagnostic (not turned into code).
    fn error_node(&mut self) -> ExprId {
        self.node(ExprKind::Error, 0)
    }

    /// Makes an IR statement and adds it to `out`.
    fn push_stmt(&mut self, out: &mut Vec<StmtId>, kind: StmtKind, line: u32) {
        let id = self.ctx().ir.stmt(kind, line);
        out.push(id);
    }

    /// The stop point information of a stop point expression.
    fn stop_info_mut(&mut self, id: ExprId) -> &mut StopInfo {
        match &mut self.ctx().ir.exprs[id].kind {
            ExprKind::Stop(stop) => &mut stop.info,
            other => unreachable!("停止点の式のはず: {other:?}"),
        }
    }
}

mod body;
mod call;
mod command;
mod declare;
mod expr;
mod lambda;
mod migrate;
pub(crate) use migrate::signature as signature_of;
mod model;
mod modules;
mod pattern;
pub(crate) mod repl;
mod shape;
mod stmt;
mod text;
mod types;

use crate::program::MigrateCode;
use model::*;
use modules::*;
use shape::*;

#[cfg(test)]
mod tests;
