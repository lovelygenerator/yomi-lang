//! Structural checks of the inputs of save migration (`migrations` and frozen schema files; `spec/language/07-migrate.md`).

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::program::{FrozenSchema, MigrateBlock, MigrateCode, MigrateOp};
use crate::syntax::ast::{Expr, Module, TyName, TypeDeclKind};
use crate::syntax::parser::schema::{
    MigrateStmt, MigrationsAst, SchemaFileAst, VarRef, parse_migrations, parse_schema_file,
};
use crate::type_expand::Expander;

/// The module name shown in diagnostics for the `migrations` module.
pub(super) const MIGRATIONS_MODULE: &str = "migrations";

/// The result of the structural checks.
#[derive(Default)]
pub(super) struct MigrateInfo {
    pub schema_version: u32,
    pub min_save_version: u32,
    migrations: MigrationsAst,
    /// The frozen schemas read without errors, by version.
    frozen: BTreeMap<u32, SchemaFileAst>,
}

impl MigrateInfo {
    /// For compiling without `migrations` (the REPL).
    pub(super) fn none() -> Self {
        MigrateInfo {
            schema_version: 1,
            min_save_version: 1,
            ..MigrateInfo::default()
        }
    }
}

fn error(
    module: &str,
    line: u32,
    col: u32,
    code: DiagCode,
    args: Vec<(&'static str, DiagArg)>,
) -> Diagnostic {
    Diagnostic {
        module: module.to_string(),
        line,
        col,
        severity: Severity::Error,
        code,
        args,
    }
}

fn version_arg(v: i64) -> Vec<(&'static str, DiagArg)> {
    vec![("version", DiagArg::Int(v))]
}

/// Reads the version `N` of a path of the form `schema/vN.yomi` (no leading zeros, at least 1).
fn path_version(path: &str) -> Option<u32> {
    let digits = path.strip_prefix("schema/v")?.strip_suffix(".yomi")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Checks `migrations` and the frozen schema files, and decides the schema version and the lowest supported version.
pub(super) fn check_structure(options: &CompileOptions, out: &mut Vec<Diagnostic>) -> MigrateInfo {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let info = check_into(options, &mut diags);
    // Frozen schema files in path order, then `migrations`; by position within each
    diags.sort_by(|a, b| {
        let key = |d: &Diagnostic| {
            (
                d.module == MIGRATIONS_MODULE,
                d.module.clone(),
                d.line,
                d.col,
            )
        };
        key(a).cmp(&key(b))
    });
    out.append(&mut diags);
    info
}

fn check_into(options: &CompileOptions, diags: &mut Vec<Diagnostic>) -> MigrateInfo {
    let mut info = MigrateInfo {
        schema_version: 1,
        min_save_version: 1,
        ..MigrateInfo::default()
    };
    let (migrations, mut parse_diags) = match &options.migrations {
        Some(src) => parse_migrations(src, MIGRATIONS_MODULE),
        None => Default::default(),
    };
    diags.append(&mut parse_diags);

    let mut seen: HashSet<i64> = HashSet::new();
    for b in &migrations.blocks {
        if b.version < 2 {
            diags.push(error(
                MIGRATIONS_MODULE,
                b.pos.line,
                b.pos.col,
                DiagCode::InvalidMigrateVersion,
                version_arg(b.version),
            ));
        } else if !seen.insert(b.version) {
            diags.push(error(
                MIGRATIONS_MODULE,
                b.pos.line,
                b.pos.col,
                DiagCode::DuplicateMigrate,
                version_arg(b.version),
            ));
        }
    }
    let current = seen.iter().copied().max().unwrap_or(1);
    let current_u32 = u32::try_from(current).unwrap_or(u32::MAX);
    info.schema_version = current_u32;

    let mut min = 1_i64;
    for (i, (v, pos)) in migrations.min_save_version.iter().enumerate() {
        let valid = i == 0 && !migrations.min_not_first && (1..=current).contains(v);
        if valid {
            min = *v;
        } else {
            diags.push(error(
                MIGRATIONS_MODULE,
                pos.line,
                pos.col,
                DiagCode::InvalidMinSaveVersion,
                version_arg(*v),
            ));
        }
    }
    info.min_save_version = u32::try_from(min).unwrap_or(1);

    let mut frozen: BTreeMap<u32, SchemaFileAst> = BTreeMap::new();
    let mut paths: HashSet<&str> = HashSet::new();
    for (path, src) in &options.frozen_schemas {
        let module = path.strip_suffix(".yomi").unwrap_or(path);
        let path_ver = path_version(path);
        let invalid = |line: u32| {
            error(
                module,
                line,
                1,
                DiagCode::InvalidSchemaFile,
                vec![("path", DiagArg::Str(path.clone()))],
            )
        };
        let Some(path_ver) = path_ver else {
            diags.push(invalid(1));
            continue;
        };
        if !paths.insert(path) {
            diags.push(invalid(1));
            continue;
        }
        let (ast, mut d) = parse_schema_file(src, module, path);
        let header_failed = d.iter().any(|x| x.code == DiagCode::InvalidSchemaFile);
        diags.append(&mut d);
        if header_failed {
            continue;
        }
        if ast.version != Some(path_ver) {
            diags.push(invalid(1));
            continue;
        }
        let hash_ok = ast.declared_hash.as_deref() == Some(ast.computed_hash.as_str());
        frozen.insert(path_ver, ast);
        if !hash_ok {
            diags.push(error(
                module,
                1,
                1,
                DiagCode::SchemaHashMismatch,
                version_arg(i64::from(path_ver)),
            ));
        }
    }

    // The latest version's frozen schema is made at release time, so it may be missing
    for k in min..current {
        if !frozen.contains_key(&(k as u32)) {
            diags.push(error(
                MIGRATIONS_MODULE,
                1,
                1,
                DiagCode::MissingFrozenSchema,
                version_arg(k),
            ));
        }
    }
    for k in (min + 1)..=current {
        if !seen.contains(&k) {
            diags.push(error(
                MIGRATIONS_MODULE,
                1,
                1,
                DiagCode::MissingMigrate,
                version_arg(k),
            ));
        }
    }
    let newer: Vec<u32> = frozen
        .keys()
        .copied()
        .filter(|k| i64::from(*k) > current)
        .collect();
    for k in newer {
        diags.push(error(
            MIGRATIONS_MODULE,
            1,
            1,
            DiagCode::MissingMigrate,
            version_arg(i64::from(k)),
        ));
    }
    info.migrations = migrations;
    info.frozen = frozen;
    info
}

/// The variables and type declarations of one version of the schema.
struct Model {
    /// `module.variable` to the variable.
    vars: BTreeMap<String, SchemaVar>,
    /// `(name, signature)` of the `struct`s and `enum`s, as in `TypeDecl::signature`.
    decls: Vec<(String, String)>,
}

struct SchemaVar {
    persistent: bool,
    /// The type as written (declared type names are not expanded).
    ty: String,
}

fn ty_string(t: &TyName) -> String {
    if t.args.is_empty() {
        t.name.clone()
    } else {
        let args: Vec<String> = t.args.iter().map(ty_string).collect();
        format!("{}<{}>", t.name, args.join(", "))
    }
}

pub(crate) fn signature(kind: &TypeDeclKind) -> String {
    match kind {
        TypeDeclKind::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, _, t)| format!("{n}: {}", ty_string(t)))
                .collect();
            if parts.is_empty() {
                "struct {}".to_string()
            } else {
                format!("struct {{ {} }}", parts.join(", "))
            }
        }
        TypeDeclKind::Enum(variants) => {
            let parts: Vec<String> = variants
                .iter()
                .map(|v| {
                    if v.args.is_empty() {
                        v.name.clone()
                    } else {
                        let a: Vec<String> = v.args.iter().map(ty_string).collect();
                        format!("{}({})", v.name, a.join(", "))
                    }
                })
                .collect();
            format!("enum {{ {} }}", parts.join(", "))
        }
    }
}

