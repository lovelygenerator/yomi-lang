//! Saving and loading (position, variables, call stack, labels entered, `once` already chosen, random generator, history,
//! packages' state slices, host-declared variables; `persistent var`s, passed stop points and persistent slices are
//! persistent data in a separate byte string).

mod container;
mod migrate;
mod resolve;
mod schema;
mod value;

#[cfg(test)]
pub(crate) use container::crc32;
pub(super) use container::{unwrap, wrap};
pub(super) use migrate::{apply as apply_migrations, migrated_types};
pub(super) use resolve::find_label;
use resolve::*;
pub use schema::SchemaDiff;
pub(super) use schema::{
    check_version as check_schema_version, consistent as schema_consistent, script_var_diffs,
};
#[cfg(test)]
pub(super) use value::SavedNode;
pub(super) use value::SavedValue;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::persistent::PersistentReport;
use super::rng::Rng;
use super::{
    Frame, Inner, LoadPolicy, Rebuilt, State, StopRecord, Vm, VmConfig, VmError, VmErrorKind,
    Yield, default_locals,
};
use crate::program::{LabelCode, ProgRef, Program, StopInfo, StopKind};
use crate::registry::{Kind, Registry};
use crate::value::{Identity, MAX_VALUE_DEPTH, Value, finite_float};

/// The marker of save bytes (the first 4 bytes).
const MAGIC: &[u8; 4] = b"YOMS";
/// The format version. Before release, it is increased without keeping compatibility. Version 2 added the call stack,
/// version 3 labels entered and `once` already chosen, version 4 the random generator and history, version 5 host-declared
/// variables and packages' state slices, version 6 return values of stop points in the history, version 7 changed the
/// value format so it can hold `list`, `Option`, `struct` and `enum` and added `struct` and `enum` declarations (type names
/// and shapes), version 8 added `map` values (nodes in insertion order), version 9 added the label and kind of stop points
/// to the history, version 10 added packages' rule versions (`rules`), and version 11 changed the random generator to
/// xoshiro256++ (the state is 4 `u64`s), and version 12 added the schema (`schema_version`, `var_types`) and changed the
/// position hash to the canonical form (versions 1 to 11 cannot be read). `persistent var`s, passed stop points and
/// persistent slices are not included (they are persistent data; `YOMP`).
pub(super) const FORMAT_VERSION: u32 = 12;

/// One history entry.
#[derive(Serialize, Deserialize)]
struct SavedRecord {
    /// The label of the stop point (`module.label`; added in version 9).
    label: String,
    /// The kind of stop point (`StopKind`; `0` is an `@` command, `1` a plain line, `2` a quote line, `3` a choice; added in
    /// version 9).
    kind: u8,
    key: Option<String>,
    command: String,
    args: Vec<SavedValue>,
    values: Vec<Vec<String>>,
    /// The return value passed on resume (added in version 6).
    ret: Option<SavedValue>,
}

impl SavedRecord {
    /// Makes a record for saving from a record of the stop point history.
    fn from_record(r: &StopRecord) -> Self {
        Self {
            label: r.label.clone(),
            kind: match r.kind {
                StopKind::Plain => 1,
                StopKind::Quote => 2,
                StopKind::Choice => 3,
                _ => 0,
            },
            key: r.key.clone(),
            command: r.command.clone(),
            args: r.args.iter().map(SavedValue::from_value).collect(),
            values: r.values.clone(),
            ret: r.ret.as_ref().map(SavedValue::from_value),
        }
    }

    /// Turns a record for saving back into a history record. `None` if the kind value is invalid.
    fn into_record(self) -> Option<StopRecord> {
        Some(StopRecord {
            label: self.label,
            kind: match self.kind {
                0 => StopKind::Command,
                1 => StopKind::Plain,
                2 => StopKind::Quote,
                3 => StopKind::Choice,
                _ => return None,
            },
            key: self.key,
            command: self.command,
            args: self
                .args
                .iter()
                .map(SavedValue::decode)
                .collect::<Option<Vec<_>>>()?,
            values: self.values,
            ret: match self.ret {
                Some(r) => Some(r.decode()?),
                None => None,
            },
        })
    }
}

/// The list of shapes of type declarations (`name`, `signature`). Put into saves and persistent data.
pub(super) fn type_signatures(program: &Program) -> Vec<(String, String)> {
    program
        .types
        .iter()
        .map(|t| (t.name.clone(), t.signature()))
        .collect()
}

/// A tool that checks whether saved values can be accepted for the types of the current scripts. Types whose saved
/// declaration differs from the current one (and types containing them) are not accepted, even if the values fit
/// structurally.
pub(super) struct Compat<'p> {
    types: &'p [crate::value::TypeDecl],
    /// The names of types whose declarations changed (including types that contain changed types).
    pub changed: std::collections::BTreeSet<String>,
}

