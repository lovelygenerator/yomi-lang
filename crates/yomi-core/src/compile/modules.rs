//! The body of compiling multiple modules.

use super::*;

/// The body of [`compile_modules_impl`]. When `repl` is given, compiles with the REPL module (`<repl>`; the syntax trees
/// of declarations and input are held by `repl`) added after `modules` (the script's modules) (`spec/host/07-repl.md`).
pub(super) fn compile_modules_core(
    modules: &[(&str, &str)],
    registry: &Registry,
    options: &CompileOptions,
    failed_sites: &mut Vec<IdSite>,
    local_types: &mut Vec<LocalType>,
    repl: Option<&repl::ReplCompile<'_>>,
    outcome: &mut Option<repl::ReplOutcome>,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    // Module names are handled after normalizing to NFC (because file systems normalize file names differently)
    let normalized: Vec<(String, &str)> = modules
        .iter()
        .map(|(n, src)| (crate::ident::normalize_name(n), *src))
        .collect();
    let mut modules: Vec<(&str, &str)> = normalized.iter().map(|(n, s)| (n.as_str(), *s)).collect();
    let n_base = modules.len();
    let repl_idx = repl.map(|_| n_base);
    if repl.is_some() {
        modules.push((repl::REPL_MODULE, ""));
    }
    let modules = &modules[..];
    let (mut asts, mut diags) = parse_modules(&modules[..n_base]);
    let repl_cur: Option<usize> = repl.and_then(|rc| {
        rc.cur_module
            .and_then(|m| modules[..n_base].iter().position(|(n, _)| *n == m))
    });
    if let Some(rc) = repl {
        asts.push(repl::build_ast(rc, &asts, repl_cur));
    }
    let (module_index, decls) = declare_modules(modules, repl_idx, registry, &asts, &mut diags);
    let mut migrate_diags: Vec<Diagnostic> = Vec::new();
    let migrate_info = if repl.is_some() {
        migrate::MigrateInfo::none()
    } else {
        migrate::check_structure(options, &mut migrate_diags)
    };
    let mut label_base = Vec::new();
    let mut fn_base = Vec::new();
    let mut total_labels = 0;
    let mut total_fns = 0;
    for ast in &asts {
        label_base.push(total_labels);
        total_labels += ast.labels.len();
        fn_base.push(total_fns);
        total_fns += ast.fns.len();
    }
    let mut module_last: HashMap<String, usize> = HashMap::new();
    for (path, &i) in &module_index {
        let e = module_last
            .entry(last_segment(path).to_string())
            .or_insert(i);
        *e = (*e).min(i);
    }
    let mut c = Compiler {
        registry,
        diags: Vec::new(),
        modules: decls,
        module_index,
        used_imports: std::cell::RefCell::new(HashSet::new()),
        import_sites: Vec::new(),
        module_last,
        cur_mod: 0,
        globals: Vec::new(),
        sigs: vec![Sig::default(); total_labels],
        fn_sigs: vec![Sig::default(); total_fns],
        commands: Vec::new(),
        natives: Vec::new(),
        host_vars: Vec::new(),
        cur: None,
        literals: Vec::new(),
        types: registry.types().to_vec(),
        registry_types: registry.types().len(),
        types_ready: false,
        pending_keys: Vec::new(),
        require_text_ids: options.require_text_ids,
        id_sites: Vec::new(),
        local_types: Vec::new(),
        repl: None,
        reg_overflow: Vec::new(),
        unused_vars: Vec::new(),
        fn_total: total_fns as u32,
        lambdas: Vec::new(),
        hofs: HashMap::new(),
        migrate_expr: false,
    };
    if let (Some(rc), Some(idx)) = (repl, repl_idx) {
        c.repl = Some(repl::ReplState::new(rc, idx, repl_cur));
    }
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        c.repl_seed(repl::Seed::Imports);
        c.declare_imports(ast);
    }
    let mut type_slots: Vec<Vec<Option<u32>>> = Vec::new();
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        c.repl_seed(repl::Seed::Types);
        type_slots.push(c.declare_type_names(ast));
    }
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        c.resolve_type_decls(ast, &type_slots[m]);
    }
    c.check_type_graph(&asts, &type_slots);
    c.types_ready = true;
    for (m, pos, key) in std::mem::take(&mut c.pending_keys) {
        c.cur_mod = m;
        if !key.is_key_in(&c.types) {
            c.error(pos, DiagCode::InvalidMapKey, vec![("type", s(&key.name()))]);
        }
    }
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        c.repl_seed(repl::Seed::EarlyIdentities);
        for d in &ast.identities {
            c.modules[m]
                .early_identities
                .entry(d.name.clone())
                .or_insert_with(|| d.display.clone());
        }
    }
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        c.repl_seed(repl::Seed::Decls);
        c.declare(ast, label_base[m], fn_base[m]);
    }
    let mut labels: Vec<LabelCode> = Vec::new();
    let mut refs: Vec<LabelRefs> = Vec::new();
    let mut falls: Vec<bool> = Vec::new();
    let mut label_pos: Vec<(usize, Pos)> = Vec::new();
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        for (i, l) in ast.labels.iter().enumerate() {
            let (code, falls_through, label_refs) = c.label(l, (label_base[m] + i) as u32);
            labels.push(code);
            refs.push(label_refs);
            falls.push(falls_through);
            label_pos.push((m, l.pos));
        }
    }
    // `fn` (compiled after labels; `fn`s do not affect the order of label diagnostics)
    let mut fns: Vec<FnCode> = Vec::new();
    for (m, ast) in asts.iter().enumerate() {
        c.cur_mod = m;
        for (i, f) in ast.fns.iter().enumerate() {
            fns.push(c.fn_code(f, (fn_base[m] + i) as u32));
        }
    }
    let mut migrate_labels: Vec<LabelCode> = Vec::new();
    let mut migrate_code = MigrateCode::default();
    if repl.is_none() {
        let n_types = c.types.len();
        let planned = migrate::plan(&migrate_info, &mut c, labels.len(), &mut migrate_diags);
        // The versioned copies of old types were only for compiling the expressions
        c.types.truncate(n_types);
        migrate_labels = planned.labels;
        migrate_code = planned.code;
    }
    fns.extend(
        std::mem::take(&mut c.lambdas)
            .into_iter()
            .map(|f| f.expect("ラムダ式の fn は、作り終えている")),
    );
    c.check_literals(&labels, &refs);
    if !c.diags.iter().any(|d| d.severity == Severity::Error) {
        for (m, pos) in std::mem::take(&mut c.reg_overflow) {
            c.cur_mod = m;
            c.error(
                pos,
                DiagCode::TooManyRegisters,
                vec![("limit", DiagArg::Int(i64::from(MAX_REGS)))],
            );
        }
    }
    // When there are errors the usage records are cut off partway, so only warn when there are no errors
    if !c.diags.iter().any(|d| d.severity == Severity::Error) {
        c.warn_unused_imports();
        c.warn_unused_variables();
    }
    // The warning for labels that are never `call`ed or `jump`ed to, are not the start label, and may reach the end
    let mut referenced: HashSet<u32> = HashSet::new();
    for r in &refs {
        referenced.extend(&r.targets);
    }
    let single_module = modules.len() == 1;
    let is_entry = |m: usize, name: &str| {
        if options.entry_labels.is_empty() {
            return name == "start";
        }
        options
            .entry_labels
            .iter()
            .any(|e| match e.split_once('.') {
                Some((em, el)) => el == name && modules[m].0 == em,
                None => single_module && e == name,
            })
    };
    for flat in 0..labels.len() {
        let (m, pos) = label_pos[flat];
        if Some(m) == repl_idx {
            continue;
        }
        // When a label with a return type may reach the end, it is already an error (`missing_return`)
        if matches!(c.sigs[flat].ret, RetTy::None)
            && falls[flat]
            && !referenced.contains(&(flat as u32))
            && !is_entry(m, &labels[flat].name)
        {
            c.cur_mod = m;
            c.warn(
                pos,
                DiagCode::LabelFallsThrough,
                vec![("name", s(&labels[flat].name))],
            );
        }
    }
    diags.append(&mut c.diags);
    diags.append(&mut migrate_diags);
    sort_diagnostics(&mut diags, modules);
    let module_names: Vec<String> = modules.iter().map(|(n, _)| (*n).to_string()).collect();
    // The code of `convert` expressions is attributed to the first module (the `migrations` module is not in the program)
    for l in &mut migrate_labels {
        l.module = 0;
    }
    for f in &mut fns {
        if f.module as usize >= module_names.len() {
            f.module = 0;
        }
    }
    labels.extend(migrate_labels);
    *local_types = c
        .local_types
        .iter()
        .filter(|(m, _, _)| *m < modules.len())
        .map(|(m, pos, ty)| LocalType {
            module: modules[*m].0.to_string(),
            line: pos.line,
            col: pos.col,
            ty: ty.clone(),
        })
        .collect();
    if diags.iter().any(|d| d.severity == Severity::Error) {
        *failed_sites = c.id_sites;
        return Err(diags);
    }
    if let Some(r) = c.repl.take() {
        let flat = label_base[r.idx];
        *outcome = Some(r.finish(flat));
    }
    let program = Program {
        modules: module_names,
        globals: c.globals,
        labels,
        fns,
        commands: c.commands,
        natives: c.natives,
        host_vars: c.host_vars,
        types: c.types,
        id_sites: c.id_sites,
        identities: collect_identities(&asts),
        sources: modules[..n_base]
            .iter()
            .map(|(_, src)| (*src).to_string())
            .collect(),
        schema_version: migrate_info.schema_version,
        min_save_version: migrate_info.min_save_version,
        migrate: migrate_code,
    };
    Ok((program, diags))
}

