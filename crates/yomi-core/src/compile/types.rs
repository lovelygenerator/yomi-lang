//! Compilation: resolving type names and checking type declarations.

use super::*;

impl Compiler<'_> {
    /// Turns a type name into a type. Diagnostics are emitted here.
    pub(super) fn ty_from_name(&mut self, t: &TyName) -> Option<Ty> {
        let ty = self.ty_rec(t)?;
        if self.types_ready && ty.depth_in(&self.types) > MAX_TYPE_DEPTH {
            self.error(
                t.pos,
                DiagCode::TooDeep,
                vec![("limit", DiagArg::Int(i64::from(MAX_TYPE_DEPTH)))],
            );
            return None;
        }
        Some(ty)
    }

    /// Turns a type name (`TyName`) into a type. If the number of type arguments or an undeclared name is wrong, emits a
    /// diagnostic and returns `None`.
    pub(super) fn ty_rec(&mut self, t: &TyName) -> Option<Ty> {
        // Check the number of type arguments
        let arity = |this: &mut Self, want: usize| -> bool {
            if t.args.len() == want {
                return true;
            }
            this.error(
                t.pos,
                DiagCode::ArgCount,
                vec![
                    ("name", s(&t.name)),
                    ("expected", DiagArg::Int(want as i64)),
                    ("found", DiagArg::Int(t.args.len() as i64)),
                ],
            );
            false
        };
        match t.name.as_str() {
            "int" | "float" | "bool" | "str" => {
                if !arity(self, 0) {
                    return None;
                }
                Some(match t.name.as_str() {
                    "int" => Ty::Int,
                    "float" => Ty::Float,
                    "bool" => Ty::Bool,
                    _ => Ty::Str,
                })
            }
            "list" | "Option" => {
                if !arity(self, 1) {
                    return None;
                }
                let inner = self.ty_rec(&t.args[0])?;
                Some(if t.name == "list" {
                    Ty::List(Box::new(inner))
                } else {
                    Ty::Option(Box::new(inner))
                })
            }
            "map" => {
                if !arity(self, 2) {
                    return None;
                }
                let key = self.ty_rec(&t.args[0]);
                let value = self.ty_rec(&t.args[1]);
                let (key, value) = (key?, value?);
                self.check_map_key(&key, t.args[0].pos)?;
                Some(Ty::Map(Box::new(key), Box::new(value)))
            }
            "Identity" => {
                if !arity(self, 0) {
                    return None;
                }
                Some(Ty::Identity)
            }
            // `fn(parameter types, ...) -> return type` (the parser made the return type the last argument)
            "fn" => {
                let (ret, params) = t.args.split_last()?;
                let params: Vec<Option<Ty>> = params.iter().map(|p| self.ty_rec(p)).collect();
                let ret = self.ty_rec(ret)?;
                Some(Ty::Fn(
                    params.into_iter().collect::<Option<Vec<_>>>()?,
                    Box::new(ret),
                ))
            }
            _ => {
                let idx = self.find_type_named(&t.name, t.pos)?;
                if !arity(self, 0) {
                    return None;
                }
                let decl = &self.types[idx as usize];
                Some(match decl.kind {
                    TypeKind::Struct { .. } => Ty::Struct(decl.name.clone()),
                    TypeKind::Enum { .. } => Ty::Enum(decl.name.clone()),
                })
            }
        }
    }

    /// Checks that a type containing function types is not placed where it goes into saves (`what`). If it cannot be, emits a
    /// diagnostic and returns `None`.
    pub(super) fn saveable(&mut self, ty: Option<Ty>, pos: Pos, what: &str) -> Option<Ty> {
        let ty = ty?;
        if ty.contains_fn() {
            self.error(
                pos,
                DiagCode::FnNotSaveable,
                vec![("type", s(&ty.name())), ("what", s(what))],
            );
            return None;
        }
        Some(ty)
    }

    /// Checks whether the type can be a `map` key. If not, emits a diagnostic and returns `None`. While type declarations are
    /// being read, user-defined types are not decided yet, so it is checked later (returning `Some` then).
    pub(super) fn check_map_key(&mut self, key: &Ty, pos: Pos) -> Option<()> {
        let mut used = Vec::new();
        key.collect_user(&mut used);
        if !self.types_ready && !used.is_empty() {
            self.pending_keys.push((self.cur_mod, pos, key.clone()));
            return Some(());
        }
        if key.is_key_in(&self.types) {
            Some(())
        } else {
            self.error(pos, DiagCode::InvalidMapKey, vec![("type", s(&key.name()))]);
            None
        }
    }

    /// Finds the index into `types` from a type name (`TypeName` or `module.TypeName`). If not found, emits a diagnostic and
    /// returns `None`.
    pub(super) fn find_type_named(&mut self, written: &str, pos: Pos) -> Option<u32> {
        let found = match written.split_once('.') {
            None => self.md().type_map.get(written).copied(),
            Some((m, name)) => match self.resolve_target(m, pos)? {
                Target::Module(midx) => self.modules[midx].type_map.get(name).copied(),
                Target::Package => self.registry_type_index(m, name),
            },
        };
        if found.is_none() && !self.md().poisoned.contains(written) {
            self.name_error(pos, DiagCode::UnknownType, written);
        }
        found
    }

    /// The index into `types` of the registry type `namespace.TypeName`.
    pub(super) fn registry_type_index(&self, ns: &str, name: &str) -> Option<u32> {
        let full = format!("{ns}.{name}");
        self.types[..self.registry_types]
            .iter()
            .position(|t| t.name == full)
            .map(|i| i as u32)
    }

    /// Registers the names of `struct`s and `enum`s (their contents are decided later). Returns the index into `types` for
    /// each declaration (`None` for declarations whose names collided).
    pub(super) fn declare_type_names(&mut self, ast: &Module) -> Vec<Option<u32>> {
        let mut slots = Vec::new();
        for d in &ast.types {
            if self.declared_here(&d.name) {
                self.name_error(d.pos, DiagCode::DuplicateName, &d.name);
                slots.push(None);
                continue;
            }
            let idx = self.types.len() as u32;
            let module = self.md().name.clone();
            self.types.push(TypeDecl {
                name: format!("{module}.{}", d.name),
                kind: match d.kind {
                    TypeDeclKind::Struct(_) => TypeKind::Struct { fields: Vec::new() },
                    TypeDeclKind::Enum(_) => TypeKind::Enum {
                        variants: Vec::new(),
                    },
                },
            });
            self.md_mut().type_map.insert(d.name.clone(), idx);
            slots.push(Some(idx));
        }
        slots
    }

    /// Decides the contents of `struct`s and `enum`s (the types of fields and variants).
    pub(super) fn resolve_type_decls(&mut self, ast: &Module, slots: &[Option<u32>]) {
        for (d, slot) in ast.types.iter().zip(slots) {
            let Some(idx) = *slot else {
                continue;
            };
            let kind = match &d.kind {
                TypeDeclKind::Struct(fields) => {
                    let mut out: Vec<(String, Ty)> = Vec::new();
                    for (name, pos, ty) in fields {
                        let resolved = self.ty_from_name(ty);
                        let resolved = self.saveable(resolved, ty.pos, "field");
                        if out.iter().any(|(n, _)| n == name) {
                            self.name_error(*pos, DiagCode::DuplicateField, name);
                            continue;
                        }
                        // Fields whose type could not be decided are not used (a diagnostic was emitted)
                        out.push((name.clone(), resolved.unwrap_or(Ty::Int)));
                    }
                    TypeKind::Struct { fields: out }
                }
                TypeDeclKind::Enum(variants) => {
                    let mut out: Vec<VariantDecl> = Vec::new();
                    for v in variants {
                        let args: Vec<Ty> = v
                            .args
                            .iter()
                            .map(|a| {
                                let t = self.ty_from_name(a);
                                self.saveable(t, a.pos, "field").unwrap_or(Ty::Int)
                            })
                            .collect();
                        if out.iter().any(|o| o.name == v.name) {
                            self.name_error(v.pos, DiagCode::DuplicateName, &v.name);
                            continue;
                        }
                        out.push(VariantDecl {
                            name: v.name.clone(),
                            args,
                        });
                    }
                    TypeKind::Enum { variants: out }
                }
            };
            self.types[idx as usize].kind = kind;
        }
    }

    /// Checks type declarations: types containing themselves (recursion), and nesting depth. Recursive types are replaced with
    /// empty declarations (so later processing does not run forever).
    pub(super) fn check_type_graph(&mut self, asts: &[Module], slots: &[Vec<Option<u32>>]) {
        let n = self.types.len();
        // From type i to type j (directly)
        let edges: Vec<Vec<usize>> = (0..n)
            .map(|i| {
                self.types[i]
                    .used_types()
                    .iter()
                    .filter_map(|name| self.types.iter().position(|t| t.name == *name))
                    .collect()
            })
            .collect();
        let mut recursive = vec![false; n];
        for (start, flag) in recursive.iter_mut().enumerate() {
            // Whether it can go from start back to start
            let mut seen = vec![false; n];
            let mut todo: Vec<usize> = edges[start].clone();
            while let Some(k) = todo.pop() {
                if k == start {
                    *flag = true;
                    break;
                }
                if !std::mem::replace(&mut seen[k], true) {
                    todo.extend(edges[k].iter().copied());
                }
            }
        }
        for (m, (ast, sl)) in asts.iter().zip(slots).enumerate() {
            self.cur_mod = m;
            for (d, slot) in ast.types.iter().zip(sl) {
                let Some(idx) = *slot else {
                    continue;
                };
                let idx = idx as usize;
                if recursive[idx] {
                    let name = d.name.clone();
                    self.name_error(d.pos, DiagCode::RecursiveType, &name);
                }
            }
        }
        for (i, &r) in recursive.iter().enumerate() {
            if r {
                self.types[i].kind = TypeKind::Struct { fields: Vec::new() };
            }
        }
        for (m, (ast, sl)) in asts.iter().zip(slots).enumerate() {
            self.cur_mod = m;
            for (d, slot) in ast.types.iter().zip(sl) {
                let Some(idx) = *slot else {
                    continue;
                };
                let decl = &self.types[idx as usize];
                let name = decl.name.clone();
                let ty = match decl.kind {
                    TypeKind::Struct { .. } => Ty::Struct(name),
                    TypeKind::Enum { .. } => Ty::Enum(name),
                };
                if !recursive[idx as usize] && ty.depth_in(&self.types) > MAX_TYPE_DEPTH {
                    self.error(
                        d.pos,
                        DiagCode::TooDeep,
                        vec![("limit", DiagArg::Int(i64::from(MAX_TYPE_DEPTH)))],
                    );
                    self.types[idx as usize].kind = TypeKind::Struct { fields: Vec::new() };
                }
            }
        }
    }

    /// If the expression refers to a type name (`TypeName`, `module.TypeName`), the index into `types`. Variable names are
    /// not type names.
    pub(super) fn type_index_of(&self, head: &Expr) -> Option<u32> {
        match head {
            Expr::Name(a, _) if !self.var_exists(a) => self.md().type_map.get(a).copied(),
            Expr::Qualified(m, t, _) if !self.var_exists(m) => match self.peek_module(m) {
                Some(midx) => self.modules[midx].type_map.get(t).copied(),
                None if self.md().pkg_imports.contains(m) => {
                    self.mark_import(m);
                    self.registry_type_index(m, t)
                }
                None => None,
            },
            _ => None,
        }
    }
}
