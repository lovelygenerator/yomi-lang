//! Compilation: `@` commands and calls of package functions.

use super::*;

impl Compiler<'_> {
    /// Checks the number and types of a command's arguments (emitting diagnostics). `true` if they match.
    pub(super) fn check_command_args(
        &mut self,
        name: &str,
        want: &[Ty],
        args: &[Expr],
        types: &[Option<Ty>],
        pos: Pos,
    ) -> bool {
        if want.len() != args.len() {
            self.arg_count_error(name, want.len(), args.len(), pos);
            return false;
        }
        for ((want, got), a) in want.iter().zip(types).zip(args) {
            self.check_arg_type(a, Some(want), got.as_ref());
        }
        true
    }

    /// When using a package's command, checks that the package is `import`ed.
    pub(super) fn check_package_imported(&mut self, package: &Option<String>, pos: Pos) {
        if let Some(p) = package {
            if self.md().pkg_imports.contains(p) {
                self.mark_import(p);
            } else {
                self.name_error(pos, DiagCode::ModuleNotImported, p);
            }
        }
    }

    /// A call of an effect command (`@package.command(args)`). If `bind` is false, it is a statement and the return value is
    /// discarded. If true, it is the right-hand side of `let` or `=`, and the return type is returned (commands returning `()`
    /// give `invalid_command_use`).
    pub(super) fn effect_call(
        &mut self,
        info: &crate::registry::CommandInfo,
        call: &CommandCall,
        bind: bool,
    ) -> Typed {
        let (written, pos) = (call.name.as_str(), call.pos);
        let specs = Self::command_specs(&info.params, &info.defaults);
        let arranged = self.arrange_args(written, &call.args, &specs, pos);
        let args = arranged.as_slice();
        let name = info.name.as_str();
        // Only stop lines can have stable IDs
        if let Some(sid) = &call.id {
            self.error(sid.pos, DiagCode::InvalidStableId, vec![("id", s(&sid.id))]);
        }
        let (types, ids): (Vec<Option<Ty>>, Vec<ExprId>) = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.expr_ir(a, info.args.get(i)))
            .unzip();
        self.check_package_imported(&info.package, pos);
        if bind && info.ret == Ty::Unit {
            self.error(
                pos,
                DiagCode::InvalidCommandUse,
                vec![("name", s(written)), ("kind", s(info.kind.name()))],
            );
            return self.failed();
        }
        if !self.check_command_args(written, &info.args, args, &types, pos) {
            return (bind.then(|| info.ret.clone()), self.error_node());
        }
        let native = self.native_index(name);
        // Even for functions that return values, an `@` statement discards the return value
        let id = self.node(ExprKind::CallEffect { native, args: ids }, pos.line);
        (bind.then(|| info.ret.clone()), id)
    }

    /// The sequential number of an effect or query command used (added on first use).
    pub(super) fn native_index(&mut self, name: &str) -> u32 {
        match self.natives.iter().position(|c| c == name) {
            Some(i) => i as u32,
            None => {
                self.natives.push(name.to_string());
                (self.natives.len() - 1) as u32
            }
        }
    }

    /// A call of a query command (`package.command(args)` in an expression).
    pub(super) fn query_call(
        &mut self,
        package: &str,
        member: &str,
        args: &[Expr],
        p: Pos,
    ) -> Typed {
        let name = format!("{package}.{member}");
        // A `fn` of this module or an `import`ed module (`module.name(...)`)
        if let Some(midx) = self.peek_module(package) {
            if let Some(&flat) = self.modules[midx].fn_map.get(member) {
                let (t, id) = self.fn_call(flat, &name, args, p, true);
                return (t, id);
            }
            self.discard_exprs(args);
            self.unknown_function(midx, package, member, p);
            return self.failed();
        }
        if self.module_last.contains_key(package) && !self.registry.has_namespace(package) {
            self.discard_exprs(args);
            self.name_error(p, DiagCode::ModuleNotImported, package);
            return self.failed();
        }
        let found_info = self.registry.find_command(&name);
        if found_info.is_some() && self.impure_use(p, "command") {
            self.discard_exprs(args);
            return self.failed();
        }
        let arranged = match &found_info {
            Some(i) => {
                let specs = Self::command_specs(&i.params, &i.defaults);
                self.arrange_args(&name, args, &specs, p)
            }
            None => args.to_vec(),
        };
        let args = arranged.as_slice();
        let want = found_info.as_ref().map(|i| i.args.clone());
        let (types, ids): (Vec<Option<Ty>>, Vec<ExprId>) = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.expr_ir(a, want.as_ref().and_then(|w| w.get(i))))
            .unzip();
        let Some(info) = self.registry.find_command(&name) else {
            if self.registry.has_namespace(package) {
                if self.md().pkg_imports.contains(package) {
                    self.mark_import(package);
                    let found = suggest(&name, self.registry.command_names());
                    self.error(p, DiagCode::UnknownCommand, name_args(&name, found));
                } else {
                    self.name_error(p, DiagCode::ModuleNotImported, package);
                }
            } else if self.module_last.contains_key(package) || self.is_import(package) {
                let found = suggest(&name, self.registry.command_names());
                self.error(p, DiagCode::UnknownCommand, name_args(&name, found));
            } else {
                self.unknown_module(package, p, false);
            }
            return self.failed();
        };
        if info.kind != CommandKind::Query {
            self.error(
                p,
                DiagCode::InvalidCommandUse,
                vec![("name", s(&name)), ("kind", s(info.kind.name()))],
            );
            return self.failed();
        }
        self.warn_alias(&name, &info, p);
        self.check_package_imported(&info.package, p);
        if !self.check_command_args(&name, &info.args, args, &types, p) {
            return (Some(info.ret), self.error_node());
        }
        let native = self.native_index(&info.name);
        let id = self.node(ExprKind::CallQuery { native, args: ids }, p.line);
        (Some(info.ret), id)
    }

    /// An `@` statement.
    pub(super) fn command(&mut self, call: &CommandCall, out: &mut Vec<StmtId>) {
        let found = if self.registry.find_command(&call.name).is_none() {
            self.fn_statement(call, false)
        } else {
            None
        };
        let (_, expr) = match found {
            Some(r) => r,
            None => self.command_call(call, false),
        };
        self.push_stmt(out, StmtKind::Eval(expr), call.pos.line);
    }

    /// `@command(args)` on the right-hand side of `let` or `=`. Returns the return type (the type of the `let` variable). Only
    /// effect and stop commands whose return value is not `()` and whose type can be stored in script variables (`int`,
    /// `float`, `bool`, `str`, `Identity`, and `list`, `map` and `Option` of them) can be received.
    pub(super) fn bound_command(&mut self, call: &CommandCall) -> Typed {
        let (t, id) = self.command_call(call, true);
        let Some(t) = t else {
            return (None, id);
        };
        if bindable(&t) {
            (Some(t), id)
        } else {
            self.error(
                call.pos,
                DiagCode::NotYetSupported,
                vec![("what", s(&t.name()))],
            );
            (None, id)
        }
    }

    /// `@command(args)` written in the middle of an expression (`invalid_command_use`; `unknown_command` if the command does
    /// not exist).
    pub(super) fn misplaced_command(&mut self, call: &CommandCall) {
        self.discard_exprs(&call.args);
        match self.registry.find_command(&call.name) {
            Some(info) => self.error(
                call.pos,
                DiagCode::InvalidCommandUse,
                vec![("name", s(&call.name)), ("kind", s(info.kind.name()))],
            ),
            None => {
                let found = suggest(&call.name, self.registry.command_names());
                self.error(
                    call.pos,
                    DiagCode::UnknownCommand,
                    name_args(&call.name, found),
                );
            }
        }
    }

    /// The warning for calling by an old name (alias) (`deprecated_command_name`).
    pub(super) fn warn_alias(
        &mut self,
        written: &str,
        info: &crate::registry::CommandInfo,
        pos: Pos,
    ) {
        if info.via_alias {
            self.warn(
                pos,
                DiagCode::DeprecatedCommandName,
                vec![("name", s(written)), ("new", s(&info.name))],
            );
        }
    }

    /// A command call. If `bind` is true, the return value is received (the return type is returned).
    pub(super) fn command_call(&mut self, call: &CommandCall, bind: bool) -> Typed {
        let (written, args, id, pos) = (call.name.as_str(), &call.args, call.id.as_ref(), call.pos);
        let found = self.registry.find_command(written);
        if self.in_fn()
            && let Some(info) = &found
            && info.kind != CommandKind::Query
        {
            // Effect and stop commands cannot be called inside a `fn` (errors in arguments are still found)
            self.discard_exprs(args);
            self.not_allowed_in_fn(&format!("@{written}"), pos);
            return self.failed();
        }
        // From here, `name` is the official name (if called by an old name, emit a warning)
        let name = found
            .as_ref()
            .map_or(written, |i| i.name.as_str())
            .to_string();
        let name = name.as_str();
        if let Some(info) = &found {
            self.warn_alias(written, info, pos);
            match info.kind {
                CommandKind::Effect => return self.effect_call(info, call, bind),
                CommandKind::Query => {
                    // Query commands are called inside expressions (without `@`)
                    self.discard_exprs(args);
                    self.error(
                        pos,
                        DiagCode::InvalidCommandUse,
                        vec![("name", s(written)), ("kind", s(info.kind.name()))],
                    );
                    return self.failed();
                }
                CommandKind::Stop => {}
            }
        }
        let arranged = match &found {
            Some(i) => {
                let specs = Self::command_specs(&i.params, &i.defaults);
                self.arrange_args(written, args, &specs, pos)
            }
            None => args.clone(),
        };
        let args = &arranged;
        let want = self
            .registry
            .command_id(name)
            .map(|id| self.registry.command(id).args.clone());
        let (types, stop_args): (Vec<Option<Ty>>, Vec<StopArg>) = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.stop_arg(a, want.as_ref().and_then(|w| w.get(i))))
            .unzip();
        let Some(cid) = self.registry.command_id(name) else {
            let found = suggest(written, self.registry.command_names());
            self.error(pos, DiagCode::UnknownCommand, name_args(written, found));
            return self.failed();
        };
        let decl = self.registry.command(cid).clone();
        let package = found.as_ref().and_then(|i| i.package.clone());
        self.check_package_imported(&package, pos);
        if bind && decl.ret == Ty::Unit {
            self.error(
                pos,
                DiagCode::InvalidCommandUse,
                vec![("name", s(written)), ("kind", s(CommandKind::Stop.name()))],
            );
            return self.failed();
        }
        if !self.check_command_args(written, &decl.args, args, &types, pos) {
            return (bind.then(|| decl.ret.clone()), self.error_node());
        }
        let written_args = crate::syntax::canon::args(&call.args);
        let hash = fnv1a(&[name.as_bytes(), written_args.as_bytes()]);
        self.note_id_site(StopKind::Command, id, call.id_at);
        // When a translatable argument (such as an input prompt) is written as a string literal, require an ID on that line
        // (`require_text_ids`) and keep the original text
        let text_arg = decl.text_param.and_then(|i| match args.get(i) {
            Some(Expr::Str(parts, _)) => Some((i, parts)),
            _ => None,
        });
        if text_arg.is_some() {
            self.require_id(id, "command", pos);
        }
        let stop = self.make_stop(name, stop_args, id, hash, pos, bind);
        if let Some((i, parts)) = text_arg {
            let values = args[..i]
                .iter()
                .filter(|a| matches!(a, Expr::Str(..)))
                .count() as u32;
            let text = text_parts_of(parts);
            let info = self.stop_info_mut(stop);
            info.text = text;
            info.text_arg = Some(TextArg {
                arg: i as u32,
                values,
            });
        }
        (bind.then(|| decl.ret.clone()), stop)
    }

    /// The sequential number of a command used (added on first use).
    pub(super) fn command_index(&mut self, name: &str) -> u32 {
        match self.commands.iter().position(|c| c == name) {
            Some(i) => i as u32,
            None => {
                self.commands.push(name.to_string());
                (self.commands.len() - 1) as u32
            }
        }
    }

    /// Checks that the string literals of `seen` and `visited` point to existing stop points and labels (warnings).
    pub(super) fn check_literals(&mut self, labels: &[LabelCode], refs: &[LabelRefs]) {
        let literals = std::mem::take(&mut self.literals);
        for lit in literals {
            let (target, id) = match lit.text.split_once('#') {
                Some((t, id)) if lit.seen => (t, Some(id)),
                _ => (lit.text.as_str(), None),
            };
            let (module, label) = match target.split_once('.') {
                Some((m, l)) => (self.module_index.get(m).copied(), l),
                None => (Some(self.repl_name_module(lit.module)), target),
            };
            let found = module.and_then(|m| {
                labels
                    .iter()
                    .position(|l| l.module as usize == m && l.name == label)
            });
            let exists = match (found, id) {
                (Some(i), Some(id)) => refs[i].stop_ids.iter().any(|s| s == id),
                (Some(_), None) => !lit.seen,
                (None, _) => false,
            };
            if !exists {
                self.cur_mod = lit.module;
                if lit.seen {
                    self.warn(
                        lit.pos,
                        DiagCode::UnknownPosition,
                        vec![("position", s(&lit.text))],
                    );
                } else {
                    self.warn(
                        lit.pos,
                        DiagCode::UnknownLabelLiteral,
                        vec![("label", s(&lit.text))],
                    );
                }
            }
        }
    }
}
