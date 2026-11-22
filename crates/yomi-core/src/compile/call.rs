//! Compilation: calls of functions and labels, and built-in functions.

use super::*;

impl Compiler<'_> {
    /// Arranges the arguments of a call (positional and named) in the order of the callee's parameters. Omitted arguments are
    /// filled with default values (`Expr::Default`). Errors (unknown names, duplicates, wrong order, positional arguments after
    /// named ones, missing ones) emit diagnostics and fill that place with `()` (so as not to also report the number of
    /// arguments).
    ///
    /// When there are no named arguments and no parameters with defaults, returns them as is (a count mismatch becomes
    /// `arg_count` on the caller's side). When there are too many positional arguments, returns only the positional ones.
    pub(super) fn arrange_args(
        &mut self,
        callee: &str,
        args: &[Expr],
        params: &[ParamSpec],
        pos: Pos,
    ) -> Vec<Expr> {
        let named = args.iter().any(|a| matches!(a, Expr::Named(..)));
        let has_default = params.iter().any(|p| p.default.is_some());
        if !named && (!has_default || args.len() >= params.len()) {
            return args.to_vec();
        }
        let positional: Vec<&Expr> = args
            .iter()
            .filter(|a| !matches!(a, Expr::Named(..)))
            .collect();
        if positional.len() > params.len() {
            return positional.into_iter().cloned().collect();
        }
        let mut slots: Vec<Option<Expr>> = vec![None; params.len()];
        let mut last: Option<usize> = None;
        let mut after_named = false;
        let mut reported_positional = false;
        let mut k = 0;
        for a in args {
            match a {
                Expr::Named(arg, value, p) => {
                    after_named = true;
                    let Some(i) = params.iter().position(|q| q.name.as_deref() == Some(arg)) else {
                        self.error(
                            *p,
                            DiagCode::UnknownArgument,
                            vec![("name", s(callee)), ("arg", s(arg))],
                        );
                        continue;
                    };
                    if slots[i].is_some() {
                        self.error(
                            *p,
                            DiagCode::DuplicateArgument,
                            vec![("name", s(callee)), ("arg", s(arg))],
                        );
                    } else if last.is_some_and(|l| l > i) {
                        self.error(
                            *p,
                            DiagCode::ArgumentOutOfOrder,
                            vec![("name", s(callee)), ("arg", s(arg))],
                        );
                        slots[i] = Some(Expr::Default(Value::Unit, *p));
                    } else {
                        slots[i] = Some((**value).clone());
                        last = Some(i);
                    }
                }
                _ => {
                    if after_named {
                        if !reported_positional {
                            self.name_error(a.pos(), DiagCode::PositionalAfterNamed, callee);
                            reported_positional = true;
                        }
                        continue;
                    }
                    slots[k] = Some(a.clone());
                    last = Some(k);
                    k += 1;
                }
            }
        }
        slots
            .into_iter()
            .zip(params)
            .enumerate()
            .map(|(i, (slot, spec))| {
                slot.unwrap_or_else(|| match &spec.default {
                    Some(v) => Expr::Default(v.clone(), pos),
                    None => {
                        let arg = spec.name.clone().unwrap_or_else(|| i.to_string());
                        self.error(
                            pos,
                            DiagCode::MissingArgument,
                            vec![("name", s(callee)), ("arg", s(&arg))],
                        );
                        Expr::Default(Value::Unit, pos)
                    }
                })
            })
            .collect()
    }

    /// The parameter names and default values of a registry command.
    pub(super) fn command_specs(
        params: &[Option<String>],
        defaults: &[Option<Value>],
    ) -> Vec<ParamSpec> {
        params
            .iter()
            .zip(defaults)
            .map(|(n, d)| ParamSpec {
                name: n.clone(),
                default: d.clone(),
            })
            .collect()
    }

    /// Looks up the name of a `fn` written in a script (`name` or `module.name`) without emitting diagnostics.
    /// A plain name is a `fn` of this module. `module.name` is a `fn` of this module or an `import`ed module.
    pub(super) fn lookup_fn(&self, written: &str) -> Option<u32> {
        match written.split_once('.') {
            None => self.md().fn_map.get(written).copied(),
            Some((m, member)) => {
                let midx = self.peek_module(m)?;
                self.modules[midx].fn_map.get(member).copied()
            }
        }
    }

    /// A call of a `fn`. If `want_value` is true, the return value is used (a `fn` without a return value gives
    /// `call_without_value`). Returns the return type and the IR expression.
    pub(super) fn fn_call(
        &mut self,
        flat: u32,
        written: &str,
        args: &[Expr],
        pos: Pos,
        want_value: bool,
    ) -> (Option<Ty>, ExprId) {
        if self.impure_use(pos, "fn") {
            self.discard_exprs(args);
            return self.failed();
        }
        let sig = self.fn_sigs[flat as usize].clone();
        let arranged = self.arrange_args(written, args, &sig.specs(), pos);
        let args = arranged.as_slice();
        let (types, ids): (Vec<Option<Ty>>, Vec<ExprId>) = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.expr_ir(a, sig.params.get(i).and_then(Option::as_ref)))
            .unzip();
        if sig.params.len() != args.len() {
            self.arg_count_error(written, sig.params.len(), args.len(), pos);
        } else {
            for ((want, got), a) in sig.params.iter().zip(&types).zip(args) {
                self.check_arg_type(a, want.as_ref(), got.as_ref());
            }
        }
        let id = self.node(ExprKind::CallFn { f: flat, args: ids }, pos.line);
        match sig.ret {
            RetTy::None => {
                if want_value {
                    self.name_error(pos, DiagCode::CallWithoutValue, written);
                }
                (None, id)
            }
            RetTy::Ty(t) => (Some(t), id),
            RetTy::Bad => (None, id),
        }
    }

    /// The diagnostic for an undeclared `fn` (`module.name`). Close names are chosen from that module's `fn`s.
    pub(super) fn unknown_function(&mut self, midx: usize, module: &str, member: &str, pos: Pos) {
        if self.modules[midx].poisoned.contains(member) {
            return;
        }
        let found: Vec<String> =
            suggest(member, self.modules[midx].fn_map.keys().map(String::as_str))
                .into_iter()
                .map(|f| format!("{module}.{f}"))
                .collect();
        self.error(
            pos,
            DiagCode::UnknownFunction,
            name_args(&format!("{module}.{member}"), found),
        );
    }

    /// A label call (`call`). Checks the arguments and makes an IR expression. `want_value` tells whether it is written where
    /// the return value is received (the right-hand side of `let` or `=`). Returns the return type (`None` if no value) and
    /// the IR expression.
    pub(super) fn label_call(
        &mut self,
        call: &LabelCall,
        want_value: bool,
    ) -> (Option<Ty>, ExprId) {
        if self.in_fn() {
            self.not_allowed_in_fn("call", call.pos);
            return (None, self.error_node());
        }
        let resolved = self.resolve_label(&call.target, call.pos);
        let arranged = match &resolved {
            Some((_, sig)) => {
                let written = call.target.written();
                self.arrange_args(&written, &call.args, &sig.specs(), call.pos)
            }
            None => call.args.clone(),
        };
        let (types, ids): (Vec<Option<Ty>>, Vec<ExprId>) = arranged
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let hint = resolved
                    .as_ref()
                    .and_then(|(_, sig)| sig.params.get(i))
                    .and_then(Option::as_ref);
                self.expr_ir(a, hint)
            })
            .unzip();
        let Some((flat, sig)) = resolved else {
            return (None, self.error_node());
        };
        self.fn_stop(call.pos, "call");
        let written = call.target.written();
        if sig.params.len() != arranged.len() {
            self.arg_count_error(&written, sig.params.len(), arranged.len(), call.pos);
        } else {
            for ((want, got), a) in sig.params.iter().zip(&types).zip(&arranged) {
                self.check_arg_type(a, want.as_ref(), got.as_ref());
            }
        }
        self.ctx().refs.targets.push(flat);
        let written_args = crate::syntax::canon::args(&call.args);
        let hash = fnv1a(&[b"call", written.as_bytes(), written_args.as_bytes()]);
        let id = self.node(
            ExprKind::CallLabel {
                label: flat,
                args: ids,
                hash,
            },
            call.pos.line,
        );
        match sig.ret {
            RetTy::None => {
                if want_value {
                    self.name_error(call.pos, DiagCode::CallWithoutValue, &written);
                }
                (None, id)
            }
            RetTy::Ty(t) => (Some(t), id),
            RetTy::Bad => (None, id),
        }
    }

    /// A call of a built-in function.
    pub(super) fn call(&mut self, name: &str, args: &[Expr], p: Pos) -> Typed {
        // A `fn` of this module takes priority over a built-in function with the same name
        if let Some(&flat) = self.md().fn_map.get(name) {
            let (t, id) = self.fn_call(flat, name, args, p, true);
            return (t, id);
        }
        // A variable of a function type takes priority over a built-in function with the same name (a `fn` of the same module
        // takes priority over the variable)
        if let Some((local, slot, ty @ Some(Ty::Fn(..)))) = self.lookup(name) {
            let place = if local {
                self.fn_use(slot);
                self.mark_read(slot);
                PlaceRef::Local(slot)
            } else {
                PlaceRef::Global(slot)
            };
            if !local && self.impure_use(p, "variable") {
                self.discard_exprs(args);
                return self.failed();
            }
            let callee = (ty, self.node(ExprKind::Var(place), p.line));
            return self.call_value(callee, name, args, p);
        }
        // Built-in functions do not accept named arguments (their parameters have no names)
        let unnamed = self.reject_named(name, args);
        let args = unnamed.as_slice();
        let (types, ids): (Vec<Option<Ty>>, Vec<ExprId>) =
            args.iter().map(|a| self.expr_ir(a, None)).unzip();
        // A variable or command with the same name takes priority over the built-in function. However, variables that are not of
        // function types and commands cannot be called in the form `name(…)`, so it becomes `NotYetSupported`.
        if self.lookup(name).is_some() || self.registry.command_id(name).is_some() {
            self.error(p, DiagCode::NotYetSupported, vec![("what", s("call"))]);
            return self.failed();
        }
        let Some(b) = Builtin::from_name(name) else {
            if self.md().label_map.contains_key(name) {
                self.error(p, DiagCode::NotYetSupported, vec![("what", s("call"))]);
            } else if !self.md().poisoned.contains(name) {
                let found = suggest(
                    name,
                    BUILTIN_NAMES
                        .iter()
                        .copied()
                        .chain(self.md().fn_map.keys().map(String::as_str)),
                );
                self.error(p, DiagCode::UnknownFunction, name_args(name, found));
            }
            return self.failed();
        };
        let state = match b {
            Builtin::Random | Builtin::RandomFloat => Some("random"),
            Builtin::Seen => Some("seen"),
            Builtin::Visited => Some("visited"),
            _ => None,
        };
        if let Some(what) = state
            && self.impure_use(p, what)
        {
            return self.failed();
        }
        let params = b.params();
        if args.len() != params.len() {
            self.arg_count_error(name, params.len(), args.len(), p);
            return (Some(b.ret()), self.error_node());
        }
        for ((want, got), a) in params.iter().zip(&types).zip(args) {
            self.check_arg_type(a, Some(want), got.as_ref());
        }
        // The existence of the string literals of `seen` and `visited` is checked after all labels are compiled
        if matches!(b, Builtin::Seen | Builtin::Visited)
            && let Expr::Str(parts, _) = &args[0]
            && let [StrPart::Text(text, _)] = parts.as_slice()
        {
            self.literals.push(LiteralArg {
                module: self.cur_mod,
                pos: args[0].pos(),
                seen: matches!(b, Builtin::Seen),
                text: text.clone(),
            });
        }
        let id = self.node(
            ExprKind::Builtin {
                builtin: b,
                args: ids,
            },
            p.line,
        );
        (Some(b.ret()), id)
    }
}