impl Model {
    fn from_frozen(ast: &SchemaFileAst) -> Model {
        let mut model = Model {
            vars: BTreeMap::new(),
            decls: Vec::new(),
        };
        for m in &ast.modules {
            for t in &m.types {
                model
                    .decls
                    .push((format!("{}.{}", m.path, t.name), signature(&t.kind)));
            }
            for v in &m.vars {
                model.vars.insert(
                    format!("{}.{}", m.path, v.name),
                    SchemaVar {
                        persistent: v.persistent,
                        ty: ty_string(&v.ty),
                    },
                );
            }
        }
        model
    }

    /// The type string with the declared types expanded, so that types are compared by what they contain.
    fn expand(&self, ty: &str) -> String {
        Expander::new(&self.decls).expand(ty)
    }
}

/// The name as written in a `migrate` block (`v2::game.hp`).
fn written(r: &VarRef) -> String {
    let mut out = String::new();
    if let Some(v) = r.version {
        out.push_str(&format!("v{v}::"));
    }
    if !r.module.is_empty() {
        out.push_str(&r.module);
        out.push('.');
    }
    out.push_str(&r.name);
    out
}

/// One side of a `migrate` block: the schema and the version prefix names on that side must have (`None`: no prefix, for the
/// current scripts).
struct Side<'a> {
    model: &'a Model,
    prefix: Option<u32>,
}

