//! Line parsing: classifies lines and builds the syntax tree.

use std::collections::{HashMap, HashSet};

use super::ast::{
    AssignOp, ChoiceOpt, Expr, IdentRef, IdentityDecl, Import, LabelCall, LabelDecl, LabelRef,
    MatchArm, Module, Param, PathElem, Pos, STATEMENT_METHODS, StableId, Stmt, StrPart, TyName,
    TypeDeclAst, TypeDeclKind, VarDecl, VariantAst,
};
use super::expr::{
    ExprParser, Line, PErr, PResult, Seg, err, first_word, is_ident_continue, is_ident_start,
    unexpected, unexpected_char, unexpected_eol,
};
use super::keywords::{is_block_statement, is_declaration, is_reserved, starts_block};
use crate::diag::{DiagArg, DiagCode, Diagnostic, Severity};

mod block;
mod branch;
mod continuation;
mod decl;
pub(crate) mod format;
pub(crate) mod schema;

/// Module name → the identity names declared by that module. Used to classify lines (whether `name「` is a quote line).
pub(crate) type IdentMap = HashMap<String, HashSet<String>>;

/// If `IdentMap` has this key, all names are treated as identities (so that REPL's `completeness` can judge multi-line
/// quote lines as incomplete without knowing the environment; section 7.5.1 of `spec/host/07-repl.md`). It is never a
/// module name.
pub(crate) const ANY_IDENTITY: &str = "*";

/// Picks up the paths of `import`ed modules (or packages) (`story::ch1`) from the `import` lines of the source (a
/// preliminary scan, like `scan_identities`; `{ }` is expanded and `as` aliases are skipped; the correctness of the syntax
/// is checked by the real parse).
pub(crate) fn scan_imports(source: &str) -> Vec<String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut out = Vec::new();
    for l in source.split('\n') {
        let t = l.trim_start_matches([' ', '\t']);
        if let Some(rest) = t.strip_prefix("import")
            && rest.starts_with([' ', '\t', '{'])
        {
            scan_import_tree(rest, &mut Vec::new(), &mut out, 0);
        }
    }
    out
}

/// The body of `scan_imports`. Returns the rest after reading (`None` if it cannot be read).
fn scan_import_tree<'a>(
    s: &'a str,
    prefix: &mut Vec<String>,
    out: &mut Vec<String>,
    depth: usize,
) -> Option<&'a str> {
    const WS: [char; 2] = [' ', '\t'];
    if depth > 16 {
        return None;
    }
    let s = s.trim_start_matches(WS);
    if let Some(mut r) = s.strip_prefix('{') {
        loop {
            r = scan_import_tree(r, prefix, out, depth + 1)?.trim_start_matches(WS);
            match r.strip_prefix(',') {
                Some(n) => {
                    let n = n.trim_start_matches(WS);
                    if let Some(end) = n.strip_prefix('}') {
                        return Some(end);
                    }
                    r = n;
                }
                None => return r.strip_prefix('}'),
            }
        }
    }
    let base = prefix.len();
    let mut cur = s;
    let result = loop {
        let w = first_word(cur);
        if w.is_empty() {
            break None;
        }
        prefix.push(crate::ident::normalize_name(w));
        cur = cur[w.len()..].trim_start_matches(WS);
        if let Some(r) = cur.strip_prefix("::") {
            let r = r.trim_start_matches(WS);
            if r.starts_with('{') {
                break scan_import_tree(r, prefix, out, depth + 1);
            }
            cur = r;
            continue;
        }
        out.push(prefix.join("::"));
        break Some(match cur.strip_prefix("as") {
            Some(a) if !a.starts_with(is_ident_continue) => {
                let a = a.trim_start_matches(WS);
                &a[first_word(a).len()..]
            }
            _ => cur,
        });
    };
    prefix.truncate(base);
    result
}

