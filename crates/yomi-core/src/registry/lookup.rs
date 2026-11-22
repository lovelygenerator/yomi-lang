//! Queries of the registry (looking up commands, variables, types and sugar bindings).

use super::*;

impl Registry {
    /// Finds the ID of a stop command by name (effect and query commands have no IDs).
    pub fn command_id(&self, name: &str) -> Option<CommandId> {
        self.stops
            .iter()
            .position(|d| d.name == name)
            .map(|i| CommandId(i as u32))
    }

    /// The declaration of a stop command.
    ///
    /// # Panics
    /// If the ID was not made by this registry.
    pub fn command(&self, id: CommandId) -> &StopDecl {
        &self.stops[id.0 as usize]
    }

    /// Finds the kind of a command by name (stop, effect, query).
    pub fn command_kind(&self, name: &str) -> Option<CommandKind> {
        self.find_command(name).map(|c| c.kind)
    }

    /// The registered packages (in registration order).
    pub fn packages(&self) -> impl Iterator<Item = PackageDecl> + '_ {
        self.packages.iter().map(|e| PackageDecl {
            name: e.name.to_string(),
            version: e.version,
            persistent_version: e.persistent_version,
            depends: e.depends.iter().map(|d| (*d).to_string()).collect(),
        })
    }

    /// Variables declared by the host (in registration order).
    pub fn vars(&self) -> &[VarDecl] {
        &self.vars
    }

    /// The names of the registered commands (stop commands, then effect and query commands; each in registration order).
    pub(crate) fn command_names(&self) -> impl Iterator<Item = &str> {
        self.stops
            .iter()
            .map(|d| d.name.as_str())
            .chain(self.natives.iter().map(|d| d.name.as_str()))
    }

    /// Looks up a command by name. For an old name (alias), returns the information of the official command (`via_alias`).
    pub(crate) fn find_command(&self, name: &str) -> Option<CommandInfo> {
        if let Some((_, target)) = self.aliases.iter().find(|(old, _)| old == name) {
            let mut info = self.find_canonical(target)?;
            info.via_alias = true;
            if info.package.is_some() {
                // When using an old name, `import` the old name's namespace
                info.package = name.split_once('.').map(|(ns, _)| ns.to_string());
            }
            return Some(info);
        }
        self.find_canonical(name)
    }

    /// Looks up only commands with official names, without resolving aliases.
    pub(super) fn find_canonical(&self, name: &str) -> Option<CommandInfo> {
        if let Some(i) = self.stops.iter().position(|d| d.name == name) {
            let d = &self.stops[i];
            return Some(CommandInfo {
                name: d.name.clone(),
                via_alias: false,
                kind: CommandKind::Stop,
                args: d.args.clone(),
                ret: d.ret.clone(),
                package: d.package.clone(),
                params: d.params.clone(),
                defaults: d.defaults.clone(),
            });
        }
        let i = self.natives.iter().position(|d| d.name == name)?;
        let d = &self.natives[i];
        Some(CommandInfo {
            name: d.name.clone(),
            via_alias: false,
            kind: match d.kind {
                NativeKind::Effect => CommandKind::Effect,
                NativeKind::Query => CommandKind::Query,
            },
            args: d.args.clone(),
            ret: d.ret.clone(),
            package: Some(self.packages[d.package].name.to_string()),
            params: d.params.clone(),
            defaults: d.defaults.clone(),
        })
    }

    /// The old names of registered commands and their official names (in registration order).
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases.iter().map(|(a, b)| (a.as_str(), b.as_str()))
    }

    /// The registered `struct`s and `enum`s (in registration order).
    pub fn types(&self) -> &[TypeDecl] {
        &self.types
    }

    /// The description of a type.
    pub fn type_doc(&self, name: &str) -> Option<&str> {
        let i = self.types.iter().position(|t| t.name == name)?;
        self.type_docs[i].as_deref()
    }

    /// Whether the registry was made from a declaration file. If so, it can only be used for compiling (checking) scripts
    /// and cannot run in a VM (`Vm::new` and `Vm::load` return errors).
    pub fn is_decl_only(&self) -> bool {
        self.decl_only
    }

    /// The sugar binding. `Some(Some(command name))` if bound, `Some(None)` if ignored, `None` if neither.
    pub fn sugar_binding(&self, sugar: Sugar) -> Option<Option<&str>> {
        match self.slot(sugar)? {
            Binding::Command(id) => Some(Some(self.command(id).name.as_str())),
            Binding::Ignored => Some(None),
        }
    }

    /// The declarations of all commands (in name order; stop, effect and query).
    pub fn commands(&self) -> Vec<CommandDecl> {
        let aliases_of = |name: &str| -> Vec<String> {
            let mut v: Vec<String> = self
                .aliases
                .iter()
                .filter(|(_, t)| t == name)
                .map(|(a, _)| a.clone())
                .collect();
            v.sort();
            v
        };
        let mut out: Vec<CommandDecl> = Vec::new();
        for d in &self.stops {
            out.push(CommandDecl {
                name: d.name.clone(),
                kind: CommandKind::Stop,
                args: d.args.clone(),
                ret: d.ret.clone(),
                params: d.params.clone(),
                aliases: aliases_of(&d.name),
                doc: d.doc.clone(),
                package: d.package.clone(),
                defaults: d.defaults.clone(),
            });
        }
        for d in &self.natives {
            out.push(CommandDecl {
                name: d.name.clone(),
                kind: match d.kind {
                    NativeKind::Effect => CommandKind::Effect,
                    NativeKind::Query => CommandKind::Query,
                },
                args: d.args.clone(),
                ret: d.ret.clone(),
                params: d.params.clone(),
                aliases: aliases_of(&d.name),
                doc: d.doc.clone(),
                package: Some(self.packages[d.package].name.to_string()),
                defaults: d.defaults.clone(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The list of declarations of effect and query commands (in registration order).
    pub(crate) fn natives(&self) -> &[NativeDecl] {
        &self.natives
    }

    /// Finds the position of the declaration of an effect or query command by name.
    pub(crate) fn native_index(&self, name: &str) -> Option<usize> {
        self.natives.iter().position(|d| d.name == name)
    }

    /// The list of registration information of packages (in registration order; the same order as the state slices).
    pub(crate) fn package_entries(&self) -> &[PackageEntry] {
        &self.packages
    }

    /// The argument check of stop command `id` (`None` if none).
    pub(crate) fn stop_check(&self, id: CommandId) -> Option<&StopCheck> {
        self.stop_checks.get(id.0 as usize).and_then(Option::as_ref)
    }

    /// The return value check of stop command `id` (`None` if none).
    pub(crate) fn stop_ret_check(&self, id: CommandId) -> Option<&RetCheck> {
        self.stop_ret_checks
            .get(id.0 as usize)
            .and_then(Option::as_ref)
    }

    /// Finds the position of a host-declared variable by name.
    pub(crate) fn var_index(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }

    /// Whether it is a standard package (`import std::name`).
    pub(crate) fn is_std_package(&self, name: &str) -> bool {
        self.packages.iter().any(|e| e.name == name && e.std)
    }

    /// Whether the name can be `import`ed (a package, a namespace in which the host declared variables or types, or the
    /// namespace of an old command name).
    pub(crate) fn has_namespace(&self, name: &str) -> bool {
        self.namespaces().iter().any(|n| n == name)
    }

    /// The list of names that can be `import`ed (no duplicates; packages, then namespaces of variables).
    pub(crate) fn namespaces(&self) -> Vec<String> {
        let mut out: Vec<String> = self.packages.iter().map(|e| e.name.to_string()).collect();
        // Namespaces of variables, types and old command names can also be `import`ed
        let named = self
            .vars
            .iter()
            .map(|v| v.name.as_str())
            .chain(self.types.iter().map(|t| t.name.as_str()))
            .chain(self.aliases.iter().map(|(old, _)| old.as_str()));
        for name in named {
            if let Some((ns, _)) = name.split_once('.')
                && !out.iter().any(|n| n == ns)
            {
                out.push(ns.to_string());
            }
        }
        out
    }

    /// The names of the variables in namespace `ns` (`ns.variable`).
    pub(crate) fn vars_in(&self, ns: &str) -> impl Iterator<Item = &VarDecl> {
        let prefix = format!("{ns}.");
        self.vars
            .iter()
            .filter(move |v| v.name.starts_with(&prefix))
    }

    /// What plain lines are bound to (`None` if not bound).
    pub(crate) fn plain_binding(&self) -> Option<Binding> {
        self.plain
    }

    /// What quote lines are bound to (`None` if not bound).
    pub(crate) fn quote_binding(&self) -> Option<Binding> {
        self.quote
    }

    /// What choices are bound to (`None` if not bound).
    pub(crate) fn choice_binding(&self) -> Option<Binding> {
        self.choice
    }
}
