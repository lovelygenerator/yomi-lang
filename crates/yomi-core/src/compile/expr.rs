//! Compilation: type checking and code generation of expressions.

use super::*;
use crate::value::EnumValue;
use std::sync::Arc;

impl Compiler<'_> {
    /// Type-checks an expression and makes an IR expression. `None` if the type could not be decided (a diagnostic was
    /// emitted).
    pub(super) fn expr(&mut self, e: &Expr) -> Typed {
        self.expr_ir(e, None)
    }

    /// Type-checks an expression and makes an IR expression. `hint` is the expected type (decides the type of `[]` and
    /// `None`).
    pub(super) fn expr_ir(&mut self, e: &Expr, hint: Option<&Ty>) -> Typed {
        if let Some(vr) = self.as_variant_ref(e) {
            return self.variant_expr(&vr);
        }
        match e {
            // A default value filled in when arranging arguments (its type is the parameter's type)
            Expr::Default(v, p) => (hint.cloned(), self.node(ExprKind::Const(v.clone()), p.line)),
            // Written in a call that does not accept named arguments (such as a method)
            Expr::Named(arg, inner, p) => {
                self.error(
                    *p,
                    DiagCode::UnknownArgument,
                    vec![("name", s("")), ("arg", s(arg))],
                );
                self.expr_ir(inner, hint)
            }
            Expr::List(items, p) => self.list_literal(items, *p, hint),
            Expr::Map(entries, p) => self.map_literal(entries, *p, hint),
            Expr::Some(inner, _) => {
                let inner_hint = match hint {
                    Some(Ty::Option(t)) => Some((**t).clone()),
                    _ => None,
                };
                let (t, i) = self.expr_ir(inner, inner_hint.as_ref());
                let id = self.node(ExprKind::Some(i), e.pos().line);
                (t.map(|t| Ty::Option(Box::new(t))), id)
            }
            Expr::None(p) => match hint {
                Some(t @ Ty::Option(_)) => (
                    Some(t.clone()),
                    self.node(ExprKind::Const(Value::Option(None)), p.line),
                ),
                Some(t) => {
                    self.type_mismatch(*p, &t.name(), "Option<_>");
                    self.failed()
                }
                None => {
                    self.error(*p, DiagCode::CannotInferType, vec![]);
                    self.failed()
                }
            },
            Expr::Index(base, idx, p) => self.index_expr(base, idx, *p),
            Expr::Field(base, name, p) => self.field_expr(base, name, *p),
            Expr::Method(base, name, args, p) => self.method_expr(base, name, args, *p),
            Expr::StructLit(tr, fields, p) => self.struct_literal(tr, fields, *p),
            Expr::Int(n, p) => (
                Some(Ty::Int),
                self.node(ExprKind::Const(Value::Int(*n)), p.line),
            ),
            Expr::Float(x, p) => (
                Some(Ty::Float),
                self.node(ExprKind::Const(Value::Float(*x)), p.line),
            ),
            Expr::Bool(b, p) => (
                Some(Ty::Bool),
                self.node(ExprKind::Const(Value::Bool(*b)), p.line),
            ),
            Expr::Str(parts, p) => (Some(Ty::Str), self.text_parts(parts, p.line, false)),
            Expr::Lambda(l) => match hint {
                Some(Ty::Fn(ps, r)) => self.lambda_expr(l, Some(ps), Some(r)),
                _ => self.lambda_expr(l, None, None),
            },
            Expr::Name(name, p) => {
                let Some((local, slot, ty)) = self.lookup(name) else {
                    // The name of a `fn` is a function value
                    if let Some(&flat) = self.md().fn_map.get(name) {
                        return self.fn_ref(flat, name, *p);
                    }
                    // If not a variable, the name of an identity declaration (a constant of type `Identity`)
                    let before = self.diags.len();
                    let r = IdentRef {
                        module: None,
                        name: name.clone(),
                        pos: *p,
                    };
                    if let Some(i) = self.resolve_identity_const(&r) {
                        let id = self.node(ExprKind::Const(Value::Identity(i.into())), p.line);
                        return (Some(Ty::Identity), id);
                    }
                    // A diagnostic was emitted for the ambiguous identity
                    if self.diags.len() == before {
                        self.unknown_name(name, *p);
                    }
                    return self.failed();
                };
                let place = if local {
                    self.mark_read(slot);
                    if ty.as_ref().is_some_and(Ty::contains_fn) {
                        self.fn_use(slot);
                    }
                    PlaceRef::Local(slot)
                } else {
                    if self.impure_use(*p, "variable") {
                        return self.failed();
                    }
                    PlaceRef::Global(slot)
                };
                (ty, self.node(ExprKind::Var(place), p.line))
            }
            Expr::Qualified(module, name, p) if self.var_exists(module) => {
                // `variable.field` when the variable is a struct
                let base = Expr::Name(module.clone(), *p);
                self.field_expr(&base, name, *p)
            }
            Expr::Qualified(module, name, p) if self.is_identity_name(module) => {
                // Fields of an identity constant: `案内人.name`, `案内人.id`
                let base = Expr::Name(module.clone(), *p);
                self.field_expr(&base, name, *p)
            }
            Expr::Qualified(module, name, p) => {
                let midx = match self.resolve_target(module, *p) {
                    None => return self.failed(),
                    Some(Target::Module(midx)) => midx,
                    Some(Target::Package) => {
                        if self.impure_use(*p, "host variable") {
                            return self.failed();
                        }
                        let Some((idx, ty, _)) = self.host_var(module, name, *p) else {
                            return self.failed();
                        };
                        let id = self.node(ExprKind::Var(PlaceRef::Host(idx)), p.line);
                        return (Some(ty), id);
                    }
                };
                let Some((slot, ty)) = self.modules[midx].global_map.get(name).cloned() else {
                    // An identity constant of another module (`module.案内人`)
                    if let Some(display) = self.modules[midx].early_identities.get(name) {
                        let v = Identity {
                            id: name.clone(),
                            name: display.clone(),
                        };
                        let id = self.node(ExprKind::Const(Value::Identity(v.into())), p.line);
                        return (Some(Ty::Identity), id);
                    }
                    if let Some(&flat) = self.modules[midx].fn_map.get(name) {
                        return self.fn_ref(flat, &format!("{module}.{name}"), *p);
                    }
                    self.unknown_qualified(midx, module, name, *p);
                    return self.failed();
                };
                if self.impure_use(*p, "variable") {
                    return self.failed();
                }
                (
                    Some(ty),
                    self.node(ExprKind::Var(PlaceRef::Global(slot)), p.line),
                )
            }
            Expr::QueryCall(package, member, args, p) if self.var_exists(package) => {
                // A method of a variable, `variable.method(...)`
                let base = Expr::Name(package.clone(), *p);
                self.method_expr(&base, member, args, *p)
            }
            Expr::QueryCall(package, member, args, p) => self.query_call(package, member, args, *p),
            Expr::LabelCall(call) => {
                let (t, id) = self.label_call(call, true);
                (t, id)
            }
            Expr::Command(call) | Expr::MisplacedCommand(call) => {
                self.misplaced_command(call);
                self.failed()
            }
            Expr::Unary(op, inner, p) => {
                let (t, operand) = self.expr_ir(inner, None);
                let id = self.node(ExprKind::Unary { op: *op, operand }, p.line);
                let t = match op {
                    UnOp::Neg => match t {
                        Some(t) if numeric_ok(&t, false) => Some(t),
                        Some(t) => {
                            self.error(
                                *p,
                                DiagCode::InvalidOperation,
                                vec![("op", s("-")), ("type", s(&t.name()))],
                            );
                            None
                        }
                        None => None,
                    },
                    UnOp::Not => {
                        if let Some(t) = t
                            && t != Ty::Bool
                        {
                            self.type_mismatch(inner.pos(), "bool", &t.name());
                        }
                        Some(Ty::Bool)
                    }
                };
                (t, id)
            }
            Expr::Binary(op, l, r, p) => self.binary(*op, l, r, *p),
            Expr::Call(name, args, p) => self.call(name, args, *p),
        }
    }

    /// Whether the name is visible as a variable (local or module variable).
    /// `xs.sort()`, `xs.reverse()` and `xs.sum()` (no arguments). `base` is the evaluated `list` expression.
    fn list_plain_method(
        &mut self,
        name: &str,
        (base, elem): (ExprId, &Ty),
        args: &[Expr],
        pos: Pos,
    ) -> Typed {
        if !args.is_empty() {
            self.discard_exprs(args);
            self.arg_count_error(name, 0, args.len(), pos);
            return self.failed();
        }
        let (method, ret) = match (name, elem) {
            ("reverse", _) => (Method::ListReverse, Ty::List(Box::new(elem.clone()))),
            ("sort", t) if lambda::is_orderable(t) => {
                (Method::ListSort, Ty::List(Box::new(elem.clone())))
            }
            ("sum", Ty::Int) => (Method::ListSum, Ty::Int),
            ("sum", Ty::Float) => (Method::ListSumFloat, Ty::Float),
            _ => {
                self.unsupported_element(name, elem, pos);
                return self.failed();
            }
        };
        let kind = ExprKind::Method {
            method,
            base,
            args: Vec::new(),
        };
        (Some(ret), self.node(kind, pos.line))
    }

    pub(super) fn var_exists(&self, name: &str) -> bool {
        if let Some(c) = &self.cur
            && c.scopes.iter().any(|sc| sc.contains_key(name))
        {
            return true;
        }
        self.md().global_map.contains_key(name) || self.repl_lookup(name).is_some()
    }

    /// If the expression is written as an `enum` variant (`TypeName.Variant`, `TypeName.Variant(expr, ...)`; the type name
    /// can also be `module.TypeName`), its contents. No diagnostics.
    pub(super) fn as_variant_ref<'e>(&self, e: &'e Expr) -> Option<VariantRef<'e>> {
        match e {
            Expr::Qualified(a, b, p) if !self.var_exists(a) => {
                let ty = *self.md().type_map.get(a)?;
                Some(VariantRef {
                    ty,
                    variant: b,
                    args: &[],
                    pos: *p,
                })
            }
            Expr::QueryCall(a, v, args, p) if !self.var_exists(a) => {
                let ty = *self.md().type_map.get(a)?;
                Some(VariantRef {
                    ty,
                    variant: v,
                    args,
                    pos: *p,
                })
            }
            Expr::Field(base, v, p) => Some(VariantRef {
                ty: self.type_index_of(base)?,
                variant: v,
                args: &[],
                pos: *p,
            }),
            Expr::Method(base, v, args, p) => Some(VariantRef {
                ty: self.type_index_of(base)?,
                variant: v,
                args,
                pos: *p,
            }),
            _ => None,
        }
    }

    /// Turns a reference to a variant into `(type index, variant index)`. If it is not an `enum` or the variant does not exist,
    /// emits a diagnostic and returns `None`.
    pub(super) fn variant_lookup(&mut self, vr: &VariantRef<'_>) -> Option<(u32, usize)> {
        let decl = self.types[vr.ty as usize].clone();
        let TypeKind::Enum { variants } = &decl.kind else {
            self.error(
                vr.pos,
                DiagCode::UnknownVariant,
                vec![("name", s(vr.variant)), ("type", s(&decl.name))],
            );
            return None;
        };
        match variants.iter().position(|v| v.name == vr.variant) {
            Some(i) => Some((vr.ty, i)),
            None => {
                let found = suggest(vr.variant, variants.iter().map(|v| v.name.as_str()));
                let mut args = vec![("name", s(vr.variant)), ("type", s(&decl.name))];
                if !found.is_empty() {
                    args.push(("suggestions", DiagArg::List(found)));
                }
                self.error(vr.pos, DiagCode::UnknownVariant, args);
                None
            }
        }
    }

    /// An expression of an `enum` value (`TypeName.Variant(expr, ...)`).
    pub(super) fn variant_expr(&mut self, vr: &VariantRef<'_>) -> Typed {
        let looked = self.variant_lookup(vr);
        let Some((ty_idx, vi)) = looked else {
            self.discard_exprs(vr.args);
            return self.failed();
        };
        let decl = self.types[ty_idx as usize].clone();
        let TypeKind::Enum { variants } = &decl.kind else {
            return self.failed();
        };
        let want = &variants[vi].args;
        let written = format!("{}.{}", decl.name, vr.variant);
        if want.len() != vr.args.len() {
            self.discard_exprs(vr.args);
            self.arg_count_error(&written, want.len(), vr.args.len(), vr.pos);
            return (Some(Ty::Enum(decl.name)), self.error_node());
        }
        let mut args = Vec::new();
        for (a, w) in vr.args.iter().zip(want) {
            let (got, id) = self.expr_ir(a, Some(w));
            self.expect_ty(a, got, w.clone());
            args.push(id);
        }
        let kind = if want.is_empty() {
            ExprKind::Const(Value::Enum(Arc::new(EnumValue {
                ty: decl.name.clone().into(),
                variant: vr.variant.to_string().into(),
                args: Vec::new(),
            })))
        } else {
            ExprKind::Enum {
                ty: ty_idx,
                variant: vi as u32,
                args,
            }
        };
        (Some(Ty::Enum(decl.name)), self.node(kind, vr.pos.line))
    }

    /// Checks a struct literal: the type and field names (unknown, duplicate, missing). If there are no problems, returns the
    /// type index and, for each field in the order written, its position in the declaration.
    pub(super) fn struct_lit_plan(
        &mut self,
        tr: &TypeRef,
        fields: &[FieldInit],
        pos: Pos,
    ) -> Option<(u32, Vec<(usize, usize)>)> {
        let written = tr.written();
        let idx = self.find_type_named(&written, tr.pos)?;
        let decl = self.types[idx as usize].clone();
        let TypeKind::Struct { fields: decls } = &decl.kind else {
            self.name_error(tr.pos, DiagCode::UnknownType, &written);
            return None;
        };
        let mut ok = true;
        let mut plan: Vec<(usize, usize)> = Vec::new();
        let mut given = vec![false; decls.len()];
        for (src, f) in fields.iter().enumerate() {
            match decls.iter().position(|(n, _)| *n == f.name) {
                None => {
                    let found = suggest(&f.name, decls.iter().map(|(n, _)| n.as_str()));
                    let mut args = vec![("name", s(&f.name)), ("type", s(&decl.name))];
                    if !found.is_empty() {
                        args.push(("suggestions", DiagArg::List(found)));
                    }
                    self.error(f.pos, DiagCode::UnknownField, args);
                    ok = false;
                }
                Some(di) if given[di] => {
                    self.name_error(f.pos, DiagCode::DuplicateField, &f.name);
                    ok = false;
                }
                Some(di) => {
                    given[di] = true;
                    plan.push((src, di));
                }
            }
        }
        for (di, g) in given.iter().enumerate() {
            if !g {
                self.error(
                    pos,
                    DiagCode::MissingField,
                    vec![("name", s(&decls[di].0)), ("type", s(&decl.name))],
                );
                ok = false;
            }
        }
        ok.then_some((idx, plan))
    }

    /// An expression of a struct literal. Evaluated in the order written.
    pub(super) fn struct_literal(&mut self, tr: &TypeRef, fields: &[FieldInit], pos: Pos) -> Typed {
        let Some((idx, plan)) = self.struct_lit_plan(tr, fields, pos) else {
            for f in fields {
                self.expr_ir(&f.value, None);
            }
            return self.failed();
        };
        let decl = self.types[idx as usize].clone();
        let TypeKind::Struct { fields: decls } = &decl.kind else {
            return self.failed();
        };
        let mut order = Vec::new();
        let mut values = Vec::new();
        for (src, di) in plan {
            let want = &decls[di].1;
            let (got, id) = self.expr_ir(&fields[src].value, Some(want));
            self.expect_ty(&fields[src].value, got, want.clone());
            order.push(di as u32);
            values.push(id);
        }
        let id = self.node(
            ExprKind::Struct {
                ty: idx,
                order,
                fields: values,
            },
            pos.line,
        );
        (Some(Ty::Struct(decl.name)), id)
    }

    /// A list literal `[a, b]`. The type of an empty list is decided from the expected type (`hint`).
    pub(super) fn list_literal(&mut self, items: &[Expr], pos: Pos, hint: Option<&Ty>) -> Typed {
        let mut elem: Option<Ty> = match hint {
            Some(Ty::List(t)) => Some((**t).clone()),
            _ => None,
        };
        if let Some(h) = hint
            && !matches!(h, Ty::List(_))
        {
            self.type_mismatch(pos, &h.name(), "list<_>");
            self.discard_exprs(items);
            return self.failed();
        }
        let mut failed = false;
        let mut ids = Vec::new();
        for it in items {
            let (got, id) = self.expr_ir(it, elem.as_ref());
            ids.push(id);
            match (&elem, got) {
                (Some(want), Some(g)) if *want != g => {
                    self.type_mismatch(it.pos(), &want.name(), &g.name());
                }
                (None, Some(g)) => elem = Some(g),
                (None, None) => failed = true,
                _ => {}
            }
        }
        if items.is_empty() && elem.is_none() {
            self.error(pos, DiagCode::CannotInferType, vec![]);
            return self.failed();
        }
        let id = self.node(ExprKind::List(ids), pos.line);
        if failed {
            (None, id)
        } else {
            (elem.map(|t| Ty::List(Box::new(t))), id)
        }
    }

    /// A `map` literal `map { key: value, ... }`. The type of an empty `map {}` is decided from the expected type (`hint`).
    pub(super) fn map_literal(
        &mut self,
        entries: &[(Expr, Expr)],
        pos: Pos,
        hint: Option<&Ty>,
    ) -> Typed {
        let (mut kt, mut vt): (Option<Ty>, Option<Ty>) = match hint {
            Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
            _ => (None, None),
        };
        if let Some(h) = hint
            && !matches!(h, Ty::Map(..))
        {
            self.type_mismatch(pos, &h.name(), "map<_, _>");
            for (k, v) in entries {
                self.expr_ir(k, None);
                self.expr_ir(v, None);
            }
            return self.failed();
        }
        let mut failed = false;
        let mut ids = Vec::new();
        for (k, v) in entries {
            let (got_k, kid) = self.expr_ir(k, kt.as_ref());
            match (&kt, got_k) {
                (Some(want), Some(g)) if *want != g => {
                    self.type_mismatch(k.pos(), &want.name(), &g.name());
                }
                (None, Some(g)) => {
                    // The key type is decided by the first one (check that it can be a key)
                    if self.check_map_key(&g, k.pos()).is_none() {
                        failed = true;
                    }
                    kt = Some(g);
                }
                (None, None) => failed = true,
                _ => {}
            }
            let (got_v, vid) = self.expr_ir(v, vt.as_ref());
            ids.push((kid, vid));
            match (&vt, got_v) {
                (Some(want), Some(g)) if *want != g => {
                    self.type_mismatch(v.pos(), &want.name(), &g.name());
                }
                (None, Some(g)) => vt = Some(g),
                (None, None) => failed = true,
                _ => {}
            }
        }
        if entries.is_empty() && (kt.is_none() || vt.is_none()) {
            self.error(pos, DiagCode::CannotInferType, vec![]);
            return self.failed();
        }
        let id = self.node(ExprKind::Map(ids), pos.line);
        if failed {
            return (None, id);
        }
        match (kt, vt) {
            (Some(k), Some(v)) => (Some(Ty::Map(Box::new(k), Box::new(v))), id),
            _ => (None, id),
        }
    }

    /// An index `expr[expr]`. `int` for a `list`, the key type for a `map`.
    pub(super) fn index_expr(&mut self, base: &Expr, idx: &Expr, pos: Pos) -> Typed {
        let (bt, b) = self.expr_ir(base, None);
        let key_ty = match &bt {
            Some(Ty::Map(k, _)) => (**k).clone(),
            _ => Ty::Int,
        };
        let (it, i) = self.expr_ir(idx, Some(&key_ty));
        self.expect_ty(idx, it, key_ty);
        let Some(bt) = bt else {
            return self.failed();
        };
        let node = ExprKind::Index { base: b, index: i };
        match bt {
            Ty::List(t) => (Some(*t), self.node(node, pos.line)),
            Ty::Map(_, v) => (Some(*v), self.node(node, pos.line)),
            other => {
                self.error(
                    pos,
                    DiagCode::InvalidOperation,
                    vec![("op", s("[]")), ("type", s(&other.name()))],
                );
                self.failed()
            }
        }
    }

    /// A struct field `expr.name`.
    pub(super) fn field_expr(&mut self, base: &Expr, name: &str, pos: Pos) -> Typed {
        let (bt, b) = self.expr_ir(base, None);
        let Some(bt) = bt else {
            return self.failed();
        };
        match &bt {
            Ty::Struct(n) => {
                let decl = find_type(&self.types, n).cloned();
                let Some(TypeDecl {
                    kind: TypeKind::Struct { fields },
                    ..
                }) = decl
                else {
                    return self.failed();
                };
                match fields.iter().position(|(f, _)| f == name) {
                    Some(i) => {
                        let field = i as u32;
                        let id = self.node(ExprKind::Field { base: b, field }, pos.line);
                        (Some(fields[i].1.clone()), id)
                    }
                    None => {
                        let found = suggest(name, fields.iter().map(|(f, _)| f.as_str()));
                        let mut args = vec![("name", s(name)), ("type", s(n))];
                        if !found.is_empty() {
                            args.push(("suggestions", DiagArg::List(found)));
                        }
                        self.error(pos, DiagCode::UnknownField, args);
                        self.failed()
                    }
                }
            }
            Ty::Identity => match name {
                "id" | "name" => {
                    let kind = ExprKind::IdField {
                        base: b,
                        name: name == "name",
                    };
                    (Some(Ty::Str), self.node(kind, pos.line))
                }
                _ => {
                    let found = suggest(name, ["id", "name"]);
                    let mut args = vec![("name", s(name)), ("type", s("Identity"))];
                    if !found.is_empty() {
                        args.push(("suggestions", DiagArg::List(found)));
                    }
                    self.error(pos, DiagCode::UnknownField, args);
                    self.failed()
                }
            },
            other => {
                self.error(
                    pos,
                    DiagCode::InvalidOperation,
                    vec![("op", s(".")), ("type", s(&other.name()))],
                );
                self.failed()
            }
        }
    }

    /// The arguments of a call that does not accept named arguments (a method). Named arguments become `unknown_argument`,
    /// keeping only the expression inside.
    pub(super) fn reject_named(&mut self, callee: &str, args: &[Expr]) -> Vec<Expr> {
        args.iter()
            .map(|a| match a {
                Expr::Named(arg, inner, p) => {
                    self.error(
                        *p,
                        DiagCode::UnknownArgument,
                        vec![("name", s(callee)), ("arg", s(arg))],
                    );
                    (**inner).clone()
                }
                _ => a.clone(),
            })
            .collect()
    }

    /// Methods called inside expressions. For `list`: `len()`, `is_empty()`, `contains(x)`; for `map`: `len()`, `is_empty()`,
    /// `has(k)`, `get(k)`, `keys()`, `values()`.
    pub(super) fn method_expr(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        pos: Pos,
    ) -> Typed {
        let unnamed = self.reject_named(name, args);
        let args = unnamed.as_slice();
        let (bt, b) = self.expr_ir(base, None);
        let Some(bt) = bt else {
            self.discard_exprs(args);
            return self.failed();
        };
        if let Ty::List(elem) = &bt
            && let Some(hof) = lambda::Hof::from_name(name)
        {
            return self.list_hof(hof, (b, elem), args, pos);
        }
        if let Ty::List(elem) = &bt
            && matches!(name, "sort" | "reverse" | "sum")
        {
            return self.list_plain_method(name, (b, elem), args, pos);
        }
        // (method name, parameter types, instruction, return type)
        let (methods, statement): (Vec<MethodSig>, &[&str]) = match &bt {
            Ty::List(elem) => (
                vec![
                    ("len", vec![], Method::Len, Ty::Int),
                    ("is_empty", vec![], Method::IsEmpty, Ty::Bool),
                    (
                        "contains",
                        vec![(**elem).clone()],
                        Method::ListContains,
                        Ty::Bool,
                    ),
                ],
                &["push", "insert", "remove", "clear", "shuffle"],
            ),
            Ty::Map(k, v) => (
                vec![
                    ("len", vec![], Method::Len, Ty::Int),
                    ("is_empty", vec![], Method::IsEmpty, Ty::Bool),
                    ("has", vec![(**k).clone()], Method::MapHas, Ty::Bool),
                    (
                        "get",
                        vec![(**k).clone()],
                        Method::MapGet,
                        Ty::Option(v.clone()),
                    ),
                    ("keys", vec![], Method::MapKeys, Ty::List(k.clone())),
                    ("values", vec![], Method::MapValues, Ty::List(v.clone())),
                ],
                &["remove", "clear"],
            ),
            other => {
                self.discard_exprs(args);
                self.error(
                    pos,
                    DiagCode::UnknownMethod,
                    vec![("name", s(name)), ("type", s(&other.name()))],
                );
                return self.failed();
            }
        };
        if let Some((_, want, method, ret)) = methods.iter().find(|(n, ..)| *n == name) {
            if want.len() != args.len() {
                self.discard_exprs(args);
                self.arg_count_error(name, want.len(), args.len(), pos);
                return (Some(ret.clone()), self.error_node());
            }
            let mut ids = Vec::new();
            for (a, w) in args.iter().zip(want) {
                let (got, id) = self.expr_ir(a, Some(w));
                self.expect_ty(a, got, w.clone());
                ids.push(id);
            }
            let kind = ExprKind::Method {
                method: *method,
                base: b,
                args: ids,
            };
            return (Some(ret.clone()), self.node(kind, pos.line));
        }
        self.discard_exprs(args);
        if statement.contains(&name) {
            self.name_error(pos, DiagCode::StatementMethodInExpression, name);
            return self.failed();
        }
        let (hofs, plain): (&[&str], &[&str]) = if matches!(bt, Ty::List(_)) {
            (lambda::Hof::NAMES, &["sort", "reverse", "sum"])
        } else {
            (&[], &[])
        };
        let found = suggest(
            name,
            methods
                .iter()
                .map(|(n, ..)| *n)
                .chain(hofs.iter().copied())
                .chain(plain.iter().copied()),
        );
        let mut dargs = vec![("name", s(name)), ("type", s(&bt.name()))];
        if !found.is_empty() {
            dargs.push(("suggestions", DiagArg::List(found)));
        }
        self.error(pos, DiagCode::UnknownMethod, dargs);
        self.failed()
    }

    /// Compiles a binary operation (including the short circuits of `&&` and `||`) and returns the result type. `None` if the
    /// type cannot be decided.
    pub(super) fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr, p: Pos) -> Typed {
        if matches!(op, BinOp::And | BinOp::Or) {
            let (lt, lhs) = self.expr_ir(l, None);
            self.expect_ty(l, lt, Ty::Bool);
            let (rt, rhs) = self.expr_ir(r, None);
            self.expect_ty(r, rt, Ty::Bool);
            let ty = Operand::Other;
            return (
                Some(Ty::Bool),
                self.node(ExprKind::Binary { op, ty, lhs, rhs }, p.line),
            );
        }
        let (lt, lhs) = self.expr_ir(l, None);
        // The right-hand side of `==` / `!=` has the same type as the left (decides the type of `xs == []` and `x == None`)
        let (rt, rhs) = if matches!(op, BinOp::Eq | BinOp::Ne) {
            self.expr_ir(r, lt.as_ref())
        } else {
            self.expr_ir(r, None)
        };
        let ty = Operand::of(lt.as_ref());
        let id = self.node(ExprKind::Binary { op, ty, lhs, rhs }, p.line);
        let comparison = matches!(
            op,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        if matches!(op, BinOp::Eq | BinOp::Ne) {
            if let Some(a) = &lt
                && a.contains_fn()
            {
                self.error(
                    p,
                    DiagCode::InvalidOperation,
                    vec![("op", s(bin_op_name(op))), ("type", s(&a.name()))],
                );
            } else if let (Some(a), Some(b)) = (lt, rt)
                && a != b
            {
                self.type_mismatch(r.pos(), &a.name(), &b.name());
            }
            return (Some(Ty::Bool), id);
        }
        let operand = self.numeric_operands(bin_op_name(op), (lt, rt), r, p, op == BinOp::Rem);
        (if comparison { Some(Ty::Bool) } else { operand }, id)
    }

    /// An expression whose type could not be decided (a diagnostic was emitted).
    pub(super) fn failed(&mut self) -> Typed {
        (None, self.error_node())
    }

    /// Checks an expression only for diagnostics (not turned into code).
    pub(super) fn discard_exprs(&mut self, es: &[Expr]) {
        for e in es {
            self.expr_ir(e, None);
        }
    }

    /// Checks the operand types of arithmetic and ordering comparisons. Returns the result type (`None` if it cannot be
    /// decided).
    ///
    /// `invalid_operation` if the left side has a type that cannot be used with this operation, `type_mismatch` if the left
    /// and right types differ. `int_only` is for operations like `%` that can only be used on `int`.
    pub(super) fn numeric_operands(
        &mut self,
        op: &str,
        (lt, rt): (Option<Ty>, Option<Ty>),
        r: &Expr,
        p: Pos,
        int_only: bool,
    ) -> Option<Ty> {
        let invalid = |this: &mut Self, t: Ty| {
            this.error(
                p,
                DiagCode::InvalidOperation,
                vec![("op", s(op)), ("type", s(&t.name()))],
            );
        };
        match (lt, rt) {
            (Some(a), _) if !numeric_ok(&a, int_only) => {
                invalid(self, a);
                None
            }
            (Some(a), Some(b)) if a != b => {
                self.type_mismatch(r.pos(), &a.name(), &b.name());
                Some(a)
            }
            (Some(a), _) => Some(a),
            (None, Some(b)) if !numeric_ok(&b, int_only) => {
                invalid(self, b);
                None
            }
            (None, _) => None,
        }
    }

    /// If the type `found` of expression `e` differs from `want`, emits a type mismatch error (if `found` is `None`, a
    /// diagnostic was already emitted, so it does nothing).
    pub(super) fn expect_ty(&mut self, e: &Expr, found: Option<Ty>, want: Ty) {
        if let Some(f) = found
            && f != want
        {
            self.type_mismatch(e.pos(), &want.name(), &f.name());
        }
    }
}
