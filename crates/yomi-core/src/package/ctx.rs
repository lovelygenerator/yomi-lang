//! The entry to state that command functions receive.

use super::*;
use crate::vm::InterruptHandle;

/// The type-erased entry that the VM passes to functions (the material for making a `Ctx`).
pub(crate) struct Env<'a> {
    pub index: usize,
    pub packages: &'a [PackageEntry],
    pub before: &'a [SliceBox],
    pub own: &'a mut SliceBox,
    pub after: &'a [SliceBox],
    pub persistent: &'a mut SliceBox,
    pub rng: &'a mut Rng,
    pub interrupt: &'a InterruptHandle,
}

/// The entry to state that command functions receive.
///
/// Effect commands get `&mut Ctx<P>`, query commands get `&Ctx<P>`. They can read and write only the state slice and
/// persistent slice of their own package `P`, and can read the state of other packages only for those declared in
/// `P::DEPENDS` ([`Ctx::dep`]). There is no time and no files.
pub struct Ctx<'a, P: Package> {
    pub(super) state: &'a mut P::State,
    pub(super) persistent: &'a mut P::Persistent,
    pub(super) before: &'a [SliceBox],
    pub(super) after: &'a [SliceBox],
    pub(super) index: usize,
    pub(super) packages: &'a [PackageEntry],
    pub(super) rng: &'a mut Rng,
    pub(super) interrupt: &'a InterruptHandle,
}

impl<'a, P: Package> Ctx<'a, P> {
    /// Makes a context from the environment of running a command. `Err` if the state types do not match.
    pub(crate) fn new(env: Env<'a>) -> Result<Self, CommandError> {
        let state = env.own.downcast_mut::<P::State>().ok_or_else(|| {
            CommandError::with_code("core.state_type_mismatch", [("package", P::NAME.into())])
        })?;
        let persistent = env
            .persistent
            .downcast_mut::<P::Persistent>()
            .ok_or_else(|| {
                CommandError::with_code(
                    "core.persistent_type_mismatch",
                    [("package", P::NAME.into())],
                )
            })?;
        Ok(Self {
            state,
            persistent,
            before: env.before,
            after: env.after,
            index: env.index,
            packages: env.packages,
            rng: env.rng,
            interrupt: env.interrupt,
        })
    }

    /// Whether an interrupt is requested for the running VM (`spec/host/01-execution.md`, 1.9a). A long command checks this
    /// now and then, and returns [`CommandError::interrupted`] if it is `true`.
    pub fn interrupted(&self) -> bool {
        self.interrupt.is_interrupted()
    }

    /// The state slice of its own package (read).
    pub fn state(&self) -> &P::State {
        self.state
    }

    /// The state slice of its own package (write). Only effect commands receive `&mut Ctx`.
    pub fn state_mut(&mut self) -> &mut P::State {
        self.state
    }

    /// The persistent slice of its own package (read).
    pub fn persistent(&self) -> &P::Persistent {
        self.persistent
    }

    /// The persistent slice of its own package (write). Not undone by rollback.
    pub fn persistent_mut(&mut self) -> &mut P::Persistent {
        self.persistent
    }

    /// The core's random generator (the same sequence as `random` in scripts). Needs `&mut`, so only effect commands can use
    /// it.
    pub fn rng(&mut self) -> &mut Rng {
        self.rng
    }

    /// The state slice of dependency package `Q` (read only).
    ///
    /// `Err` if `Q` is not in `P::DEPENDS` or is not registered (becomes a runtime error).
    pub fn dep<Q: Package>(&self) -> Result<&Q::State, CommandError> {
        if !P::DEPENDS.contains(&Q::NAME) {
            return Err(CommandError::with_code(
                "core.dep_not_declared",
                [("package", P::NAME.into()), ("dep", Q::NAME.into())],
            ));
        }
        let j = self
            .packages
            .iter()
            .position(|e| e.name == Q::NAME)
            .ok_or_else(|| {
                CommandError::with_code("core.dep_not_registered", [("dep", Q::NAME.into())])
            })?;
        let slot = match j.cmp(&self.index) {
            std::cmp::Ordering::Less => &self.before[j],
            std::cmp::Ordering::Greater => &self.after[j - self.index - 1],
            std::cmp::Ordering::Equal => {
                return Err(CommandError::with_code(
                    "core.dep_is_self",
                    [("package", P::NAME.into())],
                ));
            }
        };
        slot.downcast_ref::<Q::State>().ok_or_else(|| {
            CommandError::with_code("core.state_type_mismatch", [("package", Q::NAME.into())])
        })
    }
}
