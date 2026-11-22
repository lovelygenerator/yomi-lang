//! The REPL core API (section 7.8 of `spec/host/07-repl.md`).
//!
//! A [`Repl`] runs expressions, statements, declarations and script lines on a **copy** of the script's environment. REPL
//! declarations (`fn`, `struct`, `enum`, `import`, `identity`) and variables (`let`) exist only inside the `Repl`. The
//! script's `Program` is not changed, and neither is the `Vm` unless [`Repl::apply`] is called.
//!
//! How it works: for each input, the `Repl` builds an extended program that adds a `<repl>` module (the REPL declarations
//! and an `<input>` label that runs the input) to the script's program (which keeps the compiled sources), swaps it into a
//! copy of the `Vm`, and runs it. REPL variables, and the local variables of a stop point entered with `:repl`, are held as
//! variables of the `<repl>` module. Input runs on a further copy of the copy (`Core`). On a runtime error or
//! [`Repl::abort`], that copy is simply discarded, returning to the state before the input.

mod classify;
#[cfg(test)]
mod tests;
mod types;

use std::collections::HashSet;
use std::sync::Arc;

use crate::compile::compile_modules;
use crate::compile::repl::{
    DeclEntry, InputAst, REPL_MODULE, ReplCompile, ReplVar, VarKind, compile_repl,
};
use crate::diag::{DiagArg, DiagCode, Diagnostic, Severity};
use crate::package::Package;
use crate::program::{ProgRef, Program};
use crate::registry::{CommandId, Kind, Registry};
use crate::syntax::ast::{Pos, TypeDeclKind};
use crate::syntax::parser::{
    ANY_IDENTITY, IdentMap, parse_expr_line, scan_identities, scan_imports,
};
use crate::value::{Ty, Value};
use crate::vm::InterruptHandle;
use crate::vm::repl::{ApplyFail, ApplyTarget, start_module};
use crate::vm::{Input, Variable, Vm, VmConfig, VmError, VmErrorKind, Yield};

use classify::{Parsed, classify};
pub use types::*;

/// The copied environment, and the REPL declarations and variables.
#[derive(Debug)]
struct Core<'a> {
    vm: Vm<'a>,
    /// Things declared in the REPL (in declaration order).
    decls: Vec<DeclEntry>,
    /// REPL variables (the variables of the `<repl>` module that come after the variables of the script's modules).
    vars: Vec<ReplVar>,
}

impl<'a> Core<'a> {
    /// Copies the execution state (for trial runs that can be undone on failure).
    fn fork(&self) -> Core<'a> {
        Core {
            vm: self.vm.fork(),
            decls: self.decls.clone(),
            vars: self.vars.clone(),
        }
    }
}

/// The input being run.
#[derive(Debug)]
struct Pending<'a> {
    core: Core<'a>,
    input_label: usize,
    expr_ty: Option<Ty>,
    lets: Vec<String>,
    declared: Vec<Declared>,
}

/// A REPL.
#[derive(Debug)]
pub struct Repl<'a> {
    registry: &'a Registry,
    /// The script's program (before declarations are added).
    base: ProgRef<'a>,
    /// The name of the current module.
    cur_module: Option<String>,
    /// The (committed) state before the input.
    core: Core<'a>,
    /// The state when started (or, after `apply` has applied changes, at that time). `reset` returns to it.
    baseline: Core<'a>,
    /// The input being run.
    pending: Option<Pending<'a>>,
    /// The number of the `attach`ed VM, and how many times it has advanced past stop points.
    attached: Option<(u64, u64)>,
}

/// Turns a misuse of the API into a `VmError`.
fn misuse(kind: VmErrorKind) -> VmError {
    VmError {
        kind,
        module: None,
        label: None,
        line: None,
    }
}

impl<'a> Repl<'a> {
    /// Starts with an empty environment without a script.
    pub fn empty(registry: &'a Registry, cfg: VmConfig) -> Result<Self, VmError> {
        let program = compile_modules(&[], registry).map_err(|_| misuse(VmErrorKind::Poisoned))?;
        let base = ProgRef::Shared(Arc::new(program));
        let vm = Vm::new_inner(base.clone(), registry, cfg, false)?;
        Self::build(registry, base, vm, None, Vec::new(), None)
    }

