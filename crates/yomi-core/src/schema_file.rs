//! Frozen schema files (`yomi schema freeze`, `yomi schema extract`) and the template of `migrate` blocks
//! (`yomi migrate-init`); sections 7.3 and 6.14 of the spec.

use std::collections::BTreeMap;

use crate::diag::{Diagnostic, Severity};
use crate::program::Program;
use crate::syntax::parser::schema::{fnv1a_bytes, parse_schema_file};
use crate::type_expand::Expander;
use crate::vm::{LoadError, persistent, save};

/// The schema inside a save or persistent data.
pub(crate) struct Embedded {
    pub version: u32,
    pub types: Vec<(String, String)>,
    /// The script variables (`module.variable`, type).
    pub var_types: Vec<(String, String)>,
}

/// A variable of a schema.
struct Var {
    name: String,
    persistent: bool,
    ty: String,
}

fn split_name(name: &str) -> (&str, &str) {
    name.rsplit_once('.').unwrap_or(("", name))
}

/// `struct { a: int }` of the type `game.Item` becomes `struct Item { a: int }`.
fn declaration(name: &str, signature: &str) -> String {
    let (kind, rest) = signature.split_once(' ').unwrap_or((signature, ""));
    format!("{kind} {} {rest}", split_name(name).1)
}

/// The declarations and the variables of one module (`name`, text).
type ModuleItems<'a> = (Vec<(&'a str, String)>, Vec<(&'a str, String)>);

/// Writes a frozen schema file (7.3). `types` are `(module.Type, shape)`.
fn render(version: u32, types: &[(String, String)], vars: &[Var]) -> String {
    let mut modules: BTreeMap<&str, ModuleItems<'_>> = BTreeMap::new();
    for (name, sig) in types {
        let (module, short) = split_name(name);
        modules
            .entry(module)
            .or_default()
            .0
            .push((short, declaration(name, sig)));
    }
    for v in vars {
        let (module, short) = split_name(&v.name);
        let kind = if v.persistent { "persistent" } else { "save" };
        modules
            .entry(module)
            .or_default()
            .1
            .push((short, format!("{kind} var {short}: {}", v.ty)));
    }
    let mut body = String::from("\n");
    for (module, (mut decls, mut vars)) in modules {
        decls.sort();
        vars.sort();
        body.push_str(&format!("module {module} {{\n"));
        for (_, text) in decls.iter().chain(&vars) {
            body.push_str(&format!("    {text}\n"));
        }
        body.push_str("}\n");
    }
    format!(
        "schema {version} hash \"{:016x}\"\n{body}",
        fnv1a_bytes(body.as_bytes())
    )
}

fn program_vars(program: &Program) -> Vec<Var> {
    program
        .globals
        .iter()
        .map(|g| Var {
            name: format!("{}.{}", program.modules[g.module as usize], g.name),
            persistent: g.persistent,
            ty: g.ty.name(),
        })
        .collect()
}

fn program_types(program: &Program) -> Vec<(String, String)> {
    program
        .types
        .iter()
        .map(|t| (t.name.clone(), t.signature()))
        .collect()
}

/// The content of `schema/vN.yomi` for the current scripts (`N` is [`Program::schema_version`]).
pub fn frozen_schema(program: &Program) -> String {
    render(
        program.schema_version(),
        &program_types(program),
        &program_vars(program),
    )
}

/// Why the schema could not be taken out of a save.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    /// Neither a save nor persistent data was given.
    NothingGiven,
    /// The bytes cannot be read (bad magic, old format version, bad checksum, ...).
    Load(LoadError),
    /// The save and the persistent data have different schema versions.
    VersionsDiffer {
        /// The schema version of the save.
        save: u32,
        /// The schema version of the persistent data.
        persistent: u32,
    },
}