impl Side<'_> {
    /// Looks up a variable named in the block. Returns the key (`module.variable`) and the variable.
    fn find(&self, r: &VarRef) -> Option<(String, &SchemaVar)> {
        if r.version != self.prefix || r.module.is_empty() {
            return None;
        }
        let key = format!("{}.{}", r.module, r.name);
        self.model.vars.get(&key).map(|v| (key, v))
    }

    /// A type written in the block, without the version prefix. The flag is false when a user-defined type has a wrong or
    /// a missing prefix (the type can then never be equal to the schema's).
    fn ty(&self, t: &TyName) -> (String, bool) {
        let mut ok = true;
        let text = self.ty_into(t, &mut ok);
        (text, ok)
    }

    fn ty_into(&self, t: &TyName, ok: &mut bool) -> String {
        let mut name = t.name.as_str();
        let versioned = name
            .split_once("::")
            .filter(|(v, _)| crate::syntax::expr::is_version_name(v));
        match (versioned, self.prefix) {
            (Some((v, rest)), Some(p)) if v[1..].parse::<u32>().ok() == Some(p) => name = rest,
            (Some(_), _) => *ok = false,
            (None, Some(_)) if name.contains('.') => *ok = false,
            _ => {}
        }
        if t.args.is_empty() {
            name.to_string()
        } else {
            let args: Vec<String> = t.args.iter().map(|a| self.ty_into(a, ok)).collect();
            format!("{name}<{}>", args.join(", "))
        }
    }
}

fn unknown_name(module: &str, r: &VarRef, side: &Side<'_>, version: u32, pos: Pos) -> Diagnostic {
    let mut args = vec![
        ("name", DiagArg::Str(written(r))),
        ("version", DiagArg::Int(i64::from(version))),
    ];
    let key = format!("{}.{}", r.module, r.name);
    let found = crate::suggest::suggest(&key, side.model.vars.keys().map(String::as_str));
    if !found.is_empty() {
        args.push(("suggestions", DiagArg::List(found)));
    }
    error(module, pos.line, pos.col, DiagCode::UnknownSchemaName, args)
}

/// The compiled result of the `migrations` module.
#[derive(Default)]
pub(super) struct Planned {
    pub code: MigrateCode,
    /// The hidden labels that evaluate the `convert` expressions (to be put after the scripts' labels).
    pub labels: Vec<LabelCode>,
}

