//! Saving and restoring packages' state slices and persistent slices (sections 3.2 and 3.5 of
//! `spec/host/03-packages.md`).
//!
//! The core does not interpret the contents of slices. It only carries a version and bytes (`postcard`) per package;
//! restoring is done by the functions captured at registration (`PackageEntry`).

use super::save::SavedSlice;
use super::{LoadError, LoadErrorKind, Vm};
use crate::package::SliceBox;

/// The report of restoring slices.
#[derive(Debug, Default, Clone)]
pub(super) struct SliceReport {
    /// Slices of packages that are not registered, which were ignored (package names).
    pub ignored: Vec<String>,
    /// Not in the saved data, so given default values (package names).
    pub defaulted: Vec<String>,
    /// Versions differed, so migrated with `migrate` (package names).
    pub migrated: Vec<String>,
}

/// The restored slices (the index of the package in the registry, and the slice), and the report.
pub(super) struct Resolved {
    pub slices: Vec<(usize, SliceBox)>,
    pub report: SliceReport,
}

impl Vm<'_> {
    /// Turns the slices into the form to save (packages whose `State` is `()` are not included). If a package could not be
    /// serialized, returns its name as `Err` (when `persistent` is false; for persistent data, those that cannot be are
    /// skipped).
    pub(super) fn save_slices(&self, persistent: bool) -> Result<Vec<SavedSlice>, String> {
        let slices = if persistent {
            &self.persist
        } else {
            &self.states
        };
        let mut out = Vec::new();
        for (e, slice) in self.registry.package_entries().iter().zip(slices) {
            let (unit, version) = if persistent {
                (e.unit_persistent, e.persistent_version)
            } else {
                (e.unit_state, e.version)
            };
            if unit {
                continue;
            }
            match slice.to_bytes() {
                Ok(bytes) => out.push(SavedSlice {
                    name: e.name.to_string(),
                    version,
                    bytes,
                }),
                Err(()) if persistent => {}
                Err(()) => return Err(e.name.to_string()),
            }
        }
        Ok(out)
    }

    /// Restores the saved slices to fit the registry (not yet put into the VM).
    ///
    /// - Slices of packages that are not registered are ignored.
    /// - Registered packages that were not saved get default values (`defaulted`; those whose `State` is `()` are not
    ///   counted).
    /// - If the versions differ, `migrate` (`migrated`). An `Err` or broken bytes give `PackageSlice`.
    pub(super) fn resolve_slices(
        &self,
        saved: &[SavedSlice],
        persistent: bool,
    ) -> Result<Resolved, LoadError> {
        let entries = self.registry.package_entries();
        let mut report = SliceReport::default();
        let mut slices: Vec<(usize, SliceBox)> = Vec::new();
        for s in saved {
            let Some(i) = entries.iter().position(|e| e.name == s.name) else {
                report.ignored.push(s.name.clone());
                continue;
            };
            if slices.iter().any(|(j, _)| *j == i) {
                // There are two or more slices of the same package. It is broken, so the later ones are ignored
                report.ignored.push(s.name.clone());
                continue;
            }
            let load = if persistent {
                entries[i].load_persistent
            } else {
                entries[i].load_state
            };
            match load(s.version, &s.bytes) {
                Ok((slice, migrated)) => {
                    if migrated {
                        report.migrated.push(s.name.clone());
                    }
                    slices.push((i, slice));
                }
                Err(_) => {
                    return Err(LoadError {
                        kind: LoadErrorKind::PackageSlice(s.name.clone()),
                    });
                }
            }
        }
        for (i, e) in entries.iter().enumerate() {
            let unit = if persistent {
                e.unit_persistent
            } else {
                e.unit_state
            };
            if !unit && !slices.iter().any(|(j, _)| *j == i) {
                report.defaulted.push(e.name.to_string());
            }
        }
        Ok(Resolved { slices, report })
    }

    /// Puts the saved state slices into the VM (packages that were not saved get default values).
    pub(super) fn apply_state_slices(
        &mut self,
        saved: &[SavedSlice],
    ) -> Result<SliceReport, LoadError> {
        let resolved = self.resolve_slices(saved, false)?;
        self.install_slices(resolved.slices, false);
        Ok(resolved.report)
    }

    /// Puts in the restored slices. Packages not in `slices` go back to default values.
    pub(super) fn install_slices(&mut self, slices: Vec<(usize, SliceBox)>, persistent: bool) {
        let entries = self.registry.package_entries();
        let target = if persistent {
            &mut self.persist
        } else {
            &mut self.states
        };
        for (i, e) in entries.iter().enumerate() {
            target[i] = if persistent {
                (e.new_persistent)()
            } else {
                (e.new_state)()
            };
        }
        for (i, slice) in slices {
            target[i] = slice;
        }
    }
}