/// Picks up declared identity names from `identity name = ...` in the source (a preliminary scan for classifying lines;
/// the correctness of the syntax is checked by the real parse).
pub(crate) fn scan_identities(source: &str) -> HashSet<String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut out = HashSet::new();
    for l in source.split('\n') {
        let t = l.trim_start_matches([' ', '\t']);
        if let Some(rest) = t.strip_prefix("identity")
            && rest.starts_with([' ', '\t'])
        {
            let name = first_word(rest.trim_start_matches([' ', '\t']));
            if !name.is_empty() {
                out.insert(crate::ident::normalize_name(name));
            }
        }
    }
    out
}

/// Parses the source of a module (knowing only that module's identities).
#[cfg(test)]
pub(crate) fn parse(source: &str, module: &str) -> (Module, Vec<Diagnostic>) {
    let mut idents = IdentMap::new();
    idents.insert(module.to_string(), scan_identities(source));
    parse_with(source, module, &idents)
}

/// Parses the source of a module. Diagnostics are returned in the order found (reads line by line so errors do not
/// cascade). `idents` are the identities of all modules (used to classify lines).
pub(crate) fn parse_with(
    source: &str,
    module: &str,
    idents: &IdentMap,
) -> (Module, Vec<Diagnostic>) {
    let mut p = Parser::new(source, module, idents);
    p.parse_module();
    (p.out, p.diags)
}

/// Parses a sequence of statements of REPL input (the same grammar as a label body; a top-level `}` is
/// `unexpected_close`). Diagnostics are in the order found (same as `parse_with`).
pub(crate) fn parse_repl_body(
    source: &str,
    module: &str,
    idents: &IdentMap,
) -> (Vec<Stmt>, Vec<Diagnostic>) {
    let mut p = Parser::new(source, module, idents);
    p.join_lines = false;
    let mut body = Vec::new();
    while p.i < p.lines.len() {
        let raw = p.lines[p.i];
        let no = p.i as u32 + 1;
        p.i += 1;
        let off = indent_len(raw);
        let t = &raw[off..];
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if t.starts_with('}') {
            let e = PErr {
                code: DiagCode::UnexpectedClose,
                args: Vec::new(),
                pos: Line::new(raw, no).pos(off),
            };
            p.push(e);
            continue;
        }
        let joined = match p.join_statement(raw, no, off, off) {
            Ok(j) => j,
            Err(e) => {
                p.push(e);
                continue;
            }
        };
        let last = p.lines[p.i - 1];
        let line = match &joined {
            Some(j) => Line::joined(&j.text, no, j.segs.clone()),
            None => Line::new(raw, no),
        };
        match p.parse_stmt(&line, off) {
            Ok(Some(st)) => body.push(st),
            Ok(None) => {}
            Err(e) => {
                if starts_block(first_word(t)) && opens_block(last) {
                    p.skip_block();
                }
                p.push(e);
            }
        }
    }
    (body, p.diags)
}

/// Checks whether one line of REPL input can be read as a single expression (③ of section 7.2.1 of
/// `spec/host/07-repl.md`). If not, returns the diagnostics.
pub(crate) fn parse_expr_line(text: &str, module: &str, line_no: u32) -> Result<Expr, Diagnostic> {
    let line = Line::new(text, line_no);
    let mut ep = ExprParser::new(&line, 0);
    let res = (|| {
        let e = ep.parse_expr()?;
        ep.expect_end()?;
        Ok(e)
    })();
    res.map_err(|e: PErr| Diagnostic {
        module: module.to_string(),
        line: e.pos.line,
        col: e.pos.col,
        severity: Severity::Error,
        code: e.code,
        args: e.args,
    })
}

/// The form of the body of an option.
enum OptionBody {
    /// A multi-line block (the position of `{`).
    Block(Pos),
    /// A one-line block (the byte position right after `{`, and the position of `{`).
    Inline(usize, Pos),
    /// One statement (the starting byte position).
    Stmt(usize),
}

