//! Registering command aliases, descriptions, parameter names and default values.

use super::*;

impl Registry {
    /// Registers an old name (alias) of a command (section 2.2 of `spec/host/02-registry.md`). Scripts can call it by the
    /// old name (the warning `deprecated_command_name` is emitted).
    ///
    /// `old` is `namespace.name` (identifiers; it may be the name of another package). `command` is the official name of a
    /// registered command. If the old name collides with another command, variable, type or alias, `DuplicateCommand`; if
    /// `command` does not exist, `UnknownCommand`.
    pub fn alias(&mut self, old: &str, command: &str) -> Result<(), RegistryError> {
        let valid = is_qualified_name(old);
        if !valid {
            return Err(RegistryError::InvalidName(old.to_string()));
        }
        if self.stops.iter().all(|d| d.name != command)
            && self.natives.iter().all(|d| d.name != command)
        {
            return Err(RegistryError::UnknownCommand(command.to_string()));
        }
        if self.name_taken(old) {
            return Err(RegistryError::DuplicateCommand(old.to_string()));
        }
        self.aliases.push((old.to_string(), command.to_string()));
        Ok(())
    }

    /// Adds a description to a command, variable or type (`doc` in declaration files). `UnknownCommand` if the name does not
    /// exist.
    pub fn doc(&mut self, name: &str, text: &str) -> Result<(), RegistryError> {
        let text = Some(text.to_string());
        if let Some(d) = self.stops.iter_mut().find(|d| d.name == name) {
            d.doc = text;
        } else if let Some(d) = self.natives.iter_mut().find(|d| d.name == name) {
            d.doc = text;
        } else if let Some(d) = self.vars.iter_mut().find(|d| d.name == name) {
            d.doc = text;
        } else if let Some(i) = self.types.iter().position(|d| d.name == name) {
            self.type_docs[i] = text;
        } else {
            return Err(RegistryError::UnknownCommand(name.to_string()));
        }
        Ok(())
    }

    /// Names the parameters of a command (`params[].name` in declaration files; used for completion in tools). The count must
    /// be the same as the parameters (`InvalidParams`).
    pub fn param_names(&mut self, command: &str, names: &[&str]) -> Result<(), RegistryError> {
        let slot = if let Some(d) = self.stops.iter_mut().find(|d| d.name == command) {
            &mut d.params
        } else if let Some(d) = self.natives.iter_mut().find(|d| d.name == command) {
            &mut d.params
        } else {
            return Err(RegistryError::UnknownCommand(command.to_string()));
        };
        if slot.len() != names.len() {
            return Err(RegistryError::InvalidParams(command.to_string()));
        }
        *slot = names.iter().map(|n| Some((*n).to_string())).collect();
        Ok(())
    }

    /// Adds default values to the parameters of a command (the names are those given with [`Registry::param_names`]). Default
    /// values must match the parameter types. Parameters with defaults must come last (otherwise `InvalidParams`). Scripts can
    /// omit parameters with defaults (section 3.5 of `spec/language/03-statements.md`).
    pub fn param_defaults(
        &mut self,
        command: &str,
        defaults: &[(&str, Value)],
    ) -> Result<(), RegistryError> {
        let types = &self.types;
        let (args, params, slot) =
            if let Some(d) = self.stops.iter_mut().find(|d| d.name == command) {
                (&d.args, &d.params, &mut d.defaults)
            } else if let Some(d) = self.natives.iter_mut().find(|d| d.name == command) {
                (&d.args, &d.params, &mut d.defaults)
            } else {
                return Err(RegistryError::UnknownCommand(command.to_string()));
            };
        let bad = || RegistryError::InvalidParams(command.to_string());
        let mut next = slot.clone();
        for (name, value) in defaults {
            let i = params
                .iter()
                .position(|p| p.as_deref() == Some(*name))
                .ok_or_else(bad)?;
            if !value.matches_in(&args[i], types) {
                return Err(bad());
            }
            next[i] = Some(value.clone());
        }
        // Parameters with defaults come last
        if next.windows(2).any(|w| w[0].is_some() && w[1].is_none()) {
            return Err(bad());
        }
        *slot = next;
        Ok(())
    }

    /// Makes parameter `index` (`str`) of a stop command a body that text tables can translate (such as input prompts;
    /// section 5.2a of `spec/host/05-texts.md`). Only one per command. `UnknownCommand` if the command does not exist,
    /// `InvalidParams` if `index` is out of range or not `str`.
    pub fn text_param(&mut self, command: &str, index: usize) -> Result<(), RegistryError> {
        let Some(d) = self.stops.iter_mut().find(|d| d.name == command) else {
            return Err(RegistryError::UnknownCommand(command.to_string()));
        };
        if d.args.get(index) != Some(&Ty::Str) {
            return Err(RegistryError::InvalidParams(command.to_string()));
        }
        d.text_param = Some(index);
        Ok(())
    }
}