    /// Starts on a copy of the state at the start of the script's start label (before running). The start label may be
    /// missing.
    pub fn from_program(
        program: &'a Program,
        registry: &'a Registry,
        cfg: VmConfig,
    ) -> Result<Self, VmError> {
        let base = ProgRef::Borrowed(program);
        let cur = start_module(program, &cfg.start).map(|i| program.modules[i].clone());
        let vm = Vm::new_inner(base.clone(), registry, cfg, false)?;
        Self::build(registry, base, vm, cur, Vec::new(), None)
    }

    /// Starts on a copy of a stopped VM. `vm` is not changed (until `apply` is called). `Err` if it is not stopped.
    pub fn attach(vm: &Vm<'a>) -> Result<Self, VmError> {
        if !vm.is_stopped() {
            return Err(misuse(VmErrorKind::NotStopped));
        }
        let base = vm.program_ref().clone();
        let (cur, locals) = Self::stop_context(&base, vm);
        Self::build(
            vm.registry(),
            base,
            vm.fork(),
            Some(cur),
            locals,
            Some(vm.identity()),
        )
    }

    /// The name of the module of the stop point, and the label's local variables (excluding hidden `for` variables; for
    /// duplicate names, the later one).
    fn stop_context(base: &ProgRef<'a>, vm: &Vm<'_>) -> (String, Vec<(String, Ty, Value)>) {
        let code = &base.labels[vm.label_index()];
        let module = base.modules[code.module as usize].clone();
        let mut locals: Vec<(String, Ty, Value)> = Vec::new();
        for (l, v) in code.locals.iter().zip(vm.raw_locals()) {
            // Variables of function types are not used at the stop point (their values may refer to an old program)
            if l.name.starts_with('$') || l.ty.contains_fn() {
                continue;
            }
            match locals.iter_mut().find(|(n, _, _)| *n == l.name) {
                Some(slot) => *slot = (l.name.clone(), l.ty.clone(), v.clone()),
                None => locals.push((l.name.clone(), l.ty.clone(), v.clone())),
            }
        }
        (module, locals)
    }

    /// Builds the environment that runs REPL input.
    fn build(
        registry: &'a Registry,
        base: ProgRef<'a>,
        vm: Vm<'a>,
        cur_module: Option<String>,
        locals: Vec<(String, Ty, Value)>,
        attached: Option<(u64, u64)>,
    ) -> Result<Self, VmError> {
        let core = Self::initial_core(registry, &base, vm, cur_module.as_deref(), locals)?;
        let baseline = core.fork();
        Ok(Self {
            registry,
            base,
            cur_module,
            core,
            baseline,
            pending: None,
            attached,
        })
    }

