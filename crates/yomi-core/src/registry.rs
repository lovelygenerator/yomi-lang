//! The command registry and sugar bindings (stop commands, and bindings of plain lines, quote lines and choices).

mod bind;
mod decl;
mod error;
mod lookup;
mod meta;
mod register;
#[cfg(test)]
mod tests;
mod types;

pub use decl::*;
pub use error::RegistryError;
pub use types::{FromArgs, FromValue, IntoValue, YomiArgs, YomiDecl, YomiMap, YomiType};

use std::sync::Arc;

use crate::package::{CommandError, Env, Package, PackageEntry, PackageReg};
use crate::syntax::keywords::is_reserved;
use crate::value::{Identity, MAX_TYPE_DEPTH, Ty, TypeDecl, TypeKind, Value, find_type};

/// The command registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub(crate) stops: Vec<StopDecl>,
    /// Argument validation, in the same order as `stops`.
    stop_checks: Vec<Option<StopCheck>>,
    /// Return value validation, in the same order as `stops`.
    stop_ret_checks: Vec<Option<RetCheck>>,
    pub(crate) natives: Vec<NativeDecl>,
    pub(crate) packages: Vec<PackageEntry>,
    pub(crate) vars: Vec<VarDecl>,
    /// Registered `struct`s and `enum`s (in registration order; used types come before the types that use them).
    pub(crate) types: Vec<TypeDecl>,
    /// Descriptions, in the same order as `types`.
    pub(crate) type_docs: Vec<Option<String>>,
    /// Old names of commands (old name, official name).
    pub(crate) aliases: Vec<(String, String)>,
    pub(crate) plain: Option<Binding>,
    pub(crate) quote: Option<Binding>,
    pub(crate) choice: Option<Binding>,
    /// Whether the registry was made from a declaration file (it has no functions to run commands).
    pub(crate) decl_only: bool,
}

/// Whether `name` is an identifier that is not a reserved word.
fn is_plain_ident(name: &str) -> bool {
    // A UAX #31 identifier normalized to NFC (names that are not normalized cannot be looked up from scripts, so they are
    // rejected at registration)
    crate::ident::is_identifier(name) && crate::ident::is_nfc(name) && !is_reserved(name)
}

/// Whether `name` is an identifier (reserved words allowed). Used for the name after `namespace.`.
fn is_member_ident(name: &str) -> bool {
    crate::ident::is_identifier(name) && crate::ident::is_nfc(name)
}

/// Whether it has the form `namespace.name`, where the namespace is an identifier that is not a reserved word and the name
/// is an identifier (reserved words allowed).
fn is_qualified_name(name: &str) -> bool {
    name.split_once('.')
        .is_some_and(|(ns, m)| is_plain_ident(ns) && is_member_ident(m))
}

/// Whether it is valid as the name of a command not belonging to a package: `namespace.name`, or a single identifier
/// without a namespace.
fn is_command_name(name: &str) -> bool {
    if name.contains('.') {
        is_qualified_name(name)
    } else {
        is_plain_ident(name)
    }
}

/// Extracts the second half (an identifier) of `package.name`. `None` if the first half differs or the second half is not
/// an identifier.
fn member_of<'n>(package: &str, name: &'n str) -> Option<&'n str> {
    let rest = name.strip_prefix(package)?.strip_prefix('.')?;
    is_member_ident(rest).then_some(rest)
}

/// Collects the key types of `map`s in a type.
fn collect_map_keys<'a>(ty: &'a Ty, out: &mut Vec<&'a Ty>) {
    match ty {
        Ty::List(t) | Ty::Option(t) => collect_map_keys(t, out),
        Ty::Map(k, v) => {
            out.push(k);
            collect_map_keys(k, out);
            collect_map_keys(v, out);
        }
        _ => {}
    }
}

/// Whether a type contains `()` (it cannot be a field or variable).
fn contains_unit(ty: &Ty) -> bool {
    match ty {
        Ty::Unit => true,
        Ty::List(t) | Ty::Option(t) => contains_unit(t),
        Ty::Map(k, v) => contains_unit(k) || contains_unit(v),
        _ => false,
    }
}