impl<'p> Compat<'p> {
    /// `saved` are the saved type declarations ([`type_signatures`]).
    pub(super) fn new(program: &'p Program, saved: &[(String, String)]) -> Self {
        let mut changed: std::collections::BTreeSet<String> = program
            .types
            .iter()
            .filter(|t| {
                saved
                    .iter()
                    .any(|(n, sig)| *n == t.name && *sig != t.signature())
            })
            .map(|t| t.name.clone())
            .collect();
        // Types that contain changed types are also treated as changed
        loop {
            let before = changed.len();
            for t in &program.types {
                if !changed.contains(&t.name) && t.used_types().iter().any(|u| changed.contains(*u))
                {
                    changed.insert(t.name.clone());
                }
            }
            if changed.len() == before {
                break;
            }
        }
        Self {
            types: &program.types,
            changed,
        }
    }

    /// Whether the value can be accepted as type `ty`.
    pub(super) fn ok(&self, v: &Value, ty: &crate::value::Ty) -> bool {
        let mut used = Vec::new();
        ty.collect_user(&mut used);
        !used.iter().any(|n| self.changed.contains(*n)) && v.matches_in(ty, self.types)
    }
}

/// The position of a saved stop point (module, label, stable ID, position hash, ordinal within the label).
#[derive(Serialize, Deserialize)]
struct Position {
    module: String,
    label: String,
    id: Option<String>,
    hash: u64,
    ordinal: u32,
}

/// One frame of the call stack. The position is the call line (the line with the `call`).
#[derive(Serialize, Deserialize)]
struct FrameBody {
    /// The position of the call line. Call lines have no stable ID, so `id` is always `None` (searched by hash and ordinal).
    position: Position,
    locals: Vec<(u32, String, SavedValue)>,
}

/// One state slice of a package (the core does not interpret its contents).
#[derive(Serialize, Deserialize, Clone)]
pub(super) struct SavedSlice {
    /// The name of the package.
    pub name: String,
    /// The version when saved (`Package::VERSION`, `PERSISTENT_VERSION`).
    pub version: u32,
    /// The bytes serialized with `postcard`.
    pub bytes: Vec<u8>,
}

/// The body of a save (position, type declarations, module variables, local variables, call stack, random generator,
/// history, packages' state, and so on).
#[derive(Serialize, Deserialize)]
struct SaveBody {
    position: Position,
    /// The schema version of the scripts when saved.
    schema_version: u32,
    /// The `struct` and `enum` declarations when saved (type names and the shapes of declarations). Compared with the
    /// scripts' declarations on load.
    types: Vec<(String, String)>,
    /// The types of `vars` then `host_vars` (same names, same order), as type strings.
    var_types: Vec<(String, String)>,
    /// Module variables. Names are `module.variable`.
    vars: Vec<(String, SavedValue)>,
    /// Host-declared variables (`Kind::Save`). Names are `package.variable`.
    host_vars: Vec<(String, SavedValue)>,
    /// Packages' state slices (in registration order; those whose `State` is `()` are not included).
    slices: Vec<SavedSlice>,
    locals: Vec<(u32, String, SavedValue)>,
    /// The call stack (the bottom is the first caller).
    frames: Vec<FrameBody>,
    /// Labels entered (`module`, `label`).
    visited: Vec<(String, String)>,
    /// `once` options already chosen (`module`, `label`, stable ID).
    once_done: Vec<(String, String, String)>,
    /// The state of the random generator, as of when computing the arguments of the current stop point began (rebuilding the
    /// arguments on load brings it back to the current state).
    rng: [u64; 4],
    /// The history of stop points (oldest first; up to the newest `history_limit` entries).
    history: Vec<SavedRecord>,
    /// The rule versions of the registered packages (`Package::RULES_VERSION`; in registration order; all of them, with or
    /// without state).
    rules: Vec<(String, u32)>,
}

/// How the position was found.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBy {
    /// Label + stable ID.
    Id,
    /// Label + hash.
    Hash,
    /// Label + ordinal (the nearest stop point).
    Ordinal,
    /// The start of the label (the stop point could not be determined). In this case the VM is not stopped and starts with
    /// `run`.
    LabelStart,
}

/// The stop point a save is stopped at, read without restoring the VM ([`pending_stop`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PendingStop {
    /// The label the stop point is in (`module.label`).
    pub label: String,
    /// `module.label#id` (`None` for stop points without a stable ID and for choices).
    pub key: Option<String>,
    /// The name of the command it is stopped at (the name in the registry when the save was made).
    pub command: String,
    /// The arguments of the stop notification, as saved (the evaluated original).
    pub args: Vec<Value>,
    /// The version of the schema of the save (`spec/host/04-save.md`, 4.3).
    pub schema_version: u32,
}