fn mod_decl(name: &str) -> ModDecl {
    ModDecl {
        name: name.to_string(),
        global_map: HashMap::new(),
        label_map: HashMap::new(),
        fn_map: HashMap::new(),
        identities: HashMap::new(),
        early_identities: HashMap::new(),
        poisoned: HashSet::new(),
        imports: HashMap::new(),
        pkg_imports: HashSet::new(),
        bad_imports: HashSet::new(),
        type_map: HashMap::new(),
    }
}

impl Model {
    fn from_compiler(c: &Compiler<'_>) -> Model {
        Model {
            vars: c
                .globals
                .iter()
                .map(|g| {
                    (
                        format!("{}.{}", c.modules[g.module as usize].name, g.name),
                        SchemaVar {
                            persistent: g.persistent,
                            ty: g.ty.name(),
                        },
                    )
                })
                .collect(),
            decls: c
                .types
                .iter()
                .map(|t| (t.name.clone(), t.signature()))
                .collect(),
        }
    }
}

/// Registers the types of a frozen schema as versioned copies (`v2::game.Item`) so that the expressions of `migrate` blocks
/// can name them. Returns the names (`game.Item`) of the types that got a copy. Modules of registered packages are not copied
/// (their types are shared with the current scripts).
fn register_old_types(c: &mut Compiler<'_>, version: u32, ast: &SchemaFileAst) -> Vec<String> {
    let mut made: Vec<(usize, &crate::syntax::parser::schema::SchemaModuleAst)> = Vec::new();
    for m in ast
        .modules
        .iter()
        .filter(|m| !c.registry.has_namespace(&m.path))
    {
        let name = format!("v{version}::{}", m.path);
        let idx = c.modules.len();
        c.modules.push(mod_decl(&name));
        c.module_index.insert(name, idx);
        made.push((idx, m));
    }
    let same_version: HashMap<String, usize> =
        made.iter().map(|(i, m)| (m.path.clone(), *i)).collect();
    let packages: HashSet<String> = ast
        .modules
        .iter()
        .filter(|m| c.registry.has_namespace(&m.path))
        .map(|m| m.path.clone())
        .collect();
    for (idx, _) in &made {
        c.modules[*idx].imports = same_version.clone();
        c.modules[*idx].pkg_imports = packages.clone();
    }
    let asts: Vec<Module> = made
        .iter()
        .map(|(_, m)| Module {
            types: m.types.clone(),
            ..Module::default()
        })
        .collect();
    let mut slots = Vec::new();
    for ((idx, _), ast) in made.iter().zip(&asts) {
        c.cur_mod = *idx;
        slots.push(c.declare_type_names(ast));
    }
    for (((idx, _), ast), slot) in made.iter().zip(&asts).zip(&slots) {
        c.cur_mod = *idx;
        c.resolve_type_decls(ast, slot);
    }
    made.iter()
        .flat_map(|(_, m)| m.types.iter().map(|t| format!("{}.{}", m.path, t.name)))
        .collect()
}

