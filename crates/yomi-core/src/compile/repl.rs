//! Compiling REPL input (`spec/host/07-repl.md`).
//!
//! Compiles with a `<repl>` module added after the script's modules. The `<repl>` module inherits the name table of the
//! current module (for `:repl`, the module of the stop point's label), and adds to it the `fn`s, types, `import`s and
//! identities declared in the REPL, plus the input as an `<input>` label. REPL variables (`let` and the local variables of
//! the stop point) become variables of the `<repl>` module (`Variable::module` is `<repl>`).

use std::collections::{HashMap, HashSet};

use super::Compiler;
use crate::diag::{DiagCode, Diagnostic};
use crate::ir::ExprId;
use crate::program::Program;
use crate::registry::Registry;
use crate::syntax::ast::{Expr, IdentityDecl, Import, LabelDecl, Module, Pos, Stmt, TypeDeclAst};
use crate::value::Ty;

/// The name of the REPL module. It is not an identifier, so it does not collide with the names of the script's modules.
pub(crate) const REPL_MODULE: &str = "<repl>";

/// The name of the label that runs input. It is not an identifier, so it does not collide with the script's labels.
pub(crate) const INPUT_LABEL: &str = "<input>";

/// One thing declared in the REPL (kept in declaration order).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DeclEntry {
    Import(Import),
    Identity(IdentityDecl),
    Type(TypeDeclAst),
    Fn(LabelDecl),
}

impl DeclEntry {
    /// The position of the declaration (within the input).
    pub(crate) fn pos(&self) -> Pos {
        match self {
            DeclEntry::Import(i) => i.pos,
            DeclEntry::Identity(i) => i.pos,
            DeclEntry::Type(t) => t.pos,
            DeclEntry::Fn(f) => f.pos,
        }
    }
}

/// The kind of a REPL variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarKind {
    /// A variable `let` in the REPL.
    Let,
    /// A local variable (parameter or `let`) of the label of the stop point entered with `:repl`.
    StopLocal,
}

/// One REPL variable. The sequential number across the whole program is decided by the order in the `list` of variables of
/// the `<repl>` module (following the variables of the script's modules).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplVar {
    pub name: String,
    pub ty: Ty,
    pub kind: VarKind,
}

/// The syntax tree of input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InputAst {
    /// One expression (returns a value).
    Expr(Expr),
    /// A sequence of statements (empty for input with only declarations).
    Stmts(Vec<Stmt>),
}

/// The REPL state and input passed to compilation.
pub(crate) struct ReplCompile<'r> {
    /// The current module (a script module name). `None` if none.
    pub cur_module: Option<&'r str>,
    /// Things declared in the REPL (in declaration order).
    pub decls: &'r [DeclEntry],
    /// REPL variables (in sequential number order).
    pub vars: &'r [ReplVar],
    /// The input.
    pub input: &'r InputAst,
}

/// The parts of the compilation result that are given outside the program.
#[derive(Debug, Clone)]
pub(crate) struct ReplOutcome {
    /// The sequential number of the `<input>` label across the whole program.
    pub input_label: usize,
    /// The type of expression input (`None` if not an expression).
    pub input_ty: Option<Ty>,
    /// REPL variables (reflecting the input's `let`s; in sequential number order).
    pub vars: Vec<ReplVar>,
    /// The names of variables `let` at the top level of the input (in the order written).
    pub lets: Vec<String>,
}

/// The REPL state held by the compiler.
pub(crate) struct ReplState {
    /// The index of the `<repl>` module.
    pub idx: usize,
    /// The index of the current module.
    pub cur: Option<usize>,
    /// REPL variable name → (sequential number, type).
    vars: HashMap<String, (u32, Ty)>,
    /// REPL variables (in sequential number order).
    list: Vec<ReplVar>,
    /// The number of variables of the script's modules (where the sequential numbers of REPL variables start).
    base_globals: u32,
    /// The names of top-level `let`s of the input.
    lets: Vec<String>,
    /// Expression input.
    expr: Option<Expr>,
    expr_ty: Option<Ty>,
}

/// The stage that inherits the name table from the current module.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Seed {
    Imports,
    Types,
    EarlyIdentities,
    Decls,
}

impl ReplState {
    /// Makes the compilation state for one REPL input (`idx` is the number of the input label, `cur` is the original
    /// module).
    pub(super) fn new(rc: &ReplCompile<'_>, idx: usize, cur: Option<usize>) -> Self {
        let expr = match rc.input {
            InputAst::Expr(e) => Some(e.clone()),
            InputAst::Stmts(_) => None,
        };
        Self {
            idx,
            cur,
            vars: HashMap::new(),
            list: rc.vars.to_vec(),
            base_globals: 0,
            lets: Vec::new(),
            expr,
            expr_ty: None,
        }
    }