/// Reads the command and arguments a save is stopped at, without the scripts, the registry or a VM (`spec/host/04-save.md`,
/// 4.9). `Ok(None)` if the save has no history (`VmConfig::history_limit` was 0).
///
/// It checks the magic, the format version, the checksum and the body like a load does. It does not check the schema, apply
/// migrations or resolve the position.
pub fn pending_stop(save: &[u8]) -> Result<Option<PendingStop>, LoadError> {
    let (body, _, history) = decode_save(save)?;
    Ok(history.last().map(|r| PendingStop {
        label: r.label.clone(),
        key: r.key.clone(),
        command: r.command.clone(),
        args: r.args.clone(),
        schema_version: body.schema_version,
    }))
}

/// The report of a load.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    /// How the position was found. When there is a call stack, the least reliable method among the stop point and the call
    /// lines (`Hash` is less reliable than `Id`, `Ordinal` than `Hash`, `LabelStart` than `Ordinal`).
    pub resolved_by: ResolvedBy,
    /// Variables filled with default values (`module.variable`; only `save var`s; `persistent var`s are in the persistent data
    /// report).
    pub added_vars: Vec<String>,
    /// Variables that were removed from the scripts and discarded (`module.variable`).
    pub removed_vars: Vec<String>,
    /// Variables whose type changed (`module.variable`; if any, it goes back to the start of the label).
    pub type_changed_vars: Vec<String>,
    /// `struct`s and `enum`s whose declarations changed (`module.TypeName`; including types that contain changed types).
    /// Variables holding values of those types go into `type_changed_vars`.
    pub changed_types: Vec<String>,
    /// Local variables whose type changed (`module.label.variable`; the current label and the label of each call stack frame;
    /// if any, it goes back to the start of the label). When only names or counts changed, they do not appear here
    /// (`resolved_by` becomes `LabelStart`).
    pub type_changed_locals: Vec<String>,
    /// Slices of packages that are not registered, which were ignored (package names).
    pub ignored_slices: Vec<String>,
    /// Packages' state slices that were not in the save and got default values (`State::default()`) (package names).
    pub defaulted_slices: Vec<String>,
    /// Packages' state slices whose versions differed and were migrated with `migrate` (package names).
    pub migrated_slices: Vec<String>,
    /// Packages whose rule version (`Package::RULES_VERSION`) differs from when saved (package names, in registration order).
    /// Execution continues with the current rules, so the same command with the same arguments may give a different result
    /// than before the save. Packages that were not in the save are not included.
    pub changed_rules: Vec<String>,
    /// The schema version of the save.
    pub from_schema_version: u32,
    /// The schema version of the scripts.
    pub to_schema_version: u32,
    /// Variables converted by `migrate` (`module.variable`; renamed variables have the new name).
    pub migrated_vars: Vec<String>,
    /// Variables whose `migrate` conversion failed and were reset to their initial values under `LoadPolicy::Lenient`
    /// (if any, it goes back to the start of the label).
    pub migrate_failed_vars: Vec<String>,
    /// The report from reading the persistent data in [`VmConfig::persistent`] (`None` if not given).
    pub persistent: Option<PersistentReport>,
}

/// The kind of load failure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadErrorKind {
    /// The magic does not match.
    BadMagic,
    /// An unknown format version.
    UnsupportedVersion(u32),
    /// The checksum does not match.
    ChecksumMismatch,
    /// The body is broken.
    Corrupt,
    /// The saved label is not in the scripts.
    LabelNotFound(String),
    /// The names of the saved modules differ from the scripts' modules.
    ModuleMismatch(String),
    /// A command used by the program is not in the registry.
    CommandMissing(String),
    /// The arguments of the stop point could not be rebuilt.
    StopArgsFailed,
    /// A package's slice could not be restored (broken, or `migrate` returned `Err`). The value is the package name.
    PackageSlice(String),
    /// Cannot load with a registry made from a declaration file (it has no functions to run commands).
    DeclOnlyRegistry,
    /// The save is from a newer schema version than the scripts.
    SaveFromNewerVersion {
        /// The schema version of the save.
        saved: u32,
        /// The schema version of the scripts.
        current: u32,
    },
    /// The save is older than the oldest schema version the scripts support (`min_save_version`).
    SaveVersionUnsupported {
        /// The schema version of the save.
        saved: u32,
        /// The oldest supported version.
        min: u32,
    },
    /// Under `LoadPolicy::Strict`, variables whose saved type differs from the scripts (all of them).
    SchemaMismatch(Vec<SchemaDiff>),
    /// Under `LoadPolicy::Strict`, variables whose `migrate` conversion failed (all of them).
    MigrateFailed(Vec<String>),
}

