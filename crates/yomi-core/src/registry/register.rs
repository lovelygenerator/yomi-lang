//! Registering commands, types, variables and packages.

use super::*;

impl Registry {
    /// Makes an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a command, variable, type or alias with the same name already exists.
    pub(crate) fn name_taken(&self, name: &str) -> bool {
        self.stops.iter().any(|d| d.name == name)
            || self.natives.iter().any(|d| d.name == name)
            || self.vars.iter().any(|d| d.name == name)
            || self.types.iter().any(|d| d.name == name)
            || self.aliases.iter().any(|(old, _)| old == name)
    }

    /// Registers a stop command by declaring only its types.
    ///
    /// The entry for commands that do not belong to a package (the host's own, such as `ui.say`). Usable without `import`.
    /// Package commands are registered with [`Registry::package`] and [`PackageReg`]. Register the `struct`s and `enum`s
    /// used by parameters and return values first with [`Registry::ty`].
    pub fn stop<A: YomiArgs, R: YomiType>(
        &mut self,
        name: &str,
    ) -> Result<CommandId, RegistryError> {
        self.add_stop(name, A::types(), R::ty(), None, None, None)
    }

    /// Whether the parameter and return types can be used (the `struct`s and `enum`s used are registered, `map` keys are
    /// valid, and they are not too deep).
    pub(crate) fn check_ty(&self, owner: &str, ty: &Ty) -> Result<(), RegistryError> {
        let mut used = Vec::new();
        ty.collect_user(&mut used);
        for n in used {
            if find_type(&self.types, n).is_none() {
                return Err(RegistryError::UnknownType {
                    name: n.to_string(),
                    used_by: owner.to_string(),
                });
            }
        }
        let mut keys = Vec::new();
        collect_map_keys(ty, &mut keys);
        if keys.iter().any(|k| !k.is_key_in(&self.types)) {
            return Err(RegistryError::InvalidMapKey(owner.to_string()));
        }
        if ty.depth_in(&self.types) > MAX_TYPE_DEPTH {
            return Err(RegistryError::TypeTooDeep(owner.to_string()));
        }
        Ok(())
    }

    /// Registers a stop command and returns its number. A bad name or a duplicate is `Err`. `package` is the package it
    /// belongs to (`None` if none).
    pub(crate) fn add_stop(
        &mut self,
        name: &str,
        args: Vec<Ty>,
        ret: Ty,
        package: Option<&str>,
        check: Option<StopCheck>,
        ret_check: Option<RetCheck>,
    ) -> Result<CommandId, RegistryError> {
        match package {
            Some(p) if member_of(p, name).is_none() => {
                return Err(RegistryError::InvalidName(name.to_string()));
            }
            // Commands that do not belong to a package are also limited to identifiers (UAX #31, NFC) joined with `.`
            None if !is_command_name(name) => {
                return Err(RegistryError::InvalidName(name.to_string()));
            }
            _ => {}
        }
        if self.name_taken(name) {
            return Err(RegistryError::DuplicateCommand(name.to_string()));
        }
        for t in args.iter().chain(std::iter::once(&ret)) {
            self.check_ty(name, t)?;
        }
        let id = CommandId(self.stops.len() as u32);
        self.stops.push(StopDecl {
            name: name.to_string(),
            params: vec![None; args.len()],
            defaults: vec![None; args.len()],
            args,
            ret,
            package: package.map(str::to_string),
            doc: None,
            text_param: None,
        });
        self.stop_checks.push(check);
        self.stop_ret_checks.push(ret_check);
        Ok(id)
    }

    /// Registers an effect or query command and returns the position of the declaration. A bad name or a duplicate is `Err`.
    /// `package` is the position of a registered package (the first half of the name must be that package's name).
    pub(crate) fn add_native(
        &mut self,
        package: usize,
        name: &str,
        kind: NativeKind,
        args: Vec<Ty>,
        ret: Ty,
        func: NativeFn,
    ) -> Result<(), RegistryError> {
        if member_of(&self.packages[package].name, name).is_none() {
            return Err(RegistryError::InvalidName(name.to_string()));
        }
        if self.name_taken(name) {
            return Err(RegistryError::DuplicateCommand(name.to_string()));
        }
        if kind == NativeKind::Query && ret == Ty::Unit {
            return Err(RegistryError::QueryWithoutValue(name.to_string()));
        }
        for t in args.iter().chain(std::iter::once(&ret)) {
            self.check_ty(name, t)?;
        }
        self.natives.push(NativeDecl {
            name: name.to_string(),
            kind,
            params: vec![None; args.len()],
            defaults: vec![None; args.len()],
            args,
            ret,
            package,
            func,
            doc: None,
        });
        Ok(())
    }