/// Checks the `migrate` blocks against the frozen schemas and the current scripts, and warns about changes that no block
/// handles (`spec/language/07-migrate.md`, 7.4 and 7.5). Compiles the `convert` expressions. Runs after the scripts' labels
/// and `fn`s are compiled. `base_label` is the number of labels of the scripts.
pub(super) fn plan(
    info: &MigrateInfo,
    c: &mut Compiler<'_>,
    base_label: usize,
    found: &mut Vec<Diagnostic>,
) -> Planned {
    let current = info.schema_version;
    let script = Model::from_compiler(c);
    let frozen: BTreeMap<u32, Model> = info
        .frozen
        .iter()
        .map(|(v, ast)| (*v, Model::from_frozen(ast)))
        .collect();
    let mut out = Planned::default();
    for (v, m) in &frozen {
        out.code.schemas.insert(
            *v,
            FrozenSchema {
                decls: m.decls.clone(),
                versioned: Vec::new(),
            },
        );
    }
    let blocks: Vec<_> = {
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        info.migrations
            .blocks
            .iter()
            .filter(|b| b.version >= 2 && seen.insert(b.version))
            .collect()
    };
    // The blocks whose schemas are available; the old side is `v{n-1}`, the new side the frozen `v{n}` or the scripts
    let usable: Vec<_> = blocks
        .iter()
        .filter(|b| frozen.contains_key(&(b.version as u32 - 1)))
        .collect();
    // Versioned copies of types are needed for the blocks that have a `convert`
    let mut needed: BTreeSet<u32> = BTreeSet::new();
    for b in &usable {
        let n = b.version as u32;
        if b.stmts
            .iter()
            .any(|(_, st)| matches!(st, MigrateStmt::Convert { .. }))
        {
            needed.insert(n - 1);
            if frozen.contains_key(&n) {
                needed.insert(n);
            }
        }
    }
    let prev_mod = c.cur_mod;
    let migrate_idx = c.modules.len();
    if !needed.is_empty() {
        c.modules.push(mod_decl(MIGRATIONS_MODULE));
        let mut imports: HashMap<String, usize> = c
            .module_index
            .iter()
            .filter(|(_, i)| **i < migrate_idx)
            .map(|(p, i)| (p.clone(), *i))
            .collect();
        for k in &needed {
            let versioned = register_old_types(c, *k, &info.frozen[k]);
            if let Some(f) = out.code.schemas.get_mut(k) {
                f.versioned = versioned;
            }
        }
        for (name, i) in c.module_index.iter().filter(|(_, i)| **i > migrate_idx) {
            imports.insert(name.clone(), *i);
        }
        let packages: HashSet<String> = c.types[..c.registry_types]
            .iter()
            .filter_map(|t| t.name.rsplit_once('.').map(|(ns, _)| ns.to_string()))
            .collect();
        c.modules[migrate_idx].imports = imports;
        c.modules[migrate_idx].pkg_imports = packages;
    }
    let mut label_no = 0;
    for block in usable {
        let n = block.version as u32;
        let old_model = &frozen[&(n - 1)];
        let new_frozen = frozen.contains_key(&n);
        let new_model = frozen.get(&n).unwrap_or(&script);
        let old = Side {
            model: old_model,
            prefix: Some(n - 1),
        };
        let new = Side {
            model: new_model,
            prefix: new_frozen.then_some(n),
        };
        let m = MIGRATIONS_MODULE;
        let mut ops: Vec<MigrateOp> = Vec::new();
        let mut handled_old: BTreeSet<String> = BTreeSet::new();
        let mut created: BTreeSet<String> = BTreeSet::new();
        for (pos, stmt) in &block.stmts {
            let first = found.len();
            let (old_ref, new_ref, old_ty, new_ty) = match stmt {
                MigrateStmt::Rename { old, new } => (old, Some(new), None, None),
                MigrateStmt::Convert {
                    old,
                    old_ty,
                    new,
                    new_ty,
                    ..
                } => (old, new.as_ref(), Some(old_ty), Some(new_ty)),
                MigrateStmt::Drop { old } => (old, None, None, None),
            };
            let found_old = old.find(old_ref);
            if found_old.is_none() {
                found.push(unknown_name(m, old_ref, &old, n - 1, old_ref.pos));
            }
            // `convert` without a new name keeps the name
            let same_name;
            let new_ref = match (new_ref, stmt) {
                (Some(r), _) => Some(r),
                (None, MigrateStmt::Convert { old, .. }) => {
                    same_name = VarRef {
                        version: new.prefix,
                        ..old.clone()
                    };
                    Some(&same_name)
                }
                (None, _) => None,
            };
            let found_new = new_ref.and_then(|r| {
                let f = new.find(r);
                if f.is_none() {
                    found.push(unknown_name(m, r, &new, n, r.pos));
                }
                f
            });
            if let Some((key, _)) = &found_old
                && !handled_old.insert(key.clone())
            {
                found.push(error(
                    m,
                    old_ref.pos.line,
                    old_ref.pos.col,
                    DiagCode::DuplicateMigrateTarget,
                    vec![("name", DiagArg::Str(written(old_ref)))],
                ));
            }
            if let (Some((key, _)), Some(r)) = (&found_new, new_ref)
                && !created.insert(key.clone())
            {
                found.push(error(
                    m,
                    r.pos.line,
                    r.pos.col,
                    DiagCode::DuplicateMigrateTarget,
                    vec![("name", DiagArg::Str(written(r)))],
                ));
            }
            if let (Some((_, v)), Some(t)) = (&found_old, old_ty) {
                type_check(m, *pos, old_ref, &old, v, t, found);
            }
            if let (Some((_, v)), Some(t), Some(r)) = (&found_new, new_ty, new_ref) {
                type_check(m, *pos, r, &new, v, t, found);
            }
            if let (Some((_, old_var)), Some((_, new_var))) = (&found_old, &found_new) {
                if old_var.persistent != new_var.persistent {
                    found.push(error(
                        m,
                        pos.line,
                        pos.col,
                        DiagCode::MigrateKindMismatch,
                        vec![("name", DiagArg::Str(written(old_ref)))],
                    ));
                }
                if let MigrateStmt::Rename { new: new_r, .. } = stmt {
                    let (a, b) = (old.model.expand(&old_var.ty), new.model.expand(&new_var.ty));
                    if a != b {
                        found.push(error(
                            m,
                            pos.line,
                            pos.col,
                            DiagCode::MigrateTypeMismatch,
                            vec![
                                ("name", DiagArg::Str(written(new_r))),
                                ("expected", DiagArg::Str(a)),
                                ("found", DiagArg::Str(b)),
                            ],
                        ));
                    }
                }
            }
            if found[first..].iter().any(|d| d.severity == Severity::Error) {
                continue;
            }
            let (Some((old_key, old_var)), found_new_opt) = (found_old, found_new) else {
                continue;
            };
            match stmt {
                MigrateStmt::Drop { .. } => ops.push(MigrateOp::Drop {
                    old: old_key,
                    persistent: old_var.persistent,
                }),
                MigrateStmt::Rename { .. } => {
                    if let Some((new_key, _)) = found_new_opt {
                        ops.push(MigrateOp::Rename {
                            old: old_key,
                            new: new_key,
                            persistent: old_var.persistent,
                        });
                    }
                }
                MigrateStmt::Convert {
                    old_ty,
                    new_ty,
                    lambda,
                    ..
                } => {
                    let Some((new_key, _)) = found_new_opt else {
                        continue;
                    };
                    let Some(label) = compile_convert(
                        c,
                        migrate_idx,
                        &format!("$migrate.{n}.{label_no}"),
                        (old_ty, new_ty),
                        lambda,
                        *pos,
                    ) else {
                        continue;
                    };
                    label_no += 1;
                    ops.push(MigrateOp::Convert {
                        old: old_key,
                        new: new_key,
                        persistent: old_var.persistent,
                        old_ty: old.ty(old_ty).0,
                        new_ty: new.ty(new_ty).0,
                        label: base_label + out.labels.len(),
                        new_frozen,
                    });
                    out.labels.push(label);
                }
            }
        }
        out.code.blocks.push(MigrateBlock { version: n, ops });
        // Variables whose type changed and that the block does not handle
        for (key, old_var) in &old.model.vars {
            let Some(new_var) = new.model.vars.get(key) else {
                continue;
            };
            if !handled_old.contains(key)
                && old.model.expand(&old_var.ty) != new.model.expand(&new_var.ty)
            {
                let mut d = error(
                    m,
                    block.pos.line,
                    block.pos.col,
                    DiagCode::UnhandledTypeChange,
                    vec![
                        ("name", DiagArg::Str(key.clone())),
                        ("version", DiagArg::Int(i64::from(n))),
                    ],
                );
                d.severity = Severity::Warning;
                found.push(d);
            }
        }
    }
    c.cur_mod = prev_mod;
    // The latest frozen schema differs from the current scripts, with no newer `migrate`
    if let Some((&latest, model)) = frozen.iter().next_back()
        && latest == current
        && schema_differs(model, &script)
    {
        let mut d = error(
            &format!("schema/v{latest}"),
            1,
            1,
            DiagCode::SchemaChangedSinceFreeze,
            version_arg(i64::from(latest)),
        );
        d.severity = Severity::Warning;
        found.push(d);
    }
    out
}

