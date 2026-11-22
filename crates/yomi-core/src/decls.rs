//! Writing and reading declaration files (JSON) (section 2.8 of `spec/host/02-registry.md`).
//!
//! JSON is read and written with `serde_json`. Reading has limits on the size of the input ([`MAX_DECLS_BYTES`]) and the
//! nesting depth ([`MAX_JSON_DEPTH`]), and does not panic on broken or huge input.

mod convert;
mod json;
mod ty;

use convert::*;
use json::*;
use ty::*;

use std::collections::HashMap;
use std::sync::Arc;

use crate::package::{CommandError, Env, PackageEntry};
use crate::registry::{CommandKind, Kind, NativeKind, Registry, RegistryError, Sugar};
use crate::value::{Identity, MAX_TYPE_DEPTH, Ty, TypeDecl, TypeKind, Value, VariantDecl};

/// The `format` of a declaration file.
const FORMAT: &str = "yomi-decls";

/// The version of the declaration file format that can be read.
/// Format versions: version 2 added default values of command parameters (`params[].default`). Version 1 is also read
/// (without defaults).
const VERSION: i64 = 2;

/// The maximum size of a declaration file to read (bytes).
pub const MAX_DECLS_BYTES: usize = 16 * 1024 * 1024;

/// The maximum nesting depth of JSON.
pub const MAX_JSON_DEPTH: usize = 128;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// The reason a declaration file could not be read.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclErrorKind {
    /// Cannot be read as JSON (positions are 1-based).
    Syntax {
        /// Line.
        line: u32,
        /// Column (in characters).
        col: u32,
    },
    /// The input is too large ([`MAX_DECLS_BYTES`]).
    TooLarge,
    /// The JSON nesting is too deep ([`MAX_JSON_DEPTH`]), or the nesting of a type name is too deep.
    TooDeep,
    /// Not a declaration file (the whole is not an object, or `format` is not `"yomi-decls"`).
    NotDecls,
    /// A declaration file with a version that cannot be read.
    UnsupportedVersion(i64),
    /// A required item is missing.
    MissingField,
    /// The JSON type of an item does not match.
    WrongType,
    /// A type name is invalid (an unregistered type, or an unreadable notation). The value is the string as written.
    InvalidType(String),
    /// A value is invalid (an unknown `kind`, a `default` that does not match the type, and so on).
    InvalidValue,
    /// Invalid as a registry (colliding names, errors in type declarations, and so on).
    Registry(RegistryError),
}

impl DeclErrorKind {
    /// The stable English name.
    pub fn code(&self) -> &'static str {
        match self {
            DeclErrorKind::Syntax { .. } => "syntax",
            DeclErrorKind::TooLarge => "too_large",
            DeclErrorKind::TooDeep => "too_deep",
            DeclErrorKind::NotDecls => "not_decls",
            DeclErrorKind::UnsupportedVersion(_) => "unsupported_version",
            DeclErrorKind::MissingField => "missing_field",
            DeclErrorKind::WrongType => "wrong_type",
            DeclErrorKind::InvalidType(_) => "invalid_type",
            DeclErrorKind::InvalidValue => "invalid_value",
            DeclErrorKind::Registry(_) => "registry",
        }
    }
}

/// The error when a declaration file could not be read. Has no message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclError {
    /// Kind.
    pub kind: DeclErrorKind,
    /// Where the problem is (in the form `commands[3].params[0].type`; empty for the whole).
    pub path: String,
}