    /// Registers a `struct` or `enum` (a type with `#[derive(YomiType)]`) as a type usable from scripts (section 2.5 of
    /// `spec/host/02-registry.md`).
    ///
    /// The type name is `namespace.TypeName` (the name of `T::decl()`). Scripts `import` the namespace to use it. Register the
    /// types used by fields and variants first (`UnknownType` otherwise; registration order makes recursion impossible).
    /// On failure, the registry is unchanged.
    pub fn ty<T: YomiDecl>(&mut self) -> Result<(), RegistryError> {
        self.add_type(None, T::decl())
    }

    /// Registers the declaration of a `struct` or `enum` type. `package` is the package it belongs to (`None` if none). A bad
    /// name or a duplicate is `Err`.
    pub(crate) fn add_type(
        &mut self,
        package: Option<&str>,
        decl: TypeDecl,
    ) -> Result<(), RegistryError> {
        let name = decl.name.clone();
        let valid = match package {
            Some(p) => member_of(p, &name).is_some(),
            None => is_qualified_name(&name),
        };
        if !valid {
            return Err(RegistryError::InvalidName(name));
        }
        if self.types.iter().any(|t| t.name == name) {
            return Err(RegistryError::DuplicateType(name));
        }
        if self.name_taken(&name) {
            return Err(RegistryError::DuplicateCommand(name));
        }
        // Check the contents
        let mut tys: Vec<&Ty> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let bad = |what: &str| RegistryError::InvalidTypeDecl(format!("{name}: {what}"));
        match &decl.kind {
            TypeKind::Struct { fields } => {
                for (f, t) in fields {
                    if !is_plain_ident(f) {
                        return Err(bad(f));
                    }
                    if seen.contains(&f.as_str()) {
                        return Err(bad(f));
                    }
                    seen.push(f);
                    tys.push(t);
                }
            }
            TypeKind::Enum { variants } => {
                if variants.is_empty() {
                    return Err(bad("no variants"));
                }
                for v in variants {
                    if !is_plain_ident(&v.name) || seen.contains(&v.name.as_str()) {
                        return Err(bad(&v.name));
                    }
                    seen.push(&v.name);
                    tys.extend(v.args.iter());
                }
            }
        }
        for t in tys {
            if contains_unit(t) {
                return Err(bad("()"));
            }
            let mut used = Vec::new();
            t.collect_user(&mut used);
            if used.contains(&name.as_str()) {
                return Err(RegistryError::RecursiveType(name));
            }
            self.check_ty(&name, t)?;
        }
        self.types.push(decl);
        self.type_docs.push(None);
        let me = match &self.types[self.types.len() - 1].kind {
            TypeKind::Struct { .. } => Ty::Struct(name.clone()),
            TypeKind::Enum { .. } => Ty::Enum(name.clone()),
        };
        if me.depth_in(&self.types) > MAX_TYPE_DEPTH {
            self.types.pop();
            self.type_docs.pop();
            return Err(RegistryError::TypeTooDeep(name));
        }
        Ok(())
    }