/// Decides the form of the body of an option (the non-recursive part; keeps the frame off the recursive path).
#[inline(never)]
fn classify_option_body(jl: &Line<'_>, body_at: usize) -> PResult<OptionBody> {
    let mut ep = ExprParser::new(jl, body_at);
    if ep.peek() == Some('{') {
        let open = ep.here();
        ep.bump();
        let after = ep.pos;
        Ok(if ep.at_end()? {
            OptionBody::Block(open)
        } else {
            OptionBody::Inline(after, open)
        })
    } else if ep.at_end()? {
        unexpected_eol(ep.here())
    } else {
        Ok(OptionBody::Stmt(ep.pos))
    }
}

/// The kind of continuation after `}` (the body has not been read yet).
enum TailKind {
    /// No continuation (or an error was recorded and reading stopped).
    End,
    /// `else {` (the position of `else`).
    Else(Pos),
    /// `else if cond {` (the condition and the position of `if`).
    ElseIf(Expr, Pos),
}

/// The continuation after the `}` that closed the body of an `if`.
enum Tail<'a> {
    /// No continuation (or an error was recorded and reading stopped).
    End,
    /// `else { ... }` (the body has been read).
    Else(Vec<Stmt>),
    /// `else if cond { ... }` (the body has been read). If it continues further, holds the closing line.
    ElseIf(Expr, Vec<Stmt>, Option<Close<'a>>),
}

/// The line that closed a block (what follows `}` is read for `else`).
#[derive(Clone, Copy)]
struct Close<'a> {
    raw: &'a str,
    no: u32,
    /// The byte position right after `}`.
    off: usize,
}

/// The state of line-by-line parsing.
struct Parser<'a> {
    module: &'a str,
    idents: &'a IdentMap,
    lines: Vec<&'a str>,
    i: usize,
    diags: Vec<Diagnostic>,
    out: Module,
    /// The nesting depth of the block being read.
    depth: u32,
    /// Stopped reading because the nesting was too deep.
    aborted: bool,
    /// Whether a declaration other than `import` has already been read (`import`s are written before other declarations).
    seen_decl: bool,
    /// Whether statements continue over several lines when a bracket is open (not for REPL input, which is judged one
    /// input at a time).
    join_lines: bool,
}

impl<'a> Parser<'a> {
    /// Splits into lines, removing the leading BOM and trailing `\r`.
    fn new(source: &'a str, module: &'a str, idents: &'a IdentMap) -> Self {
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        Parser {
            module,
            idents,
            lines: source
                .split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect(),
            i: 0,
            diags: Vec::new(),
            out: Module::default(),
            depth: 0,
            aborted: false,
            seen_decl: false,
            join_lines: true,
        }
    }
}

/// The maximum nesting of blocks (`label`, `if`, `else`, `while`, `choice`).
const MAX_BLOCK_DEPTH: u32 = 64;

/// The number of bytes of leading whitespace (spaces and tabs).
fn indent_len(raw: &str) -> usize {
    raw.len() - raw.trim_start_matches([' ', '\t']).len()
}

/// Removes a comment in the middle of a line (`//` outside string literals).
fn strip_comment(s: &str) -> &str {
    let mut in_str = false;
    let mut esc = false;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            return &s[..i];
        }
        i += 1;
    }
    s
}

/// Whether the line ends with `{` (opens a block).
fn opens_block(s: &str) -> bool {
    strip_comment(s).trim_end().ends_with('{')
}

