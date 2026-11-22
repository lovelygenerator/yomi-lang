//! Compilation: statements (control, assignment, local variables, return).

use super::*;

/// The result of the part of a `for` before the body (the expression, how to iterate, the two hidden local variables, the
/// index or key variable, the element variable).
type ForParts = (ExprId, ForKind, u32, u32, Option<u32>, u32);

impl Compiler<'_> {
    /// Looks up a name. If found, (whether local, slot, type). For names whose type could not be decided, the type is `None`.
    pub(super) fn lookup(&self, name: &str) -> Option<(bool, u32, Option<Ty>)> {
        let c = self.cur.as_ref().expect("ラベルの中");
        for scope in c.scopes.iter().rev() {
            if let Some((slot, ty)) = scope.get(name) {
                return Some((true, *slot, ty.clone()));
            }
        }
        self.md()
            .global_map
            .get(name)
            .map(|(slot, ty)| (false, *slot, Some(ty.clone())))
            .or_else(|| self.repl_lookup(name))
    }

    /// Records that a local variable is read (for the warning `unused_variable`).
    pub(super) fn mark_read(&mut self, slot: u32) {
        self.ctx().lets.read.insert(slot);
    }

    /// The diagnostic for an undeclared name (with candidate close names). Not emitted for names whose declaration failed.
    pub(super) fn unknown_name(&mut self, name: &str, pos: Pos) {
        if self.md().poisoned.contains(name) {
            return;
        }
        let c = self.cur.as_ref().expect("ラベルの中");
        let mut names: Vec<&str> = self.md().global_map.keys().map(String::as_str).collect();
        for scope in &c.scopes {
            names.extend(scope.keys().map(String::as_str));
        }
        names.extend(self.repl_var_names());
        let found = suggest(name, names);
        self.error(pos, DiagCode::UnknownName, name_args(name, found));
    }

    /// Compiles a sequence of statements in a new scope.
    pub(super) fn block(&mut self, body: &[Stmt]) -> Vec<StmtId> {
        self.ctx().scopes.push(HashMap::new());
        let outer = self.fn_block_enter();
        let mut out = Vec::new();
        for st in body {
            self.stmt(st, &mut out);
        }
        self.fn_block_exit(outer);
        self.ctx().scopes.pop();
        out
    }

    /// Checks a condition and makes an IR expression.
    pub(super) fn condition(&mut self, cond: &Expr) -> ExprId {
        let (t, id) = self.expr(cond);
        if let Some(t) = t
            && t != Ty::Bool
        {
            self.error(
                cond.pos(),
                DiagCode::ConditionNotBool,
                vec![("found", s(&t.name()))],
            );
        }
        id
    }

    /// Compiles one statement and adds it to `out`. Only nesting statements (`if`, `while`, `choice`) are dispatched here; the
    /// others are left to [`Compiler::simple_stmt`] (so that nesting up to the limit fits in the 1 MiB stack of debug
    /// builds).
    pub(super) fn stmt(&mut self, st: &Stmt, out: &mut Vec<StmtId>) {
        match st {
            Stmt::If {
                arms,
                else_body,
                pos,
            } => self.if_stmt(arms, else_body, *pos, out),
            Stmt::While { cond, body, pos } => self.while_stmt(cond, body, *pos, out),
            Stmt::For {
                key,
                var,
                var_pos,
                iter,
                body,
                pos,
            } => self.for_stmt(key.as_ref(), (var, *var_pos), iter, body, *pos, out),
            Stmt::Choice { options, pos } => {
                if self.ctx().in_fn {
                    self.not_allowed_in_fn("choice", *pos);
                } else {
                    self.choice(options, *pos, out);
                }
            }
            Stmt::Match {
                scrutinee,
                arms,
                pos,
            } => self.match_stmt(scrutinee, arms, *pos, out),
            _ => self.simple_stmt(st, out),
        }
    }

    /// `if`, `else if`, `else`.
    pub(super) fn if_stmt(
        &mut self,
        arms: &[(Expr, Vec<Stmt>)],
        else_body: &[Stmt],
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let mut ir_arms = Vec::new();
        for (i, (cond, body)) in arms.iter().enumerate() {
            let line = if i == 0 { pos.line } else { cond.pos().line };
            let cond = self.condition(cond);
            let body = self.block(body);
            ir_arms.push(IfArm { cond, body, line });
        }
        let else_body = self.block(else_body);
        self.push_stmt(
            out,
            StmtKind::If {
                arms: ir_arms,
                else_body,
            },
            pos.line,
        );
    }

    /// `while`.
    pub(super) fn while_stmt(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        self.fn_loop_enter();
        let cond = self.condition(cond);
        let body = self.loop_block(body);
        self.fn_loop_exit();
        self.push_stmt(out, StmtKind::While { cond, body }, pos.line);
    }

    /// The body of a loop (`break` and `continue` can be written).
    fn loop_block(&mut self, body: &[Stmt]) -> Vec<StmtId> {
        self.ctx().loop_depth += 1;
        let ids = self.block(body);
        self.ctx().loop_depth -= 1;
        ids
    }

    /// `for name in expr { ... }`. Evaluates the expression (a `list`), puts it in hidden local variables (the copied `list`
    /// and the next position), and takes elements one by one. It iterates the copy, so changes in the body do not affect the
    /// iteration. The hidden local variables are named `$for<number>.list` and `$for<number>.idx` (they do not collide with
    /// script names).
    pub(super) fn for_stmt(
        &mut self,
        key: Option<&(String, Pos)>,
        var: (&str, Pos),
        iter: &Expr,
        body: &[Stmt],
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let (iter, kind, list, idx, key, var) = self.for_prepare(key, var, iter);
        // When iterating a `list` that contains function types, the hidden `list` stays in use during the iteration
        let list_ty = self.ctx().locals[list as usize].ty.clone();
        let hidden = list_ty.contains_fn();
        if hidden {
            self.ctx().live.hidden.push((list, list_ty.name()));
        }
        self.fn_loop_enter();
        let body = self.loop_block(body);
        if hidden {
            self.fn_use(list);
            self.ctx().live.hidden.pop();
        }
        self.fn_loop_exit();
        self.ctx().scopes.pop();
        self.push_stmt(
            out,
            StmtKind::For {
                iter,
                list,
                idx,
                var,
                key,
                kind,
                body,
            },
            pos.line,
        );
    }

    /// The part of a `for` before the body (the expression, hidden local variables, variable declarations). Returns with the
    /// body's scope still open (closed by [`Compiler::for_stmt`]). Returns the expression and the slots of the hidden local
    /// variables and of the variable that receives elements.
    /// The frame is large, so it is kept off the recursive path (`for_stmt`).
    #[inline(never)]
    pub(super) fn for_prepare(
        &mut self,
        key: Option<&(String, Pos)>,
        var: (&str, Pos),
        iter: &Expr,
    ) -> ForParts {
        let (found, mut iter_id) = self.expr(iter);
        let mut kind = ForKind::List;
        // (index type, element type, type of the hidden `list`)
        let (key_ty, elem, hidden_ty): (Option<Ty>, Option<Ty>, Option<Ty>) = match found {
            Some(Ty::List(t)) => {
                // `range(a, b)` with one variable is iterated without building a list
                if key.is_none()
                    && let ExprKind::Builtin {
                        builtin: Builtin::Range,
                        args,
                    } = &self.ctx().ir.exprs[iter_id].kind
                    && let [a, b] = args[..]
                {
                    iter_id = a;
                    kind = ForKind::Range { end: b };
                    (None, Some(Ty::Int), Some(Ty::Int))
                } else {
                    (Some(Ty::Int), Some((*t).clone()), Some(Ty::List(t)))
                }
            }
            Some(Ty::Map(k, v)) if key.is_some() => {
                kind = ForKind::Map;
                (Some((*k).clone()), Some((*v).clone()), Some(Ty::Map(k, v)))
            }
            Some(other) => {
                let expected = if key.is_some() {
                    "list<_> | map<_, _>"
                } else {
                    "list<_>"
                };
                self.type_mismatch(iter.pos(), expected, &other.name());
                (None, None, None)
            }
            None => (None, None, None),
        };
        let (list_slot, idx_slot) = {
            let c = self.ctx();
            let n = c
                .locals
                .iter()
                .filter(|l| l.name.starts_with("$for"))
                .count()
                / 2;
            let list_slot = c.locals.len() as u32;
            c.locals.push(LocalInfo {
                name: format!("$for{n}.list"),
                ty: hidden_ty.unwrap_or_else(|| Ty::List(Box::new(Ty::Int))),
            });
            c.locals.push(LocalInfo {
                name: format!("$for{n}.idx"),
                ty: Ty::Int,
            });
            (list_slot, list_slot + 1)
        };
        // Variables are local variables of the body (not visible outside the body)
        self.ctx().scopes.push(HashMap::new());
        let key_slot = key.map(|(name, pos)| self.declare_loop_var(name, *pos, key_ty));
        let var_slot = self.declare_loop_var(var.0, var.1, elem);
        (iter_id, kind, list_slot, idx_slot, key_slot, var_slot)
    }

    /// Compiles statements that do not nest. The frame is large, so it is kept off the recursive path
    /// ([`Compiler::stmt`]).
    #[inline(never)]
    pub(super) fn simple_stmt(&mut self, st: &Stmt, out: &mut Vec<StmtId>) {
        match st {
            Stmt::Let {
                name,
                name_pos,
                ty,
                init,
                pos,
            } => {
                let declared = ty.as_ref().and_then(|t| self.ty_from_name(t));
                let (found, value) = self.rhs(init, declared.as_ref());
                let ty_final = match (declared, found) {
                    (Some(d), Some(f)) if d != f => {
                        self.type_mismatch(init.pos(), &d.name(), &f.name());
                        Some(d)
                    }
                    (Some(d), _) => Some(d),
                    (None, f) => f,
                };
                if self.repl_is_top_level() {
                    // A top-level `let` of REPL input becomes a REPL variable (a variable of the `<repl>` module)
                    let slot = self.repl_let(name, *name_pos, ty_final);
                    let place = PlaceRef::Global(slot);
                    self.push_stmt(out, StmtKind::Store { place, value }, pos.line);
                    return;
                }
                if self.lookup(name).is_some() || self.is_import(name) {
                    self.name_error(*name_pos, DiagCode::DuplicateName, name);
                }
                self.warn_if_builtin(name, *name_pos);
                if let Some(t) = &ty_final {
                    let m = self.cur_mod;
                    self.local_types.push((m, *name_pos, t.name()));
                }
                let c = self.ctx();
                let slot = c.locals.len() as u32;
                c.locals.push(LocalInfo {
                    name: name.clone(),
                    // When the type could not be decided, it is not used (there is a diagnostic, so it does not run)
                    ty: ty_final.clone().unwrap_or(Ty::Int),
                });
                c.scopes
                    .last_mut()
                    .expect("スコープ")
                    .insert(name.clone(), (slot, ty_final));
                if !name.starts_with('_') {
                    c.lets.decls.push((slot, name.clone(), *name_pos));
                }
                let place = PlaceRef::Local(slot);
                self.push_stmt(out, StmtKind::Store { place, value }, pos.line);
            }
            Stmt::Assign {
                module,
                name,
                path,
                op,
                value,
                pos,
            } => self.assign((module.as_ref(), name, path), *op, value, *pos, out),
            Stmt::ListMethod {
                module,
                name,
                path,
                method,
                method_pos,
                args,
                pos,
            } => self.method_stmt(
                (module.as_ref(), name, path),
                (method, *method_pos),
                args,
                *pos,
                out,
            ),
            Stmt::Break(pos) | Stmt::Continue(pos) => {
                let (kind, what) = match st {
                    Stmt::Break(_) => (StmtKind::Break, "break"),
                    _ => (StmtKind::Continue, "continue"),
                };
                if self.ctx().loop_depth == 0 {
                    self.error(*pos, DiagCode::OutsideLoop, vec![("what", s(what))]);
                } else {
                    self.push_stmt(out, kind, pos.line);
                }
            }
            Stmt::Jump { pos, .. } if self.ctx().in_fn => self.not_allowed_in_fn("jump", *pos),
            Stmt::Jump { target, pos } => {
                let Some((flat, sig)) = self.resolve_label(target, *pos) else {
                    return;
                };
                if sig.callable() {
                    self.name_error(*pos, DiagCode::JumpToCallableLabel, &target.written());
                } else if !matches!(self.ctx().ret, RetTy::None) {
                    // Leaving a label with a return type would make it impossible to return a value to the caller
                    let name = self.ctx().name.clone();
                    self.name_error(*pos, DiagCode::JumpInReturningLabel, &name);
                }
                self.ctx().refs.targets.push(flat);
                self.push_stmt(out, StmtKind::JumpLabel(flat), pos.line);
            }
            Stmt::Call(call) => {
                // For `call` inside a `fn`, `label_call` emits the diagnostic
                let (_, expr) = self.label_call(call, false);
                self.push_stmt(out, StmtKind::Eval(expr), call.pos.line);
            }
            Stmt::Return { value, pos } => self.return_stmt(value.as_ref(), *pos, out),
            Stmt::End(pos) if self.ctx().in_fn => self.not_allowed_in_fn("end", *pos),
            Stmt::End(pos) => self.push_stmt(out, StmtKind::End, pos.line),
            Stmt::Command(call) => self.command(call, out),
            Stmt::Plain { pos, .. } if self.ctx().in_fn => self.not_allowed_in_fn("plain", *pos),
            Stmt::Quote { pos, .. } if self.ctx().in_fn => self.not_allowed_in_fn("quote", *pos),
            Stmt::Plain {
                parts,
                raw,
                id,
                id_at,
                typo,
                pos,
            } => {
                if let Some((name, tpos)) = typo {
                    self.warn_identity_typo(name, *tpos);
                }
                self.plain(parts, raw, (id.as_ref(), *id_at), *pos, out)
            }
            Stmt::Quote {
                identity,
                parts,
                id,
                id_at,
                pos,
            } => self.quote(identity.as_ref(), parts, (id.as_ref(), *id_at), *pos, out),
            // Nesting statements are dispatched by `stmt`
            Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::For { .. }
            | Stmt::Choice { .. }
            | Stmt::Match { .. } => {
                unreachable!("stmt が振り分ける")
            }
        }
    }

    /// The diagnostic for statements that cannot be written inside a `fn`.
    pub(super) fn not_allowed_in_fn(&mut self, what: &str, pos: Pos) {
        self.error(pos, DiagCode::NotAllowedInFn, vec![("what", s(what))]);
    }

    /// Whether the body of a `fn` is being compiled now.
    pub(super) fn in_fn(&self) -> bool {
        self.cur.as_ref().is_some_and(|c| c.in_fn)
    }

    /// If `@name(...)` is a call of a `fn`, compiles it (called only when it is not a command name). `None` if it is not a
    /// `fn` (the caller makes it `unknown_command`). If `bind` is true, the return value is received (the type is returned);
    /// if false, the return value is discarded.
    pub(super) fn fn_statement(&mut self, call: &CommandCall, bind: bool) -> Option<Typed> {
        let name = call.name.as_str();
        let pos = call.pos;
        if let Some(sid) = &call.id
            && (self.lookup_fn(name).is_some() || self.module_part(name).is_some())
        {
            self.error(sid.pos, DiagCode::InvalidStableId, vec![("id", s(&sid.id))]);
        }
        if let Some(flat) = self.lookup_fn(name) {
            let (ty, id) = self.fn_call(flat, name, &call.args, pos, bind);
            return Some((ty, id));
        }
        // `module.name` where the module exists but the `fn` does not
        let (m, member) = name.split_once('.')?;
        if let Some(midx) = self.peek_module(m) {
            self.discard_exprs(&call.args);
            self.unknown_function(midx, m, member, pos);
            return Some(self.failed());
        }
        if self.module_last.contains_key(m) && !self.registry.has_namespace(m) {
            self.discard_exprs(&call.args);
            self.name_error(pos, DiagCode::ModuleNotImported, m);
            return Some(self.failed());
        }
        None
    }

    /// If the `module` of `module.name` is a module (this one, `import`ed or not), its index.
    pub(super) fn module_part(&self, written: &str) -> Option<usize> {
        let (m, _) = written.split_once('.')?;
        self.module_named(m)
    }

    /// The initial value of `let` or the right-hand side of `=`. If the whole is `@command(...)`, the return value is
    /// received.
    pub(super) fn rhs(&mut self, e: &Expr, hint: Option<&Ty>) -> Typed {
        match e {
            Expr::Command(call) => {
                if self.registry.find_command(&call.name).is_none()
                    && let Some(r) = self.fn_statement(call, true)
                {
                    return r;
                }
                self.bound_command(call)
            }
            other => self.expr_ir(other, hint),
        }
    }

    /// Compiles an assignment or compound assignment. If the whole right-hand side of `=` is `@command(...)`, its return value
    /// is received.
    pub(super) fn assign(
        &mut self,
        (module, name, path): (Option<&(String, Pos)>, &str, &[PathElem]),
        op: AssignOp,
        value: &Expr,
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let target = self.target(module, name, path, pos);
        // Only a plain `x = value` does not read `x`. A compound assignment and a write into a part of `x` do
        if let Some(TargetInfo {
            place: PlaceRef::Local(slot),
            ..
        }) = &target
            && (op != AssignOp::Set || !path.is_empty() || module.is_some())
        {
            self.mark_read(*slot);
        }
        let hint = target.as_ref().and_then(|t| t.ty.clone());
        // An `@command(...)` that is the whole right-hand side of `=` receives the return value. The right-hand side of a compound
        // assignment is an expression
        let (found, value_id) = if op == AssignOp::Set {
            self.rhs(value, hint.as_ref())
        } else {
            self.expr(value)
        };
        let Some(TargetInfo {
            place,
            ty,
            steps,
            indices,
        }) = target
        else {
            return;
        };
        let operand = Operand::of(ty.as_ref());
        if let Some(t) = ty {
            if op == AssignOp::Set {
                if let Some(f) = found
                    && f != t
                {
                    self.type_mismatch(value.pos(), &t.name(), &f.name());
                }
            } else if !numeric_ok(&t, op == AssignOp::Rem) {
                self.error(
                    pos,
                    DiagCode::InvalidOperation,
                    vec![("op", s(assign_op_name(op))), ("type", s(&t.name()))],
                );
            } else if let Some(f) = found
                && f != t
            {
                self.type_mismatch(value.pos(), &t.name(), &f.name());
            }
        }
        let arith = match op {
            AssignOp::Set => None,
            AssignOp::Add => Some(ArithOp::Add),
            AssignOp::Sub => Some(ArithOp::Sub),
            AssignOp::Mul => Some(ArithOp::Mul),
            AssignOp::Div => Some(ArithOp::Div),
            AssignOp::Rem => Some(ArithOp::Rem),
        };
        // A whole-value assignment to a variable of a function type does not use the previous value (even after a stop point, it
        // can be used once reassigned)
        if let PlaceRef::Local(slot) = place
            && self.ctx().locals[slot as usize].ty.contains_fn()
        {
            self.fn_access(slot, steps.is_empty() && arith.is_none());
        }
        let kind = if !steps.is_empty() {
            // Updates through fields and indices rewrite the place in place
            StmtKind::Mutate {
                place,
                path: steps,
                indices,
                operands: vec![value_id],
                action: arith.map_or(Action::Set, Action::Arith),
            }
        } else if let Some(op) = arith {
            StmtKind::Compound {
                place,
                op,
                ty: operand,
                value: value_id,
            }
        } else {
            StmtKind::Store {
                place,
                value: value_id,
            }
        };
        self.push_stmt(out, kind, pos.line);
    }

    /// Decides the target of an assignment or update (the variable and how to follow fields and indices). Index expressions
    /// are compiled here (evaluated before the value). If it cannot be resolved, emits a diagnostic and returns `None`.
    pub(super) fn target(
        &mut self,
        module: Option<&(String, Pos)>,
        name: &str,
        path: &[PathElem],
        pos: Pos,
    ) -> Option<TargetInfo> {
        // A leading `name.name` is a field of a variable or a variable of a module
        let (base_var, mut items): (Option<&str>, Vec<PathItem<'_>>) = match module {
            Some((m, _)) if self.var_exists(m) => {
                (Some(m.as_str()), vec![PathItem::Field(name, pos)])
            }
            Some(_) => (None, Vec::new()),
            None => (Some(name), Vec::new()),
        };
        items.extend(path.iter().map(|p| match p {
            PathElem::Field(f, fp) => PathItem::Field(f, *fp),
            PathElem::Index(e, ip) => PathItem::Index(e, *ip),
        }));
        let (place, mut ty) = match base_var {
            Some(v) => {
                let Some((local, slot, ty)) = self.lookup(v) else {
                    self.unknown_name(v, pos);
                    return None;
                };
                (
                    if local {
                        PlaceRef::Local(slot)
                    } else {
                        PlaceRef::Global(slot)
                    },
                    ty,
                )
            }
            None => {
                let (m, mpos) = module.expect("モジュールの変数");
                match self.resolve_target(m, *mpos)? {
                    Target::Module(midx) => match self.modules[midx].global_map.get(name) {
                        Some((slot, ty)) => (PlaceRef::Global(*slot), Some(ty.clone())),
                        None => {
                            self.unknown_qualified(midx, m, name, pos);
                            return None;
                        }
                    },
                    Target::Package => match self.host_var(m, name, pos) {
                        Some((_, _, true)) => {
                            self.error(
                                pos,
                                DiagCode::AssignToReadOnly,
                                vec![("name", s(&format!("{m}.{name}")))],
                            );
                            return None;
                        }
                        Some((idx, ty, false)) => (PlaceRef::Host(idx), Some(ty)),
                        None => return None,
                    },
                }
            }
        };
        // A `fn` can read module variables and host-declared variables, but cannot write them (it can write only its own local
        // variables)
        if self.in_fn() && !matches!(place, PlaceRef::Local(_)) {
            let written = match (base_var, module) {
                (Some(v), _) => v.to_string(),
                (None, Some((m, _))) => format!("{m}.{name}"),
                (None, None) => name.to_string(),
            };
            self.name_error(pos, DiagCode::AssignInFn, &written);
            return None;
        }
        let mut steps = Vec::new();
        let mut indices = Vec::new();
        for item in items {
            let Some(t) = ty.take() else {
                // Variables whose type is not decided (a diagnostic was emitted) cannot be followed
                if let PathItem::Index(e, _) = item {
                    self.expr(e);
                }
                return None;
            };
            match item {
                PathItem::Field(f, fp) => {
                    let Ty::Struct(n) = &t else {
                        self.error(
                            fp,
                            DiagCode::InvalidOperation,
                            vec![("op", s(".")), ("type", s(&t.name()))],
                        );
                        return None;
                    };
                    let Some(TypeDecl {
                        kind: TypeKind::Struct { fields },
                        ..
                    }) = find_type(&self.types, n).cloned()
                    else {
                        return None;
                    };
                    match fields.iter().position(|(fname, _)| fname == f) {
                        Some(i) => {
                            steps.push(PathStep::Field(i as u32));
                            ty = Some(fields[i].1.clone());
                        }
                        None => {
                            let found = suggest(f, fields.iter().map(|(n, _)| n.as_str()));
                            let mut args = vec![("name", s(f)), ("type", s(n))];
                            if !found.is_empty() {
                                args.push(("suggestions", DiagArg::List(found)));
                            }
                            self.error(fp, DiagCode::UnknownField, args);
                            return None;
                        }
                    }
                }
                PathItem::Index(e, ip) => {
                    if let Ty::Map(k, v) = t {
                        let (it, id) = self.expr_ir(e, Some(&k));
                        self.expect_ty(e, it, *k);
                        steps.push(PathStep::Key);
                        indices.push(id);
                        ty = Some(*v);
                        continue;
                    }
                    let (it, id) = self.expr(e);
                    self.expect_ty(e, it, Ty::Int);
                    indices.push(id);
                    let Ty::List(el) = t else {
                        self.error(
                            ip,
                            DiagCode::InvalidOperation,
                            vec![("op", s("[]")), ("type", s(&t.name()))],
                        );
                        return None;
                    };
                    steps.push(PathStep::Index);
                    ty = Some(*el);
                }
            }
        }
        Some(TargetInfo {
            place,
            ty,
            steps,
            indices,
        })
    }

    /// Statements with methods that update a variable (`xs.push(x)`, `xs.insert(i, x)`, `xs.remove(i)`, `xs.clear()`,
    /// `m.remove(k)`, `m.clear()`).
    pub(super) fn method_stmt(
        &mut self,
        (module, name, path): (Option<&(String, Pos)>, &str, &[PathElem]),
        (method, method_pos): (&str, Pos),
        args: &[Expr],
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let unnamed = self.reject_named(method, args);
        let args = unnamed.as_slice();
        let Some(t) = self.target(module, name, path, pos) else {
            self.discard_exprs(args);
            return;
        };
        if let PlaceRef::Local(slot) = t.place {
            self.mark_read(slot);
            if self.ctx().locals[slot as usize].ty.contains_fn() {
                self.fn_use(slot);
            }
        }
        let Some(ty) = t.ty.clone() else {
            self.discard_exprs(args);
            return;
        };
        let (want, action): (Vec<Ty>, Action) = match (&ty, method) {
            (Ty::List(elem), "push") => (vec![(**elem).clone()], Action::Push),
            (Ty::List(elem), "insert") => (vec![Ty::Int, (**elem).clone()], Action::Insert),
            (Ty::List(_), "remove") => (vec![Ty::Int], Action::Remove),
            (Ty::Map(k, _), "remove") => (vec![(**k).clone()], Action::Remove),
            (Ty::List(_) | Ty::Map(..), "clear") => (Vec::new(), Action::Clear),
            (Ty::List(_), "shuffle") => (Vec::new(), Action::Shuffle),
            _ => {
                self.discard_exprs(args);
                self.error(
                    method_pos,
                    DiagCode::UnknownMethod,
                    vec![("name", s(method)), ("type", s(&ty.name()))],
                );
                return;
            }
        };
        if want.len() != args.len() {
            self.discard_exprs(args);
            self.arg_count_error(method, want.len(), args.len(), method_pos);
            return;
        }
        let mut operands = Vec::new();
        for (a, w) in args.iter().zip(&want) {
            let (got, id) = self.expr_ir(a, Some(w));
            self.expect_ty(a, got, w.clone());
            operands.push(id);
        }
        let kind = StmtKind::Mutate {
            place: t.place,
            path: t.steps,
            indices: t.indices,
            operands,
            action,
        };
        self.push_stmt(out, kind, pos.line);
    }

    /// Declares a variable to bind (such as `Some(x)` of `match`) as a local variable. Returns the slot.
    /// Declares a loop variable. `_` is a name that discards: it only takes a place for the value and is not put in scope (it
    /// can be written any number of times and cannot be referred to from the body).
    fn declare_loop_var(&mut self, name: &str, pos: Pos, ty: Option<Ty>) -> u32 {
        if name != "_" {
            return self.declare_local(name, pos, ty);
        }
        let c = self.ctx();
        c.locals.push(LocalInfo {
            name: "$_".to_string(),
            ty: ty.unwrap_or(Ty::Int),
        });
        c.locals.len() as u32 - 1
    }

    pub(super) fn declare_local(&mut self, name: &str, pos: Pos, ty: Option<Ty>) -> u32 {
        if self.lookup(name).is_some() || self.is_import(name) {
            self.name_error(pos, DiagCode::DuplicateName, name);
        }
        self.warn_if_builtin(name, pos);
        if let Some(t) = &ty {
            let m = self.cur_mod;
            self.local_types.push((m, pos, t.name()));
        }
        let c = self.ctx();
        let slot = c.locals.len() as u32;
        c.locals.push(LocalInfo {
            name: name.to_string(),
            ty: ty.clone().unwrap_or(Ty::Int),
        });
        c.scopes
            .last_mut()
            .expect("スコープ")
            .insert(name.to_string(), (slot, ty));
        slot
    }

    /// Compiles `return`. Checks whether there is a return value, and its type, against the declaration of the label or `fn`.
    pub(super) fn return_stmt(&mut self, value: Option<&Expr>, pos: Pos, out: &mut Vec<StmtId>) {
        let ret = self.ctx().ret.clone();
        let value = match (ret, value) {
            (RetTy::Ty(t), Some(e)) => {
                let (found, id) = self.expr_ir(e, Some(&t));
                self.expect_ty(e, found, t);
                Some(id)
            }
            (RetTy::Ty(t), None) => {
                self.error(
                    pos,
                    DiagCode::MissingReturnValue,
                    vec![("expected", s(&t.name()))],
                );
                None
            }
            (RetTy::Bad, Some(e)) => Some(self.expr(e).1),
            (RetTy::Bad, None) => None,
            (RetTy::None, Some(e)) => {
                self.expr(e);
                self.error(e.pos(), DiagCode::UnexpectedReturnValue, vec![]);
                None
            }
            (RetTy::None, None) => None,
        };
        self.push_stmt(out, StmtKind::Return(value), pos.line);
    }
}