impl LoadErrorKind {
    /// The stable English name.
    pub fn code(&self) -> &'static str {
        match self {
            LoadErrorKind::BadMagic => "bad_magic",
            LoadErrorKind::UnsupportedVersion(_) => "unsupported_version",
            LoadErrorKind::ChecksumMismatch => "checksum_mismatch",
            LoadErrorKind::Corrupt => "corrupt",
            LoadErrorKind::LabelNotFound(_) => "label_not_found",
            LoadErrorKind::ModuleMismatch(_) => "module_mismatch",
            LoadErrorKind::CommandMissing(_) => "command_missing",
            LoadErrorKind::StopArgsFailed => "stop_args_failed",
            LoadErrorKind::PackageSlice(_) => "package_slice",
            LoadErrorKind::DeclOnlyRegistry => "decl_only_registry",
            LoadErrorKind::SaveFromNewerVersion { .. } => "save_from_newer_version",
            LoadErrorKind::SaveVersionUnsupported { .. } => "save_version_unsupported",
            LoadErrorKind::SchemaMismatch(_) => "schema_mismatch",
            LoadErrorKind::MigrateFailed(_) => "migrate_failed",
        }
    }
}

/// A load failure. Has no message text.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// Kind.
    pub kind: LoadErrorKind,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind.code())
    }
}

impl std::error::Error for LoadError {}

/// Makes a load failure of the given kind.
fn fail<T>(kind: LoadErrorKind) -> Result<T, LoadError> {
    Err(LoadError { kind })
}

/// Why [`Vm::reload`] failed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadError {
    /// The VM is not at a stop point.
    NotStopped,
    /// The state could not be handed over (the reasons of [`LoadError`]).
    Load(LoadError),
    /// The stop point could only be resolved as the start of a label.
    PositionLost(ResolvedBy),
}

impl ReloadError {
    /// The stable name of the failure.
    pub fn code(&self) -> &'static str {
        match self {
            ReloadError::NotStopped => "not_stopped",
            ReloadError::Load(e) => e.kind.code(),
            ReloadError::PositionLost(_) => "position_lost",
        }
    }
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl std::error::Error for ReloadError {}

/// The bytes that identify a state ([`Vm::state_key`]).
pub(crate) struct StateKey {
    /// The save without the history.
    pub save: Vec<u8>,
    /// The persistent data.
    pub persistent: Vec<u8>,
}

impl<'a> Vm<'a> {
    /// Makes the bytes of a save at a stop point.
    ///
    /// Includes the position, `save var`s, local variables, the call stack, labels entered, `once` already chosen, the random
    /// generator and the history. `persistent var`s and passed stop points are not included ([`Vm::save_persistent`]).
    pub fn save(&self) -> Result<Vec<u8>, VmError> {
        let body = self.save_body(true)?;
        let body = postcard::to_stdvec(&body).expect("メモリへの直列化は失敗しない");
        Ok(wrap(MAGIC, FORMAT_VERSION, &body))
    }

    /// Hands the state at the current stop point over to a new program (`spec/host/04-save.md`, 4.10).
    ///
    /// Works like saving and loading, but the persistent data is carried over from this VM, and a position that can only be
    /// resolved as the start of a label is an error. This VM is not changed.
    pub fn reload(&self, program: &'a Program) -> Result<(Vm<'a>, LoadReport), ReloadError> {
        let bytes = self.save().map_err(|_| ReloadError::NotStopped)?;
        let mut cfg = self.cfg.clone();
        cfg.persistent = Some(self.save_persistent());
        let (vm, report) =
            Vm::load(program, self.registry, cfg, &bytes).map_err(ReloadError::Load)?;
        if report.resolved_by == ResolvedBy::LabelStart || !vm.is_stopped() {
            return Err(ReloadError::PositionLost(report.resolved_by));
        }
        Ok((vm, report))
    }

    /// What identifies the state at a stop point: a save without the history, and the persistent data (`persistent var`s,
    /// host persistent variables, persistent slices, passed stop points). Two stop points with the same bytes behave the
    /// same from here on. Used by the exploration (`spec/host/08-test.md`).
    pub(crate) fn state_key(&self) -> Result<StateKey, VmError> {
        let body = self.save_body(false)?;
        Ok(StateKey {
            save: postcard::to_stdvec(&body).expect("メモリへの直列化は失敗しない"),
            persistent: self.save_persistent(),
        })
    }

