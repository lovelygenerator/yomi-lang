//! Collecting declarations (puts the top-level names of a module into the symbol table).

use super::*;

pub(super) fn source_lines(source: &str) -> Vec<&str> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// The top-level names of a module (name to symbol index).
#[derive(Default)]
pub(super) struct Tops {
    pub(super) labels: HashMap<String, usize>,
    pub(super) fns: HashMap<String, usize>,
    pub(super) vars: HashMap<String, usize>,
    pub(super) identities: HashMap<String, usize>,
    pub(super) types: HashMap<String, usize>,
    pub(super) variants: HashMap<String, usize>,
    /// Type name and field name to symbol.
    pub(super) fields: HashMap<(String, String), usize>,
}

pub(super) fn chars(s: &str) -> u32 {
    s.chars().count() as u32
}

pub(super) fn span_of(pos: Pos, name: &str) -> Span {
    Span {
        line: pos.line,
        col: pos.col,
        len: chars(name),
    }
}

pub(super) fn ty_text(t: &TyName) -> String {
    if t.args.is_empty() {
        t.name.clone()
    } else {
        let args: Vec<String> = t.args.iter().map(ty_text).collect();
        format!("{}<{}>", t.name, args.join(", "))
    }
}

pub(super) fn params_text(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| format!("{}: {}", p.name, ty_text(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn signature(keyword: &str, d: &LabelDecl) -> String {
    let ret = d
        .ret
        .as_ref()
        .map(|t| format!(" -> {}", ty_text(t)))
        .unwrap_or_default();
    format!("{keyword} {}({}){ret}", d.name, params_text(&d.params))
}

/// Adds top-level declarations to the symbol table.
pub(super) fn declare(out: &mut Analysis, module: &str, ast: &Module, _lines: &[&str]) -> Tops {
    let mut tops = Tops::default();
    let add = |out: &mut Analysis,
               kind: SymbolKind,
               name: &str,
               pos: Pos,
               detail: String,
               container: Option<String>| {
        out.symbols.push(Symbol {
            kind,
            name: name.to_string(),
            module: module.to_string(),
            span: span_of(pos, name),
            detail,
            container,
        });
        out.symbols.len() - 1
    };
    for d in &ast.identities {
        let id = add(
            out,
            SymbolKind::Identity,
            &d.name,
            d.pos,
            format!("identity {} = \"{}\"", d.name, d.display),
            None,
        );
        tops.identities.insert(d.name.clone(), id);
    }
    for v in &ast.vars {
        let kw = if v.persistent {
            "persistent var"
        } else {
            "var"
        };
        let id = add(
            out,
            SymbolKind::Var,
            &v.name,
            v.pos,
            format!("{kw} {}: {}", v.name, ty_text(&v.ty)),
            None,
        );
        tops.vars.insert(v.name.clone(), id);
    }
    for t in &ast.types {
        match &t.kind {
            TypeDeclKind::Struct(fields) => {
                let id = add(
                    out,
                    SymbolKind::Struct,
                    &t.name,
                    t.pos,
                    format!("struct {}", t.name),
                    None,
                );
                tops.types.insert(t.name.clone(), id);
                for (fname, fpos, fty) in fields {
                    let fid = add(
                        out,
                        SymbolKind::Field,
                        fname,
                        *fpos,
                        format!("{fname}: {}", ty_text(fty)),
                        Some(t.name.clone()),
                    );
                    tops.fields.insert((t.name.clone(), fname.clone()), fid);
                }
            }
            TypeDeclKind::Enum(variants) => {
                let id = add(
                    out,
                    SymbolKind::Enum,
                    &t.name,
                    t.pos,
                    format!("enum {}", t.name),
                    None,
                );
                tops.types.insert(t.name.clone(), id);
                for v in variants {
                    let args: Vec<String> = v.args.iter().map(ty_text).collect();
                    let detail = if args.is_empty() {
                        format!("{}.{}", t.name, v.name)
                    } else {
                        format!("{}.{}({})", t.name, v.name, args.join(", "))
                    };
                    let vid = add(
                        out,
                        SymbolKind::Variant,
                        &v.name,
                        v.pos,
                        detail,
                        Some(t.name.clone()),
                    );
                    tops.variants.insert(v.name.clone(), vid);
                }
            }
        }
    }
    for l in &ast.labels {
        let id = add(
            out,
            SymbolKind::Label,
            &l.name,
            l.pos,
            signature("label", l),
            None,
        );
        tops.labels.insert(l.name.clone(), id);
    }
    for f in &ast.fns {
        let id = add(
            out,
            SymbolKind::Fn,
            &f.name,
            f.pos,
            signature("fn", f),
            None,
        );
        tops.fns.insert(f.name.clone(), id);
    }
    tops
}
