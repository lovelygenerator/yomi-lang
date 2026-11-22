//! Parsing expressions (reads through one line by character position).

use std::borrow::Cow;

use super::ast::{
    BinOp, CommandCall, Expr, FieldInit, Lambda, LambdaParam, Pattern, Pos, StrPart, TyName,
    TypeRef, UnOp,
};
use super::keywords::is_reserved;
use crate::diag::{DiagArg, DiagCode};
use crate::value::finite_float;

mod call;
mod error;
mod line;
mod literal;
mod pattern;
mod ty;

pub(crate) use crate::ident::{is_ident_continue, is_ident_start};
pub(crate) use error::*;
pub(crate) use line::*;

/// The leading identifier (empty if none).
/// A name of a version namespace (`v2`): `v` followed by digits.
pub(crate) fn is_version_name(name: &str) -> bool {
    name.strip_prefix('v')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn first_word(s: &str) -> &str {
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let ok = if i == 0 {
            is_ident_start(c)
        } else {
            is_ident_continue(c)
        };
        if !ok {
            break;
        }
        end = i + c.len_utf8();
    }
    &s[..end]
}

/// The expression parser.
pub(crate) struct ExprParser<'a> {
    pub line: &'a Line<'a>,
    pub pos: usize,
    /// The remaining number of expression nodes that can be made (so deep nesting and long chains do not exhaust the stack).
    budget: u32,
    /// The nesting depth of parentheses and unary operators.
    nest: u32,
    /// If true, `name {` is not read as a struct literal (in the headers of `if`, `while` and `match`, where `{` starts a
    /// block; it can be read again inside `( )`, `[ ]` and call arguments).
    pub no_struct: bool,
    /// If true, type names can have a version prefix and a module path (`v2::story::ch1.Item`), as in `migrate` blocks.
    pub schema_names: bool,
}

/// The maximum number of nodes in one expression (the inside of `${}` is counted separately).
pub(crate) const MAX_EXPR_NODES: u32 = 200;

/// The maximum nesting of parentheses and unary operators.
const MAX_EXPR_NEST: u32 = 24;

impl<'a> ExprParser<'a> {
    /// Makes a reader that reads an expression from `pos` (bytes).
    pub fn new(line: &'a Line<'a>, pos: usize) -> Self {
        Self {
            line,
            pos,
            budget: MAX_EXPR_NODES,
            nest: 0,
            no_struct: false,
            schema_names: false,
        }
    }

    /// A parser for the header expressions of `if`, `while` and `match` (does not read `name {` as a struct literal).
    pub fn new_header(line: &'a Line<'a>, pos: usize) -> Self {
        let mut p = Self::new(line, pos);
        p.no_struct = true;
        p
    }

    /// Makes one expression node. An error if the limit is exceeded.
    fn nest_in(&mut self, pos: Pos) -> PResult<()> {
        if self.nest >= MAX_EXPR_NEST {
            return err(
                DiagCode::TooDeep,
                vec![("limit", DiagArg::Int(i64::from(MAX_EXPR_NEST)))],
                pos,
            );
        }
        self.nest += 1;
        Ok(())
    }

    /// Counts one expression node. `TooDeep` if there are too many.
    fn node(&mut self, pos: Pos) -> PResult<()> {
        if self.budget == 0 {
            return err(
                DiagCode::TooDeep,
                vec![("limit", DiagArg::Int(i64::from(MAX_EXPR_NODES)))],
                pos,
            );
        }
        self.budget -= 1;
        Ok(())
    }

    pub fn here(&self) -> Pos {
        self.line.pos(self.pos)
    }

