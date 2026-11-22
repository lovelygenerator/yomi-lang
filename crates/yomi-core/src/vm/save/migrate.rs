//! Applying the `migrate` blocks to the variables of a save or persistent data when loading it
//! (`spec/host/04-save.md`, 4.6; `spec/language/07-migrate.md`).

use std::collections::{BTreeMap, BTreeSet};

use super::value::SavedNode;
use super::*;
use crate::program::MigrateOp;
use crate::type_expand::Expander;

/// The result of the migration.
#[derive(Default)]
pub(in crate::vm) struct Migration {
    /// Variables made by `convert` or `rename` (the new names).
    pub migrated: Vec<String>,
    /// Variables whose `convert` failed and that were left out (`Lenient`).
    pub failed: Vec<String>,
}

/// Which declarations the type of a variable is written in.
#[derive(Clone, Copy)]
enum Decls {
    /// The declarations in the save.
    Saved,
    /// A frozen schema of that version.
    Frozen(u32),
    /// The current scripts.
    Script,
}

#[derive(Clone)]
struct Var {
    value: SavedValue,
    /// The type as written (declared types are not expanded).
    ty: String,
    decls: Decls,
    migrated: bool,
}

/// Renames the types of the `struct` and `enum` values in `value`. `f` returns the new name, or `None` to keep it.
fn rename_types(value: &mut SavedValue, f: &dyn Fn(&str) -> Option<String>) {
    for node in &mut value.0 {
        match node {
            SavedNode::Struct { ty, .. } | SavedNode::Enum { ty, .. } => {
                if let Some(n) = f(ty) {
                    *ty = n;
                }
            }
            _ => {}
        }
    }
}

/// Applies `migrate (from+1)` … `migrate (to)` to the script variables `vars` (`save var`s, or `persistent var`s when
/// `persistent` is true) and rewrites their types in `var_types` as expanded type strings. Does nothing when the save is as
/// new as the scripts. `Strict` fails on a `convert` that fails at run time; `Lenient` leaves the variable out.
#[allow(clippy::too_many_arguments)]
pub(in crate::vm) fn apply(
    program: &Program,
    registry: &Registry,
    cfg: &VmConfig,
    from: u32,
    persistent: bool,
    saved_types: &[(String, String)],
    vars: &mut Vec<(String, SavedValue)>,
    var_types: &mut Vec<(String, String)>,
) -> Result<Migration, LoadError> {
    let to = program.schema_version();
    if from >= to {
        return Ok(Migration::default());
    }
    let script_count = vars.len();
    let host_types = var_types.split_off(script_count.min(var_types.len()));
    let mut state: BTreeMap<String, Var> = BTreeMap::new();
    for (name, value) in vars.drain(..) {
        let Some((_, ty)) = var_types.iter().find(|(n, _)| *n == name) else {
            return fail(LoadErrorKind::Corrupt);
        };
        state.insert(
            name,
            Var {
                value,
                ty: ty.clone(),
                decls: Decls::Saved,
                migrated: false,
            },
        );
    }
    var_types.clear();
    let mut evaluator: Option<Vm<'_>> = None;
    let mut failed: Vec<String> = Vec::new();
    for block in program
        .migrate
        .blocks
        .iter()
        .filter(|b| b.version > from && b.version <= to)
    {
        let n = block.version;
        let mut handled: BTreeSet<String> = BTreeSet::new();
        let mut made: Vec<(String, Var)> = Vec::new();
        for op in block.ops.iter().filter(|o| op_persistent(o) == persistent) {
            match op {
                MigrateOp::Drop { old, .. } => {
                    handled.insert(old.clone());
                }
                MigrateOp::Rename { old, new, .. } => {
                    handled.insert(old.clone());
                    if let Some(v) = state.get(old) {
                        made.push((
                            new.clone(),
                            Var {
                                migrated: true,
                                ..v.clone()
                            },
                        ));
                    }
                }
                MigrateOp::Convert {
                    old,
                    new,
                    new_ty,
                    label,
                    new_frozen,
                    ..
                } => {
                    handled.insert(old.clone());
                    let Some(v) = state.get(old) else {
                        continue;
                    };
                    let old_names = program
                        .migrate
                        .schemas
                        .get(&(n - 1))
                        .map(|s| &s.versioned[..])
                        .unwrap_or(&[]);
                    let mut arg = v.value.clone();
                    rename_types(&mut arg, &|t| {
                        old_names
                            .iter()
                            .any(|x| x == t)
                            .then(|| format!("v{}::{t}", n - 1))
                    });
                    let prefix = format!("v{n}::");
                    let Some(arg) = arg.decode() else {
                        return fail(LoadErrorKind::Corrupt);
                    };
                    if evaluator.is_none() {
                        evaluator = Some(Vm::blank_for_load(program, registry, cfg.clone())?);
                    }
                    let vm = evaluator.as_mut().expect("作った");
                    match vm.eval_label(*label, arg) {
                        Some(res) => {
                            let mut out = SavedValue::from_value(&res);
                            if *new_frozen {
                                rename_types(&mut out, &|t| {
                                    t.strip_prefix(&prefix).map(str::to_string)
                                });
                            }
                            made.push((
                                new.clone(),
                                Var {
                                    value: out,
                                    ty: new_ty.clone(),
                                    decls: if *new_frozen {
                                        Decls::Frozen(n)
                                    } else {
                                        Decls::Script
                                    },
                                    migrated: true,
                                },
                            ));
                        }
                        None => failed.push(new.clone()),
                    }
                }
            }
        }
        for old in &handled {
            state.remove(old);
        }
        state.extend(made);
    }
    if cfg.load_policy == LoadPolicy::Strict && !failed.is_empty() {
        failed.sort();
        failed.dedup();
        return fail(LoadErrorKind::MigrateFailed(failed));
    }
    let current: Vec<(String, String)> = program
        .types
        .iter()
        .map(|t| (t.name.clone(), t.signature()))
        .collect();
    let expand = |v: &Var| -> String {
        match v.decls {
            Decls::Saved => Expander::new(saved_types).expand(&v.ty),
            Decls::Frozen(k) => match program.migrate.schemas.get(&k) {
                Some(s) => Expander::new(&s.decls).expand(&v.ty),
                None => v.ty.clone(),
            },
            Decls::Script => Expander::new(&current).expand(&v.ty),
        }
    };
    let mut migrated = Vec::new();
    for (name, v) in state {
        var_types.push((name.clone(), expand(&v)));
        if v.migrated {
            migrated.push(name.clone());
        }
        vars.push((name, v.value));
    }
    var_types.extend(host_types);
    failed.sort();
    failed.dedup();
    Ok(Migration { migrated, failed })
}

/// Whether loading data of schema version `from` runs migrations (the types of its variables are then already expanded).
pub(in crate::vm) fn migrated_types(program: &Program, from: u32) -> bool {
    from < program.schema_version()
}

fn op_persistent(op: &MigrateOp) -> bool {
    match op {
        MigrateOp::Rename { persistent, .. }
        | MigrateOp::Convert { persistent, .. }
        | MigrateOp::Drop { persistent, .. } => *persistent,
    }
}

impl Vm<'_> {
    /// Runs a hidden label made for a `convert` with `arg` as its parameter. `None` if it failed at run time.
    fn eval_label(&mut self, label: usize, arg: Value) -> Option<Value> {
        self.begin_input(label);
        self.locals[0] = arg;
        let ran = self.run();
        let value = self.stack_top();
        self.clear_stack();
        ran.ok().and(value)
    }
}
