//! Compilation: registering a module's declarations (import, variables, functions, labels) and resolving names.

use super::*;
use crate::value::{EnumValue, StructValue};
use std::sync::Arc;

impl Compiler<'_> {
    /// The name of this module (the last word of the path; `ch1` for `story::ch1`).
    pub(super) fn own_name(&self) -> &str {
        last_segment(&self.md().name)
    }

    /// Whether a name written in the script is the name of this module.
    pub(super) fn is_own_name(&self, name: &str) -> bool {
        self.module_index.contains_key(&self.md().name) && self.own_name() == name
    }

    /// The index of the module whose path ends with the word `name` (whether `import`ed or not).
    pub(super) fn module_named(&self, name: &str) -> Option<usize> {
        self.module_last.get(name).copied()
    }

    /// The index of this module or an `import`ed module (no diagnostics).
    pub(super) fn peek_module(&self, name: &str) -> Option<usize> {
        if self.is_own_name(name) {
            return Some(self.cur_mod);
        }
        let found = self.md().imports.get(name).copied();
        if found.is_some() {
            self.mark_import(name);
        }
        found
    }

    /// Marks the `import` with the script name (the name used by the `import`) as used.
    pub(super) fn mark_import(&self, name: &str) {
        self.used_imports
            .borrow_mut()
            .insert((self.cur_mod, name.to_string()));
    }

    /// Warnings for unused `import`s (not for the REPL).
    pub(super) fn warn_unused_imports(&mut self) {
        if self.repl.is_some() {
            return;
        }
        let sites = std::mem::take(&mut self.import_sites);
        for site in sites {
            if self
                .used_imports
                .borrow()
                .contains(&(site.module, site.local.clone()))
            {
                continue;
            }
            self.cur_mod = site.module;
            self.warn(
                site.pos,
                DiagCode::UnusedImport,
                vec![("name", s(&site.path))],
            );
        }
    }

    /// Warnings for `let` variables that are never read, in the order of declaration.
    pub(super) fn warn_unused_variables(&mut self) {
        for (m, pos, name) in std::mem::take(&mut self.unused_vars) {
            self.cur_mod = m;
            self.warn(pos, DiagCode::UnusedVariable, vec![("name", s(&name))]);
        }
    }

    /// Registers `import`s (done for all modules before type declarations).
    pub(super) fn declare_imports(&mut self, ast: &Module) {
        for imp in &ast.imports {
            let local = &imp.local;
            let site = |c: &Self, local: &str| ImportSite {
                module: c.cur_mod,
                local: local.to_string(),
                path: imp.path.clone(),
                pos: imp.pos,
            };
            if self.md().imports.contains_key(local) || self.md().pkg_imports.contains(local) {
                self.name_error(imp.pos, DiagCode::DuplicateImport, local);
            } else if self.is_own_name(local) && self.md().name != imp.path {
                self.name_error(imp.pos, DiagCode::DuplicateName, local);
            } else if let Some(&i) = self.module_index.get(&imp.path) {
                self.md_mut().imports.insert(local.clone(), i);
                let st = site(self, local);
                self.import_sites.push(st);
            } else if let Some(pkg) = self.package_path(&imp.path) {
                if imp.aliased {
                    self.name_error(imp.pos, DiagCode::PackageAlias, &imp.path);
                } else {
                    self.md_mut().pkg_imports.insert(pkg.to_string());
                    let st = site(self, pkg);
                    self.import_sites.push(st);
                }
            } else {
                self.unknown_module(&imp.path, imp.pos, true);
                self.md_mut().bad_imports.insert(local.clone());
            }
        }
    }

    /// If the path of an `import` points to a package, its name. Standard packages are `std::name`; host packages (and
    /// namespaces in which the host declared variables or types) are `name`.
    fn package_path<'p>(&self, path: &'p str) -> Option<&'p str> {
        match path.split_once("::") {
            Some((ns, name)) if ns == STD_NAMESPACE => {
                self.registry.is_std_package(name).then_some(name)
            }
            Some(_) => None,
            None => (self.registry.has_namespace(path) && !self.registry.is_std_package(path))
                .then_some(path),
        }
    }

    /// Whether the name collides with a variable or label of this module, or an `import`ed module.
    pub(super) fn declared_here(&self, name: &str) -> bool {
        let m = self.md();
        self.repl_has_var(name)
            || m.global_map.contains_key(name)
            || m.label_map.contains_key(name)
            || m.fn_map.contains_key(name)
            || m.imports.contains_key(name)
            || m.pkg_imports.contains(name)
            || m.identities.contains_key(name)
            || m.type_map.contains_key(name)
    }

    /// Decides the parameter and return types of a label (emits diagnostics if type names are invalid).
    pub(super) fn signature(&mut self, l: &LabelDecl) -> Sig {
        let params: Vec<Option<Ty>> = l.params.iter().map(|p| self.ty_from_name(&p.ty)).collect();
        let names = l.params.iter().map(|p| p.name.clone()).collect();
        let mut defaults = Vec::with_capacity(l.params.len());
        let mut seen_default = false;
        for (p, ty) in l.params.iter().zip(&params) {
            // If a default value is invalid (a diagnostic was emitted), fill in the default value of the type (so that calls do not
            // also report missing arguments)
            let value = match (&p.default, ty) {
                (Some(e), Some(ty)) => match self.const_value(e, Some(ty)) {
                    Some(v) if v.matches_in(ty, &self.types) => Some(v),
                    Some(v) => {
                        self.type_mismatch(e.pos(), &ty.name(), &v.ty().name());
                        Some(ty.default_in(&self.types))
                    }
                    None => Some(ty.default_in(&self.types)),
                },
                (Some(_), None) => Some(Value::Unit),
                (None, _) => None,
            };
            if p.default.is_some() {
                seen_default = true;
            } else if seen_default {
                self.error(
                    p.pos,
                    DiagCode::RequiredAfterDefault,
                    vec![("name", s(&l.name)), ("arg", s(&p.name))],
                );
            }
            defaults.push(value);
        }
        let ret = match &l.ret {
            None => RetTy::None,
            Some(t) => match self.ty_from_name(t) {
                Some(ty) => RetTy::Ty(ty),
                None => RetTy::Bad,
            },
        };
        Sig {
            params,
            names,
            defaults,
            ret,
        }
    }

    /// Registers a module's `import`s, variables and labels in the order written. For duplicate names, reports the one written
    /// later. `label_base` is the sequential number, across the whole program, of this module's first label.
    pub(super) fn declare(&mut self, ast: &Module, label_base: usize, fn_base: usize) {
        enum Decl<'x> {
            Var(&'x VarDecl),
            Label(usize, &'x LabelDecl),
            Fn(usize, &'x LabelDecl),
            Identity(&'x IdentityDecl),
        }
        let mut order: Vec<(Pos, Decl)> = Vec::new();
        order.extend(ast.vars.iter().map(|v| (v.pos, Decl::Var(v))));
        order.extend(ast.identities.iter().map(|d| (d.pos, Decl::Identity(d))));
        order.extend(
            ast.labels
                .iter()
                .enumerate()
                .map(|(i, l)| (l.pos, Decl::Label(i, l))),
        );
        order.extend(
            ast.fns
                .iter()
                .enumerate()
                .map(|(i, f)| (f.pos, Decl::Fn(i, f))),
        );
        if ast.repl_order.is_empty() {
            order.sort_by_key(|(p, _)| (p.line, p.col));
        } else {
            // The REPL module gathers declarations from multiple inputs (positions are per input). Order them by declaration
            let rank = |kind: u8, i: usize| {
                ast.repl_order
                    .iter()
                    .position(|&(k, j)| k == kind && j == i)
                    .unwrap_or(usize::MAX)
            };
            let mut ranked: Vec<(usize, (Pos, Decl))> = Vec::new();
            let mut ids = 0;
            let mut labels = 0;
            let mut fns = 0;
            for (p, d) in order {
                let r = match &d {
                    Decl::Var(_) => 0,
                    Decl::Identity(_) => {
                        ids += 1;
                        rank(1, ids - 1)
                    }
                    Decl::Fn(..) => {
                        fns += 1;
                        rank(2, fns - 1)
                    }
                    Decl::Label(..) => {
                        labels += 1;
                        rank(3, labels - 1)
                    }
                };
                ranked.push((r, (p, d)));
            }
            ranked.sort_by_key(|(r, _)| *r);
            order = ranked.into_iter().map(|(_, x)| x).collect();
        }
        for (_, d) in order {
            match d {
                Decl::Var(v) => self.declare_var(v),
                Decl::Identity(d) => {
                    if self.declared_here(&d.name) {
                        self.name_error(d.pos, DiagCode::DuplicateName, &d.name);
                    } else {
                        self.md_mut()
                            .identities
                            .insert(d.name.clone(), d.display.clone());
                    }
                }
                Decl::Label(i, l) => {
                    let flat = label_base + i;
                    self.sigs[flat] = self.signature(l);
                    if self.declared_here(&l.name) {
                        self.name_error(l.pos, DiagCode::DuplicateName, &l.name);
                        continue;
                    }
                    self.md_mut().label_map.insert(l.name.clone(), flat as u32);
                }
                Decl::Fn(i, f) => {
                    let flat = fn_base + i;
                    self.fn_sigs[flat] = self.signature(f);
                    // A name cannot be the same as a command (calls of `@name(...)` would become confusing)
                    let command = self.registry.find_command(&f.name).is_some()
                        || self.registry.command_id(&f.name).is_some();
                    if self.declared_here(&f.name) || command {
                        self.name_error(f.pos, DiagCode::DuplicateName, &f.name);
                        continue;
                    }
                    self.warn_if_builtin(&f.name, f.pos);
                    self.md_mut().fn_map.insert(f.name.clone(), flat as u32);
                }
            }
        }
    }

    /// Checks the type and initial value (a constant) of `save var` / `persistent var` declarations and registers them in the
    /// module.
    pub(super) fn declare_var(&mut self, v: &VarDecl) {
        let ty = self.ty_from_name(&v.ty);
        let ty = self.saveable(ty, v.ty.pos, "var");
        // If the type is invalid, a diagnostic was emitted. The initial value is not checked (diagnostics about being unable to
        // decide the type of `[]` or `map {}` would cascade)
        let init = if ty.is_some() {
            self.const_value(&v.init, ty.as_ref())
        } else {
            None
        };
        if self.declared_here(&v.name) {
            self.name_error(v.pos, DiagCode::DuplicateName, &v.name);
            return;
        }
        self.warn_if_builtin(&v.name, v.pos);
        let Some(ty) = ty else {
            // The type is invalid. Do not report "undeclared" when it is used.
            self.md_mut().poisoned.insert(v.name.clone());
            return;
        };
        let value = match init {
            Some(init) if init.matches_in(&ty, &self.types) => init,
            Some(init) => {
                self.type_mismatch(v.init.pos(), &ty.name(), &init.ty().name());
                ty.default_in(&self.types)
            }
            None => ty.default_in(&self.types),
        };
        let slot = self.globals.len() as u32;
        let module = self.cur_mod as u32;
        self.md_mut()
            .global_map
            .insert(v.name.clone(), (slot, ty.clone()));
        self.globals.push(GlobalInfo {
            module,
            persistent: v.persistent,
            name: v.name.clone(),
            ty,
            init: value,
        });
    }

    /// The diagnostic for a module (or package) that does not exist (with candidate close names). If `full`, it is the path
    /// written in the `import` (`story::ch1`), so candidates are paths too. Otherwise, the name used in the script (the last
    /// word of the path).
    pub(super) fn unknown_module(&mut self, name: &str, pos: Pos, full: bool) {
        let mut names: Vec<String> = if full {
            self.module_index.keys().cloned().collect()
        } else {
            self.module_last.keys().cloned().collect()
        };
        for ns in self.registry.namespaces() {
            // Standard packages are `std::name` in `import`s, and `name` inside scripts
            if full && self.registry.is_std_package(&ns) {
                names.push(format!("{STD_NAMESPACE}::{ns}"));
            } else {
                names.push(ns);
            }
        }
        let mut found = suggest(name, names.iter().map(String::as_str));
        // When `std::` was forgotten, as in `import audio`, make that the first candidate
        if full && self.registry.is_std_package(name) {
            found.insert(0, format!("{STD_NAMESPACE}::{name}"));
            found.truncate(3);
        }
        self.error(pos, DiagCode::UnknownModule, name_args(name, found));
    }

    /// Whether the name is an `import`ed module or package.
    pub(super) fn is_import(&self, name: &str) -> bool {
        if self.md().imports.contains_key(name) || self.md().pkg_imports.contains(name) {
            self.mark_import(name);
            return true;
        }
        self.md().type_map.contains_key(name)
    }

    /// Resolves the first half of `name.` written in a script: this module, an `import`ed module, or an `import`ed package.
    /// If it cannot be resolved, emits a diagnostic and returns `None`.
    pub(super) fn resolve_target(&mut self, name: &str, pos: Pos) -> Option<Target> {
        if self.is_own_name(name) {
            return Some(Target::Module(self.cur_mod));
        }
        if let Some(&i) = self.md().imports.get(name) {
            self.mark_import(name);
            return Some(Target::Module(i));
        }
        if self.md().pkg_imports.contains(name) {
            self.mark_import(name);
            return Some(Target::Package);
        }
        if self.md().bad_imports.contains(name) {
            return None;
        }
        if self.module_last.contains_key(name) || self.registry.has_namespace(name) {
            self.name_error(pos, DiagCode::ModuleNotImported, name);
        } else {
            self.unknown_module(name, pos, false);
        }
        None
    }

    /// Resolves a module name written in a script: this module or an `import`ed module. If it cannot be resolved, emits a
    /// diagnostic and returns `None` (an `import`ed package is not a module, so `unknown_module`).
    pub(super) fn resolve_module(&mut self, name: &str, pos: Pos) -> Option<usize> {
        match self.resolve_target(name, pos)? {
            Target::Module(i) => Some(i),
            Target::Package => {
                self.unknown_module(name, pos, false);
                None
            }
        }
    }

    /// Looks up the host-declared variable `package.name`. If not found, emits a diagnostic (with candidate close names) and
    /// returns `None`. If found, returns the index into `Program::host_vars` and the variable's declaration.
    pub(super) fn host_var(
        &mut self,
        package: &str,
        name: &str,
        pos: Pos,
    ) -> Option<(u32, Ty, bool)> {
        let full = format!("{package}.{name}");
        let Some(i) = self.registry.var_index(&full) else {
            let found = suggest(
                &full,
                self.registry.vars_in(package).map(|v| v.name.as_str()),
            );
            self.error(pos, DiagCode::UnknownName, name_args(&full, found));
            return None;
        };
        let decl = &self.registry.vars()[i];
        let (ty, read_only) = (decl.ty.clone(), decl.read_only);
        let idx = match self.host_vars.iter().position(|n| *n == full) {
            Some(k) => k,
            None => {
                self.host_vars.push(full);
                self.host_vars.len() - 1
            }
        };
        Some((idx as u32, ty, read_only))
    }

    /// Resolves a label name (`label` or `module.label`). If not found, emits a diagnostic and returns `None`.
    /// `pos` is the position of the label diagnostic (the module diagnostic is at the position of the module name).
    pub(super) fn resolve_label(&mut self, target: &LabelRef, pos: Pos) -> Option<(u32, Sig)> {
        let midx = match &target.module {
            Some((m, mp)) => self.resolve_module(m, *mp)?,
            None => self.cur_mod,
        };
        if let Some(&flat) = self.modules[midx].label_map.get(&target.name) {
            return Some((flat, self.sigs[flat as usize].clone()));
        }
        if !self.modules[midx].poisoned.contains(&target.name) {
            let mut found = suggest(
                &target.name,
                self.modules[midx].label_map.keys().map(String::as_str),
            );
            if let Some((m, _)) = &target.module {
                for f in &mut found {
                    *f = format!("{m}.{f}");
                }
            }
            self.error(
                pos,
                DiagCode::UnknownLabel,
                name_args(&target.written(), found),
            );
        }
        None
    }

    /// The diagnostic for a variable of another module. Not emitted for names whose declaration failed.
    pub(super) fn unknown_qualified(&mut self, midx: usize, module: &str, name: &str, pos: Pos) {
        if self.modules[midx].poisoned.contains(name) {
            return;
        }
        let found: Vec<String> = suggest(
            name,
            self.modules[midx].global_map.keys().map(String::as_str),
        )
        .into_iter()
        .map(|f| format!("{module}.{f}"))
        .collect();
        self.error(
            pos,
            DiagCode::UnknownName,
            name_args(&format!("{module}.{name}"), found),
        );
    }

    /// A constant initial value (literals, and expressions of only literals; values of `list`, `Some`, `None`, `struct` and
    /// `enum` built only from constants can be written). `hint` is the declared type (decides the type of `[]` and `None`).
    /// If it is not a constant, emits a diagnostic and returns `None`.
    pub(super) fn const_value(&mut self, e: &Expr, hint: Option<&Ty>) -> Option<Value> {
        let before = self.diags.len();
        let v = self.const_rec(e, hint);
        // If no diagnostic was emitted inside, the whole expression is not a constant
        if v.is_none() && self.diags.len() == before {
            self.error(e.pos(), DiagCode::NotConstant, vec![]);
        }
        v
    }

    /// Turns an expression that can be evaluated as a constant into a value. If it is not a constant, emits a diagnostic and
    /// returns `None`. `hint` is the expected type (for deciding the type of `[]` and `map {}`).
    pub(super) fn const_rec(&mut self, e: &Expr, hint: Option<&Ty>) -> Option<Value> {
        match e {
            Expr::Int(n, _) => Some(Value::Int(*n)),
            Expr::Float(x, _) => Some(Value::Float(*x)),
            Expr::Bool(b, _) => Some(Value::Bool(*b)),
            Expr::Str(parts, _) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        StrPart::Text(t, _) => out.push_str(t),
                        StrPart::Interp(_) => {
                            self.error(e.pos(), DiagCode::NotConstant, vec![]);
                            return None;
                        }
                    }
                }
                Some(Value::Str(out.into()))
            }
            Expr::Unary(UnOp::Neg, inner, _) => match &**inner {
                Expr::Int(n, _) => n.checked_neg().map(Value::Int),
                Expr::Float(x, _) => finite_float(-x).map(Value::Float),
                _ => None,
            },
            Expr::Unary(UnOp::Not, inner, _) => match &**inner {
                Expr::Bool(b, _) => Some(Value::Bool(!b)),
                _ => None,
            },
            Expr::None(p) => match hint {
                Some(Ty::Option(_)) => Some(Value::Option(None)),
                Some(_) => None,
                None => {
                    self.error(*p, DiagCode::CannotInferType, vec![]);
                    None
                }
            },
            Expr::Some(inner, _) => {
                let inner_hint = match hint {
                    Some(Ty::Option(t)) => Some((**t).clone()),
                    _ => None,
                };
                let v = self.const_rec(inner, inner_hint.as_ref())?;
                Some(Value::Option(Some(Box::new(v))))
            }
            Expr::List(items, p) => {
                let elem = match hint {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                if items.is_empty() && elem.is_none() && hint.is_none() {
                    self.error(*p, DiagCode::CannotInferType, vec![]);
                    return None;
                }
                let mut out = Vec::new();
                for it in items {
                    let v = self.const_rec(it, elem.as_ref())?;
                    if let Some(t) = &elem
                        && !v.matches_in(t, &self.types)
                    {
                        self.type_mismatch(it.pos(), &t.name(), &v.ty().name());
                        return None;
                    }
                    out.push(v);
                }
                Some(Value::List(out.into()))
            }
            Expr::Map(entries, p) => {
                let (kt, vt) = match hint {
                    Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                if entries.is_empty() && kt.is_none() && hint.is_none() {
                    self.error(*p, DiagCode::CannotInferType, vec![]);
                    return None;
                }
                let mut out: Vec<(Value, Value)> = Vec::new();
                for (k, v) in entries {
                    let kv = self.const_rec(k, kt.as_ref())?;
                    let vv = self.const_rec(v, vt.as_ref())?;
                    for (item, want, at) in [(&kv, &kt, k), (&vv, &vt, v)] {
                        if let Some(t) = want
                            && !item.matches_in(t, &self.types)
                        {
                            self.type_mismatch(at.pos(), &t.name(), &item.ty().name());
                            return None;
                        }
                    }
                    out.push((kv, vv));
                }
                // If a key repeats, the later value updates it (the position stays the first one)
                Some(Value::Map(crate::value::MapData::from_pairs(out)))
            }
            Expr::StructLit(tr, fields, p) => {
                let (idx, plan) = self.struct_lit_plan(tr, fields, *p)?;
                let decl = self.types[idx as usize].clone();
                let TypeKind::Struct { fields: decls } = &decl.kind else {
                    return None;
                };
                let mut values: Vec<Option<Value>> = vec![None; decls.len()];
                for (src, di) in plan {
                    let want = &decls[di].1;
                    let v = self.const_rec(&fields[src].value, Some(want))?;
                    if !v.matches_in(want, &self.types) {
                        self.type_mismatch(fields[src].value.pos(), &want.name(), &v.ty().name());
                        return None;
                    }
                    values[di] = Some(v);
                }
                Some(Value::Struct(Arc::new(StructValue {
                    ty: decl.name.clone().into(),
                    fields: decls
                        .iter()
                        .zip(values)
                        .map(|((n, _), v)| (n.clone(), v.unwrap_or(Value::Unit)))
                        .collect::<Vec<_>>(),
                })))
            }
            Expr::Name(name, p) => {
                // The name of an identity declaration (an `Identity` constant)
                let r = IdentRef {
                    module: None,
                    name: name.clone(),
                    pos: *p,
                };
                self.resolve_identity_const(&r)
                    .map(|i| Value::Identity(i.into()))
            }
            _ => {
                if let Some(vr) = self.as_variant_ref(e) {
                    let (ty_idx, vi) = self.variant_lookup(&vr)?;
                    let decl = self.types[ty_idx as usize].clone();
                    let TypeKind::Enum { variants } = &decl.kind else {
                        return None;
                    };
                    let want = &variants[vi].args;
                    if want.len() != vr.args.len() {
                        self.arg_count_error(
                            &format!("{}.{}", decl.name, vr.variant),
                            want.len(),
                            vr.args.len(),
                            vr.pos,
                        );
                        return None;
                    }
                    let mut args = Vec::new();
                    for (a, w) in vr.args.iter().zip(want) {
                        let v = self.const_rec(a, Some(w))?;
                        if !v.matches_in(w, &self.types) {
                            self.type_mismatch(a.pos(), &w.name(), &v.ty().name());
                            return None;
                        }
                        args.push(v);
                    }
                    return Some(Value::Enum(Arc::new(EnumValue {
                        ty: decl.name.clone().into(),
                        variant: vr.variant.to_string().into(),
                        args,
                    })));
                }
                // An identity constant of another module (`module.案内人`)
                if let Expr::Qualified(module, name, p) = e
                    && self.peek_module(module).is_some()
                {
                    let r = IdentRef {
                        module: Some((module.clone(), *p)),
                        name: name.clone(),
                        pos: *p,
                    };
                    return self
                        .resolve_identity_const(&r)
                        .map(|i| Value::Identity(i.into()));
                }
                None
            }
        }
    }
}