    fn save_body(&self, with_history: bool) -> Result<SaveBody, VmError> {
        let State::Stopped { ordinal } = self.state else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let program = &self.program;
        let code = &program.labels[self.label];
        let stop = &code.stops[ordinal as usize];
        let frames = self
            .frames
            .iter()
            .map(|f| {
                let lc = &program.labels[f.label];
                // The instruction just before the return point is the `Call` instruction
                let site = lc
                    .calls
                    .iter()
                    .position(|c| c.pc as usize + 1 == f.ret_pc)
                    .unwrap_or(0);
                FrameBody {
                    position: Position {
                        module: program.modules[lc.module as usize].clone(),
                        label: lc.name.clone(),
                        id: None,
                        hash: lc.calls.get(site).map_or(0, |c| c.hash),
                        ordinal: site as u32,
                    },
                    locals: save_locals(lc, &f.locals),
                }
            })
            .collect();
        let vars: Vec<(String, SavedValue)> = program
            .globals
            .iter()
            .zip(&self.globals)
            .filter(|(g, _)| !g.persistent)
            .map(|(g, v)| {
                (
                    global_name(program, g.module, &g.name),
                    SavedValue::from_value(v),
                )
            })
            .collect();
        let host_vars: Vec<(String, SavedValue)> = self
            .registry
            .vars()
            .iter()
            .zip(&self.host_vars)
            .filter(|(d, _)| d.kind == Kind::Save)
            .map(|(d, v)| (d.name.clone(), SavedValue::from_value(v)))
            .collect();
        let var_types = program
            .globals
            .iter()
            .filter(|g| !g.persistent)
            .map(|g| (global_name(program, g.module, &g.name), g.ty.name()))
            .chain(
                self.registry
                    .vars()
                    .iter()
                    .filter(|d| d.kind == Kind::Save)
                    .map(|d| (d.name.clone(), d.ty.name())),
            )
            .collect();
        let body = SaveBody {
            schema_version: program.schema_version(),
            types: type_signatures(program),
            var_types,
            position: Position {
                module: program.modules[code.module as usize].clone(),
                label: code.name.clone(),
                id: stop.id.clone(),
                hash: stop.hash,
                ordinal,
            },
            vars,
            host_vars,
            slices: match self.save_slices(false) {
                Ok(s) => s,
                Err(name) => return Err(self.misuse(VmErrorKind::SliceSerialize(name))),
            },
            locals: save_locals(code, &self.locals),
            frames,
            visited: self
                .visited
                .iter()
                .map(|&l| {
                    let lc = &program.labels[l as usize];
                    (program.modules[lc.module as usize].clone(), lc.name.clone())
                })
                .collect(),
            once_done: self
                .once_done
                .iter()
                .map(|(l, id)| {
                    let lc = &program.labels[*l as usize];
                    (
                        program.modules[lc.module as usize].clone(),
                        lc.name.clone(),
                        id.clone(),
                    )
                })
                .collect(),
            rng: self.args_rng.state(),
            history: if with_history {
                self.history()
                    .iter()
                    .map(|r| SavedRecord::from_record(r))
                    .collect()
            } else {
                Vec::new()
            },
            rules: self
                .registry
                .package_entries()
                .iter()
                .map(|e| (e.name.clone(), e.rules_version))
                .collect(),
        };
        Ok(body)
    }