/// An assignment operator at the start of the line (`=`, `+=`, and so on) and the number of bytes up to its end.
fn assign_op_at(rest: &str) -> Option<(AssignOp, usize)> {
    let r = rest.trim_start_matches([' ', '\t']);
    let skipped = rest.len() - r.len();
    let table: [(&str, AssignOp); 6] = [
        ("+=", AssignOp::Add),
        ("-=", AssignOp::Sub),
        ("*=", AssignOp::Mul),
        ("/=", AssignOp::Div),
        ("%=", AssignOp::Rem),
        ("=", AssignOp::Set),
    ];
    for (s, op) in table {
        if r.starts_with(s) {
            if op == AssignOp::Set && (r.starts_with("==") || r.starts_with("=>")) {
                return None;
            }
            return Some((op, skipped + s.len()));
        }
    }
    None
}

impl<'a> Parser<'a> {
    /// Collects syntax errors as diagnostics.
    fn push(&mut self, e: PErr) {
        self.diags.push(Diagnostic {
            module: self.module.to_string(),
            line: e.pos.line,
            col: e.pos.col,
            severity: Severity::Error,
            code: e.code,
            args: e.args,
        });
    }

    /// Whether the name is a usable identity (this module's or an `import`ed module's).
    fn is_identity(&self, name: &str) -> bool {
        if self.idents.contains_key(ANY_IDENTITY) {
            return true;
        }
        let has = |m: &str| self.idents.get(m).is_some_and(|s| s.contains(name));
        has(self.module) || self.out.imports.iter().any(|i| has(&i.path))
    }

    /// Whether `module.name` is a declared identity.
    fn is_qualified_identity(&self, module: &str, name: &str) -> bool {
        self.idents.contains_key(ANY_IDENTITY)
            || self.idents.get(module).is_some_and(|s| s.contains(name))
    }

