//! Registry errors.

use super::*;

/// A registry error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// A command (or variable) with the same name was registered twice.
    DuplicateCommand(String),
    /// Tried to bind a command that is not registered.
    UnknownCommand(String),
    /// The types of the command to bind do not match the parameter shape of the sugar.
    SugarMismatch(String),
    /// The same kind of sugar was bound twice.
    AlreadyBound(Sugar),
    /// The name is invalid (not an identifier, or the first half of the package name differs from `Package::NAME`).
    InvalidName(String),
    /// A package with the same name was registered twice.
    DuplicatePackage(String),
    /// A package declared in `DEPENDS` is neither registered nor in the same bulk registration.
    MissingDependency {
        /// The package being registered.
        package: String,
        /// The unregistered dependency.
        dependency: String,
    },
    /// Package dependencies form a cycle (including a dependency on itself).
    DependencyCycle(String),
    /// The return type of a query is `()` (a query that returns no value cannot be made).
    QueryWithoutValue(String),
    /// The type of a variable cannot be used (it contains `()`).
    UnsupportedVarType(String),
    /// The initial value of a variable is `NaN` or `±∞` (which never appear as script values), or does not match the type of
    /// the variable.
    InvalidVarValue(String),
    /// A type with the same name was registered twice.
    DuplicateType(String),
    /// A type used by a type, command or variable is not registered yet (types are registered before they are used).
    UnknownType {
        /// The name of the unregistered type.
        name: String,
        /// The name of the type, command or variable that uses that type.
        used_by: String,
    },
    /// A type contains itself (even directly).
    RecursiveType(String),
    /// Uses a type that cannot be a `map` key (`float`, `list`, `map`, `Option`, and so on). The argument is the name of the
    /// user.
    InvalidMapKey(String),
    /// The nesting of types is too deep (`MAX_TYPE_DEPTH`). The argument is the name of the user.
    TypeTooDeep(String),
    /// A type declaration is invalid (names of fields or variants are not identifiers or collide, there are no variants, or a
    /// field is `()`).
    InvalidTypeDecl(String),
    /// There is no command to attach an alias, description or parameter names to (no command was registered just before).
    NoCommand,
    /// The number of parameter names differs from the number of parameters of the command. Or a default has no name, has the
    /// wrong type, or parameters with defaults are not at the end.
    InvalidParams(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::DuplicateCommand(n) => write!(f, "duplicate command: {n}"),
            RegistryError::UnknownCommand(n) => write!(f, "unknown command: {n}"),
            RegistryError::SugarMismatch(n) => write!(f, "type mismatch for sugar binding: {n}"),
            RegistryError::AlreadyBound(s) => write!(f, "sugar already bound: {}", s.name()),
            RegistryError::InvalidName(n) => write!(f, "invalid name: {n}"),
            RegistryError::DuplicatePackage(n) => write!(f, "duplicate package: {n}"),
            RegistryError::MissingDependency {
                package,
                dependency,
            } => write!(
                f,
                "package {package} depends on {dependency}, which is not registered"
            ),
            RegistryError::DependencyCycle(n) => write!(f, "package {n} depends on itself"),
            RegistryError::QueryWithoutValue(n) => write!(f, "query returns no value: {n}"),
            RegistryError::UnsupportedVarType(n) => write!(f, "unsupported variable type: {n}"),
            RegistryError::InvalidVarValue(n) => write!(f, "invalid initial value: {n}"),
            RegistryError::DuplicateType(n) => write!(f, "duplicate type: {n}"),
            RegistryError::UnknownType { name, used_by } => {
                write!(f, "type {name} (used by {used_by}) is not registered")
            }
            RegistryError::RecursiveType(n) => write!(f, "type {n} contains itself"),
            RegistryError::InvalidMapKey(n) => write!(f, "invalid map key type in {n}"),
            RegistryError::TypeTooDeep(n) => write!(f, "type nesting too deep in {n}"),
            RegistryError::InvalidTypeDecl(n) => write!(f, "invalid type declaration: {n}"),
            RegistryError::NoCommand => write!(f, "no command to attach the item to"),
            RegistryError::InvalidParams(n) => {
                write!(f, "invalid parameter names or defaults: {n}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}