    /// Makes a VM restored to a stop point from the bytes of a save.
    ///
    /// If `cfg` has [`VmConfig::persistent`], the persistent data is put in first (so that when the arguments of the stop
    /// point are rebuilt, scripts that read `persistent var`s and `seen` get the same arguments as when saved).
    pub fn load(
        program: &'a Program,
        registry: &'a Registry,
        cfg: VmConfig,
        bytes: &[u8],
    ) -> Result<(Self, LoadReport), LoadError> {
        let (mut body, rng, mut history) = decode_save(bytes)?;
        schema::check_version(program, body.schema_version)?;
        let migration = apply_migrations(
            program,
            registry,
            &cfg,
            body.schema_version,
            false,
            &body.types,
            &mut body.vars,
            &mut body.var_types,
        )?;
        // After a migration, the types of the variables are already expanded
        let var_decls: &[(String, String)] = if migrated_types(program, body.schema_version) {
            &[]
        } else {
            &body.types
        };
        let diffs =
            schema::script_var_diffs(program, var_decls, &body.var_types, &body.vars, false)?;
        if cfg.load_policy == LoadPolicy::Strict && !diffs.is_empty() {
            return fail(LoadErrorKind::SchemaMismatch(diffs));
        }
        let mismatched: std::collections::BTreeSet<String> =
            diffs.into_iter().map(|d| d.name).collect();
        let compat = Compat::new(program, &body.types);
        let persistent_body = match &cfg.persistent {
            Some(b) => Some(super::persistent::parse(b)?),
            None => None,
        };
        let mut vm = Vm::blank_for_load(program, registry, cfg)?;
        let persistent_report = match persistent_body {
            Some(b) => Some(vm.apply_persistent(b)?),
            None => None,
        };
        // Packages' state slices (restored before rebuilding the arguments of the stop point, because query commands read them)
        let slice_report = vm.apply_state_slices(&body.slices)?;
        // Packages whose rule versions differ from when saved (only a warning; the load continues)
        let changed_rules: Vec<String> = vm
            .registry
            .package_entries()
            .iter()
            .filter(|e| {
                body.rules
                    .iter()
                    .any(|(n, v)| *n == e.name && *v != e.rules_version)
            })
            .map(|e| e.name.clone())
            .collect();
        vm.clear_cues();
        vm.rng = rng.clone();
        vm.args_rng = rng.clone();
        let keep = history.len().saturating_sub(vm.cfg.history_limit);
        history.drain(..keep);
        vm.history = history;
        let pos = &body.position;
        if !program.modules.contains(&pos.module) {
            return fail(LoadErrorKind::ModuleMismatch(pos.module.clone()));
        }
        let Some(label) = find_label(program, &pos.module, &pos.label) else {
            return fail(LoadErrorKind::LabelNotFound(pos.label.clone()));
        };
        let code = &program.labels[label];

        // Labels entered and `once` already chosen (those removed from the scripts are discarded)
        for (m, l) in &body.visited {
            if let Some(i) = find_label(program, m, l) {
                vm.visited.insert(i as u32);
            }
        }
        for (m, l, id) in &body.once_done {
            if let Some(i) = find_label(program, m, l) {
                vm.once_done.insert((i as u32, id.clone()));
            }
        }
        vm.visited.insert(label as u32);

        let VarChanges {
            added,
            removed,
            type_changed,
        } = vm.restore_save_vars(program, &body, &compat, &mismatched);
        // A variable whose `convert` failed becomes its initial value (reported as `migrate_failed_vars`, not as added)
        let added: Vec<String> = added
            .into_iter()
            .filter(|n| !migration.failed.contains(n))
            .collect();
        // Local variables whose type changed (the current label and each call stack frame)
        let mut type_changed_locals = local_type_changes(program, code, &body.locals, &compat);
        for fb in &body.frames {
            if let Some(fl) = find_label(program, &fb.position.module, &fb.position.label) {
                type_changed_locals.extend(local_type_changes(
                    program,
                    &program.labels[fl],
                    &fb.locals,
                    &compat,
                ));
            }
        }
        // Local variables. If even one name, type or count differs from when saved, the position of the stop point cannot be
        // trusted, so go back to the start of the label.
        let locals = restore_locals(program, code, &body.locals, &compat);
        // The call stack. If any frame cannot be resolved, also go back to the start of the label (the call stack is discarded).
        let stack = restore_frames(program, &body.frames, label, &compat);

        // Resolve the position
        let (ordinal, mut resolved_by) = match (&locals, &stack) {
            (Some(_), Some(_)) if type_changed.is_empty() && migration.failed.is_empty() => {
                resolve(code, pos)
            }
            _ => (None, ResolvedBy::LabelStart),
        };
        vm.label = label;
        let report = |by: ResolvedBy| LoadReport {
            resolved_by: by,
            added_vars: added.clone(),
            removed_vars: removed.clone(),
            type_changed_vars: type_changed.clone(),
            changed_types: compat.changed.iter().cloned().collect(),
            type_changed_locals: type_changed_locals.clone(),
            ignored_slices: slice_report.ignored.clone(),
            defaulted_slices: slice_report.defaulted.clone(),
            migrated_slices: slice_report.migrated.clone(),
            changed_rules: changed_rules.clone(),
            from_schema_version: body.schema_version,
            to_schema_version: program.schema_version(),
            migrated_vars: migration.migrated.clone(),
            migrate_failed_vars: migration.failed.clone(),
            persistent: persistent_report.clone(),
        };
        let (Some(ordinal), Some(locals), Some((frames, frames_by))) = (ordinal, locals, stack)
        else {
            // Go back to the start of the label (not stopped)
            vm.restart_label(program, code);
            return Ok((vm, report(ResolvedBy::LabelStart)));
        };
        vm.locals = locals;
        vm.frames = frames;
        for f in &vm.frames {
            vm.visited.insert(f.label as u32);
        }
        // Make the stop notification from the stopping instruction (if the arguments cannot be re-evaluated, go back to the start
        // of the label)
        let Some(y) = vm.rebuild_yield(ordinal)? else {
            // The arguments cannot be re-evaluated (the scripts changed). Go back to the start of the label.
            vm.rng = rng.clone();
            vm.args_rng = rng;
            vm.stack.clear();
            vm.frames.clear();
            vm.restart_label(program, code);
            return Ok((vm, report(ResolvedBy::LabelStart)));
        };
        vm.state = State::Stopped { ordinal };
        vm.install_current(y, ordinal);
        vm.sync_current_record();
        resolved_by = weakest(resolved_by, frames_by);
        Ok((vm, report(resolved_by)))
    }