    /// Reads as a quote line (`identity「…」`, `「…」`). `None` if it is not a quote line (a plain line).
    /// For a quote line, also reads the following physical lines (multi-line quote lines).
    fn try_quote(&mut self, line: &Line<'_>, off: usize) -> PResult<Option<Stmt>> {
        let t = &line.text[off..];
        let pos = line.pos(off);
        let (identity, quote_at) = if t.starts_with('「') {
            (None, off)
        } else {
            let word = first_word(t);
            if word.is_empty() {
                return Ok(None);
            }
            let after = &t[word.len()..];
            if after.starts_with('「') && self.is_identity(&crate::ident::nfc(word)) {
                let ident = IdentRef {
                    module: None,
                    name: crate::ident::normalize_name(word),
                    pos,
                };
                (Some(ident), off + word.len())
            } else if let Some(r) = after.strip_prefix('.')
                && r.chars().next().is_some_and(is_ident_start)
            {
                let member = first_word(r);
                if r[member.len()..].starts_with('「')
                    && self
                        .is_qualified_identity(&crate::ident::nfc(word), &crate::ident::nfc(member))
                {
                    let ident = IdentRef {
                        module: Some((crate::ident::normalize_name(word), pos)),
                        name: crate::ident::normalize_name(member),
                        pos: line.pos(off + word.len() + 1),
                    };
                    (Some(ident), off + word.len() + 1 + member.len())
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        };
        let quote_pos = line.pos(quote_at);
        let following = &self.lines[self.i.min(self.lines.len())..];
        let Some(joined) = join_quote(line, quote_at, following, self.i as u32 + 1) else {
            return match identity {
                Some(_) => err(DiagCode::UnterminatedQuote, vec![], quote_pos),
                None => Ok(None),
            };
        };
        let jl = Line::joined(&joined.text, line.no, joined.segs.clone());
        // After `」`: only whitespace and a stable ID can be written
        let rest = &joined.text[joined.end..];
        let trimmed = rest.trim_start_matches([' ', '\t']);
        let tail_at = joined.end + (rest.len() - trimmed.len());
        let id_text = trimmed.trim_end_matches([' ', '\t']);
        let id = if id_text.is_empty() {
            None
        } else if let Some(name) = id_text.strip_prefix('#')
            && is_stable_id_shape(name)
        {
            let id_pos = jl.pos(tail_at);
            if let Err(e) = check_stable_id(name, id_pos) {
                self.i += joined.consumed;
                return Err(e);
            }
            Some(StableId {
                id: name.to_string(),
                pos: id_pos,
            })
        } else {
            // Characters follow: an error with an identity, a plain line without one
            if identity.is_none() {
                return Ok(None);
            }
            self.i += joined.consumed;
            return err(DiagCode::TextAfterQuote, vec![], jl.pos(tail_at));
        };
        self.i += joined.consumed;
        let parts = parse_body(&jl, quote_at, joined.end)?;
        Ok(Some(Stmt::Quote {
            identity,
            parts,
            id,
            id_at: jl.pos(joined.end),
            pos,
        }))
    }
}

/// If the line starts with `target = expr`, `target += expr` or `target.push(expr)`, reads that statement. The target is
/// a variable followed by `.field` and `[index]` (also `module.variable`). If the form does not match, `None` (read as a
/// plain line or the like). Errors after the assignment symbol or the `(` of an updating method has been read are
/// errors.
fn parse_target_stmt(line: &Line<'_>, off: usize, word: &str, pos: Pos) -> PResult<Option<Stmt>> {
    let t = &line.text[off..];
    let after = &t[word.len()..];
    // If the word at the start of the line is not immediately followed by `.`, `[` or an assignment symbol, it is not a
    // statement with a target
    if !(after.starts_with('.') || after.starts_with('[') || assign_op_at(after).is_some()) {
        return Ok(None);
    }
    let mut ep = ExprParser::new(line, off + word.len());
    let mut module: Option<(String, Pos)> = None;
    let word_nfc = crate::ident::normalize_name(word);
    let mut name = word_nfc.clone();
    let mut path: Vec<PathElem> = Vec::new();
    loop {
        match ep.peek() {
            Some('.') if after_dot_is_ident(&ep) => {
                ep.bump();
                let Some((member, mpos)) = ep.ident().ok().flatten() else {
                    return Ok(None);
                };
                if ep.peek() == Some('(') {
                    if !STATEMENT_METHODS.contains(&member.as_str()) {
                        return Ok(None);
                    }
                    if module.is_none() && path.is_empty() && is_reserved(&name) {
                        return Ok(None);
                    }
                    ep.bump();
                    let args = ep.parse_call_args()?;
                    ep.expect_end()?;
                    return Ok(Some(Stmt::ListMethod {
                        module,
                        name,
                        path,
                        method: member,
                        method_pos: mpos,
                        args,
                        pos,
                    }));
                }
                if module.is_none() && path.is_empty() && name == word_nfc {
                    // `module.variable` (`variable.field` for a struct variable has the same form)
                    module = Some((std::mem::replace(&mut name, member), pos));
                } else {
                    path.push(PathElem::Field(member, mpos));
                }
            }
            Some('[') => {
                let at = ep.here();
                ep.bump();
                let Ok(idx) = ep.parse_expr() else {
                    return Ok(None);
                };
                if ep.expect_char(']').is_err() {
                    return Ok(None);
                }
                path.push(PathElem::Index(idx, at));
            }
            _ => break,
        }
    }
    let Some((op, len)) = assign_op_at(ep.rest()) else {
        return Ok(None);
    };
    ep.pos += len;
    let value = if op == AssignOp::Set {
        parse_value(&mut ep)?
    } else {
        ep.parse_expr()?
    };
    ep.expect_end()?;
    Ok(Some(Stmt::Assign {
        module,
        name,
        path,
        op,
        value,
        pos,
    }))
}

/// Whether what follows the current `.` starts an identifier.
fn after_dot_is_ident(ep: &ExprParser<'_>) -> bool {
    ep.rest()[1..].chars().next().is_some_and(is_ident_start)
}

/// The difference between `{` and `}` outside strings (positive if there are more `{`).
fn brace_balance(s: &str) -> i32 {
    let mut d = 0;
    let mut in_str = false;
    let mut esc = false;
    for c in s.chars() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => d += 1,
            '}' => d -= 1,
            _ => {}
        }
    }
    d
}