/// Rebuilds the content of a frozen schema file from the schema embedded in a save and/or persistent data. The version in
/// the header is the schema version of the save.
pub fn extract_schema(
    save: Option<&[u8]>,
    persistent: Option<&[u8]>,
) -> Result<String, ExtractError> {
    let s = save
        .map(save::embedded_schema)
        .transpose()
        .map_err(ExtractError::Load)?;
    let p = persistent
        .map(persistent::embedded_schema)
        .transpose()
        .map_err(ExtractError::Load)?;
    let (version, mut types, mut vars) = match (&s, &p) {
        (None, None) => return Err(ExtractError::NothingGiven),
        (Some(s), Some(p)) if s.version != p.version => {
            return Err(ExtractError::VersionsDiffer {
                save: s.version,
                persistent: p.version,
            });
        }
        (Some(s), _) => (s.version, Vec::new(), Vec::new()),
        (None, Some(p)) => (p.version, Vec::new(), Vec::new()),
    };
    for (e, is_persistent) in [(&s, false), (&p, true)] {
        let Some(e) = e else { continue };
        for t in &e.types {
            if !types.contains(t) {
                types.push(t.clone());
            }
        }
        vars.extend(e.var_types.iter().map(|(name, ty)| Var {
            name: name.clone(),
            persistent: is_persistent,
            ty: ty.clone(),
        }));
    }
    Ok(render(version, &types, &vars))
}

/// Writes the type with the names of its declared types prefixed by `v<version>::`.
fn prefixed(ty: &str, version: u32, decls: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        if decls.iter().any(|(n, _)| n == token) {
            out.push_str(&format!("v{version}::"));
        }
        out.push_str(token);
        token.clear();
    };
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '_' || c == '.' || c == ':' {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

/// The `migrate` block that `yomi migrate-init` makes from the latest frozen schema (`frozen`, the content of
/// `schema/v<version>.yomi`) and the current scripts: every line inside is a comment (candidates for `rename` and
/// `convert`). `None` if the variables did not change. If the frozen schema cannot be read, its diagnostics.
pub fn migrate_template(
    program: &Program,
    frozen: &str,
    version: u32,
) -> Result<Option<String>, Vec<Diagnostic>> {
    let module = format!("schema/v{version}");
    let (ast, diags) = parse_schema_file(frozen, &module, &format!("{module}.yomi"));
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(diags);
    }
    let mut old_types: Vec<(String, String)> = Vec::new();
    let mut old_vars: Vec<Var> = Vec::new();
    for m in &ast.modules {
        for t in &m.types {
            old_types.push((
                format!("{}.{}", m.path, t.name),
                crate::compile::signature_of(&t.kind),
            ));
        }
        for v in &m.vars {
            old_vars.push(Var {
                name: format!("{}.{}", m.path, v.name),
                persistent: v.persistent,
                ty: crate::syntax::canon::ty(&v.ty),
            });
        }
    }
    let new_types = program_types(program);
    let new_vars = program_vars(program);
    let old_x = Expander::new(&old_types);
    let new_x = Expander::new(&new_types);
    let mut lines: Vec<String> = Vec::new();
    let removed: Vec<&Var> = old_vars
        .iter()
        .filter(|o| !new_vars.iter().any(|n| n.name == o.name))
        .collect();
    let added: Vec<&Var> = new_vars
        .iter()
        .filter(|n| !old_vars.iter().any(|o| o.name == n.name))
        .collect();
    for r in &removed {
        for a in &added {
            if r.persistent == a.persistent && old_x.expand(&r.ty) == new_x.expand(&a.ty) {
                lines.push(format!("// rename v{version}::{} -> {}", r.name, a.name));
            }
        }
    }
    for o in &old_vars {
        let Some(n) = new_vars.iter().find(|n| n.name == o.name) else {
            continue;
        };
        if o.persistent == n.persistent && old_x.expand(&o.ty) != new_x.expand(&n.ty) {
            lines.push(format!(
                "// convert v{version}::{}: {} -> {} = |v| v",
                o.name,
                prefixed(&o.ty, version, &old_types),
                n.ty
            ));
        }
    }
    let differs = !removed.is_empty() || !added.is_empty() || !lines.is_empty();
    if !differs {
        return Ok(None);
    }
    let mut out = format!("migrate {} {{\n", version + 1);
    for l in lines {
        out.push_str(&format!("    {l}\n"));
    }
    out.push_str("}\n");
    Ok(Some(out))
}