    /// An empty VM for loading.
    fn blank_for_load(
        program: &'a Program,
        registry: &'a Registry,
        cfg: VmConfig,
    ) -> Result<Self, LoadError> {
        match Vm::blank(ProgRef::Borrowed(program), registry, cfg) {
            Ok(vm) => Ok(vm),
            Err(VmError {
                kind: VmErrorKind::CommandMissing(n),
                ..
            }) => fail(LoadErrorKind::CommandMissing(n)),
            Err(VmError {
                kind: VmErrorKind::DeclOnlyRegistry,
                ..
            }) => fail(LoadErrorKind::DeclOnlyRegistry),
            Err(_) => fail(LoadErrorKind::Corrupt),
        }
    }

    /// Compatibility of variables (`save var`s and host-declared `Kind::Save`; `persistent var`s are persistent data). Only
    /// values of matching types are put in.
    fn restore_save_vars(
        &mut self,
        program: &Program,
        body: &SaveBody,
        compat: &Compat<'_>,
        mismatched: &std::collections::BTreeSet<String>,
    ) -> VarChanges {
        let mut added = Vec::new();
        let mut type_changed = Vec::new();
        for (i, g) in program
            .globals
            .iter()
            .enumerate()
            .filter(|(_, g)| !g.persistent)
        {
            let name = global_name(program, g.module, &g.name);
            match body.vars.iter().find(|(n, _)| *n == name) {
                // The type was checked against the saved type in `schema::script_var_diffs`
                Some((_, v)) if !mismatched.contains(&name) => {
                    self.globals[i] = v.clone().into_value();
                }
                Some(_) => type_changed.push(name),
                None => added.push(name),
            }
        }
        // Host-declared variables (`Kind::Save`). Same rules as script variables
        for (i, d) in self
            .registry
            .vars()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == Kind::Save)
        {
            match body.host_vars.iter().find(|(n, _)| *n == d.name) {
                Some((_, v)) if compat.ok(&v.clone().into_value(), &d.ty) => {
                    self.host_vars[i] = v.clone().into_value();
                }
                Some(_) => type_changed.push(d.name.clone()),
                None => added.push(d.name.clone()),
            }
        }
        let mut removed: Vec<String> = body
            .vars
            .iter()
            .filter(|(n, _)| {
                !program
                    .globals
                    .iter()
                    .any(|g| !g.persistent && global_name(program, g.module, &g.name) == *n)
            })
            .map(|(n, _)| n.clone())
            .collect();
        removed.extend(
            body.host_vars
                .iter()
                .filter(|(n, _)| {
                    !self
                        .registry
                        .vars()
                        .iter()
                        .any(|d| d.kind == Kind::Save && d.name == *n)
                })
                .map(|(n, _)| n.clone()),
        );
        VarChanges {
            added,
            removed,
            type_changed,
        }
    }

    /// Goes back to the start of the label (not stopped).
    fn restart_label(&mut self, program: &Program, code: &LabelCode) {
        self.pc = 0;
        self.locals = default_locals(program, code);
        self.state = State::Ready;
    }

    /// Rebuilds the arguments of the `ordinal`-th stop point of the current label by running the instructions.
    ///
    /// The random generator starts from the state when computing the arguments began (`args_rng`). If rebuilt, empties the
    /// stack, advances past the stopping instruction, and returns the stop notification. If they could not be re-evaluated
    /// (a runtime error, or no options to choose), `None`. `state` is left as it was before the call.
    pub(super) fn rebuild_yield(&mut self, ordinal: u32) -> Result<Option<Yield>, LoadError> {
        let program_ref = self.program.clone();
        let program: &Program = &program_ref;
        let code = &program.labels[self.label];
        let stop = &code.stops[ordinal as usize];
        let reached = matches!(self.rerun_stop_args(stop), Ok(Inner::Reached));
        if !reached {
            return Ok(None);
        }
        let rebuilt = match self.rebuilt_yield(code, stop) {
            Rebuilt::Yield(y) => Some(y),
            // Choice: if the list is empty, the scripts changed and there are no options to choose
            Rebuilt::NoChoice => None,
            Rebuilt::NotStop => return fail(LoadErrorKind::Corrupt),
        };
        self.stack.clear();
        self.pc = stop.pc as usize + 1;
        Ok(rebuilt)
    }

    /// Reruns the instructions that compute the arguments of the stop point, from the random state `args_rng`, up to just
    /// before the stopping instruction. Even on failure, follows the `fn` frames back to the label's local variables, and
    /// leaves `state` as it was before the call.
    pub(super) fn rerun_stop_args(&mut self, stop: &StopInfo) -> Result<Inner, VmError> {
        let before = self.state.clone();
        self.stack.clear();
        self.cap.clear();
        self.rng.clone_from(&self.args_rng);
        self.pc = stop.args_pc as usize;
        self.fn_frames.clear();
        self.cur_fn = None;
        self.total = self.recount_total();
        self.total_valid = true;
        let result = self.exec(Some(stop.pc as usize));
        while let Some(fr) = self.fn_frames.pop() {
            self.locals = fr.locals;
        }
        self.cur_fn = None;
        self.total_valid = false;
        self.state = before;
        result
    }

    /// Also reflects the rebuilt arguments in the last history record (the current stop point). So that `current()` and the
    /// history do not diverge when `persistent var`s or `seen` changed after saving and the arguments changed.
    pub(super) fn sync_current_record(&mut self) {
        let (Some(Yield::Stop { command, id, .. }), Some(src)) = (&self.current, &self.cur_src)
        else {
            return;
        };
        let key = id
            .as_ref()
            .map(|id| format!("{}#{id}", super::label_path(&self.program, self.label)));
        let name = &self.registry.command(*command).name;
        if let Some(last) = self.history.last_mut()
            && last.key == key
            && last.command == *name
        {
            let last = Arc::make_mut(last);
            last.args = src.args.clone();
            last.values = src.values.clone();
        }
    }
}