/// To classify lines (whether `name「` is a quote line), first picks up the identity names of all modules, then parses the
/// modules.
fn parse_modules(modules: &[(&str, &str)]) -> (Vec<Module>, Vec<Diagnostic>) {
    let mut asts: Vec<Module> = Vec::new();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut idents = IdentMap::new();
    for (name, source) in modules {
        idents
            .entry((*name).to_string())
            .or_insert_with(|| scan_identities(source));
    }
    for (name, source) in modules {
        let (ast, mut d) = parse_with(source, name, &idents);
        diags.append(&mut d);
        asts.push(ast);
    }
    (asts, diags)
}

/// Checks module names, and makes a table to look them up by name and the containers for declarations. Only the first
/// module with a valid name can be looked up by name.
fn declare_modules(
    modules: &[(&str, &str)],
    repl_idx: Option<usize>,
    registry: &Registry,
    asts: &[Module],
    diags: &mut Vec<Diagnostic>,
) -> (HashMap<String, usize>, Vec<ModDecl>) {
    let mut module_index: HashMap<String, usize> = HashMap::new();
    let mut decls: Vec<ModDecl> = Vec::new();
    for (i, (name, _)) in modules.iter().enumerate() {
        let problem = if Some(i) == repl_idx {
            None
        } else if !is_module_name(name) {
            Some(DiagCode::InvalidModuleName)
        } else if module_index.contains_key(*name) || registry.has_namespace(name) {
            // Packages (and namespaces in which the host declared variables) and module names cannot collide
            Some(DiagCode::DuplicateModule)
        } else {
            module_index.insert((*name).to_string(), i);
            None
        };
        if let Some(code) = problem {
            diags.push(Diagnostic {
                module: (*name).to_string(),
                line: 1,
                col: 1,
                severity: Severity::Error,
                code,
                args: vec![("name", s(name))],
            });
        }
        decls.push(ModDecl {
            name: (*name).to_string(),
            global_map: HashMap::new(),
            label_map: HashMap::new(),
            fn_map: HashMap::new(),
            identities: HashMap::new(),
            early_identities: HashMap::new(),
            poisoned: asts[i].poisoned.clone(),
            imports: HashMap::new(),
            pkg_imports: HashSet::new(),
            bad_imports: HashSet::new(),
            type_map: HashMap::new(),
        });
    }
    (module_index, decls)
}

