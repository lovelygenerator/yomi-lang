//! Sugar bindings.

use super::*;

impl Registry {
    /// Binds a sugar to a stop command.
    ///
    /// The parameter and return types of the command must match the shape of the sugar (`SugarMismatch`).
    /// - Plain line: `(text: str)`, no return value.
    /// - Quote line: `(identity: Option<Identity>, text: str)`, no return value.
    /// - Choice: `(options: list<str>, ids: list<Option<str>>)`, returns `int` (the chosen number).
    pub fn bind(&mut self, sugar: Sugar, command: &str) -> Result<(), RegistryError> {
        let id = self
            .command_id(command)
            .ok_or_else(|| RegistryError::UnknownCommand(command.to_string()))?;
        if self.slot(sugar).is_some() {
            return Err(RegistryError::AlreadyBound(sugar));
        }
        let d = self.command(id);
        let (args, ret) = sugar_shape(sugar);
        if d.args != args || d.ret != ret {
            return Err(RegistryError::SugarMismatch(command.to_string()));
        }
        *self.slot_mut(sugar) = Some(Binding::Command(id));
        Ok(())
    }

    /// Ignores a sugar (syntax and types are checked, but no code is made and it is not a stop point).
    pub fn ignore(&mut self, sugar: Sugar) -> Result<(), RegistryError> {
        if self.slot(sugar).is_some() {
            return Err(RegistryError::AlreadyBound(sugar));
        }
        *self.slot_mut(sugar) = Some(Binding::Ignored);
        Ok(())
    }

    /// What sugar `sugar` is bound to (`None` if not bound).
    pub(crate) fn slot(&self, sugar: Sugar) -> Option<Binding> {
        match sugar {
            Sugar::Plain => self.plain,
            Sugar::Quote => self.quote,
            Sugar::Choice => self.choice,
        }
    }

    /// Returns what sugar `sugar` is bound to, in a mutable form.
    fn slot_mut(&mut self, sugar: Sugar) -> &mut Option<Binding> {
        match sugar {
            Sugar::Plain => &mut self.plain,
            Sugar::Quote => &mut self.quote,
            Sugar::Choice => &mut self.choice,
        }
    }
}

/// The parameter types and return type of a sugar (spec 5.2).
fn sugar_shape(sugar: Sugar) -> (Vec<Ty>, Ty) {
    match sugar {
        Sugar::Plain => (vec![Ty::Str], Ty::Unit),
        Sugar::Quote => (vec![Ty::Option(Box::new(Ty::Identity)), Ty::Str], Ty::Unit),
        Sugar::Choice => (
            vec![
                Ty::List(Box::new(Ty::Str)),
                Ty::List(Box::new(Ty::Option(Box::new(Ty::Str)))),
            ],
            Ty::Int,
        ),
    }
}