    /// Gathers the state into the compilation result (the input label, the type of the expression, the list of variables, the
    /// names of `let`s).
    pub(super) fn finish(self, input_label: usize) -> ReplOutcome {
        ReplOutcome {
            input_label,
            input_ty: self.expr_ty,
            vars: self.list,
            lets: self.lets,
        }
    }
}

/// Builds the syntax tree of the `<repl>` module from the REPL declarations and input. `cur` is the index of the current
/// module. `import`s of modules that the current module already `import`s are not added (writing them again does
/// nothing).
pub(super) fn build_ast(rc: &ReplCompile<'_>, base: &[Module], cur: Option<usize>) -> Module {
    let mut m = Module::default();
    let mut imported: HashSet<(String, String)> = cur
        .map(|i| {
            base[i]
                .imports
                .iter()
                .map(|x| (x.path.clone(), x.local.clone()))
                .collect()
        })
        .unwrap_or_default();
    for d in rc.decls {
        match d {
            DeclEntry::Import(i) => {
                if imported.insert((i.path.clone(), i.local.clone())) {
                    m.imports.push(i.clone());
                }
            }
            DeclEntry::Identity(x) => {
                m.repl_order.push((1, m.identities.len()));
                m.identities.push(x.clone());
            }
            DeclEntry::Type(t) => m.types.push(t.clone()),
            DeclEntry::Fn(f) => {
                m.repl_order.push((2, m.fns.len()));
                m.fns.push(f.clone());
            }
        }
    }
    let body = match rc.input {
        InputAst::Stmts(b) => b.clone(),
        InputAst::Expr(_) => Vec::new(),
    };
    m.repl_order.push((3, 0));
    m.labels.push(LabelDecl {
        name: INPUT_LABEL.to_string(),
        params: Vec::new(),
        ret: None,
        body,
        pos: Pos { line: 1, col: 1 },
    });
    if let Some(i) = cur {
        m.poisoned = base[i].poisoned.clone();
    }
    m
}

/// Compiles the script's program with the REPL declarations and input added. On success, returns the extended program,
/// what to return to the REPL, and the warnings (including warnings of modules other than `<repl>`).
pub(crate) fn compile_repl(
    base: &Program,
    registry: &Registry,
    rc: &ReplCompile<'_>,
) -> Result<(Program, ReplOutcome, Vec<Diagnostic>), Vec<Diagnostic>> {
    let modules: Vec<(&str, &str)> = base
        .modules
        .iter()
        .zip(&base.sources)
        .map(|(m, src)| (m.as_str(), src.as_str()))
        .collect();
    let mut outcome = None;
    let mut sites = Vec::new();
    let result = super::compile_modules_core(
        &modules,
        registry,
        &super::CompileOptions::default(),
        &mut sites,
        &mut Vec::new(),
        Some(rc),
        &mut outcome,
    );
    result.map(|(p, d)| (p, outcome.expect("REPL の結果"), d))
}

