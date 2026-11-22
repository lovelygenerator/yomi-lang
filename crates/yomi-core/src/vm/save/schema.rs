//! The schema of variables in saves and persistent data: type strings, expansion of declared types, and the comparison of
//! saved variables with the current scripts.

use super::{LoadError, LoadErrorKind, SavedValue, fail, global_name};
use crate::program::Program;
use crate::type_expand::Expander;

/// One variable whose saved type differs from the type in the scripts. Both types are expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDiff {
    /// The variable (`module.variable`).
    pub name: String,
    /// The type when saved.
    pub saved: String,
    /// The type in the scripts.
    pub current: String,
}

/// Checks that the saved variables have the same names as their types.
pub(in crate::vm) fn consistent(
    var_types: &[(String, String)],
    vars: &[(String, SavedValue)],
    host_vars: &[(String, SavedValue)],
) -> bool {
    var_types.len() == vars.len() + host_vars.len()
        && var_types
            .iter()
            .zip(vars.iter().chain(host_vars))
            .all(|(t, v)| t.0 == v.0)
}

/// Compares the saved script variables with the scripts' declarations. `persistent` selects `persistent var`s (persistent
/// data) or `save var`s (saves). Returns the variables whose types differ. A saved value that does not fit its own saved
/// type, or a value that does not fit an unchanged type, is broken (`Corrupt`).
pub(in crate::vm) fn script_var_diffs(
    program: &Program,
    saved_types: &[(String, String)],
    saved_var_types: &[(String, String)],
    vars: &[(String, SavedValue)],
    persistent: bool,
) -> Result<Vec<SchemaDiff>, LoadError> {
    let current_sigs = super::type_signatures(program);
    let saved = Expander::new(saved_types);
    let current = Expander::new(&current_sigs);
    let mut diffs = Vec::new();
    for g in program
        .globals
        .iter()
        .filter(|g| g.persistent == persistent)
    {
        let name = global_name(program, g.module, &g.name);
        let Some((_, value)) = vars.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let Some((_, saved_ty)) = saved_var_types.iter().find(|(n, _)| *n == name) else {
            return fail(LoadErrorKind::Corrupt);
        };
        let saved_ty = saved.expand(saved_ty);
        let current_ty = current.expand(&g.ty.name());
        if saved_ty == current_ty {
            if !value.clone().into_value().matches_in(&g.ty, &program.types) {
                return fail(LoadErrorKind::Corrupt);
            }
        } else {
            diffs.push(SchemaDiff {
                name,
                saved: saved_ty,
                current: current_ty,
            });
        }
    }
    Ok(diffs)
}

/// Checks the saved schema version against the scripts (`SaveFromNewerVersion`, `SaveVersionUnsupported`).
pub(in crate::vm) fn check_version(program: &Program, saved: u32) -> Result<(), LoadError> {
    let current = program.schema_version();
    if saved > current {
        return fail(LoadErrorKind::SaveFromNewerVersion { saved, current });
    }
    let min = program.min_save_version();
    if saved < min {
        return fail(LoadErrorKind::SaveVersionUnsupported { saved, min });
    }
    Ok(())
}