    /// The remaining unread string.
    pub fn rest(&self) -> &'a str {
        &self.line.text[self.pos..]
    }

    /// The next character (without advancing).
    pub fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// Reads the next character and advances.
    pub fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Skips whitespace (half-width spaces and tabs). A full-width space is an error.
    pub fn skip_ws(&mut self) -> PResult<()> {
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' => {
                    self.pos += 1;
                }
                '\u{3000}' => return err(DiagCode::FullWidthSpace, vec![], self.here()),
                _ => break,
            }
        }
        Ok(())
    }

    /// Whether, after whitespace, it is the end of the line or a comment.
    pub fn at_end(&mut self) -> PResult<bool> {
        self.skip_ws()?;
        Ok(self.rest().is_empty() || self.rest().starts_with("//"))
    }

    /// Requires the end of the line (or a comment).
    pub fn expect_end(&mut self) -> PResult<()> {
        if self.at_end()? {
            Ok(())
        } else {
            unexpected(&self.found_here(), self.here())
        }
    }

    /// The unexpected token at the current position (a whole word for words, otherwise one character).
    pub fn found_here(&self) -> String {
        let rest = self.rest();
        let word_len = rest
            .char_indices()
            .take_while(|&(i, c)| {
                if i == 0 {
                    is_ident_start(c)
                } else {
                    is_ident_continue(c)
                }
            })
            .map(|(i, c)| i + c.len_utf8())
            .last();
        match word_len {
            Some(n) => rest[..n].to_string(),
            None => rest.chars().next().map(String::from).unwrap_or_default(),
        }
    }

    /// Requires one character (skips preceding whitespace).
    pub fn expect_char(&mut self, ch: char) -> PResult<()> {
        self.skip_ws()?;
        if self.peek() == Some(ch) {
            self.pos += ch.len_utf8();
            Ok(())
        } else {
            unexpected_char(self.peek(), self.here())
        }
    }

    /// Reads an identifier (skips whitespace first).
    pub fn ident(&mut self) -> PResult<Option<(String, Pos)>> {
        self.skip_ws()?;
        let start = self.pos;
        match self.peek() {
            Some(c) if is_ident_start(c) => {
                self.bump();
                while let Some(c) = self.peek() {
                    if is_ident_continue(c) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                // Identifiers are handled after normalizing to NFC (positions are the columns as written)
                Ok(Some((
                    crate::ident::normalize_name(&self.line.text[start..self.pos]),
                    self.line.pos(start),
                )))
            }
            _ => Ok(None),
        }
    }

    /// Requires an identifier. Reserved words are not allowed.
    pub fn expect_name(&mut self) -> PResult<(String, Pos)> {
        match self.ident()? {
            Some((n, p)) if !is_reserved(&n) => Ok((n, p)),
            Some((n, p)) => unexpected(&n, p),
            None => unexpected_char(self.peek(), self.here()),
        }
    }

    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_or()
    }

    /// Skips whitespace and advances past `op` if present.
    pub fn eat(&mut self, op: &str) -> PResult<bool> {
        self.skip_ws()?;
        if self.rest().starts_with(op) {
            self.pos += op.len();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Reads only if it starts with `op` and is not immediately followed by `not_next`.
    fn eat_not_followed(&mut self, op: &str, not_next: &[char]) -> PResult<bool> {
        self.skip_ws()?;
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix(op) {
            if after.chars().next().is_some_and(|c| not_next.contains(&c)) {
                return Ok(false);
            }
            self.pos += op.len();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut left = self.parse_and()?;
        loop {
            let pos = {
                self.skip_ws()?;
                self.here()
            };
            if self.eat("||")? {
                let right = self.parse_and()?;
                self.node(pos)?;
                left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right), pos);
            } else {
                return Ok(left);
            }
        }
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut left = self.parse_eq()?;
        loop {
            let pos = {
                self.skip_ws()?;
                self.here()
            };
            if self.eat("&&")? {
                let right = self.parse_eq()?;
                self.node(pos)?;
                left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right), pos);
            } else {
                return Ok(left);
            }
        }
    }

    fn parse_eq(&mut self) -> PResult<Expr> {
        let left = self.parse_cmp()?;
        self.skip_ws()?;
        let pos = self.here();
        let op = if self.eat("==")? {
            BinOp::Eq
        } else if self.eat("!=")? {
            BinOp::Ne
        } else {
            return Ok(left);
        };
        let right = self.parse_cmp()?;
        self.skip_ws()?;
        if self.rest().starts_with("==") || self.rest().starts_with("!=") {
            return unexpected(&self.rest()[..2], self.here());
        }
        self.node(pos)?;
        Ok(Expr::Binary(op, Box::new(left), Box::new(right), pos))
    }

    fn parse_cmp(&mut self) -> PResult<Expr> {
        let left = self.parse_add()?;
        self.skip_ws()?;
        let pos = self.here();
        let op = if self.eat("<=")? {
            BinOp::Le
        } else if self.eat(">=")? {
            BinOp::Ge
        } else if self.eat_not_followed("<", &['='])? {
            BinOp::Lt
        } else if self.eat_not_followed(">", &['='])? {
            BinOp::Gt
        } else {
            return Ok(left);
        };
        let right = self.parse_add()?;
        self.skip_ws()?;
        let r = self.rest();
        if r.starts_with('<') || r.starts_with('>') {
            let n = if r[1..].starts_with('=') { 2 } else { 1 };
            return unexpected(&r[..n], self.here());
        }
        self.node(pos)?;
        Ok(Expr::Binary(op, Box::new(left), Box::new(right), pos))
    }

    fn parse_add(&mut self) -> PResult<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            self.skip_ws()?;
            let pos = self.here();
            let op = if self.eat_not_followed("+", &['='])? {
                BinOp::Add
            } else if self.eat_not_followed("-", &['=', '>'])? {
                BinOp::Sub
            } else {
                return Ok(left);
            };
            let right = self.parse_mul()?;
            self.node(pos)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), pos);
        }
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws()?;
            let pos = self.here();
            let op = if self.eat_not_followed("*", &['='])? {
                BinOp::Mul
            } else if self.eat_not_followed("/", &['=', '/'])? {
                BinOp::Div
            } else if self.eat_not_followed("%", &['='])? {
                BinOp::Rem
            } else {
                return Ok(left);
            };
            let right = self.parse_unary()?;
            self.node(pos)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), pos);
        }
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        self.skip_ws()?;
        let pos = self.here();
        if self.eat_not_followed("-", &['=', '>'])? {
            self.node(pos)?;
            self.nest_in(pos)?;
            let e = self.parse_unary();
            self.nest -= 1;
            let e = e?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e), pos));
        }
        if self.eat_not_followed("!", &['='])? {
            self.node(pos)?;
            self.nest_in(pos)?;
            let e = self.parse_unary();
            self.nest -= 1;
            let e = e?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e), pos));
        }
        let e = self.parse_primary()?;
        self.parse_postfix(e)
    }

    /// Reads postfix `.name`, `.name(expr, ...)` and `[expr]` as long as they continue.
    fn parse_postfix(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            match self.peek() {
                Some('.') if self.rest()[1..].chars().next().is_some_and(is_ident_start) => {
                    let dot = self.here();
                    self.bump();
                    let (member, mpos) = self.ident()?.expect("識別子の始まりを確かめた");
                    self.node(dot)?;
                    if self.peek() == Some('(') {
                        self.nest_in(mpos)?;
                        self.bump();
                        let args = self.parse_call_args();
                        self.nest -= 1;
                        e = Expr::Method(Box::new(e), member, args?, mpos);
                    } else {
                        e = Expr::Field(Box::new(e), member, mpos);
                    }
                }
                Some('[') => {
                    let at = self.here();
                    self.node(at)?;
                    self.nest_in(at)?;
                    self.bump();
                    let saved = std::mem::replace(&mut self.no_struct, false);
                    let idx = self.parse_expr();
                    self.no_struct = saved;
                    self.nest -= 1;
                    let idx = idx?;
                    self.expect_char(']')?;
                    e = Expr::Index(Box::new(e), Box::new(idx), at);
                }
                // Calls are read only right after a name (parse_primary)
                Some('(') => return not_yet("postfix expression", self.here()),
                _ => return Ok(e),
            }
        }
    }

    /// A versioned name (`v2::game.hp`) can only be written in the `migrations` module (`spec/language/07-migrate.md`).
    /// When `name` starts one outside it, reads the whole name and fails with `schema_name_outside_migrations`.
    pub(crate) fn reject_versioned_name(&mut self, name: &str, pos: Pos) -> PResult<()> {
        if self.schema_names || !is_version_name(name) || !self.rest().starts_with("::") {
            return Ok(());
        }
        let mut full = name.to_string();
        while self.rest().starts_with("::")
            && self.rest()[2..].chars().next().is_some_and(is_ident_start)
        {
            self.pos += 2;
            let (seg, _) = self.ident()?.expect("識別子の始まりを確かめた");
            full.push_str("::");
            full.push_str(&seg);
        }
        if self.peek() == Some('.') && self.rest()[1..].chars().next().is_some_and(is_ident_start) {
            self.bump();
            let (member, _) = self.ident()?.expect("識別子の始まりを確かめた");
            full.push('.');
            full.push_str(&member);
        }
        err(
            DiagCode::SchemaNameOutsideMigrations,
            vec![("name", DiagArg::Str(full))],
            pos,
        )
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        self.skip_ws()?;
        let pos = self.here();
        match self.peek() {
            None => unexpected_eol(pos),
            Some('(') => {
                self.node(pos)?;
                self.nest_in(pos)?;
                self.bump();
                let saved = std::mem::replace(&mut self.no_struct, false);
                let e = self.parse_expr();
                self.no_struct = saved;
                self.nest -= 1;
                let e = e?;
                self.expect_char(')')?;
                Ok(e)
            }
            Some('[') => {
                self.node(pos)?;
                self.nest_in(pos)?;
                self.bump();
                let saved = std::mem::replace(&mut self.no_struct, false);
                let items = self.parse_list_items();
                self.no_struct = saved;
                self.nest -= 1;
                Ok(Expr::List(items?, pos))
            }
            Some('"') => self.parse_string(),
            Some('|') => {
                self.node(pos)?;
                self.nest_in(pos)?;
                let l = self.parse_lambda(pos);
                self.nest -= 1;
                Ok(Expr::Lambda(Box::new(l?)))
            }
            // An `@` in the middle of an expression (`invalid_command_use` at compilation)
            Some('@') => {
                self.node(pos)?;
                self.nest_in(pos)?;
                let call = self.parse_command_call();
                self.nest -= 1;
                Ok(Expr::MisplacedCommand(call?))
            }
            Some(c) if c.is_ascii_digit() => self.parse_number(),
            Some(c) if is_ident_start(c) => {
                let (mut name, p) = self.ident()?.expect("識別子の始まりを確かめた");
                self.reject_versioned_name(&name, p)?;
                // In `migrations`, module paths (`story::ch1`) and versioned names (`v2::game`) are written with `::`
                if self.schema_names {
                    while self.rest().starts_with("::")
                        && self.rest()[2..].chars().next().is_some_and(is_ident_start)
                    {
                        self.pos += 2;
                        let (seg, _) = self.ident()?.expect("識別子の始まりを確かめた");
                        name.push_str("::");
                        name.push_str(&seg);
                    }
                }
                match name.as_str() {
                    "true" => Ok(Expr::Bool(true, p)),
                    "false" => Ok(Expr::Bool(false, p)),
                    // `call` can only be written as the initial value of `let`, the right-hand side of `=`, or a statement
                    "call" => err(DiagCode::CallInExpression, vec![], p),
                    "None" => Ok(Expr::None(p)),
                    "map" => {
                        self.node(p)?;
                        self.skip_ws()?;
                        if self.peek() != Some('{') {
                            return unexpected_char(self.peek(), self.here());
                        }
                        self.nest_in(p)?;
                        self.bump();
                        let saved = std::mem::replace(&mut self.no_struct, false);
                        let entries = self.parse_map_entries();
                        self.no_struct = saved;
                        self.nest -= 1;
                        Ok(Expr::Map(entries?, p))
                    }
                    "Some" => {
                        self.node(p)?;
                        if self.peek() != Some('(') {
                            return unexpected_char(self.peek(), self.here());
                        }
                        self.nest_in(p)?;
                        self.bump();
                        let saved = std::mem::replace(&mut self.no_struct, false);
                        let inner = self.parse_expr();
                        self.no_struct = saved;
                        self.nest -= 1;
                        let inner = inner?;
                        self.expect_char(')')?;
                        Ok(Expr::Some(Box::new(inner), p))
                    }
                    n if is_reserved(n) => unexpected(n, p),
                    _ if self.peek() == Some('.')
                        && self.rest()[1..].chars().next().is_some_and(is_ident_start) =>
                    {
                        // `module.name` (a variable of another module)
                        self.node(p)?;
                        self.bump();
                        let (member, _) = self.ident()?.expect("識別子の始まりを確かめた");
                        if self.peek() == Some('(') {
                            self.nest_in(p)?;
                            self.bump();
                            let args = self.parse_call_args();
                            self.nest -= 1;
                            return Ok(Expr::QueryCall(name, member, args?, p));
                        }
                        let mpos = self.line.pos(self.pos - member.len());
                        let tr = TypeRef {
                            module: Some((name.clone(), p)),
                            name: member.clone(),
                            pos: mpos,
                        };
                        if let Some(lit) = self.try_struct_literal(tr, p)? {
                            return Ok(lit);
                        }
                        Ok(Expr::Qualified(name, member, p))
                    }
                    _ if self.peek() == Some('(') => {
                        self.node(p)?;
                        self.nest_in(p)?;
                        self.bump();
                        let args = self.parse_call_args();
                        self.nest -= 1;
                        Ok(Expr::Call(name, args?, p))
                    }
                    _ => {
                        let tr = TypeRef {
                            module: None,
                            name: name.clone(),
                            pos: p,
                        };
                        if let Some(lit) = self.try_struct_literal(tr, p)? {
                            return Ok(lit);
                        }
                        Ok(Expr::Name(name, p))
                    }
                }
            }
            Some(c) => unexpected(&c.to_string(), pos),
        }
    }

    /// Reads a lambda `|param, ...| expr` (`|| expr` has no parameters). The body is as long an expression as continues.
    fn parse_lambda(&mut self, pos: Pos) -> PResult<Lambda> {
        let mut params = Vec::new();
        if !self.eat("||")? {
            self.bump();
            self.skip_ws()?;
            if self.peek() == Some('|') {
                self.bump();
            } else {
                loop {
                    let (name, npos) = self.expect_name()?;
                    let ty = if self.eat(":")? {
                        Some(self.parse_type_name()?)
                    } else {
                        None
                    };
                    params.push(LambdaParam {
                        name,
                        pos: npos,
                        ty,
                    });
                    self.skip_ws()?;
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some('|') => {
                            self.bump();
                            break;
                        }
                        c => return unexpected_char(c, self.here()),
                    }
                }
            }
        }
        let saved = std::mem::replace(&mut self.no_struct, false);
        let body = self.parse_expr();
        self.no_struct = saved;
        Ok(Lambda {
            params,
            body: body?,
            pos,
        })
    }
}
