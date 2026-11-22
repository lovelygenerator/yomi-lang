//! Classifying and parsing REPL input (sections 7.2.1 and 7.5.1 of `spec/host/07-repl.md`).
//!
//! The kind of input is decided only by its characters. It does not depend on which names are declared.

use crate::compile::repl::{DeclEntry, REPL_MODULE};
use crate::diag::{DiagArg, DiagCode, Diagnostic, Severity};
use crate::syntax::ast::{Expr, Pos, STATEMENT_METHODS, Stmt};
use crate::syntax::expr::first_word;
use crate::syntax::parser::{IdentMap, parse_expr_line, parse_repl_body, parse_with};

/// Classified input.
#[derive(Debug, Clone)]
pub(super) enum Parsed {
    /// Only empty lines and comments (does nothing).
    Empty,
    /// An interactive command (starts with `:`).
    Command,
    /// A single line that is entirely one expression.
    Expr(Expr),
    /// A sequence of statements (or script lines).
    Stmts(Vec<Stmt>),
    /// Declarations (`fn`, `struct`, `enum`, `import`, `identity`), in declaration order.
    Decls(Vec<DeclEntry>),
}

/// Makes a diagnostic for REPL input.
fn diag(line: u32, col: u32, code: DiagCode, what: &str) -> Diagnostic {
    Diagnostic {
        module: REPL_MODULE.to_string(),
        line,
        col,
        severity: Severity::Error,
        code,
        args: vec![("what", DiagArg::Str(what.to_string()))],
    }
}

/// A diagnostic for something that cannot be written (`repl_not_allowed`).
pub(super) fn not_allowed(pos: Pos, what: &str) -> Diagnostic {
    diag(pos.line, pos.col, DiagCode::ReplNotAllowed, what)
}

/// Classifies and parses input. If `scan_ast` is true, also checks the parsed statements for things that cannot be
/// written (`jump`, `end`, `return`, `once`, stable IDs) (`completeness` looks only at syntax, so it passes false). If
/// there are errors, diagnostics in position order.
pub(super) fn classify(
    text: &str,
    idents: &IdentMap,
    scan_ast: bool,
) -> Result<Parsed, Vec<Diagnostic>> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let blank = |l: &str| l.trim().is_empty() || l.trim_start().starts_with("//");
    let Some(fi) = lines.iter().position(|l| !blank(l)) else {
        return Ok(Parsed::Empty);
    };
    let first = lines[fi];
    let indent = first.len() - first.trim_start().len();
    let t = &first[indent..];
    // ② Interactive command
    if t.starts_with(':') {
        return Ok(Parsed::Command);
    }
    // ③ A single line that is entirely one expression
    let last = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .unwrap_or(fi);
    let single = lines[fi..=last].iter().skip(1).all(|l| l.trim().is_empty());
    if single {
        let line_text = first.trim_end();
        if let Ok(e) = parse_expr_line(line_text, REPL_MODULE, fi as u32 + 1) {
            // `@` commands, `call` and updating methods (such as `push`) are read as statements
            let stmt = match &e {
                Expr::Command(_) | Expr::MisplacedCommand(_) | Expr::LabelCall(_) => true,
                Expr::Method(_, name, _, _) | Expr::QueryCall(_, name, _, _) => {
                    STATEMENT_METHODS.contains(&name.as_str())
                }
                _ => false,
            };
            if !stmt {
                return Ok(Parsed::Expr(e));
            }
        }
    }
    // ④ Declaration or statement
    let word = first_word(t);
    let pos = Pos {
        line: fi as u32 + 1,
        col: first[..indent].chars().count() as u32 + 1,
    };
    match word {
        "save" | "persistent" | "label" => Err(vec![not_allowed(pos, word)]),
        "fn" | "struct" | "enum" | "import" | "identity" => {
            let (m, diags) = parse_with(text, REPL_MODULE, idents);
            if !diags.is_empty() {
                return Err(diags);
            }
            let mut extra = Vec::new();
            for v in &m.vars {
                extra.push(not_allowed(
                    v.pos,
                    if v.persistent { "persistent" } else { "save" },
                ));
            }
            for l in &m.labels {
                extra.push(not_allowed(l.pos, "label"));
            }
            if !extra.is_empty() {
                extra.sort_by_key(|d| (d.line, d.col));
                return Err(extra);
            }
            let mut out: Vec<DeclEntry> = Vec::new();
            out.extend(m.imports.into_iter().map(DeclEntry::Import));
            out.extend(m.identities.into_iter().map(DeclEntry::Identity));
            out.extend(m.types.into_iter().map(DeclEntry::Type));
            out.extend(m.fns.into_iter().map(DeclEntry::Fn));
            out.sort_by_key(|d| (d.pos().line, d.pos().col));
            Ok(Parsed::Decls(out))
        }
        _ => {
            let (body, diags) = parse_repl_body(text, REPL_MODULE, idents);
            if !diags.is_empty() {
                return Err(diags);
            }
            if scan_ast {
                let mut found = Vec::new();
                scan(&body, &mut found);
                if !found.is_empty() {
                    found.sort_by_key(|(p, _)| (p.line, p.col));
                    return Err(found.into_iter().map(|(p, w)| not_allowed(p, w)).collect());
                }
            }
            Ok(Parsed::Stmts(body))
        }
    }
}

/// Collects things in a sequence of statements that cannot be written in REPL input (`jump`, `end`, `return`, `once`,
/// stable IDs).
fn scan(body: &[Stmt], out: &mut Vec<(Pos, &'static str)>) {
    for st in body {
        match st {
            Stmt::Jump { pos, .. } => out.push((*pos, "jump")),
            Stmt::End(pos) => out.push((*pos, "end")),
            Stmt::Return { pos, .. } => out.push((*pos, "return")),
            Stmt::Plain { id: Some(id), .. } | Stmt::Quote { id: Some(id), .. } => {
                out.push((id.pos, "stable_id"));
            }
            Stmt::Command(call) => {
                if let Some(id) = &call.id {
                    out.push((id.pos, "stable_id"));
                }
            }
            Stmt::Let { init: value, .. } | Stmt::Assign { value, .. } => {
                if let Expr::Command(call) = value
                    && let Some(id) = &call.id
                {
                    out.push((id.pos, "stable_id"));
                }
            }
            Stmt::If {
                arms, else_body, ..
            } => {
                for (_, b) in arms {
                    scan(b, out);
                }
                scan(else_body, out);
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => scan(body, out),
            Stmt::Match { arms, .. } => {
                for a in arms {
                    scan(&a.body, out);
                }
            }
            Stmt::Choice { options, .. } => {
                for o in options {
                    if let Some(p) = o.once {
                        out.push((p, "once"));
                    }
                    if let Some(id) = &o.id {
                        out.push((id.pos, "stable_id"));
                    }
                    scan(&o.body, out);
                }
            }
            _ => {}
        }
    }
}