/// Sorts diagnostics by module order, line and column. Modules that are not in `modules` (the `migrations` module and frozen
/// schema files) come after them: frozen schema files in path order, then `migrations`.
fn sort_diagnostics(diags: &mut [Diagnostic], modules: &[(&str, &str)]) {
    let order: HashMap<&str, usize> = {
        let mut order = HashMap::new();
        for (i, (name, _)) in modules.iter().enumerate() {
            order.entry(*name).or_insert(i);
        }
        order
    };
    diags.sort_by_cached_key(|d| {
        let idx = order.get(d.module.as_str()).copied();
        (
            idx.unwrap_or(usize::MAX),
            idx.is_none() && d.module == migrate::MIGRATIONS_MODULE,
            if idx.is_none() {
                d.module.clone()
            } else {
                String::new()
            },
            d.line,
            d.col,
        )
    });
}

/// Identity declarations (in module order, then declaration order; duplicates of the same name are errors, so they do not
/// come here).
fn collect_identities(asts: &[Module]) -> Vec<IdentityInfo> {
    let mut identities: Vec<IdentityInfo> = Vec::new();
    for (m, ast) in asts.iter().enumerate() {
        for d in &ast.identities {
            if !identities
                .iter()
                .any(|i| i.module as usize == m && i.name == d.name)
            {
                identities.push(IdentityInfo {
                    module: m as u32,
                    name: d.name.clone(),
                    display: d.display.clone(),
                    parts: vec![TextPart::Lit(d.display.clone())],
                });
            }
        }
    }
    identities
}

/// Turns the parts of a body into parts of the original text (characters and positions of `${expr}`).
pub(super) fn text_parts_of(parts: &[StrPart]) -> Vec<TextPart> {
    parts
        .iter()
        .map(|p| match p {
            StrPart::Text(t, _) => TextPart::Lit(t.clone()),
            StrPart::Interp(_) => TextPart::Interp,
        })
        .collect()
}

/// Whether it can be used as a module name (an identifier that is not a reserved word).
pub(super) fn is_module_name(name: &str) -> bool {
    // Each word of a path joined with `::` (`story::ch1`) is an identifier. The first word `std` is the namespace of
    // standard packages, so it cannot be used
    let first = name.split("::").next().unwrap_or("");
    name.split("::")
        .all(|seg| crate::ident::is_identifier(seg) && !is_reserved(seg))
        && first != STD_NAMESPACE
        && first != "schema"
        && first != "migrations"
        && !crate::syntax::expr::is_version_name(first)
}

/// The namespace of standard packages (`import std::audio`).
pub(super) const STD_NAMESPACE: &str = "std";

/// The last word of a module path (`story::ch1`) (the name used in the script when the `import` has no alias).
pub(super) fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}