/// Reads the fields `name: type, ...` of a `struct` up to the closing `}` (the `{` has been read).
fn parse_fields(ep: &mut ExprParser<'_>) -> PResult<Vec<(String, Pos, TyName)>> {
    let mut fields = Vec::new();
    ep.skip_ws()?;
    if ep.peek() == Some('}') {
        ep.bump();
        return Ok(fields);
    }
    loop {
        let (name, pos) = ep.expect_name()?;
        ep.expect_char(':')?;
        let ty = ep.parse_type_name()?;
        fields.push((name, pos, ty));
        ep.skip_ws()?;
        if ep.peek() == Some(',') {
            ep.bump();
            ep.skip_ws()?;
            if ep.peek() == Some('}') {
                break;
            }
        } else {
            break;
        }
    }
    ep.expect_char('}')?;
    Ok(fields)
}

/// Reads the variants `name`, `name(type, ...)` of an `enum` up to the closing `}` (the `{` has been read).
fn parse_variants(ep: &mut ExprParser<'_>) -> PResult<Vec<VariantAst>> {
    let mut variants = Vec::new();
    loop {
        let (name, pos) = ep.expect_name()?;
        let mut args = Vec::new();
        ep.skip_ws()?;
        if ep.peek() == Some('(') {
            ep.bump();
            loop {
                args.push(ep.parse_type_name()?);
                ep.skip_ws()?;
                if ep.peek() == Some(',') {
                    ep.bump();
                } else {
                    break;
                }
            }
            ep.expect_char(')')?;
        }
        variants.push(VariantAst { name, pos, args });
        ep.skip_ws()?;
        if ep.peek() == Some(',') {
            ep.bump();
            ep.skip_ws()?;
            if ep.peek() == Some('}') {
                break;
            }
        } else {
            break;
        }
    }
    ep.expect_char('}')?;
    Ok(variants)
}

/// Reads `pattern =>` of an arm. Returns the pattern and the starting byte position of the body. Not on the recursive path.
#[inline(never)]
fn parse_arm_head(line: &Line<'_>, off: usize) -> PResult<(super::ast::Pattern, usize)> {
    let mut ep = ExprParser::new(line, off);
    let pattern = ep.parse_pattern()?;
    ep.skip_ws()?;
    if ep.rest().starts_with("=>") {
        ep.pos += 2;
    } else if ep.rest().is_empty() {
        return unexpected_eol(ep.here());
    } else {
        return unexpected(&ep.found_here(), ep.here());
    }
    ep.skip_ws()?;
    Ok((pattern, ep.pos))
}

/// The initial value of `let` or the right-hand side of `=`. An expression, a label call with `call`, or a command call
/// with `@` (receives the return value; can have a stable ID).
fn parse_value(ep: &mut ExprParser<'_>) -> PResult<Expr> {
    ep.skip_ws()?;
    if ep.peek() == Some('@') {
        let mut call = ep.parse_command_call()?;
        call.id = read_stable_id(ep)?;
        return Ok(Expr::Command(call));
    }
    if first_word(ep.rest()) == "call" {
        let pos = ep.here();
        ep.pos += "call".len();
        return Ok(Expr::LabelCall(parse_label_call(ep, pos)?));
    }
    ep.parse_expr()
}

/// Reads a label name (`label` or `module.label`).
fn parse_label_ref(ep: &mut ExprParser<'_>) -> PResult<LabelRef> {
    let (first, first_pos) = ep.expect_name()?;
    if ep.peek() == Some('.') {
        ep.bump();
        let (name, _) = ep.expect_name()?;
        return Ok(LabelRef {
            module: Some((first, first_pos)),
            name,
        });
    }
    Ok(LabelRef {
        module: None,
        name: first,
    })
}

