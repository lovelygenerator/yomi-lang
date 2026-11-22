//! Persistent data (the values of `persistent var`s and the passed stop points `seen`). A byte string separate from saves,
//! shared by all saves.

use serde::{Deserialize, Serialize};

use super::save::{Compat, SavedSlice, SavedValue, type_signatures, unwrap, wrap};
use super::{LoadError, LoadErrorKind};
use super::{LoadPolicy, State, Vm};
use crate::program::Program;
use crate::registry::Kind;

/// The marker of persistent data bytes (the first 4 bytes).
const MAGIC: &[u8; 4] = b"YOMP";
/// The format version. Version 2 added host-declared variables (`Kind::Persistent`) and packages' persistent slices,
/// version 3 changed the value format to the same as save version 7 and added `struct` and `enum` declarations, and
/// version 4 matched the value format to save version 8 (which added `map` nodes), and version 5 added the schema
/// (`schema_version`, `var_types`) (versions 1 to 4 cannot be read).
const FORMAT_VERSION: u32 = 5;

/// The body of persistent data.
#[derive(Serialize, Deserialize)]
pub(super) struct PersistentBody {
    /// The schema version of the scripts when saved.
    schema_version: u32,
    /// The `struct` and `enum` declarations when saved (type names and the shapes of declarations).
    types: Vec<(String, String)>,
    /// The types of `vars` then `host_vars` (same names, same order), as type strings.
    var_types: Vec<(String, String)>,
    /// `persistent var`s. Names are `module.variable`.
    vars: Vec<(String, SavedValue)>,
    /// Host-declared variables (`Kind::Persistent`). Names are `package.variable`.
    host_vars: Vec<(String, SavedValue)>,
    /// Packages' persistent slices (in registration order; those whose `Persistent` is `()` are not included).
    slices: Vec<SavedSlice>,
    /// Passed stop points (`module.label#id`, in ascending order).
    seen: Vec<String>,
}

/// A report from reading persistent data. Compatibility of variables follows the same rules as save variables (only
/// `persistent var`s apply).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersistentReport {
    /// Variables that were not in the persistent data and got default values (the declared initial values)
    /// (`module.variable`).
    pub added_vars: Vec<String>,
    /// Variables that were removed from the scripts and discarded (`module.variable`).
    pub removed_vars: Vec<String>,
    /// Variables whose type changed and got default values (the declared initial values) (`module.variable`).
    pub type_changed_vars: Vec<String>,
    /// `struct`s and `enum`s whose declarations changed (`module.TypeName`; including types that contain changed types).
    pub changed_types: Vec<String>,
    /// Persistent slices of packages that are not registered, which were ignored (package names).
    pub ignored_slices: Vec<String>,
    /// Persistent slices that were not in the persistent data and got default values (package names).
    pub defaulted_slices: Vec<String>,
    /// Persistent slices whose versions differed and were migrated with `migrate_persistent` (package names).
    pub migrated_slices: Vec<String>,
    /// The schema version of the persistent data.
    pub from_schema_version: u32,
    /// The schema version of the scripts.
    pub to_schema_version: u32,
    /// Variables converted by `migrate` (`module.variable`; renamed variables have the new name).
    pub migrated_vars: Vec<String>,
    /// Variables whose `migrate` conversion failed and were reset to their initial values under `LoadPolicy::Lenient`.
    pub migrate_failed_vars: Vec<String>,
}

/// The schema inside persistent data (see `save::embedded_schema`).
pub(crate) fn embedded_schema(bytes: &[u8]) -> Result<crate::schema_file::Embedded, LoadError> {
    let body = parse(bytes)?;
    let mut var_types = body.var_types;
    var_types.truncate(body.vars.len());
    Ok(crate::schema_file::Embedded {
        version: body.schema_version,
        types: body.types,
        var_types,
    })
}

/// Reads persistent data bytes (checks the magic, version and checksum, and deserializes the body).
pub(super) fn parse(bytes: &[u8]) -> Result<PersistentBody, LoadError> {
    let body_bytes = unwrap(bytes, MAGIC, FORMAT_VERSION)?;
    let body: PersistentBody = match postcard::from_bytes(body_bytes) {
        Ok(b) => b,
        Err(_) => {
            return Err(LoadError {
                kind: LoadErrorKind::Corrupt,
            });
        }
    };
    if !super::save::schema_consistent(&body.var_types, &body.vars, &body.host_vars) {
        return Err(LoadError {
            kind: LoadErrorKind::Corrupt,
        });
    }
    // Check that the values are not broken (`NaN`, `±∞` and nesting that is too deep are broken)
    if body
        .vars
        .iter()
        .chain(body.host_vars.iter())
        .any(|(_, v)| v.decode().is_none())
    {
        return Err(LoadError {
            kind: LoadErrorKind::Corrupt,
        });
    }
    Ok(body)
}

