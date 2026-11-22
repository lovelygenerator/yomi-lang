//! Walks statements and expressions and collects references to names.

use super::*;

pub(super) struct Walker<'a, 'b> {
    pub(super) out: &'a mut Analysis,
    pub(super) module: &'a str,
    pub(super) lines: &'a [&'a str],
    pub(super) tops: &'a Tops,
    pub(super) all: &'a HashMap<&'b str, &'b Tops>,
    pub(super) imports: HashMap<&'a str, &'a str>,
    /// Local names (from outer to inner). Values are symbol indices.
    pub(super) scopes: Vec<HashMap<String, usize>>,
    /// The name of the current label or function.
    pub(super) current: String,
}

impl Walker<'_, '_> {
    pub(super) fn module(&mut self, ast: &Module) {
        for v in &ast.vars {
            self.ty(&v.ty);
            self.expr(&v.init);
        }
        for t in &ast.types {
            match &t.kind {
                TypeDeclKind::Struct(fields) => {
                    for (_, _, ty) in fields {
                        self.ty(ty);
                    }
                }
                TypeDeclKind::Enum(variants) => {
                    for v in variants {
                        for a in &v.args {
                            self.ty(a);
                        }
                    }
                }
            }
        }
        for l in &ast.labels {
            self.label_like(l);
        }
        for f in &ast.fns {
            self.label_like(f);
        }
    }

    pub(super) fn label_like(&mut self, d: &LabelDecl) {
        self.current = d.name.clone();
        self.scopes.push(HashMap::new());
        for p in &d.params {
            self.ty(&p.ty);
            if let Some(default) = &p.default {
                self.expr(default);
            }
            self.declare_local(SymbolKind::Param, &p.name, p.pos, &p.ty, &d.name);
        }
        if let Some(r) = &d.ret {
            self.ty(r);
        }
        self.block(&d.body);
        self.scopes.pop();
    }

    pub(super) fn declare_local(
        &mut self,
        kind: SymbolKind,
        name: &str,
        pos: Pos,
        ty: &TyName,
        container: &str,
    ) {
        self.declare_local_text(kind, name, pos, &ty_text(ty), container);
    }

