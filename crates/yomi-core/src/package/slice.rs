//! A type-erased container for state slices.

use super::*;

/// A type-erased state slice.
pub(crate) trait SliceObj: Send + Sync {
    /// Clones the slice.
    fn clone_box(&self) -> Box<dyn SliceObj>;
    /// Turns the slice into `postcard` bytes (`Err` if it cannot be serialized).
    fn to_bytes(&self) -> Result<Vec<u8>, ()>;
    /// Borrows it as `Any` (for converting to the concrete type).
    fn as_any(&self) -> &dyn Any;
    /// Borrows it as `Any` in a mutable form.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// An implementation of `SliceObj` that holds a value of type `T`.
pub(super) struct Typed<T>(T);

impl<T: Clone + Serialize + Send + Sync + 'static> SliceObj for Typed<T> {
    fn clone_box(&self) -> Box<dyn SliceObj> {
        Box::new(Typed(self.0.clone()))
    }
    fn to_bytes(&self) -> Result<Vec<u8>, ()> {
        postcard::to_stdvec(&self.0).map_err(|_| ())
    }
    fn as_any(&self) -> &dyn Any {
        &self.0
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.0
    }
}

/// One state slice (a type-erased container that can be cloned and serialized).
pub(crate) struct SliceBox {
    pub(super) inner: Box<dyn SliceObj>,
}

impl SliceBox {
    /// Makes a type-erased slice from a value.
    pub(crate) fn new<T: Clone + Serialize + Send + Sync + 'static>(value: T) -> Self {
        Self {
            inner: Box::new(Typed(value)),
        }
    }

    /// Turns the slice into `postcard` bytes (`Err` if it cannot be serialized).
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, ()> {
        self.inner.to_bytes()
    }

    /// Borrows it as type `T`. `None` if the type differs.
    pub(crate) fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.inner.as_any().downcast_ref::<T>()
    }

    /// Borrows it as type `T` in a mutable form. `None` if the type differs.
    pub(crate) fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.as_any_mut().downcast_mut::<T>()
    }
}

impl Clone for SliceBox {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
        }
    }
}

impl std::fmt::Debug for SliceBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SliceBox(..)")
    }
}

/// A function that returns, from `(saved version, bytes)`, the slice and whether `migrate` was used.
pub(crate) type LoadSliceFn = fn(u32, &[u8]) -> Result<(SliceBox, bool), MigrateError>;

/// The type-erased information of one registered package (held by `Registry`).
#[derive(Debug, Clone)]
pub(crate) struct PackageEntry {
    pub name: String,
    pub version: u32,
    pub persistent_version: u32,
    /// `Package::RULES_VERSION`.
    pub rules_version: u32,
    pub depends: Vec<String>,
    /// Whether it is a standard package (written `import std::name` in scripts).
    pub std: bool,
    /// Whether `State` is `()` (not put into saves).
    pub unit_state: bool,
    /// Whether `Persistent` is `()` (not put into persistent data).
    pub unit_persistent: bool,
    pub new_state: fn() -> SliceBox,
    pub new_persistent: fn() -> SliceBox,
    /// From `(saved version, bytes)`, the slice and whether `migrate` was used.
    pub load_state: LoadSliceFn,
    pub load_persistent: LoadSliceFn,
    /// Calls `Package::clear_cues` on a type-erased state slice.
    pub clear_cues: fn(&mut SliceBox),
    /// Calls `Package::cue_len` on a type-erased state slice.
    pub cue_len: fn(&SliceBox) -> usize,
}

impl PackageEntry {
    /// A package without state, made from a declaration file (only for checking; cannot run in a VM).
    pub(crate) fn decl_only(name: &str, version: u32, depends: Vec<String>, std: bool) -> Self {
        Self {
            name: name.to_string(),
            version,
            persistent_version: 1,
            rules_version: 1,
            depends,
            std,
            unit_state: true,
            unit_persistent: true,
            new_state: || SliceBox::new(()),
            new_persistent: || SliceBox::new(()),
            load_state: |_, _| Ok((SliceBox::new(()), false)),
            load_persistent: |_, _| Ok((SliceBox::new(()), false)),
            clear_cues: |_| {},
            cue_len: |_| 0,
        }
    }

    /// Makes the registration information of package `P`.
    pub(crate) fn of<P: Package>() -> Self {
        Self {
            name: P::NAME.to_string(),
            version: P::VERSION,
            persistent_version: P::PERSISTENT_VERSION,
            rules_version: P::RULES_VERSION,
            depends: P::DEPENDS.iter().map(|d| (*d).to_string()).collect(),
            std: P::STD,
            unit_state: std::any::TypeId::of::<P::State>() == std::any::TypeId::of::<()>(),
            unit_persistent: std::any::TypeId::of::<P::Persistent>()
                == std::any::TypeId::of::<()>(),
            new_state: || SliceBox::new(P::State::default()),
            new_persistent: || SliceBox::new(P::Persistent::default()),
            load_state: |version, raw| {
                if version == P::VERSION {
                    let v: P::State = decode_slice(raw)?;
                    Ok((SliceBox::new(v), false))
                } else {
                    Ok((SliceBox::new(P::migrate(version, raw)?), true))
                }
            },
            clear_cues: |slice| {
                if let Some(state) = slice.downcast_mut::<P::State>() {
                    P::clear_cues(state);
                }
            },
            cue_len: |slice| {
                slice
                    .downcast_ref::<P::State>()
                    .map_or(0, |state| P::cue_len(state))
            },
            load_persistent: |version, raw| {
                if version == P::PERSISTENT_VERSION {
                    let v: P::Persistent = decode_slice(raw)?;
                    Ok((SliceBox::new(v), false))
                } else {
                    Ok((SliceBox::new(P::migrate_persistent(version, raw)?), true))
                }
            },
        }
    }
}