/// A backup of the VM's state so that `load_persistent` can roll back on failure.
struct Backup {
    globals: Vec<crate::value::Value>,
    host_vars: Vec<crate::value::Value>,
    persist: Vec<crate::package::SliceBox>,
    seen: std::collections::BTreeSet<String>,
    rng: super::rng::Rng,
    pc: usize,
    stack: Vec<crate::value::Value>,
    current: Option<super::Yield>,
    cur_src: Option<super::StopSrc>,
    cap: Vec<Vec<String>>,
    choice: super::ChoiceBuild,
}

impl<'a> Vm<'a> {
    /// Makes the bytes of persistent data (values of `persistent var`s, host-declared `Kind::Persistent` variables, packages'
    /// persistent slices, passed stop points).
    ///
    /// Can be called even when not stopped. Managing where it is stored (atomic writes and so on) is the host's job.
    /// Persistent slices that cannot be serialized (a bug in the implementation of `Persistent`) are skipped.
    pub fn save_persistent(&self) -> Vec<u8> {
        let program = &self.program;
        let var_types = program
            .globals
            .iter()
            .filter(|g| g.persistent)
            .map(|g| {
                (
                    format!("{}.{}", program.modules[g.module as usize], g.name),
                    g.ty.name(),
                )
            })
            .chain(
                self.registry
                    .vars()
                    .iter()
                    .filter(|d| d.kind == Kind::Persistent)
                    .map(|d| (d.name.clone(), d.ty.name())),
            )
            .collect();
        let body = PersistentBody {
            schema_version: program.schema_version(),
            types: type_signatures(program),
            var_types,
            vars: program
                .globals
                .iter()
                .zip(&self.globals)
                .filter(|(g, _)| g.persistent)
                .map(|(g, v)| {
                    (
                        format!("{}.{}", program.modules[g.module as usize], g.name),
                        SavedValue::from_value(v),
                    )
                })
                .collect(),
            host_vars: self
                .registry
                .vars()
                .iter()
                .zip(&self.host_vars)
                .filter(|(d, _)| d.kind == Kind::Persistent)
                .map(|(d, v)| (d.name.clone(), SavedValue::from_value(v)))
                .collect(),
            slices: self.save_slices(true).unwrap_or_default(),
            seen: self.seen.iter().cloned().collect(),
        };
        let body = postcard::to_stdvec(&body).expect("メモリへの直列化は失敗しない");
        wrap(MAGIC, FORMAT_VERSION, &body)
    }

    /// Reads persistent data. The values of `persistent var`s, host-declared variables, packages' persistent slices and passed
    /// stop points are **replaced** (`persistent var`s that were not in the persistent data get their declared initial values;
    /// missing persistent slices get default values). The save (`save var`s, position, history, random generator, state
    /// slices) does not change.
    ///
    /// If stopped, the arguments of the stop point are rebuilt with the new `persistent var`s and `seen` (for scripts whose
    /// `${expr}` or `if` read them). So loading a save and then persistent data gives the same state as putting the
    /// persistent data first (`VmConfig::persistent_data`) and then loading the save. If they cannot be rebuilt (a runtime
    /// error), returns a `LoadError` (`StopArgsFailed`) and the VM is unchanged.
    ///
    /// For broken bytes or an unknown version, `LoadError`, and the VM is unchanged.
    pub fn load_persistent(&mut self, bytes: &[u8]) -> Result<PersistentReport, LoadError> {
        let body = parse(bytes)?;
        let State::Stopped { ordinal } = self.state else {
            return self.apply_persistent(body);
        };
        let backup = Backup {
            globals: self.globals.clone(),
            host_vars: self.host_vars.clone(),
            persist: self.persist.clone(),
            seen: self.seen.clone(),
            rng: self.rng.clone(),
            pc: self.pc,
            stack: self.stack.clone(),
            current: self.current.clone(),
            cur_src: self.cur_src.clone(),
            cap: self.cap.clone(),
            choice: self.choice.clone(),
        };
        let report = self.apply_persistent(body)?;
        match self.rebuild_yield(ordinal)? {
            Some(y) => {
                self.install_current(y, ordinal);
                self.sync_current_record();
                Ok(report)
            }
            None => {
                self.globals = backup.globals;
                self.host_vars = backup.host_vars;
                self.persist = backup.persist;
                self.seen = backup.seen;
                self.rng = backup.rng;
                self.pc = backup.pc;
                self.stack = backup.stack;
                self.current = backup.current;
                self.cur_src = backup.cur_src;
                self.cap = backup.cap;
                self.choice = backup.choice;
                Err(LoadError {
                    kind: LoadErrorKind::StopArgsFailed,
                })
            }
        }
    }