/// Reads `label(expr, ...)` after `call` (`call` has been read). `pos` is the position of `call`.
fn parse_label_call(ep: &mut ExprParser<'_>, pos: Pos) -> PResult<LabelCall> {
    let target = parse_label_ref(ep)?;
    ep.expect_char('(')?;
    let args = ep.parse_call_args()?;
    Ok(LabelCall { target, args, pos })
}

/// Reads a type name.
fn parse_type(ep: &mut ExprParser<'_>) -> PResult<TyName> {
    ep.parse_type_name()
}

/// Reads the parameters `name: type [= default], ...` of a label or `fn` up to the closing `)` (the `(` has been read).
fn parse_params(ep: &mut ExprParser<'_>) -> PResult<Vec<Param>> {
    let mut params = Vec::new();
    ep.skip_ws()?;
    if ep.peek() == Some(')') {
        ep.bump();
        return Ok(params);
    }
    loop {
        let (name, pos) = ep.expect_name()?;
        ep.expect_char(':')?;
        let ty = parse_type(ep)?;
        ep.skip_ws()?;
        let default = if ep.peek() == Some('=') {
            ep.bump();
            Some(ep.parse_expr()?)
        } else {
            None
        };
        params.push(Param {
            name,
            pos,
            ty,
            default,
        });
        ep.skip_ws()?;
        if ep.peek() == Some(',') {
            ep.bump();
        } else {
            break;
        }
    }
    ep.expect_char(')')?;
    Ok(params)
}

/// Checks the notation of a stable ID (lowercase letters, digits and `_`, up to 32 characters).
fn check_stable_id(id: &str, pos: Pos) -> PResult<()> {
    let ok = !id.is_empty()
        && id.chars().count() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        err(
            DiagCode::InvalidStableId,
            vec![("id", DiagArg::Str(id.to_string()))],
            pos,
        )
    }
}

/// Whether it consists only of stable ID characters (the length limit is checked by `check_stable_id`).
fn is_stable_id_shape(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Reads `#id` after whitespace, if present (the stable ID of a command or option).
fn read_stable_id(ep: &mut ExprParser<'_>) -> PResult<Option<StableId>> {
    ep.skip_ws()?;
    if ep.peek() != Some('#') {
        return Ok(None);
    }
    let p = ep.here();
    ep.bump();
    let start = ep.pos;
    while let Some(c) = ep.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            ep.bump();
        } else {
            break;
        }
    }
    let id = &ep.line.text[start..ep.pos];
    check_stable_id(id, p)?;
    Ok(Some(StableId {
        id: id.to_string(),
        pos: p,
    }))
}

/// The maximum number of physical lines (excluding the first) that a quote line or option text can have as one quote.
const MAX_QUOTE_LINES: usize = 256;

/// A quote joined from physical lines.
struct Joined {
    /// The joined string (the whole first line and the continuation lines; the last physical line includes what follows
    /// `」`).
    text: String,
    segs: Vec<Seg>,
    /// Right after the matching `」` (bytes).
    end: usize,
    /// The number of continuation physical lines read.
    consumed: usize,
}

/// Searches from `from` (the position of `「`) in `line` to the matching `」`, joining following physical lines
/// (`following`; the number of the first line is `first_no`) if needed. Counts nesting and does not count the character
/// after `\`. `None` if not found.
fn join_quote(line: &Line<'_>, from: usize, following: &[&str], first_no: u32) -> Option<Joined> {
    let mut text = line.text.to_string();
    let mut segs = line.segments();
    let mut pos = from;
    let mut depth = 0u32;
    let mut escaped = false;
    let mut consumed = 0;
    loop {
        while pos < text.len() {
            let c = text[pos..].chars().next().expect("範囲の中");
            pos += c.len_utf8();
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '「' => depth += 1,
                '」' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(Joined {
                            text,
                            segs,
                            end: pos,
                            consumed,
                        });
                    }
                }
                _ => {}
            }
        }
        if line.single || consumed >= following.len() || consumed >= MAX_QUOTE_LINES {
            return None;
        }
        // Join after removing the newline and the leading half-width spaces and tabs of continuation lines
        let raw = following[consumed];
        let stripped = raw.trim_start_matches([' ', '\t']);
        segs.push(Seg {
            start: text.len(),
            line: first_no + consumed as u32,
            col_base: (raw.len() - stripped.len()) as u32,
        });
        text.push_str(stripped);
        consumed += 1;
    }
}

