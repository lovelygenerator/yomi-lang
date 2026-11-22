//! Analysis results of scripts (for editors and tools): a table of declarations (symbols) and how they are used (references).
//!
//! The syntax tree is not public. This parses the source of modules and returns only where names are declared and used.
//! Even for broken source, it builds results for the parts the parser could read. Positions are 1-based for both line and
//! column; columns and lengths are counted in characters (`char`).

use std::collections::HashMap;

use crate::syntax::ast::{
    AssignOp, ChoiceOpt, Expr, IdentRef, LabelDecl, LabelRef, Module, Param, PathElem, Pattern,
    Pos, Stmt, StrPart, TyName, TypeDeclKind,
};
use crate::syntax::parser::{IdentMap, parse_with, scan_identities};

/// The kind of a symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// A `label`.
    Label,
    /// A `fn`.
    Fn,
    /// A module variable (`var`, `save var`, `persistent var`).
    Var,
    /// An `identity`.
    Identity,
    /// A `struct`.
    Struct,
    /// An `enum`.
    Enum,
    /// A variant of an `enum`.
    Variant,
    /// A field of a `struct`.
    Field,
    /// A parameter of a label or function.
    Param,
    /// A name bound by `let`, `for` or `match`.
    Local,
}

/// A range in the source (within one line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Line (1-based).
    pub line: u32,
    /// Column (1-based, in characters).
    pub col: u32,
    /// Length (in characters).
    pub len: u32,
}

impl Span {
    /// Whether this contains the position (line, column).
    pub fn contains(&self, line: u32, col: u32) -> bool {
        self.line == line && col >= self.col && col <= self.col + self.len
    }
}

/// One declared name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// Kind.
    pub kind: SymbolKind,
    /// Name.
    pub name: String,
    /// The module that has the declaration.
    pub module: String,
    /// The range of the declared name.
    pub span: Span,
    /// A one-line description of the declaration (such as `label start(n: int) -> str`).
    pub detail: String,
    /// The enclosing name (the type name for fields and variants, the label name for locals and parameters).
    pub container: Option<String>,
}

/// A position where a name is used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// The module where it is used.
    pub module: String,
    /// The range of the name.
    pub span: Span,
    /// The symbol it refers to (an index into [`Analysis::symbols`]).
    pub target: usize,
}

/// The result of analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// Declared names (in module order, then declaration order).
    pub symbols: Vec<Symbol>,
    /// Positions where names are used. Only those that could be resolved.
    pub references: Vec<Reference>,
}

impl Analysis {
    /// The symbol that the name at the position (a declaration itself, or a use) refers to.
    pub fn symbol_at(&self, module: &str, line: u32, col: u32) -> Option<usize> {
        let by_ref = self
            .references
            .iter()
            .find(|r| r.module == module && r.span.contains(line, col))
            .map(|r| r.target);
        by_ref.or_else(|| {
            self.symbols
                .iter()
                .position(|s| s.module == module && s.span.contains(line, col))
        })
    }

    /// References to the symbol (declarations are not included).
    pub fn references_to(&self, symbol: usize) -> impl Iterator<Item = &Reference> {
        self.references.iter().filter(move |r| r.target == symbol)
    }
}

/// Analyzes modules (name and source).
pub fn analyze(modules: &[(&str, &str)]) -> Analysis {
    let mut idents = IdentMap::new();
    for (name, source) in modules {
        idents
            .entry((*name).to_string())
            .or_insert_with(|| scan_identities(source));
    }
    let mut out = Analysis::default();
    let mut parsed: Vec<(&str, Module, Vec<&str>, Tops)> = Vec::new();
    for (name, source) in modules {
        let (ast, _) = parse_with(source, name, &idents);
        let lines = source_lines(source);
        let tops = declare(&mut out, name, &ast, &lines);
        parsed.push((name, ast, lines, tops));
    }
    let all: HashMap<&str, &Tops> = parsed.iter().map(|(n, _, _, t)| (*n, t)).collect();
    for (name, ast, lines, tops) in &parsed {
        let imports: HashMap<&str, &str> = ast
            .imports
            .iter()
            .map(|i| (i.local.as_str(), i.path.as_str()))
            .collect();
        let mut w = Walker {
            out: &mut out,
            module: name,
            lines,
            tops,
            all: &all,
            imports,
            scopes: Vec::new(),
            current: String::new(),
        };
        w.module(ast);
    }
    out
}

/// Whether the token can be used as a name (for completion and extracting names).
pub fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

mod declare;
#[cfg(test)]
mod tests;
mod walk;

use declare::*;
use walk::*;