/// Compiles the lambda of a `convert` into a hidden label. The label's parameter 0 is the old value; running it leaves the new
/// value on the stack. `None` if the types or the expression have errors (diagnostics were emitted).
fn compile_convert(
    c: &mut Compiler<'_>,
    module: usize,
    name: &str,
    (old_ast, new_ast): (&TyName, &TyName),
    lambda: &Expr,
    pos: Pos,
) -> Option<LabelCode> {
    c.cur_mod = module;
    let (old_ty, new_ty) = (c.ty_from_name(old_ast), c.ty_from_name(new_ast));
    let (old_ty, new_ty) = (old_ty?, new_ty?);
    let Expr::Lambda(l) = lambda else {
        return None;
    };
    let errors_before = c
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    c.cur = Some(LabelCtx {
        ir: Ir::default(),
        locals: Vec::new(),
        scopes: vec![HashMap::new()],
        ids: HashSet::new(),
        refs: LabelRefs::default(),
        loop_depth: 0,
        name: name.to_string(),
        ret: RetTy::None,
        in_fn: false,
        live: Default::default(),
        lets: Default::default(),
    });
    let ctx = c.ctx();
    ctx.locals.push(LocalInfo {
        name: "$arg".to_string(),
        ty: old_ty.clone(),
    });
    ctx.scopes[0].insert("$arg".to_string(), (0, Some(old_ty.clone())));
    c.migrate_expr = true;
    let (got, callee) = c.lambda_expr(l, Some(std::slice::from_ref(&old_ty)), Some(&new_ty));
    c.migrate_expr = false;
    let line = l.pos.line;
    let arg = c.node(ExprKind::Var(PlaceRef::Local(0)), line);
    let call = c.node(
        ExprKind::CallValue {
            callee,
            args: vec![arg],
        },
        line,
    );
    let ctx = c.cur.take().expect("ラベルの途中");
    let errors_after = c
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    if got.is_none() || errors_after > errors_before {
        return None;
    }
    Some(c.expr_label(ctx, call, name.to_string(), pos, 1))
}

/// Compares a type written in a `convert` with the schema's type of the variable.
fn type_check(
    module: &str,
    pos: Pos,
    r: &VarRef,
    side: &Side<'_>,
    var: &SchemaVar,
    written_ty: &TyName,
    out: &mut Vec<Diagnostic>,
) {
    let (text, ok) = side.ty(written_ty);
    let expected = side.model.expand(&var.ty);
    let found = side.model.expand(&text);
    if !ok || found != expected {
        out.push(error(
            module,
            pos.line,
            pos.col,
            DiagCode::MigrateTypeMismatch,
            vec![
                ("name", DiagArg::Str(written(r))),
                ("expected", DiagArg::Str(expected)),
                ("found", DiagArg::Str(found)),
            ],
        ));
    }
}

/// Whether the variables (names, kinds and expanded types) of two schemas differ.
fn schema_differs(a: &Model, b: &Model) -> bool {
    a.vars.len() != b.vars.len()
        || a.vars.iter().any(|(k, va)| {
            b.vars.get(k).is_none_or(|vb| {
                va.persistent != vb.persistent || a.expand(&va.ty) != b.expand(&vb.ty)
            })
        })
}