    /// Registers a variable declared by the host (section 2.7 of `spec/host/02-registry.md`).
    ///
    /// The name is `namespace.variable`. After `import namespace`, scripts can read and write it as `namespace.variable`. The
    /// type is `int`, `float`, `bool`, `str`, `list`, `map`, `Option`, or a registered `struct` or `enum` (register it first
    /// with [`Registry::ty`]). `.read_only()` can follow. Package variables are registered with [`PackageReg::var`].
    pub fn var<T: YomiType + IntoValue>(
        &mut self,
        name: &str,
        kind: Kind,
        initial: T,
    ) -> Result<VarHandle<'_>, RegistryError> {
        let index = self.add_var(None, name, T::ty(), kind, initial.into_value())?;
        Ok(self.var_handle(Some(index)))
    }

    /// Registers a host-declared variable and returns its position. A bad name or a duplicate is `Err`.
    pub(crate) fn add_var(
        &mut self,
        package: Option<&str>,
        name: &str,
        ty: Ty,
        kind: Kind,
        init: Value,
    ) -> Result<usize, RegistryError> {
        let valid = match package {
            Some(p) => member_of(p, name).is_some(),
            None => is_qualified_name(name),
        };
        if !valid {
            return Err(RegistryError::InvalidName(name.to_string()));
        }
        if self.name_taken(name) {
            return Err(RegistryError::DuplicateCommand(name.to_string()));
        }
        if contains_unit(&ty) {
            return Err(RegistryError::UnsupportedVarType(name.to_string()));
        }
        self.check_ty(name, &ty)?;
        let Some(init) = init.normalized() else {
            return Err(RegistryError::InvalidVarValue(name.to_string()));
        };
        if !init.matches_in(&ty, &self.types) {
            return Err(RegistryError::InvalidVarValue(name.to_string()));
        }
        self.vars.push(VarDecl {
            name: name.to_string(),
            ty,
            kind,
            init,
            read_only: false,
            doc: None,
        });
        Ok(self.vars.len() - 1)
    }

    /// Makes a handle for settings from the position of a variable (with `None`, a handle that changes nothing).
    pub(crate) fn var_handle(&mut self, index: Option<usize>) -> VarHandle<'_> {
        VarHandle {
            var: index.map(|i| &mut self.vars[i]),
        }
    }

    /// Registers package `P` (section 3.1 of `spec/host/03-packages.md`).
    ///
    /// `P::register` registers the commands and variables. Register the packages in `P::DEPENDS` first (`MissingDependency`
    /// if one is not registered, `DependencyCycle` if it is itself; since dependencies must be registered first, they cannot
    /// form cycles). On failure, the registry is unchanged. To register a set without caring about order, use
    /// [`Registry::package_set`] (and [`packages!`](crate::packages)).
    pub fn package<P: Package>(&mut self) -> Result<(), RegistryError> {
        self.package_set(&[PackageItem::of::<P>()])
    }

    /// Registers multiple packages together, in any order (pass the list made by `packages![A, B, C]`).
    ///
    /// - Looks at `DEPENDS` and registers dependencies first (reorders; packages unrelated by dependencies keep the given
    ///   order). Dependencies must be in the same set or already registered.
    /// - `MissingDependency` if a dependency is nowhere. `DependencyCycle` if dependencies form a cycle (including itself).
    ///   `InvalidName` if a name is invalid, `DuplicatePackage` if the same name is already registered or repeated in the set.
    /// - On failure, the registry is unchanged from before the call (all are registered, or none).
    pub fn package_set(&mut self, items: &[PackageItem]) -> Result<(), RegistryError> {
        // Check names, duplicates and the presence of dependencies in the given order
        for (i, item) in items.iter().enumerate() {
            if !is_plain_ident(item.name) {
                return Err(RegistryError::InvalidName(item.name.to_string()));
            }
            if self.packages.iter().any(|e| e.name == item.name)
                || items[..i].iter().any(|o| o.name == item.name)
            {
                return Err(RegistryError::DuplicatePackage(item.name.to_string()));
            }
            for dep in item.depends {
                if *dep == item.name {
                    return Err(RegistryError::DependencyCycle(item.name.to_string()));
                }
                let known = self.packages.iter().any(|e| e.name == *dep)
                    || items.iter().any(|o| o.name == *dep);
                if !known {
                    return Err(RegistryError::MissingDependency {
                        package: item.name.to_string(),
                        dependency: (*dep).to_string(),
                    });
                }
            }
        }
        // Put dependencies first (take out those whose dependencies are ready, in the given order)
        let mut order: Vec<usize> = Vec::with_capacity(items.len());
        let mut placed = vec![false; items.len()];
        while order.len() < items.len() {
            let next = (0..items.len()).find(|&i| {
                !placed[i]
                    && items[i].depends.iter().all(|dep| {
                        self.packages.iter().any(|e| e.name == *dep)
                            || items
                                .iter()
                                .enumerate()
                                .any(|(j, o)| placed[j] && o.name == *dep)
                    })
            });
            match next {
                Some(i) => {
                    placed[i] = true;
                    order.push(i);
                }
                None => {
                    let stuck = (0..items.len()).find(|&i| !placed[i]).unwrap_or(0);
                    return Err(RegistryError::DependencyCycle(
                        items[stuck].name.to_string(),
                    ));
                }
            }
        }
        let mut staged = self.clone();
        for i in order {
            (items[i].stage)(&mut staged)?;
        }
        *self = staged;
        Ok(())
    }
}
