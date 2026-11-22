//! Compilation: bodies (plain lines, quote lines), identities, options, stop points.

use super::*;

impl Compiler<'_> {
    /// Compiles a plain line. It becomes an instruction that calls the stop command bound to plain lines with the body as the
    /// argument (an error if not bound).
    pub(super) fn plain(
        &mut self,
        parts: &[StrPart],
        raw: &str,
        id_site: (Option<&StableId>, Pos),
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let (id, id_at) = id_site;
        let Some(binding) = self.registry.plain_binding() else {
            self.error(pos, DiagCode::UnboundSugar, vec![("kind", s("plain"))]);
            return;
        };
        self.note_id_site(StopKind::Plain, id, id_at);
        self.require_id(id, "plain", pos);
        let text = self.text_parts(parts, pos.line, true);
        // For an ignored sugar, only syntax and types are checked; no code is made
        let Binding::Command(cid) = binding else {
            return;
        };
        let name = self.registry.command(cid).name.clone();
        let body = plain_body(parts, raw);
        let hash = fnv1a(&[name.as_bytes(), body.as_bytes()]);
        let args = vec![StopArg {
            expr: text,
            text: true,
        }];
        let stop = self.make_stop(&name, args, id, hash, pos, false);
        self.mark_text_stop(stop, StopKind::Plain, parts);
        self.push_stmt(out, StmtKind::Eval(stop), pos.line);
    }

    /// The names of usable identities (this module's and those of `import`ed modules).
    pub(super) fn usable_identities(&self) -> Vec<String> {
        let mut names: Vec<String> = self.md().identities.keys().cloned().collect();
        for &i in self.md().imports.values() {
            names.extend(self.modules[i].identities.keys().cloned());
        }
        names.sort();
        names.dedup();
        names
    }

    /// If the name in `name「` at the start of a plain line has a spelling close to an identity, warns
    /// (section 6.4 of `spec/language/06-diagnostics.md`).
    pub(super) fn warn_identity_typo(&mut self, name: &str, pos: Pos) {
        let names = self.usable_identities();
        let found = suggest(name, names.iter().map(String::as_str));
        if !found.is_empty() {
            self.warn(pos, DiagCode::IdentityTypo, name_args(name, found));
        }
    }

    /// Resolves the identity of a quote line. If it cannot be resolved, emits a diagnostic and returns `None`.
    pub(super) fn resolve_identity(&mut self, r: &IdentRef) -> Option<Identity> {
        self.resolve_identity_with(r, false)
    }

    /// Resolves the name of an identity in an expression (an `Identity` constant). Searches all declarations regardless of
    /// declaration order.
    pub(super) fn resolve_identity_const(&mut self, r: &IdentRef) -> Option<Identity> {
        self.resolve_identity_with(r, true)
    }

    /// Resolves an identity. If `early`, searches the table of declarations picked up first.
    pub(super) fn resolve_identity_with(&mut self, r: &IdentRef, early: bool) -> Option<Identity> {
        let make = |display: &String| Identity {
            id: r.name.clone(),
            name: display.clone(),
        };
        let lookup = |this: &Self, midx: usize, name: &str| -> Option<String> {
            let m = &this.modules[midx];
            if early {
                m.early_identities.get(name).cloned()
            } else {
                m.identities.get(name).cloned()
            }
        };
        if let Some((m, mpos)) = &r.module {
            let midx = self.resolve_module(m, *mpos)?;
            return lookup(self, midx, &r.name).as_ref().map(make);
        }
        // Identities declared in the same module come before those of `import`ed modules
        if let Some(display) = lookup(self, self.cur_mod, &r.name) {
            return Some(make(&display));
        }
        let mut found: Vec<(String, String)> = self
            .md()
            .imports
            .iter()
            .filter_map(|(m, &i)| lookup(self, i, &r.name).map(|d| (m.clone(), d)))
            .collect();
        found.sort();
        for (m, _) in &found {
            self.mark_import(m);
        }
        match found.len() {
            0 => None,
            1 => Some(make(&found[0].1)),
            _ => {
                let candidates = found
                    .iter()
                    .map(|(m, _)| format!("{m}.{}", r.name))
                    .collect();
                self.error(
                    r.pos,
                    DiagCode::AmbiguousIdentity,
                    vec![
                        ("name", s(&r.name)),
                        ("candidates", DiagArg::List(candidates)),
                    ],
                );
                None
            }
        }
    }

    /// Compiles a quote line (`speaker「line」`). Calls what quote lines are bound to, with the speaker (identity) and the body
    /// as arguments.
    pub(super) fn quote(
        &mut self,
        identity: Option<&IdentRef>,
        parts: &[StrPart],
        id_site: (Option<&StableId>, Pos),
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let (id, id_at) = id_site;
        let resolved = identity.and_then(|r| self.resolve_identity(r));
        let Some(binding) = self.registry.quote_binding() else {
            self.error(pos, DiagCode::UnboundSugar, vec![("kind", s("quote"))]);
            return;
        };
        self.note_id_site(StopKind::Quote, id, id_at);
        self.require_id(id, "quote", pos);
        let ident_value = Value::Option(
            resolved
                .clone()
                .map(|i| Box::new(Value::Identity(i.into()))),
        );
        let who_arg = self.node(ExprKind::Const(ident_value), pos.line);
        let text = self.text_parts(parts, pos.line, true);
        // For an ignored sugar, only syntax and types are checked; no code is made
        let Binding::Command(cid) = binding else {
            return;
        };
        let name = self.registry.command(cid).name.clone();
        let who = resolved.as_ref().map_or("", |i| i.id.as_str());
        let body = quoted_body(parts);
        let hash = fnv1a(&[name.as_bytes(), who.as_bytes(), body.as_bytes()]);
        let args = vec![
            StopArg {
                expr: who_arg,
                text: false,
            },
            StopArg {
                expr: text,
                text: true,
            },
        ];
        let stop = self.make_stop(&name, args, id, hash, pos, false);
        self.mark_text_stop(stop, StopKind::Quote, parts);
        self.push_stmt(out, StmtKind::Eval(stop), pos.line);
    }

    /// `choice { ... }`. Builds the list of options, stops, and moves to the body of the chosen option.
    ///
    /// Only the part that reads the bodies (which may nest) is here. The rest is separated from the recursive path because
    /// its frame is large ([`Compiler::choice_prepare`], [`Compiler::choice_finish`]).
    pub(super) fn choice(&mut self, options: &[ChoiceOpt], pos: Pos, out: &mut Vec<StmtId>) {
        let Some(mut st) = self.choice_prepare(options, pos) else {
            return;
        };
        for (o, ir) in options.iter().zip(&mut st.options) {
            ir.body = self.block(&o.body);
        }
        self.choice_finish(st, pos, out);
    }

    /// The part of `choice` before the bodies (the list of options, the stop, the branch table). `None` if there is no sugar.
    #[inline(never)]
    pub(super) fn choice_prepare(
        &mut self,
        options: &[ChoiceOpt],
        pos: Pos,
    ) -> Option<ChoiceState> {
        let Some(binding) = self.registry.choice_binding() else {
            self.error(pos, DiagCode::UnboundSugar, vec![("kind", s("choice"))]);
            return None;
        };
        let refs_before = {
            let r = &self.ctx().refs;
            (r.stop_ids.len(), r.targets.len())
        };
        let mut infos = Vec::new();
        let mut opts = Vec::new();
        for o in options {
            let line = o.pos.line;
            if let Some(op) = o.once
                && o.id.is_none()
            {
                self.error(op, DiagCode::OnceWithoutId, vec![]);
            }
            // Stable IDs of an ignored sugar are not checked for duplicates (same as plain lines and quote lines)
            if !matches!(binding, Binding::Ignored)
                && let Some(sid) = &o.id
                && !self.ctx().ids.insert(sid.id.clone())
            {
                self.error(
                    sid.pos,
                    DiagCode::DuplicateStableId,
                    vec![("id", s(&sid.id))],
                );
            }
            self.require_id(o.id.as_ref(), "choice", o.pos);
            self.note_id_site(StopKind::Choice, o.id.as_ref(), o.id_at);
            // The condition to include in the list: `once` not already chosen, and `if` is true
            let once = o.once.and(o.id.as_ref()).map(|i| i.id.clone());
            let cond = o.cond.as_ref().map(|c| self.condition(c));
            let text = self.text_parts(&o.parts, line, true);
            let id = Value::Option(
                o.id.as_ref()
                    .map(|i| Box::new(Value::Str(i.id.clone().into()))),
            );
            opts.push(IrChoiceOpt {
                once,
                cond,
                text,
                id,
                body: Vec::new(),
                line,
            });
            infos.push(ChoiceOptInfo {
                id: o.id.as_ref().map(|i| i.id.clone()),
                text: text_parts_of(&o.parts),
                line,
            });
        }
        // Stop
        let name = match binding {
            Binding::Command(cid) => self.registry.command(cid).name.clone(),
            Binding::Ignored => String::new(),
        };
        let cmd = if name.is_empty() {
            0
        } else {
            self.command_index(&name)
        };
        let raws: Vec<String> = options.iter().map(|o| quoted_body(&o.parts)).collect();
        let mut parts: Vec<&[u8]> = vec![name.as_bytes()];
        parts.extend(raws.iter().map(|r| r.as_bytes()));
        let hash = fnv1a(&parts);
        let ids = infos.iter().filter_map(|o: &ChoiceOptInfo| o.id.clone());
        self.ctx().refs.stop_ids.extend(ids);
        let info = StopInfo {
            kind: StopKind::Choice,
            text: Vec::new(),
            text_arg: None,
            args_pc: 0,
            pc: 0,
            id: None,
            hash,
            line: pos.line,
            choice: Some(ChoiceInfo { options: infos }),
            binds: false,
        };
        self.fn_stop(pos, "stop");
        Some(ChoiceState {
            ignored: matches!(binding, Binding::Ignored),
            cmd,
            options: opts,
            info,
            refs_before,
        })
    }

    /// The part of `choice` after the bodies (makes it an IR statement).
    #[inline(never)]
    pub(super) fn choice_finish(&mut self, st: ChoiceState, pos: Pos, out: &mut Vec<StmtId>) {
        // For an ignored sugar, only syntax and types were checked. No code is made, and it is not a stop point.
        if st.ignored {
            let (ids, targets) = st.refs_before;
            let r = &mut self.ctx().refs;
            r.stop_ids.truncate(ids);
            r.targets.truncate(targets);
            return;
        }
        let choice = Choice {
            cmd: st.cmd,
            options: st.options,
            info: st.info,
        };
        self.push_stmt(out, StmtKind::Choice(Box::new(choice)), pos.line);
    }

    /// The expression of an argument of a stop command. A string literal is treated as a body, and the values of `${expr}` are
    /// recorded for the history.
    pub(super) fn stop_arg(&mut self, e: &Expr, hint: Option<&Ty>) -> (Option<Ty>, StopArg) {
        if let Expr::Str(parts, p) = e {
            let expr = self.text_parts(parts, p.line, true);
            return (Some(Ty::Str), StopArg { expr, text: true });
        }
        let (t, expr) = self.expr_ir(e, hint);
        (t, StopArg { expr, text: false })
    }

    /// Turns string parts into one string expression. If `record` is true, the values of `${expr}` are added to the current
    /// record of the body (the body of a stop point; nested string literals are not recorded).
    pub(super) fn text_parts(&mut self, parts: &[StrPart], line: u32, record: bool) -> ExprId {
        let mut pieces = Vec::new();
        for p in parts {
            match p {
                StrPart::Text(t, _) => pieces.push(TextPiece::Lit(t.clone())),
                StrPart::Interp(e) => {
                    let (t, expr) = self.expr_ir(e, None);
                    pieces.push(TextPiece::Interp {
                        expr,
                        to_str: t != Some(Ty::Str),
                    });
                }
            }
        }
        self.node(
            ExprKind::Text {
                parts: pieces,
                record,
            },
            line,
        )
    }

    /// Records a line that can have a stable ID (plain line, quote line, option text, stop command line) (for `assign-ids`).
    /// `id_at` is where `#id` is appended when there is no ID.
    pub(super) fn note_id_site(&mut self, kind: StopKind, id: Option<&StableId>, id_at: Pos) {
        let module = self.cur_mod as u32;
        let label = self.ctx().name.clone();
        self.id_sites.push(IdSite {
            module,
            label,
            kind,
            id: id.map(|i| i.id.clone()),
            line: id_at.line,
            col: id_at.col,
        });
    }

    /// When `require_text_ids` is enabled, makes text lines without a stable ID errors. `kind` is `plain`, `quote` or
    /// `choice`.
    pub(super) fn require_id(&mut self, id: Option<&StableId>, kind: &str, pos: Pos) {
        if self.require_text_ids && id.is_none() {
            self.error(pos, DiagCode::MissingTextId, vec![("kind", s(kind))]);
        }
    }

    /// Makes the stop point a sugar (plain line or quote line) stop point, holding the original text of the body.
    pub(super) fn mark_text_stop(&mut self, stop: ExprId, kind: StopKind, parts: &[StrPart]) {
        let text = text_parts_of(parts);
        let info = self.stop_info_mut(stop);
        info.kind = kind;
        info.text = text;
    }

    /// Makes the expression of the stop point of a stop command (with a stable ID and position hash). A line with a stable ID
    /// is marked passed when entered (before computing the arguments; `seen` within the line also sees the state after
    /// entering). `bind` tells whether the return value is received.
    pub(super) fn make_stop(
        &mut self,
        name: &str,
        args: Vec<StopArg>,
        id: Option<&StableId>,
        hash: u64,
        pos: Pos,
        bind: bool,
    ) -> ExprId {
        self.fn_stop(pos, "stop");
        let cmd = self.command_index(name);
        if let Some(sid) = id {
            self.ctx().refs.stop_ids.push(sid.id.clone());
        }
        if let Some(sid) = id
            && !self.ctx().ids.insert(sid.id.clone())
        {
            self.error(
                sid.pos,
                DiagCode::DuplicateStableId,
                vec![("id", s(&sid.id))],
            );
        }
        let info = StopInfo {
            kind: StopKind::Command,
            text: Vec::new(),
            text_arg: None,
            args_pc: 0,
            pc: 0,
            id: id.map(|i| i.id.clone()),
            hash,
            line: pos.line,
            choice: None,
            binds: bind,
        };
        let stop = Stop {
            cmd,
            mark_seen: id.map(|i| i.id.clone()),
            args,
            info,
        };
        self.node(ExprKind::Stop(Box::new(stop)), pos.line)
    }

    /// Whether the first half of `name.` is the name of an identity declaration (a usable identity, not a module, package or
    /// type name).
    pub(super) fn is_identity_name(&self, name: &str) -> bool {
        !self.is_import(name)
            && !self.is_own_name(name)
            && !self.registry.has_namespace(name)
            && (self.md().early_identities.contains_key(name)
                || self
                    .md()
                    .imports
                    .values()
                    .any(|&i| self.modules[i].early_identities.contains_key(name)))
    }
}

/// The text that goes into the position hash for a plain line: the body as written, with the leading `\` that forces a
/// plain line kept.
fn plain_body(parts: &[StrPart], raw: &str) -> String {
    let body = crate::syntax::canon::body(parts);
    if raw.starts_with('\\') && !body.starts_with('\\') {
        format!("\\{body}")
    } else {
        body
    }
}

/// The text that goes into the position hash for a quote line or an option: what is between `「` and `」`.
fn quoted_body(parts: &[StrPart]) -> String {
    let body = crate::syntax::canon::body(parts);
    body.strip_prefix('「')
        .and_then(|b| b.strip_suffix('」'))
        .map_or(body.clone(), str::to_string)
}