    pub(super) fn declare_local_text(
        &mut self,
        kind: SymbolKind,
        name: &str,
        pos: Pos,
        ty: &str,
        container: &str,
    ) {
        let detail = if ty.is_empty() {
            name.to_string()
        } else {
            format!("{name}: {ty}")
        };
        self.out.symbols.push(Symbol {
            kind,
            name: name.to_string(),
            module: self.module.to_string(),
            span: span_of(pos, name),
            detail,
            container: Some(container.to_string()),
        });
        let id = self.out.symbols.len() - 1;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), id);
        }
    }

    pub(super) fn container(&self) -> String {
        self.current.clone()
    }

    pub(super) fn reference(&mut self, span: Span, target: usize) {
        self.out.references.push(Reference {
            module: self.module.to_string(),
            span,
            target,
        });
    }

    /// The first position at or after `col` on line `from` where the token `name` appears (not adjacent to name characters).
    pub(super) fn find_word(&self, line: u32, col: u32, word: &str) -> Option<Pos> {
        let text = self.lines.get(line.checked_sub(1)? as usize)?;
        let cs: Vec<char> = text.chars().collect();
        let w: Vec<char> = word.chars().collect();
        let is_name = |c: char| c.is_alphanumeric() || c == '_';
        let start = col.saturating_sub(1) as usize;
        if w.is_empty() || cs.len() < w.len() {
            return None;
        }
        (start..=cs.len() - w.len()).find_map(|i| {
            let ok = cs[i..i + w.len()] == w[..]
                && (i == 0 || !is_name(cs[i - 1]))
                && cs.get(i + w.len()).is_none_or(|c| !is_name(*c));
            ok.then_some(Pos {
                line,
                col: i as u32 + 1,
            })
        })
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    pub(super) fn tops_of(&self, module_local: &str) -> Option<&Tops> {
        let path = self.imports.get(module_local)?;
        self.all.get(path).copied()
    }

    pub(super) fn ty(&mut self, t: &TyName) {
        for a in &t.args {
            self.ty(a);
        }
        let target = match t.name.split_once('.') {
            Some((m, n)) => self.tops_of(m).and_then(|tops| tops.types.get(n).copied()),
            None => self.tops.types.get(&t.name).copied(),
        };
        if let Some(id) = target {
            let (name_pos, len_name) = match t.name.split_once('.') {
                Some((m, n)) => (
                    Pos {
                        line: t.pos.line,
                        col: t.pos.col + chars(m) + 1,
                    },
                    n,
                ),
                None => (t.pos, t.name.as_str()),
            };
            self.reference(span_of(name_pos, len_name), id);
        }
    }

    pub(super) fn block(&mut self, body: &[Stmt]) {
        self.scopes.push(HashMap::new());
        for s in body {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    pub(super) fn label_ref(&mut self, target: &LabelRef, kw_pos: Pos) {
        let (found, pos) = match &target.module {
            Some((m, mp)) => (
                self.tops_of(m)
                    .and_then(|t| t.labels.get(&target.name).copied()),
                Some(Pos {
                    line: mp.line,
                    col: mp.col + chars(m) + 1,
                }),
            ),
            None => (
                self.tops.labels.get(&target.name).copied(),
                self.find_word(kw_pos.line, kw_pos.col, &target.name),
            ),
        };
        if let (Some(id), Some(pos)) = (found, pos) {
            self.reference(span_of(pos, &target.name), id);
        }
    }

    pub(super) fn ident_ref(&mut self, r: &IdentRef) {
        let (found, pos) = match &r.module {
            Some((m, mp)) => (
                self.tops_of(m)
                    .and_then(|t| t.identities.get(&r.name).copied()),
                Pos {
                    line: mp.line,
                    col: mp.col + chars(m) + 1,
                },
            ),
            None => (self.tops.identities.get(&r.name).copied(), r.pos),
        };
        if let Some(id) = found {
            self.reference(span_of(pos, &r.name), id);
        }
    }

    /// If the `name` of `name.member` is a variable (local or module variable) or an identity, adds that reference and
    /// returns true. Like the compiler, checks this before module aliases (it becomes `variable.field` or
    /// `variable.method()`).
    pub(super) fn value_base(&mut self, name: &str, pos: Pos) -> bool {
        let id = self
            .lookup_local(name)
            .or_else(|| self.tops.vars.get(name).copied())
            .or_else(|| self.tops.identities.get(name).copied());
        if let Some(id) = id {
            self.reference(span_of(pos, name), id);
        }
        id.is_some()
    }

    /// The name as a variable (local → module variable).
    pub(super) fn var_name(&mut self, module: &Option<(String, Pos)>, name: &str, pos: Pos) {
        match module {
            Some((m, mp)) if self.value_base(m, *mp) => {}
            Some((m, mp)) => {
                let id = self.tops_of(m).and_then(|t| t.vars.get(name).copied());
                if let Some(id) = id {
                    let p = Pos {
                        line: mp.line,
                        col: mp.col + chars(m) + 1,
                    };
                    self.reference(span_of(p, name), id);
                }
            }
            None => {
                let id = self
                    .lookup_local(name)
                    .or_else(|| self.tops.vars.get(name).copied());
                if let Some(id) = id {
                    self.reference(span_of(pos, name), id);
                }
            }
        }
    }

    /// Adds the reference of `TypeName.Variant` (when the expression is a type name and a variant of that type). True if
    /// added.
    pub(super) fn variant_of(&mut self, base: &Expr, name: &str, pos: Pos) -> bool {
        let (tops, ty, ty_pos) = match base {
            Expr::Name(ty, ty_pos) => {
                if self.lookup_local(ty).is_some() || self.tops.vars.contains_key(ty) {
                    return false;
                }
                (self.tops, ty, *ty_pos)
            }
            // `module.TypeName.Variant`
            Expr::Qualified(m, ty, m_pos) => {
                if self.lookup_local(m).is_some() || self.tops.vars.contains_key(m) {
                    return false;
                }
                let Some(tops) = self.tops_of(m) else {
                    return false;
                };
                let ty_pos = Pos {
                    line: m_pos.line,
                    col: m_pos.col + chars(m) + 1,
                };
                (tops, ty, ty_pos)
            }
            _ => return false,
        };
        let (Some(&ty_id), Some(&variant_id)) = (tops.types.get(ty), tops.variants.get(name))
        else {
            return false;
        };
        self.reference(span_of(ty_pos, ty), ty_id);
        self.reference(span_of(pos, name), variant_id);
        true
    }

    pub(super) fn parts(&mut self, parts: &[StrPart]) {
        for p in parts {
            if let StrPart::Interp(e) = p {
                self.expr(e);
            }
        }
    }

    pub(super) fn path(&mut self, path: &[PathElem]) {
        for p in path {
            if let PathElem::Index(e, _) = p {
                self.expr(e);
            }
        }
    }

    pub(super) fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let {
                name,
                name_pos,
                ty,
                init,
                ..
            } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(init);
                let text = ty.as_ref().map(ty_text).unwrap_or_default();
                let container = self.container();
                self.declare_local_text(SymbolKind::Local, name, *name_pos, &text, &container);
            }
            Stmt::Assign {
                module,
                name,
                path,
                value,
                pos,
                op,
            } => {
                let _: &AssignOp = op;
                self.var_name(module, name, *pos);
                self.path(path);
                self.expr(value);
            }
            Stmt::If {
                arms, else_body, ..
            } => {
                for (cond, body) in arms {
                    self.expr(cond);
                    self.block(body);
                }
                self.block(else_body);
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::For {
                key,
                var,
                var_pos,
                iter,
                body,
                ..
            } => {
                self.expr(iter);
                self.scopes.push(HashMap::new());
                let container = self.container();
                // `_` is a name that discards and is not made a symbol
                for (name, pos) in key.iter().map(|(n, p)| (n, *p)).chain([(var, *var_pos)]) {
                    if name != "_" {
                        self.declare_local_text(SymbolKind::Local, name, pos, "", &container);
                    }
                }
                self.block(body);
                self.scopes.pop();
            }
            Stmt::ListMethod {
                module,
                name,
                path,
                args,
                pos,
                ..
            } => {
                self.var_name(module, name, *pos);
                self.path(path);
                for a in args {
                    self.expr(a);
                }
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    self.pattern(&arm.pattern);
                    self.block(&arm.body);
                    self.scopes.pop();
                }
            }
            Stmt::Jump { target, pos } => self.label_ref(target, *pos),
            Stmt::Call(c) => {
                self.label_ref(&c.target, c.pos);
                for a in &c.args {
                    self.expr(a);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Stmt::Command(c) => {
                for a in &c.args {
                    self.expr(a);
                }
            }
            Stmt::Plain { parts, .. } => self.parts(parts),
            Stmt::Quote {
                identity, parts, ..
            } => {
                if let Some(i) = identity {
                    self.ident_ref(i);
                }
                self.parts(parts);
            }
            Stmt::Choice { options, .. } => {
                for o in options {
                    self.option(o);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::End(_) => {}
        }
    }

    pub(super) fn option(&mut self, o: &ChoiceOpt) {
        self.parts(&o.parts);
        if let Some(c) = &o.cond {
            self.expr(c);
        }
        self.block(&o.body);
    }

    pub(super) fn pattern(&mut self, p: &Pattern) {
        let container = self.container();
        match p {
            Pattern::Some(Some((name, pos)), _) => {
                self.declare_local_text(SymbolKind::Local, name, *pos, "", &container);
            }
            Pattern::Variant { name, binds, pos } => {
                if let Some(id) = self.tops.variants.get(name).copied() {
                    self.reference(span_of(*pos, name), id);
                }
                for (name, pos) in binds.iter().flatten() {
                    self.declare_local_text(SymbolKind::Local, name, *pos, "", &container);
                }
            }
            _ => {}
        }
    }

    pub(super) fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Name(name, pos) => {
                let id = self
                    .lookup_local(name)
                    .or_else(|| self.tops.vars.get(name).copied())
                    .or_else(|| self.tops.identities.get(name).copied())
                    .or_else(|| self.tops.variants.get(name).copied())
                    .or_else(|| self.tops.fns.get(name).copied());
                if let Some(id) = id {
                    self.reference(span_of(*pos, name), id);
                }
            }
            Expr::Qualified(m, _, pos) if self.value_base(m, *pos) => {}
            Expr::Qualified(m, name, pos) => {
                let p = Pos {
                    line: pos.line,
                    col: pos.col + chars(m) + 1,
                };
                if let (Some(&ty_id), Some(&variant_id)) =
                    (self.tops.types.get(m), self.tops.variants.get(name))
                {
                    self.reference(span_of(*pos, m), ty_id);
                    self.reference(span_of(p, name), variant_id);
                    return;
                }
                let id = self.tops_of(m).and_then(|t| {
                    t.vars
                        .get(name)
                        .or_else(|| t.identities.get(name))
                        .or_else(|| t.variants.get(name))
                        .or_else(|| t.fns.get(name))
                        .copied()
                });
                if let Some(id) = id {
                    self.reference(span_of(p, name), id);
                }
            }
            Expr::Call(name, args, pos) => {
                let id = self
                    .tops
                    .fns
                    .get(name)
                    .copied()
                    .or_else(|| self.lookup_local(name))
                    .or_else(|| self.tops.variants.get(name).copied());
                if let Some(id) = id {
                    self.reference(span_of(*pos, name), id);
                }
                for a in args {
                    self.expr(a);
                }
            }
            Expr::QueryCall(m, _, args, pos) => {
                self.value_base(m, *pos);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::List(xs, _) => {
                for x in xs {
                    self.expr(x);
                }
            }
            Expr::Map(kvs, _) => {
                for (k, v) in kvs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Str(parts, _) => self.parts(parts),
            Expr::Field(x, name, pos) => {
                if !self.variant_of(x, name, *pos) {
                    self.expr(x);
                }
            }
            Expr::Some(x, _) | Expr::Unary(_, x, _) | Expr::Named(_, x, _) => self.expr(x),
            Expr::Index(a, b, _) | Expr::Binary(_, a, b, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Method(x, name, args, pos) => {
                if !self.variant_of(x, name, *pos) {
                    self.expr(x);
                }
                for a in args {
                    self.expr(a);
                }
            }
            Expr::StructLit(t, inits, _) => {
                let ty_id = match &t.module {
                    Some((m, mp)) => {
                        let id = self.tops_of(m).and_then(|x| x.types.get(&t.name).copied());
                        if let Some(id) = id {
                            let p = Pos {
                                line: mp.line,
                                col: mp.col + chars(m) + 1,
                            };
                            self.reference(span_of(p, &t.name), id);
                        }
                        id
                    }
                    None => {
                        let id = self.tops.types.get(&t.name).copied();
                        if let Some(id) = id {
                            self.reference(span_of(t.pos, &t.name), id);
                        }
                        id
                    }
                };
                for init in inits {
                    if let Some(ty_id) = ty_id {
                        let tname = self.out.symbols[ty_id].name.clone();
                        let owner = self.out.symbols[ty_id].module.clone();
                        let field = self
                            .all
                            .get(owner.as_str())
                            .and_then(|tops| tops.fields.get(&(tname, init.name.clone())))
                            .copied();
                        if let Some(f) = field {
                            self.reference(span_of(init.pos, &init.name), f);
                        }
                    }
                    self.expr(&init.value);
                }
            }
            Expr::LabelCall(c) => {
                self.label_ref(&c.target, c.pos);
                for a in &c.args {
                    self.expr(a);
                }
            }
            Expr::Command(c) | Expr::MisplacedCommand(c) => {
                for a in &c.args {
                    self.expr(a);
                }
            }
            Expr::Lambda(l) => {
                self.scopes.push(HashMap::new());
                let container = self.container();
                for p in &l.params {
                    self.declare_local_text(SymbolKind::Local, &p.name, p.pos, "", &container);
                }
                self.expr(&l.body);
                self.scopes.pop();
            }
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::None(_)
            | Expr::Default(..) => {}
        }
    }
}