impl std::fmt::Display for DeclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind.code())?;
        if !self.path.is_empty() {
            write!(f, " at {}", self.path)?;
        }
        match &self.kind {
            DeclErrorKind::Syntax { line, col } => write!(f, " ({line}:{col})"),
            DeclErrorKind::UnsupportedVersion(v) => write!(f, " ({v})"),
            DeclErrorKind::InvalidType(t) => write!(f, " ({t})"),
            DeclErrorKind::Registry(e) => write!(f, " ({e})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for DeclError {}

/// Makes a `DeclError` from the kind of error and the location (the JSON path).
fn err(kind: DeclErrorKind, path: &str) -> DeclError {
    DeclError {
        kind,
        path: path.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Values and JSON (initial values of variables)
// ---------------------------------------------------------------------------

/// Adds a key or an index to a JSON path.
fn join(path: &str, part: &str) -> String {
    if path.is_empty() {
        part.to_string()
    } else if part.starts_with('[') {
        format!("{path}{part}")
    } else {
        format!("{path}.{part}")
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// The name of a command kind in declaration files.
fn kind_name(k: CommandKind) -> &'static str {
    k.name()
}

impl Registry {
    /// Writes the registry as a declaration file (JSON) (section 2.8 of `spec/host/02-registry.md`).
    ///
    /// Arrays are sorted by name in ascending order, so the same registry gives the same string. Tools (checking, the LSP,
    /// `assign-ids`) read this with [`Registry::from_decls`] and can check scripts without the host's Rust code.
    pub fn export_decls(&self) -> String {
        let mut root: Vec<(String, Json)> = vec![
            ("format".into(), s(FORMAT)),
            ("version".into(), Json::from(VERSION)),
        ];
        // Packages
        let mut packages: Vec<&PackageEntry> = self.packages.iter().collect();
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        root.push((
            "packages".into(),
            Json::Array(
                packages
                    .iter()
                    .map(|p| {
                        let mut deps = p.depends.clone();
                        deps.sort();
                        let mut fields = vec![
                            ("name".into(), s(&p.name)),
                            ("version".into(), Json::from(p.version)),
                            (
                                "depends".into(),
                                Json::Array(deps.iter().map(|d| s(d)).collect()),
                            ),
                        ];
                        // Mark only the standard packages (others keep the same shape as before)
                        if p.std {
                            fields.push(("std".into(), Json::Bool(true)));
                        }
                        obj(fields)
                    })
                    .collect(),
            ),
        ));
        // Commands
        root.push((
            "commands".into(),
            Json::Array(
                self.commands()
                    .iter()
                    .map(|c| {
                        let mut o: Vec<(String, Json)> = vec![
                            ("name".into(), s(&c.name)),
                            ("kind".into(), s(kind_name(c.kind))),
                        ];
                        if let Some(p) = &c.package {
                            o.push(("package".into(), s(p)));
                        }
                        o.push((
                            "params".into(),
                            Json::Array(
                                c.args
                                    .iter()
                                    .zip(&c.params)
                                    .zip(&c.defaults)
                                    .map(|((t, n), d)| {
                                        let mut p: Vec<(String, Json)> = Vec::new();
                                        if let Some(n) = n {
                                            p.push(("name".into(), s(n)));
                                        }
                                        p.push(("type".into(), s(&t.name())));
                                        if let Some(d) = d {
                                            p.push(("default".into(), value_json(d)));
                                        }
                                        obj(p)
                                    })
                                    .collect(),
                            ),
                        ));
                        o.push((
                            "returns".into(),
                            if c.ret == Ty::Unit {
                                Json::Null
                            } else {
                                s(&c.ret.name())
                            },
                        ));
                        o.push((
                            "aliases".into(),
                            Json::Array(c.aliases.iter().map(|a| s(a)).collect()),
                        ));
                        if let Some(d) = &c.doc {
                            o.push(("doc".into(), s(d)));
                        }
                        obj(o)
                    })
                    .collect(),
            ),
        ));
        // Variables
        let mut vars: Vec<&crate::registry::VarDecl> = self.vars.iter().collect();
        vars.sort_by(|a, b| a.name.cmp(&b.name));
        root.push((
            "variables".into(),
            Json::Array(
                vars.iter()
                    .map(|v| {
                        let mut o: Vec<(String, Json)> = vec![
                            ("name".into(), s(&v.name)),
                            ("type".into(), s(&v.ty.name())),
                            (
                                "kind".into(),
                                s(match v.kind {
                                    Kind::Save => "save",
                                    Kind::Persistent => "persistent",
                                }),
                            ),
                            ("default".into(), value_json(&v.init)),
                            ("read_only".into(), Json::Bool(v.read_only)),
                        ];
                        if let Some(d) = &v.doc {
                            o.push(("doc".into(), s(d)));
                        }
                        obj(o)
                    })
                    .collect(),
            ),
        ));
        // Types
        let mut types: Vec<usize> = (0..self.types.len()).collect();
        types.sort_by(|&a, &b| self.types[a].name.cmp(&self.types[b].name));
        root.push((
            "types".into(),
            Json::Array(
                types
                    .iter()
                    .map(|&i| {
                        let t = &self.types[i];
                        let mut o: Vec<(String, Json)> = vec![("name".into(), s(&t.name))];
                        match &t.kind {
                            TypeKind::Struct { fields } => {
                                o.push(("kind".into(), s("struct")));
                                o.push((
                                    "fields".into(),
                                    Json::Array(
                                        fields
                                            .iter()
                                            .map(|(n, ty)| {
                                                obj(vec![
                                                    ("name".into(), s(n)),
                                                    ("type".into(), s(&ty.name())),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ));
                            }
                            TypeKind::Enum { variants } => {
                                o.push(("kind".into(), s("enum")));
                                o.push((
                                    "variants".into(),
                                    Json::Array(
                                        variants
                                            .iter()
                                            .map(|v| {
                                                obj(vec![
                                                    ("name".into(), s(&v.name)),
                                                    (
                                                        "fields".into(),
                                                        Json::Array(
                                                            v.args
                                                                .iter()
                                                                .map(|a| {
                                                                    obj(vec![(
                                                                        "type".into(),
                                                                        s(&a.name()),
                                                                    )])
                                                                })
                                                                .collect(),
                                                        ),
                                                    ),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ));
                            }
                        }
                        if let Some(d) = &self.type_docs[i] {
                            o.push(("doc".into(), s(d)));
                        }
                        obj(o)
                    })
                    .collect(),
            ),
        ));
        // Sugar
        let mut sugar: Vec<(String, Json)> = Vec::new();
        for kind in [Sugar::Plain, Sugar::Quote, Sugar::Choice] {
            match self.sugar_binding(kind) {
                Some(Some(name)) => sugar.push((kind.name().into(), s(name))),
                Some(None) => sugar.push((kind.name().into(), Json::Null)),
                None => {}
            }
        }
        root.push(("sugar".into(), obj(sugar)));
        write_json(&obj(root))
    }

    /// Makes a registry without functions from a declaration file (JSON; the output of [`Registry::export_decls`]).
    ///
    /// The registry can only be used for compiling scripts (checking, the LSP, `assign-ids`, `yomi check`). Passing it to
    /// [`Vm::new`](crate::Vm::new) or [`Vm::load`](crate::Vm::load) is an error ([`Registry::is_decl_only`]).
    ///
    /// - `format` must be `"yomi-decls"` and `version` must be 1. Unknown items are ignored.
    /// - Broken input, input that is too large ([`MAX_DECLS_BYTES`]) and nesting that is too deep ([`MAX_JSON_DEPTH`]) do
    ///   not panic; a [`DeclError`] is returned.
    pub fn from_decls(json: &str) -> Result<Registry, DeclError> {
        let root = parse_json(json)?;
        build(&root)
    }
}

// ---------------------------------------------------------------------------
// Reading (from the JSON tree to a registry)
// ---------------------------------------------------------------------------

/// Turns a registry error into a declaration file error with a location.
fn reg_err(e: RegistryError, path: &str) -> DeclError {
    err(DeclErrorKind::Registry(e), path)
}

/// Makes the path of an array item (`array[i]`).
fn item_path(array: &str, i: usize) -> String {
    format!("{array}[{i}]")
}

/// The function of a command that cannot run (a registry made from a declaration file).
fn no_function() -> crate::registry::NativeFn {
    Arc::new(|_env: Env<'_>, _args: &[Value]| {
        Err(CommandError::with_code("core.decl_only_function", []))
    })
}

/// Builds a registry from the JSON tree. Types are registered before commands and variables (which use them), and aliases
/// after commands.
fn build(root: &Json) -> JResult<Registry> {
    check_header(root)?;
    let mut reg = Registry::new();
    reg.decl_only = true;
    build_packages(&mut reg, root)?;
    let kinds = build_types(&mut reg, root)?;
    build_commands(&mut reg, root, &kinds)?;
    build_variables(&mut reg, root, &kinds)?;
    build_sugar(&mut reg, root)?;
    Ok(reg)
}

/// Checks the format and version of the declaration file.
fn check_header(root: &Json) -> JResult<()> {
    if !matches!(root, Json::Object(_)) {
        return Err(err(DeclErrorKind::NotDecls, ""));
    }
    match root.get("format") {
        Some(Json::String(f)) if f == FORMAT => {}
        _ => return Err(err(DeclErrorKind::NotDecls, "format")),
    }
    match root.get("version") {
        Some(Json::Number(n)) => match n.as_i64() {
            Some(1 | VERSION) => {}
            Some(other) => return Err(err(DeclErrorKind::UnsupportedVersion(other), "version")),
            None => return Err(err(DeclErrorKind::WrongType, "version")),
        },
        Some(_) => return Err(err(DeclErrorKind::WrongType, "version")),
        None => return Err(err(DeclErrorKind::MissingField, "version")),
    }
    Ok(())
}

/// Packages.
fn build_packages(reg: &mut Registry, root: &Json) -> JResult<()> {
    for (i, p) in opt_arr(root, "packages", "")?.iter().enumerate() {
        let path = item_path("packages", i);
        let name = want_str(p, "name", &path)?;
        if reg.packages.iter().any(|e| e.name == name) {
            return Err(reg_err(RegistryError::DuplicatePackage(name.into()), &path));
        }
        let version = match p.get("version") {
            Some(Json::Number(n)) => n
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| err(DeclErrorKind::InvalidValue, &join(&path, "version")))?,
            Some(_) => return Err(err(DeclErrorKind::WrongType, &join(&path, "version"))),
            None => 1,
        };
        let mut depends = Vec::new();
        for (k, d) in opt_arr(p, "depends", &path)?.iter().enumerate() {
            match d {
                Json::String(t) => depends.push(t.clone()),
                _ => {
                    return Err(err(
                        DeclErrorKind::WrongType,
                        &join(&join(&path, "depends"), &format!("[{k}]")),
                    ));
                }
            }
        }
        let std = match p.get("std") {
            Some(Json::Bool(b)) => *b,
            Some(_) => return Err(err(DeclErrorKind::WrongType, &join(&path, "std"))),
            None => false,
        };
        reg.packages
            .push(PackageEntry::decl_only(name, version, depends, std));
    }
    Ok(())
}

/// Types. Sorted so that used types are registered first. Returns a table from type name to whether it is a struct
/// (false means enum).
fn build_types(reg: &mut Registry, root: &Json) -> JResult<HashMap<String, bool>> {
    let types_json = opt_arr(root, "types", "")?;
    let mut kinds: HashMap<String, bool> = HashMap::new();
    for (i, t) in types_json.iter().enumerate() {
        let path = item_path("types", i);
        let name = want_str(t, "name", &path)?;
        let is_struct = match want_str(t, "kind", &path)? {
            "struct" => true,
            "enum" => false,
            _ => return Err(err(DeclErrorKind::InvalidValue, &join(&path, "kind"))),
        };
        if kinds.insert(name.to_string(), is_struct).is_some() {
            return Err(reg_err(RegistryError::DuplicateType(name.into()), &path));
        }
    }
    let mut parsed: Vec<(usize, TypeDecl)> = Vec::new();
    for (i, t) in types_json.iter().enumerate() {
        let path = item_path("types", i);
        let name = want_str(t, "name", &path)?.to_string();
        let kind = if kinds[&name] {
            let mut fields = Vec::new();
            for (k, f) in opt_arr(t, "fields", &path)?.iter().enumerate() {
                let fp = format!("{path}.fields[{k}]");
                let fname = want_str(f, "name", &fp)?.to_string();
                let ty = parse_ty(want_str(f, "type", &fp)?, &kinds, &join(&fp, "type"))?;
                fields.push((fname, ty));
            }
            TypeKind::Struct { fields }
        } else {
            let mut variants = Vec::new();
            for (k, v) in opt_arr(t, "variants", &path)?.iter().enumerate() {
                let vp = format!("{path}.variants[{k}]");
                let vname = want_str(v, "name", &vp)?.to_string();
                let mut args = Vec::new();
                for (m, a) in opt_arr(v, "fields", &vp)?.iter().enumerate() {
                    let ap = format!("{vp}.fields[{m}]");
                    args.push(parse_ty(
                        want_str(a, "type", &ap)?,
                        &kinds,
                        &join(&ap, "type"),
                    )?);
                }
                variants.push(VariantDecl { name: vname, args });
            }
            TypeKind::Enum { variants }
        };
        parsed.push((i, TypeDecl { name, kind }));
    }
    // In dependency order (used types first)
    let index_of: HashMap<&str, usize> = parsed
        .iter()
        .enumerate()
        .map(|(k, (_, d))| (d.name.as_str(), k))
        .collect();
    let mut waiting: Vec<usize> = vec![0; parsed.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); parsed.len()];
    for (k, (_, d)) in parsed.iter().enumerate() {
        let mut used: Vec<&str> = d.used_types();
        used.sort_unstable();
        used.dedup();
        for u in used {
            if let Some(&j) = index_of.get(u)
                && j != k
            {
                waiting[k] += 1;
                dependents[j].push(k);
            }
        }
    }
    let mut ready: Vec<usize> = (0..parsed.len()).filter(|&k| waiting[k] == 0).collect();
    ready.reverse();
    let mut done = vec![false; parsed.len()];
    while let Some(k) = ready.pop() {
        done[k] = true;
        let (orig, decl) = &parsed[k];
        let path = item_path("types", *orig);
        reg.add_type(None, decl.clone())
            .map_err(|e| reg_err(e, &path))?;
        let doc = opt_str(&types_json[*orig], "doc", &path)?;
        if let Some(d) = doc {
            let last = reg.type_docs.len() - 1;
            reg.type_docs[last] = Some(d.to_string());
        }
        for &m in dependents[k].iter().rev() {
            waiting[m] -= 1;
            if waiting[m] == 0 {
                ready.push(m);
            }
        }
    }
    if let Some(k) = done.iter().position(|d| !d) {
        // Types that use each other (including themselves)
        let (orig, decl) = &parsed[k];
        return Err(reg_err(
            RegistryError::RecursiveType(decl.name.clone()),
            &item_path("types", *orig),
        ));
    }
    Ok(kinds)
}

/// Commands and their aliases.
fn build_commands(reg: &mut Registry, root: &Json, kinds: &HashMap<String, bool>) -> JResult<()> {
    let mut alias_list: Vec<(String, String, String)> = Vec::new();
    for (i, c) in opt_arr(root, "commands", "")?.iter().enumerate() {
        let path = item_path("commands", i);
        let name = want_str(c, "name", &path)?;
        let kind = match want_str(c, "kind", &path)? {
            "effect" => CommandKind::Effect,
            "query" => CommandKind::Query,
            "stop" => CommandKind::Stop,
            _ => return Err(err(DeclErrorKind::InvalidValue, &join(&path, "kind"))),
        };
        let package = opt_str(c, "package", &path)?;
        let mut args: Vec<Ty> = Vec::new();
        let mut params: Vec<Option<String>> = Vec::new();
        let mut defaults: Vec<Option<Value>> = Vec::new();
        for (k, p) in opt_arr(c, "params", &path)?.iter().enumerate() {
            let pp = format!("{path}.params[{k}]");
            let ty = parse_ty(want_str(p, "type", &pp)?, kinds, &join(&pp, "type"))?;
            let default = match p.get("default") {
                Some(Json::Null) | None => None,
                Some(d) => Some(json_value(d, &ty, &reg.types, &join(&pp, "default"))?),
            };
            args.push(ty);
            params.push(opt_str(p, "name", &pp)?.map(str::to_string));
            defaults.push(default);
        }
        // Parameters with default values have names and come last
        if defaults
            .iter()
            .zip(&params)
            .any(|(d, n)| d.is_some() && n.is_none())
            || defaults
                .windows(2)
                .any(|w| w[0].is_some() && w[1].is_none())
        {
            return Err(err(DeclErrorKind::InvalidValue, &join(&path, "params")));
        }
        let ret = match opt_str(c, "returns", &path)? {
            None | Some("()") => Ty::Unit,
            Some(t) => parse_ty(t, kinds, &join(&path, "returns"))?,
        };
        match kind {
            CommandKind::Stop => {
                reg.add_stop(name, args, ret, package, None, None)
                    .map_err(|e| reg_err(e, &path))?;
                let last = reg.stops.len() - 1;
                reg.stops[last].params = params;
                reg.stops[last].defaults = defaults;
                reg.stops[last].doc = opt_str(c, "doc", &path)?.map(str::to_string);
            }
            _ => {
                let Some(package) = package else {
                    return Err(err(DeclErrorKind::MissingField, &join(&path, "package")));
                };
                let Some(index) = reg.packages.iter().position(|e| e.name == package) else {
                    return Err(reg_err(RegistryError::InvalidName(name.into()), &path));
                };
                let native_kind = if kind == CommandKind::Effect {
                    NativeKind::Effect
                } else {
                    NativeKind::Query
                };
                reg.add_native(index, name, native_kind, args, ret, no_function())
                    .map_err(|e| reg_err(e, &path))?;
                let last = reg.natives.len() - 1;
                reg.natives[last].params = params;
                reg.natives[last].defaults = defaults;
                reg.natives[last].doc = opt_str(c, "doc", &path)?.map(str::to_string);
            }
        }
        for (k, a) in opt_arr(c, "aliases", &path)?.iter().enumerate() {
            match a {
                Json::String(old) => alias_list.push((
                    old.clone(),
                    name.to_string(),
                    format!("{path}.aliases[{k}]"),
                )),
                _ => {
                    return Err(err(
                        DeclErrorKind::WrongType,
                        &format!("{path}.aliases[{k}]"),
                    ));
                }
            }
        }
    }
    for (old, target, path) in alias_list {
        reg.alias(&old, &target).map_err(|e| reg_err(e, &path))?;
    }
    Ok(())
}

/// Variables declared by the host.
fn build_variables(reg: &mut Registry, root: &Json, kinds: &HashMap<String, bool>) -> JResult<()> {
    for (i, v) in opt_arr(root, "variables", "")?.iter().enumerate() {
        let path = item_path("variables", i);
        let name = want_str(v, "name", &path)?;
        let ty = parse_ty(want_str(v, "type", &path)?, kinds, &join(&path, "type"))?;
        let kind = match want_str(v, "kind", &path)? {
            "save" => Kind::Save,
            "persistent" => Kind::Persistent,
            _ => return Err(err(DeclErrorKind::InvalidValue, &join(&path, "kind"))),
        };
        let init = match v.get("default") {
            Some(d) => json_value(d, &ty, &reg.types, &join(&path, "default"))?,
            None => return Err(err(DeclErrorKind::MissingField, &join(&path, "default"))),
        };
        let read_only = match v.get("read_only") {
            Some(Json::Bool(b)) => *b,
            Some(Json::Null) | None => false,
            Some(_) => return Err(err(DeclErrorKind::WrongType, &join(&path, "read_only"))),
        };
        let index = reg
            .add_var(None, name, ty, kind, init)
            .map_err(|e| reg_err(e, &path))?;
        reg.vars[index].read_only = read_only;
        reg.vars[index].doc = opt_str(v, "doc", &path)?.map(str::to_string);
    }
    Ok(())
}

/// Sugar bindings.
fn build_sugar(reg: &mut Registry, root: &Json) -> JResult<()> {
    if let Some(sugar) = root.get("sugar") {
        if !matches!(sugar, Json::Object(_)) {
            return Err(err(DeclErrorKind::WrongType, "sugar"));
        }
        for kind in [Sugar::Plain, Sugar::Quote, Sugar::Choice] {
            let path = join("sugar", kind.name());
            match sugar.get(kind.name()) {
                None => {}
                Some(Json::Null) => reg.ignore(kind).map_err(|e| reg_err(e, &path))?,
                Some(Json::String(command)) => {
                    reg.bind(kind, command).map_err(|e| reg_err(e, &path))?
                }
                Some(_) => return Err(err(DeclErrorKind::WrongType, &path)),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