/// Splits the contents of a one-line block by `;` outside brackets and strings (byte ranges of each part with surrounding
/// whitespace removed; empty parts are dropped).
fn split_statements(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    let mut start = 0;
    let mut push = |a: usize, b: usize| {
        let part = &s[a..b];
        let t = part.trim_start_matches([' ', '\t']);
        let lead = part.len() - t.len();
        let t = t.trim_end_matches([' ', '\t']);
        if !t.is_empty() {
            out.push((a + lead, a + lead + t.len()));
        }
    };
    for (i, c) in s.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ';' if depth == 0 => {
                push(start, i);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(start, s.len());
    out
}

/// Splits off the stable ID at the end of the line (whitespace + `#id`). Returns the body.
fn split_stable_id<'t>(
    t: &'t str,
    line: &Line<'_>,
    off: usize,
) -> PResult<(&'t str, Option<StableId>)> {
    let trimmed = t.trim_end_matches([' ', '\t']);
    if let Some(idx) = trimmed.rfind('#')
        && idx > 0
    {
        let before = trimmed.as_bytes()[idx - 1];
        let after = &trimmed[idx + 1..];
        let shape_ok = is_stable_id_shape(after);
        if (before == b' ' || before == b'\t') && shape_ok {
            let pos = line.pos(off + idx);
            check_stable_id(after, pos)?;
            let body = trimmed[..idx].trim_end_matches([' ', '\t']);
            return Ok((
                body,
                Some(StableId {
                    id: after.to_string(),
                    pos,
                }),
            ));
        }
    }
    Ok((trimmed, None))
}

/// Splits a body into runs of characters and `${expr}` (unescaping). `start..end` is the byte range within the line.
fn parse_body(line: &Line<'_>, start: usize, end: usize) -> PResult<Vec<StrPart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut written = String::new();
    let mut pos = start;
    while pos < end {
        let c = line.text[pos..].chars().next().expect("範囲の中");
        let here = pos;
        pos += c.len_utf8();
        match c {
            '\\' => {
                let n = line.text[pos..end].chars().next();
                match n {
                    Some(n @ ('「' | '」' | '$' | '\\' | '=' | '#')) => {
                        text.push(n);
                        written.push('\\');
                        written.push(n);
                        pos += n.len_utf8();
                    }
                    Some(n) => {
                        return err(
                            DiagCode::InvalidEscape,
                            vec![("found", DiagArg::Str(format!("\\{n}")))],
                            line.pos(here),
                        );
                    }
                    None => {
                        return err(
                            DiagCode::InvalidEscape,
                            vec![("found", DiagArg::Str("\\".to_string()))],
                            line.pos(here),
                        );
                    }
                }
            }
            '$' if line.text[pos..end].starts_with('{') => {
                if !text.is_empty() {
                    parts.push(StrPart::Text(
                        std::mem::take(&mut text),
                        std::mem::take(&mut written),
                    ));
                }
                let mut ep = ExprParser::new(line, pos + 1);
                let e = ep.parse_expr()?;
                ep.expect_char('}')?;
                pos = ep.pos;
                parts.push(StrPart::Interp(e));
            }
            c => {
                text.push(c);
                written.push(c);
            }
        }
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(StrPart::Text(text, written));
    }
    Ok(parts)
}