impl Compiler<'_> {
    /// Whether the module being read is the REPL module.
    fn repl_here(&self) -> Option<&ReplState> {
        self.repl.as_ref().filter(|r| r.idx == self.cur_mod)
    }

    /// Passes the name table of the current module on to the REPL module (called right before reading the REPL module).
    pub(super) fn repl_seed(&mut self, what: Seed) {
        let Some(r) = self.repl_here() else {
            return;
        };
        let idx = r.idx;
        let cur = r.cur;
        if let Some(cur) = cur {
            let (head, tail) = self.modules.split_at_mut(idx);
            let src = &head[cur];
            let dst = &mut tail[0];
            match what {
                Seed::Imports => {
                    dst.imports = src.imports.clone();
                    dst.pkg_imports = src.pkg_imports.clone();
                    dst.bad_imports = src.bad_imports.clone();
                }
                Seed::Types => dst.type_map = src.type_map.clone(),
                Seed::EarlyIdentities => dst.early_identities = src.early_identities.clone(),
                Seed::Decls => {
                    dst.global_map = src.global_map.clone();
                    dst.label_map = src.label_map.clone();
                    dst.fn_map = src.fn_map.clone();
                    dst.identities = src.identities.clone();
                    dst.poisoned.extend(src.poisoned.iter().cloned());
                }
            }
        }
        if let Seed::Decls = what {
            self.repl_declare_vars();
        }
    }

    /// Makes REPL variables into variables of the `<repl>` module (sequential numbers follow the variables of the script's
    /// modules).
    fn repl_declare_vars(&mut self) {
        let base = self.globals.len() as u32;
        let list = self
            .repl
            .as_ref()
            .map(|r| r.list.clone())
            .unwrap_or_default();
        let module = self.cur_mod as u32;
        let mut map = HashMap::new();
        for (i, v) in list.iter().enumerate() {
            let slot = base + i as u32;
            map.insert(v.name.clone(), (slot, v.ty.clone()));
            self.globals.push(crate::program::GlobalInfo {
                module,
                persistent: false,
                name: v.name.clone(),
                ty: v.ty.clone(),
                init: v.ty.default_in(&self.types),
            });
        }
        if let Some(r) = self.repl.as_mut() {
            r.base_globals = base;
            r.vars = map;
        }
    }

    /// The list of REPL variable names (for searching candidate close names). Not visible inside a `fn`.
    pub(super) fn repl_var_names(&self) -> Vec<&str> {
        match self.repl_here() {
            Some(r) if !self.in_fn() => r.vars.keys().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether it is the name of a REPL variable (for checking name collisions; even inside a `fn`, they do not collide as
    /// declaration names).
    pub(super) fn repl_has_var(&self, name: &str) -> bool {
        self.repl_here().is_some_and(|r| r.vars.contains_key(name))
    }

    /// Looks up a REPL variable. Not visible from the body of a REPL `fn` (`let` is a top-level local variable of the REPL;
    /// 7.3.2).
    pub(super) fn repl_lookup(&self, name: &str) -> Option<(bool, u32, Option<Ty>)> {
        if self.in_fn() {
            return None;
        }
        self.repl_here()?
            .vars
            .get(name)
            .map(|(slot, ty)| (false, *slot, Some(ty.clone())))
    }

    /// The module in which label names passed to `seen` and `visited` (written without a module) are looked up. For the REPL
    /// module, the current module.
    pub(super) fn repl_name_module(&self, module: usize) -> usize {
        match &self.repl {
            Some(r) if r.idx == module => r.cur.unwrap_or(module),
            _ => module,
        }
    }

    /// Whether the top-level statements of REPL input are being read now.
    pub(super) fn repl_is_top_level(&self) -> bool {
        self.repl_here().is_some()
            && self
                .cur
                .as_ref()
                .is_some_and(|c| !c.in_fn && c.name == INPUT_LABEL && c.scopes.len() == 2)
    }

    /// Whether `name` is the label of REPL input.
    pub(super) fn repl_is_input_label(&self, name: &str) -> bool {
        self.repl_here().is_some() && name == INPUT_LABEL
    }

    /// A top-level `let` of the input. Declares a REPL variable (a REPL `let` of the same name replaces it) and returns the
    /// sequential number.
    pub(super) fn repl_let(&mut self, name: &str, pos: Pos, ty: Option<Ty>) -> u32 {
        let existing = self
            .repl
            .as_ref()
            .and_then(|r| r.list.iter().position(|v| v.name == name))
            .filter(|&i| {
                self.repl
                    .as_ref()
                    .is_some_and(|r| r.list[i].kind == VarKind::Let)
            });
        if existing.is_none() && (self.lookup(name).is_some() || self.is_import(name)) {
            self.name_error(pos, DiagCode::DuplicateName, name);
        }
        self.warn_if_builtin(name, pos);
        // REPL variables persist across inputs (function values may point elsewhere in the program of the next input)
        let ty = self.saveable(ty, pos, "repl");
        // When the type could not be decided, it is not used (there is a diagnostic, so it does not run)
        let ty = ty.unwrap_or(Ty::Int);
        let init = ty.default_in(&self.types);
        let module = self.cur_mod as u32;
        let base = self.repl.as_ref().map_or(0, |r| r.base_globals);
        let r = self.repl.as_mut().expect("REPL");
        let slot = match existing {
            Some(i) => {
                r.list[i].ty = ty.clone();
                let slot = base + i as u32;
                self.globals[slot as usize].ty = ty.clone();
                slot
            }
            None => {
                let slot = base + r.list.len() as u32;
                debug_assert_eq!(slot as usize, self.globals.len());
                r.list.push(ReplVar {
                    name: name.to_string(),
                    ty: ty.clone(),
                    kind: VarKind::Let,
                });
                self.globals.push(crate::program::GlobalInfo {
                    module,
                    persistent: false,
                    name: name.to_string(),
                    ty: ty.clone(),
                    init,
                });
                slot
            }
        };
        let r = self.repl.as_mut().expect("REPL");
        r.vars.insert(name.to_string(), (slot, ty));
        r.lets.push(name.to_string());
        slot
    }

    /// The end of the input label. For expression input, that expression (keeping its value).
    pub(super) fn repl_input_tail(&mut self) -> Option<ExprId> {
        let e = self.repl.as_mut().and_then(|r| r.expr.take())?;
        let (ty, id) = self.expr(&e);
        if let Some(r) = self.repl.as_mut() {
            r.expr_ty = ty;
        }
        Some(id)
    }
}