    /// Puts the body of persistent data into the VM. `Err` if the persistent slices cannot be restored (the VM is unchanged).
    pub(super) fn apply_persistent(
        &mut self,
        mut body: PersistentBody,
    ) -> Result<PersistentReport, LoadError> {
        let program_ref = self.program.clone();
        let program: &Program = &program_ref;
        let registry = self.registry;
        self.total_valid = false;
        // Do the parts that can fail (version and schema checks, restoring slices) first
        super::save::check_schema_version(program, body.schema_version)?;
        let migration = super::save::apply_migrations(
            program,
            registry,
            &self.cfg,
            body.schema_version,
            true,
            &body.types,
            &mut body.vars,
            &mut body.var_types,
        )?;
        let var_decls: &[(String, String)] =
            if super::save::migrated_types(program, body.schema_version) {
                &[]
            } else {
                &body.types
            };
        let diffs =
            super::save::script_var_diffs(program, var_decls, &body.var_types, &body.vars, true)?;
        if self.cfg.load_policy == LoadPolicy::Strict && !diffs.is_empty() {
            return Err(LoadError {
                kind: LoadErrorKind::SchemaMismatch(diffs),
            });
        }
        let mismatched: std::collections::BTreeSet<String> =
            diffs.into_iter().map(|d| d.name).collect();
        let resolved = self.resolve_slices(&body.slices, true)?;
        let compat = Compat::new(program, &body.types);
        let mut report = PersistentReport {
            from_schema_version: body.schema_version,
            to_schema_version: program.schema_version(),
            changed_types: compat.changed.iter().cloned().collect(),
            migrated_vars: migration.migrated.clone(),
            migrate_failed_vars: migration.failed.clone(),
            ignored_slices: resolved.report.ignored,
            defaulted_slices: resolved.report.defaulted,
            migrated_slices: resolved.report.migrated,
            ..PersistentReport::default()
        };
        self.install_slices(resolved.slices, true);
        for (i, g) in program
            .globals
            .iter()
            .enumerate()
            .filter(|(_, g)| g.persistent)
        {
            let name = format!("{}.{}", program.modules[g.module as usize], g.name);
            match body.vars.iter().find(|(n, _)| *n == name) {
                Some((_, v)) if !mismatched.contains(&name) => {
                    self.globals[i] = v.clone().into_value();
                }
                Some(_) => {
                    self.globals[i] = g.init.clone();
                    report.type_changed_vars.push(name);
                }
                None => {
                    self.globals[i] = g.init.clone();
                    if !migration.failed.contains(&name) {
                        report.added_vars.push(name);
                    }
                }
            }
        }
        // Host-declared variables (`Kind::Persistent`)
        for (i, d) in registry
            .vars()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == Kind::Persistent)
        {
            match body.host_vars.iter().find(|(n, _)| *n == d.name) {
                Some((_, v)) if compat.ok(&v.clone().into_value(), &d.ty) => {
                    self.host_vars[i] = v.clone().into_value();
                }
                Some(_) => {
                    self.host_vars[i] = d.init.clone();
                    report.type_changed_vars.push(d.name.clone());
                }
                None => {
                    self.host_vars[i] = d.init.clone();
                    report.added_vars.push(d.name.clone());
                }
            }
        }
        report.removed_vars = body
            .vars
            .iter()
            .filter(|(n, _)| {
                !program.globals.iter().any(|g| {
                    g.persistent
                        && format!("{}.{}", program.modules[g.module as usize], g.name) == *n
                })
            })
            .map(|(n, _)| n.clone())
            .collect();
        report.removed_vars.extend(
            body.host_vars
                .iter()
                .filter(|(n, _)| {
                    !registry
                        .vars()
                        .iter()
                        .any(|d| d.kind == Kind::Persistent && d.name == *n)
                })
                .map(|(n, _)| n.clone()),
        );
        self.seen = body.seen.into_iter().collect();
        Ok(report)
    }
}