/// Differences between the saved variables and the variables of the current scripts and registry.
struct VarChanges {
    added: Vec<String>,
    removed: Vec<String>,
    type_changed: Vec<String>,
}

/// Reads the bytes of a save. `Corrupt` if values (`NaN`, `±∞`, nesting that is too deep, sequences with the wrong number
/// of nodes are broken), the random generator or the history are broken.
/// The schema inside a save: its version, the type declarations, and the types of the script variables (without the
/// host-declared ones).
pub(crate) fn embedded_schema(bytes: &[u8]) -> Result<crate::schema_file::Embedded, LoadError> {
    let body_bytes = unwrap(bytes, MAGIC, FORMAT_VERSION)?;
    let body: SaveBody = match postcard::from_bytes(body_bytes) {
        Ok(b) => b,
        Err(_) => return fail(LoadErrorKind::Corrupt),
    };
    if !schema::consistent(&body.var_types, &body.vars, &body.host_vars) {
        return fail(LoadErrorKind::Corrupt);
    }
    let mut var_types = body.var_types;
    var_types.truncate(body.vars.len());
    Ok(crate::schema_file::Embedded {
        version: body.schema_version,
        types: body.types,
        var_types,
    })
}

fn decode_save(bytes: &[u8]) -> Result<(SaveBody, Rng, Vec<Arc<StopRecord>>), LoadError> {
    let body_bytes = unwrap(bytes, MAGIC, FORMAT_VERSION)?;
    let mut body: SaveBody = match postcard::from_bytes(body_bytes) {
        Ok(b) => b,
        Err(_) => return fail(LoadErrorKind::Corrupt),
    };
    if !schema::consistent(&body.var_types, &body.vars, &body.host_vars) {
        return fail(LoadErrorKind::Corrupt);
    }
    // Check that the values are not broken (`NaN`, `±∞`, nesting that is too deep, and sequences with the wrong number of
    // nodes are broken). `-0.0` is normalized to `0.0`.
    let all_values = body
        .vars
        .iter()
        .chain(body.host_vars.iter())
        .map(|(_, v)| v)
        .chain(body.locals.iter().map(|(_, _, v)| v))
        .chain(
            body.frames
                .iter()
                .flat_map(|f| f.locals.iter().map(|(_, _, v)| v)),
        );
    for v in all_values {
        if v.decode().is_none() {
            return fail(LoadErrorKind::Corrupt);
        }
    }
    let Some(rng) = Rng::from_state(body.rng) else {
        return fail(LoadErrorKind::Corrupt);
    };
    let mut history = Vec::with_capacity(body.history.len());
    for r in std::mem::take(&mut body.history) {
        match r.into_record() {
            Some(r) => history.push(Arc::new(r)),
            None => return fail(LoadErrorKind::Corrupt),
        }
    }
    Ok((body, rng, history))
}