    /// The initial environment without declarations (the local variables of the stop point are held as REPL variables).
    fn initial_core(
        registry: &'a Registry,
        base: &ProgRef<'a>,
        mut vm: Vm<'a>,
        cur_module: Option<&str>,
        locals: Vec<(String, Ty, Value)>,
    ) -> Result<Core<'a>, VmError> {
        let vars: Vec<ReplVar> = locals
            .iter()
            .map(|(n, t, _)| ReplVar {
                name: n.clone(),
                ty: t.clone(),
                kind: VarKind::StopLocal,
            })
            .collect();
        let input = InputAst::Stmts(Vec::new());
        let rc = ReplCompile {
            cur_module,
            decls: &[],
            vars: &vars,
            input: &input,
        };
        let (program, outcome, _) =
            compile_repl(base, registry, &rc).map_err(|_| misuse(VmErrorKind::Poisoned))?;
        let repl_idx = base.modules.len() as u32;
        let cur = cur_module
            .and_then(|m| base.modules.iter().position(|x| x == m))
            .map(|c| (repl_idx, c as u32));
        vm.set_program(ProgRef::Shared(Arc::new(program)), cur)?;
        let n_base = base.globals.len();
        for (i, (_, _, v)) in locals.into_iter().enumerate() {
            vm.set_global(n_base + i, v);
        }
        Ok(Core {
            vm,
            decls: Vec::new(),
            vars: outcome.vars,
        })
    }

    /// The table of identities used to parse input (identities of the current module and of `import`ed modules, including
    /// REPL declarations).
    fn idents(&self) -> IdentMap {
        let mut map = IdentMap::new();
        for (name, src) in self.base.modules.iter().zip(&self.base.sources) {
            map.insert(name.clone(), scan_identities(src));
        }
        let mut usable: HashSet<String> = HashSet::new();
        let mut imports: Vec<String> = Vec::new();
        if let Some(cur) = &self.cur_module {
            if let Some(set) = map.get(cur) {
                usable.extend(set.iter().cloned());
            }
            if let Some(i) = self.base.modules.iter().position(|m| m == cur) {
                imports.extend(scan_imports(&self.base.sources[i]));
            }
        }
        for d in &self.core.decls {
            match d {
                DeclEntry::Import(i) => imports.push(i.path.clone()),
                DeclEntry::Identity(x) => {
                    usable.insert(x.name.clone());
                }
                _ => {}
            }
        }
        for m in imports {
            if let Some(set) = map.get(&m) {
                usable.extend(set.iter().cloned());
            }
        }
        map.insert(REPL_MODULE.to_string(), usable);
        map
    }

    /// Type-checks input and prepares to run it (does not run it).
    ///
    /// If the previous input has not finished (before `Done`, a runtime error or `abort`), returns `Err` without diagnostics.
    /// When diagnostics are returned, the state of the `Repl` is unchanged.
    pub fn submit(&mut self, input: &str) -> Result<Submitted, Vec<Diagnostic>> {
        if self.pending.is_some() {
            return Err(Vec::new());
        }
        let idents = self.idents();
        let parsed = classify(input, &idents, true)?;
        let mut cand = self.core.decls.clone();
        let mut new_names: Vec<Declared> = Vec::new();
        let mut replaced: Vec<(String, Pos)> = Vec::new();
        let (ast, declaration) = match parsed {
            Parsed::Empty => (InputAst::Stmts(Vec::new()), false),
            Parsed::Command => {
                let pos = Pos { line: 1, col: 1 };
                return Err(vec![Diagnostic {
                    module: REPL_MODULE.to_string(),
                    line: pos.line,
                    col: pos.col,
                    severity: Severity::Error,
                    code: DiagCode::UnexpectedToken,
                    args: vec![("found", DiagArg::Str(":".to_string()))],
                }]);
            }
            Parsed::Expr(e) => (InputAst::Expr(e), false),
            Parsed::Stmts(b) => (InputAst::Stmts(b), false),
            Parsed::Decls(entries) => {
                for e in entries {
                    new_names.push(match &e {
                        DeclEntry::Import(i) => Declared::Import { name: i.shown() },
                        DeclEntry::Identity(i) => Declared::Identity {
                            name: i.name.clone(),
                        },
                        DeclEntry::Type(t) => Declared::Type {
                            name: t.name.clone(),
                        },
                        DeclEntry::Fn(f) => Declared::Fn {
                            name: f.name.clone(),
                        },
                    });
                    match &e {
                        // A REPL `fn` with the same name replaces the body (only when the types are the same; checked later)
                        DeclEntry::Fn(f) => {
                            let old = cand
                                .iter()
                                .position(|d| matches!(d, DeclEntry::Fn(g) if g.name == f.name));
                            match old {
                                Some(i) => {
                                    replaced.push((f.name.clone(), f.pos));
                                    cand[i] = e;
                                }
                                None => cand.push(e),
                            }
                        }
                        // Writing the same `import` again does nothing
                        DeclEntry::Import(i) => {
                            let dup = cand
                                .iter()
                                .any(|d| matches!(d, DeclEntry::Import(g) if g.path == i.path && g.local == i.local));
                            if !dup {
                                cand.push(e);
                            }
                        }
                        _ => cand.push(e),
                    }
                }
                (InputAst::Stmts(Vec::new()), true)
            }
        };
        let rc = ReplCompile {
            cur_module: self.cur_module.as_deref(),
            decls: &cand,
            vars: &self.core.vars,
            input: &ast,
        };
        let (program, outcome, diags) = match compile_repl(&self.base, self.registry, &rc) {
            Ok(x) => x,
            Err(d) => return Err(only_repl(d)),
        };
        // A `fn` can be replaced only when its parameter and return types are the same
        for (name, pos) in &replaced {
            if fn_signature(self.core.vm.program_ref(), name) != fn_signature(&program, name) {
                return Err(vec![Diagnostic {
                    module: REPL_MODULE.to_string(),
                    line: pos.line,
                    col: pos.col,
                    severity: Severity::Error,
                    code: DiagCode::DuplicateName,
                    args: vec![("name", DiagArg::Str(name.clone()))],
                }]);
            }
        }
        let warnings = only_repl(diags);
        let kind = match (&ast, declaration) {
            (InputAst::Expr(_), _) => {
                InputKind::Expression(outcome.input_ty.clone().unwrap_or(Ty::Unit))
            }
            (_, true) => InputKind::Declaration,
            _ => InputKind::Statement,
        };
        let mut core = self.core.fork();
        let repl_idx = self.base.modules.len() as u32;
        let cur = self
            .cur_module
            .as_deref()
            .and_then(|m| self.base.modules.iter().position(|x| x == m))
            .map(|c| (repl_idx, c as u32));
        let program = Arc::new(program);
        core.vm
            .set_program(ProgRef::Shared(program.clone()), cur)
            .map_err(|_| Vec::new())?;
        let n_base = self.base.globals.len();
        for (i, v) in outcome.vars.iter().enumerate() {
            let changed = self.core.vars.get(i).is_none_or(|o| o.ty != v.ty);
            if changed {
                core.vm
                    .set_global(n_base + i, v.ty.default_in(program.types()));
            }
        }
        core.decls = cand;
        core.vars = outcome.vars.clone();
        core.vm.begin_input(outcome.input_label);
        self.pending = Some(Pending {
            core,
            input_label: outcome.input_label,
            expr_ty: outcome.input_ty.clone(),
            lets: outcome.lets.clone(),
            declared: new_names,
        });
        Ok(Submitted { kind, warnings })
    }

    /// Runs the prepared input until the next stop point (or the end).
    ///
    /// On a runtime error, the input is discarded (the `Repl` returns to the state before the input, and `submit` can be
    /// called again).
    pub fn run(&mut self) -> Result<ReplYield, VmError> {
        let Some(p) = self.pending.as_mut() else {
            return Err(misuse(VmErrorKind::Ended));
        };
        match p.core.vm.run() {
            Ok(Yield::Stop { command, args, id }) => Ok(ReplYield::Stop { command, args, id }),
            Ok(_) => {
                let p = self.pending.take().expect("実行中の入力");
                Ok(self.finish(p))
            }
            Err(e) => {
                // If `run` was called while stopped (a misuse), keep the input. Otherwise, discard it
                if e.kind != VmErrorKind::AlreadyStopped {
                    self.pending = None;
                }
                Err(e)
            }
        }
    }

    /// The result when running the input has finished. Commits the state.
    fn finish(&mut self, mut p: Pending<'a>) -> ReplYield {
        let ended = p.core.vm.label_index() != p.input_label;
        let value = match (&p.expr_ty, ended) {
            (Some(ty), false) => p.core.vm.stack_top().map(|v| (v, ty.clone())),
            _ => None,
        };
        p.core.vm.clear_stack();
        let n_base = self.base.globals.len();
        let mut declared: Vec<Declared> = Vec::new();
        for name in &p.lets {
            if let Some(i) = p.core.vars.iter().position(|v| v.name == *name) {
                declared.push(Declared::Let {
                    name: name.clone(),
                    value: p.core.vm.raw_globals()[n_base + i].clone(),
                    ty: p.core.vars[i].ty.clone(),
                });
            }
        }
        declared.extend(std::mem::take(&mut p.declared));
        self.core = p.core;
        ReplYield::Done {
            value,
            declared,
            ended,
        }
    }

    /// Resumes stopped input with the return value of the stop command.
    ///
    /// Misuse such as a return value of the wrong type returns `Err` and stays stopped. On a runtime error, the input is
    /// discarded.
    pub fn resume(&mut self, input: Input) -> Result<(), VmError> {
        let Some(p) = self.pending.as_mut() else {
            return Err(misuse(VmErrorKind::NotStopped));
        };
        let r = p.core.vm.resume(input);
        if r.is_err() && p.core.vm.is_poisoned() {
            self.pending = None;
        }
        r
    }

    /// Discards input that is running (including stopped). The `Repl` returns to the state before the input.
    pub fn abort(&mut self) {
        self.pending = None;
    }

    /// Reads the state slice of package `P` (read only). While input is running, reads that input's state; otherwise, the
    /// state of the committed copy. Used by the host after `run` to read and show cues for sound and images. `None` if `P`
    /// is not registered.
    pub fn package_state<P: Package>(&self) -> Option<&P::State> {
        match &self.pending {
            Some(p) => p.core.vm.package_state::<P>(),
            None => self.core.vm.package_state::<P>(),
        }
    }

    /// Finds the type of an expression (does not evaluate it). `Err` if it is not an expression.
    pub fn type_of(&self, input: &str) -> Result<Ty, Vec<Diagnostic>> {
        let text = input.trim_end();
        if text.contains('\n') {
            return Err(vec![Diagnostic {
                module: REPL_MODULE.to_string(),
                line: 1,
                col: 1,
                severity: Severity::Error,
                code: DiagCode::UnexpectedToken,
                args: vec![("found", DiagArg::Str("\n".to_string()))],
            }]);
        }
        let e = parse_expr_line(text, REPL_MODULE, 1).map_err(|d| vec![d])?;
        let ast = InputAst::Expr(e);
        let rc = ReplCompile {
            cur_module: self.cur_module.as_deref(),
            decls: &self.core.decls,
            vars: &self.core.vars,
            input: &ast,
        };
        match compile_repl(&self.base, self.registry, &rc) {
            Ok((_, outcome, _)) => Ok(outcome.input_ty.unwrap_or(Ty::Unit)),
            Err(d) => Err(only_repl(d)),
        }
    }

    /// The list of variables of the copy: script module variables, host-declared variables, local variables of the stop point
    /// (`local` is true), and REPL `let` variables, in that order. REPL `let` variables have `module` `Some("<repl>")`.
    pub fn variables(&self) -> Vec<Variable> {
        let vm = &self.core.vm;
        let n_base = self.base.globals.len();
        let mut all = vm.module_and_host_variables();
        let host = all.split_off(vm.raw_globals().len());
        let repl_vars = all.split_off(n_base);
        let mut out = all;
        out.extend(host);
        for (v, var) in repl_vars.iter().zip(&self.core.vars) {
            if var.kind == VarKind::StopLocal {
                out.push(Variable {
                    module: None,
                    local: true,
                    ..v.clone()
                });
            }
        }
        for (v, var) in repl_vars.into_iter().zip(&self.core.vars) {
            if var.kind == VarKind::Let {
                out.push(v);
            }
        }
        out
    }

    /// The `fn`s, types, `import`s and `identity`s declared in the REPL (in declaration order).
    pub fn defs(&self) -> Vec<Def> {
        let program = self.core.vm.program_ref();
        let repl_idx = self.base.modules.len() as u32;
        self.core
            .decls
            .iter()
            .map(|d| match d {
                DeclEntry::Import(i) => Def::Import { name: i.shown() },
                DeclEntry::Identity(i) => Def::Identity {
                    name: i.name.clone(),
                },
                DeclEntry::Type(t) => match t.kind {
                    TypeDeclKind::Struct(_) => Def::Struct {
                        name: t.name.clone(),
                    },
                    TypeDeclKind::Enum(_) => Def::Enum {
                        name: t.name.clone(),
                    },
                },
                DeclEntry::Fn(f) => {
                    let code = program
                        .fns
                        .iter()
                        .find(|c| c.module == repl_idx && c.name == f.name);
                    Def::Fn {
                        name: f.name.clone(),
                        params: code
                            .map(|c| {
                                c.locals[..c.params as usize]
                                    .iter()
                                    .map(|l| (l.name.clone(), l.ty.clone()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        ret: code.and_then(|c| c.ret.clone()),
                    }
                }
            })
            .collect()
    }

    /// The token that interrupts a running input from another thread (`spec/host/01-execution.md`, 1.9a). An interrupted input
    /// is discarded like any other runtime error.
    pub fn interrupt_handle(&self) -> InterruptHandle {
        self.core.vm.interrupt_handle()
    }

    /// Discards the REPL declarations and returns the copy to the state when started (or, after `apply`, at that time).
    pub fn reset(&mut self) -> Result<(), VmError> {
        self.pending = None;
        self.core = self.baseline.fork();
        Ok(())
    }

    /// Applies the variables changed in the copy to the `attach`ed `vm` (section 7.3.6 of `spec/host/07-repl.md`). Either
    /// all are applied or none.
    ///
    /// Only `save var`s, `Kind::Save` variables and local variables of the stop point whose values differ are applied.
    /// `persistent var`s are not applied and are listed in `Applied::skipped`. A `Repl` made with `from_program` or `empty`,
    /// and one whose input is running, gives `NotAttached`.
    pub fn apply(&mut self, vm: &mut Vm<'a>) -> Result<Applied, ApplyError> {
        let Some(att) = self.attached else {
            return Err(ApplyError::NotAttached);
        };
        if self.pending.is_some() {
            return Err(ApplyError::NotAttached);
        }
        if vm.identity() != att || !vm.is_stopped() {
            return Err(ApplyError::VmChanged);
        }
        let n_base = self.base.globals.len();
        let mine = &self.core.vm;
        let mut changes: Vec<(ApplyTarget, Value)> = Vec::new();
        let mut skipped: Vec<Name> = Vec::new();
        for (i, g) in self.base.globals.iter().enumerate() {
            let (a, b) = (&mine.raw_globals()[i], &vm.raw_globals()[i]);
            if a == b {
                continue;
            }
            if g.persistent {
                skipped.push(Name {
                    module: Some(self.base.modules[g.module as usize].clone()),
                    name: g.name.clone(),
                });
            } else {
                changes.push((ApplyTarget::Global(i), a.clone()));
            }
        }
        for (i, d) in self.registry.vars().iter().enumerate() {
            let (a, b) = (&mine.raw_host_vars()[i], &vm.raw_host_vars()[i]);
            if a == b {
                continue;
            }
            match d.kind {
                Kind::Save => changes.push((ApplyTarget::Host(i), a.clone())),
                _ => skipped.push(host_name(&d.name)),
            }
        }
        let code = &self.base.labels[vm.label_index()];
        for (j, var) in self.core.vars.iter().enumerate() {
            if var.kind != VarKind::StopLocal {
                continue;
            }
            let Some(slot) = code.locals.iter().rposition(|l| l.name == var.name) else {
                continue;
            };
            let a = &mine.raw_globals()[n_base + j];
            if *a != vm.raw_locals()[slot] {
                changes.push((ApplyTarget::Local(slot), a.clone()));
            }
        }
        if changes.is_empty() {
            return Ok(Applied {
                variables: Vec::new(),
                skipped,
            });
        }
        let name_of = |vm: &Vm<'a>, t: ApplyTarget| {
            let v = vm.variable_of(t);
            Name {
                module: v.module,
                name: v.name,
            }
        };
        if let Err(fail) = vm.apply_values(&changes) {
            let invalid = |i: usize, reason| ApplyError::Invalid {
                name: name_of(vm, changes[i].0),
                reason,
            };
            return Err(match fail {
                ApplyFail::Type(i) => invalid(i, ApplyInvalid::Type),
                ApplyFail::ValueTooLarge(i) => invalid(i, ApplyInvalid::ValueTooLarge),
                ApplyFail::MemoryLimit(i) => invalid(i, ApplyInvalid::MemoryLimit),
                ApplyFail::NotSavable(i) => invalid(i, ApplyInvalid::NotSavable),
                ApplyFail::StopArgs(e) => ApplyError::StopArgs(e),
            });
        }
        let variables = changes.iter().map(|(t, _)| vm.variable_of(*t)).collect();
        // Make the base of `reset` a copy of the script after applying (the REPL declarations are discarded)
        let (cur, locals) = Self::stop_context(&self.base, vm);
        if let Ok(core) =
            Self::initial_core(self.registry, &self.base, vm.fork(), Some(&cur), locals)
        {
            self.baseline = core;
        }
        Ok(Applied { variables, skipped })
    }
}

/// The name of a host-declared variable (`namespace.variable`).
fn host_name(full: &str) -> Name {
    let (ns, name) = full.split_once('.').unwrap_or(("", full));
    Name {
        module: Some(ns.to_string()),
        name: name.to_string(),
    }
}

/// Keeps only the diagnostics of the REPL module (warnings of the script's modules were handled when the script was
/// compiled).
fn only_repl(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| d.module == REPL_MODULE)
        .collect()
}

/// The parameter and return types of a REPL `fn`.
fn fn_signature(program: &Program, name: &str) -> Option<(Vec<Ty>, Option<Ty>)> {
    let repl_idx = program.modules.iter().position(|m| m == REPL_MODULE)? as u32;
    program
        .fns
        .iter()
        .find(|c| c.module == repl_idx && c.name == name)
        .map(|c| {
            (
                c.locals[..c.params as usize]
                    .iter()
                    .map(|l| l.ty.clone())
                    .collect(),
                c.ret.clone(),
            )
        })
}
