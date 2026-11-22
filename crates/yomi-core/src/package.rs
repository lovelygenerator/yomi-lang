//! Packages: units that group commands and the state slices they use (`spec/host/03-packages.md`).
//!
//! - `Package`: the declaration of a package (name, version, dependencies, state types, command registration, `migrate`).
//! - `Ctx`: the entry to the state that effect and query command functions receive.
//! - `CommandError`: a runtime error returned by a command function.
//!
//! The core does not interpret the contents of state slices. The VM holds type-erased containers (with functions for
//! cloning and serialization), and only the functions made at registration (captured by `Registry::package`) know the
//! types.

use std::any::Any;
use std::marker::PhantomData;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::diag::DiagArg;
use crate::registry::{
    FromArgs, FromValue, IntoValue, Kind, Registry, RegistryError, VarHandle, YomiArgs, YomiType,
};
use crate::value::{Ty, Value};
use crate::vm::{Rng, VmErrorKind};

mod ctx;
mod error;
mod functions;
mod reg;
mod slice;

pub use ctx::*;
pub use error::*;
pub use functions::*;
pub use reg::*;
pub(crate) use slice::*;

/// A package. A unit that groups commands and the state slices they use (per save, and shared by all saves).
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use yomi_core::{Ctx, Package, PackageReg, Registry};
///
/// #[derive(Clone, Default, Serialize, Deserialize)]
/// struct Count(i64);
///
/// struct Counter;
/// impl Package for Counter {
///     const NAME: &'static str = "counter";
///     type State = Count;
///     type Persistent = ();
///     fn register(reg: &mut PackageReg<'_, Self>) {
///         reg.effect("counter.inc", |ctx: &mut Ctx<Counter>, n: i64| {
///             ctx.state_mut().0 += n;
///         });
///         reg.query("counter.get", |ctx: &Ctx<Counter>| -> i64 { ctx.state().0 });
///     }
/// }
///
/// let mut reg = Registry::new();
/// reg.package::<Counter>().unwrap();
/// ```
pub trait Package: Sized + 'static {
    /// The name. It becomes the first half of command names (`counter` in `counter.inc`) and the key of the state slice.
    const NAME: &'static str;
    /// The version of the state slice (`State`). Increase it when its shape changes.
    const VERSION: u32 = 1;
    /// Whether this is a standard package. If so, scripts write `import std::name` (`std` cannot be used as a module name).
    /// Packages made by the host keep the default `false` (`import name`).
    const STD: bool = false;
    /// The names of the packages whose state this reads. They must be registered first.
    const DEPENDS: &'static [&'static str] = &[];
    /// The per-save state. `()` if there is none. Requires `Send + Sync` so the VM can run on other threads.
    type State: Clone + Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    /// The version of the persistent slice (`Persistent`).
    const PERSISTENT_VERSION: u32 = 1;
    /// The version of the rules that determine command results (for example, how characters are counted). Separately from
    /// the shape of the state, increase it when a fix changes the result for the same arguments. Packages without state can
    /// use it too.
    ///
    /// It is recorded in saves, and if it differs from the current value on load, it appears in
    /// [`LoadReport::changed_rules`](crate::LoadReport::changed_rules) (the load continues; `migrate` is not called).
    const RULES_VERSION: u32 = 1;
    /// State shared by all saves (it goes into persistent data and is not undone by rollback). `()` if there is none.
    /// Requires `Send + Sync`.
    type Persistent: Clone + Serialize + DeserializeOwned + Default + Send + Sync + 'static;

    /// Registers commands and variables. The first half of command and variable names must be `NAME`.
    fn register(reg: &mut PackageReg<'_, Self>);

    /// Clears the cues in the state (`cues`, a list of one-time events). Does nothing by default.
    ///
    /// The VM calls this for all packages at the start of `run` and `resume`, and right after loading or restoring
    /// (`Vm::load`, `Vm::restore`). Cues are part of the state, so they also go into saves and snapshots, but they are cleared
    /// right after restoring, so old cues are not replayed. The host reads them from `Vm::package_state` at a stop point and
    /// finishes reading before the next `resume` (section 3.6 of `spec/host/03-packages.md`).
    fn clear_cues(state: &mut Self::State) {
        let _ = state;
    }

    /// Returns the number of cues in the state. 0 by default (no cues).
    ///
    /// The VM compares this number before and after running a command and records the increase in a table in the order they
    /// were written ([`Vm::cue_order`](crate::Vm::cue_order)). Packages that only append cues (never remove or reorder cues in
    /// the middle) return the length of `cues`. Return 0 after clearing with `clear_cues`.
    fn cue_len(state: &Self::State) -> usize {
        let _ = state;
        0
    }

    /// Makes the current `State` from the bytes of a save with an older version. Not called when the versions are the same.
    /// By default, returns `Err`, treating older versions as unreadable.
    fn migrate(old_version: u32, raw: &[u8]) -> Result<Self::State, MigrateError> {
        let _ = raw;
        Err(MigrateError::unsupported_version(old_version))
    }

    /// Makes the current `Persistent` from the bytes of a persistent slice with an older version. `Err` by default.
    fn migrate_persistent(old_version: u32, raw: &[u8]) -> Result<Self::Persistent, MigrateError> {
        let _ = raw;
        Err(MigrateError::unsupported_version(old_version))
    }
}
