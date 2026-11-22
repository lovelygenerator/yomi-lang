//! Line parsing: blocks and statements (if, while, for, let, commands).

use super::*;

/// The names of loop variables (the first when there are two, the name, its position).
type LoopNames = (Option<(String, Pos)>, String, Pos);

/// The header of `for` / `each` (the names of the loop variables and the expression to iterate).
type LoopHeader = (Option<(String, Pos)>, String, Pos, Expr);

impl<'a> Parser<'a> {
    /// Checks that nothing follows the `}` that closed a block. If something does, an error (if it ends with `{`, that new
    /// block is skipped).
    pub(super) fn check_tail(&mut self, c: Close<'a>) {
        let cl = Line::new(c.raw, c.no);
        let mut ep = ExprParser::new(&cl, c.off);
        if let Err(e) = ep.expect_end() {
            self.push(e);
            if opens_block(c.raw) {
                self.skip_block();
            }
        }
    }

    /// Skips (up to the `}` matching the line that opened `{`).
    pub(super) fn skip_block(&mut self) {
        let mut depth = 1;
        while self.i < self.lines.len() {
            let t = self.lines[self.i].trim_start_matches([' ', '\t']);
            self.i += 1;
            if t.starts_with('}') {
                depth -= 1;
            }
            if opens_block(t) {
                depth += 1;
            }
            if depth == 0 {
                return;
            }
        }
    }

    /// Reads statements up to the `}` line.
    pub(super) fn parse_block(&mut self, open: Pos) -> (Vec<Stmt>, Option<Close<'a>>) {
        if self.depth >= MAX_BLOCK_DEPTH {
            // Read no further (so deep recursion does not exhaust the stack).
            self.push(PErr {
                code: DiagCode::BlockTooDeep,
                args: vec![("limit", DiagArg::Int(i64::from(MAX_BLOCK_DEPTH)))],
                pos: open,
            });
            self.i = self.lines.len();
            self.aborted = true;
            return (Vec::new(), None);
        }
        self.depth += 1;
        let r = self.parse_block_inner(open);
        self.depth -= 1;
        r
    }

    /// Reads the statements from after `{` to the end of the block. Returns the ending line if there is one.
    pub(super) fn parse_block_inner(&mut self, open: Pos) -> (Vec<Stmt>, Option<Close<'a>>) {
        let mut body = Vec::new();
        while self.i < self.lines.len() {
            let raw = self.lines[self.i];
            let no = self.i as u32 + 1;
            self.i += 1;
            let off = indent_len(raw);
            let t = &raw[off..];
            if t.is_empty() || t.starts_with("//") {
                continue;
            }
            if t.starts_with('}') {
                return (
                    body,
                    Some(Close {
                        raw,
                        no,
                        off: off + 1,
                    }),
                );
            }
            let joined = match self.join_statement(raw, no, off, off) {
                Ok(j) => j,
                Err(e) => {
                    self.push(e);
                    continue;
                }
            };
            let last = self.lines[self.i - 1];
            let line = match &joined {
                Some(j) => Line::joined(&j.text, no, j.segs.clone()),
                None => Line::new(raw, no),
            };
            match self.parse_stmt(&line, off) {
                Ok(Some(s)) => body.push(s),
                Ok(None) => {}
                Err(e) => {
                    if starts_block(first_word(t)) && opens_block(last) {
                        self.skip_block();
                    }
                    self.push(e);
                }
            }
        }
        if !self.aborted {
            self.push(PErr {
                code: DiagCode::UnterminatedBlock,
                args: vec![],
                pos: open,
            });
        }
        (body, None)
    }

    /// Reads one statement. Only nesting statements (`if`, `while`, `choice`) are dispatched here; others are left to
    /// [`Parser::parse_simple_stmt`]. This keeps the frame on each round of recursion small, so that nesting up to the limit
    /// ([`MAX_BLOCK_DEPTH`]) fits in the 1 MiB stack of debug builds.
    pub(super) fn parse_stmt(&mut self, line: &Line<'_>, off: usize) -> PResult<Option<Stmt>> {
        match first_word(&line.text[off..]) {
            "if" => self.parse_if(line, off).map(Some),
            "while" => self.parse_while(line, off).map(Some),
            "for" => self.parse_for(line, off).map(Some),
            "match" => self.parse_match(line, off).map(Some),
            "choice" => {
                let pos = line.pos(off);
                self.parse_choice(line, off, pos).map(Some)
            }
            _ if is_each_head(&line.text[off..]) => self.parse_each(line, off),
            _ => self.parse_simple_stmt(line, off),
        }
    }

    /// Reads `expr.each { name =>` (sugar for `for name in expr {`). If it is not in header form, reads it as another
    /// statement.
    pub(super) fn parse_each(&mut self, line: &Line<'_>, off: usize) -> PResult<Option<Stmt>> {
        let Some((key, var, var_pos, iter)) = self.parse_each_header(line, off)? else {
            return self.parse_simple_stmt(line, off);
        };
        let pos = line.pos(off);
        let (body, close) = self.parse_block(pos);
        if let Some(c) = close {
            self.check_tail(c);
        }
        Ok(Some(Stmt::For {
            key,
            var,
            var_pos,
            iter,
            body,
            pos,
        }))
    }

    /// Reads the header of `each` (`expr.each { name =>`). `None` if the expression is not `….each`. The frame is large, so
    /// it is kept off the recursive path.
    #[inline(never)]
    pub(super) fn parse_each_header(
        &mut self,
        line: &Line<'_>,
        off: usize,
    ) -> PResult<Option<LoopHeader>> {
        let mut ep = ExprParser::new_header(line, off);
        // `name.each` is read in the same form as a module variable (`Qualified`)
        let base = match ep.parse_expr() {
            Ok(Expr::Field(base, member, _)) if member == "each" => *base,
            Ok(Expr::Qualified(name, member, pos)) if member == "each" => Expr::Name(name, pos),
            _ => return Ok(None),
        };
        ep.expect_char('{')?;
        let (key, var, var_pos) = loop_names(&mut ep)?;
        if !ep.eat("=>")? {
            return unexpected_char(ep.peek(), ep.here());
        }
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        Ok(Some((key, var, var_pos, base)))
    }

    /// Reads statements that do not nest (and diagnostics for words that cannot be written as statements other than `if`,
    /// `while` and `choice`). The frame is large, so it is kept off the recursive path ([`Parser::parse_stmt`]).
    #[inline(never)]
    pub(super) fn parse_simple_stmt(
        &mut self,
        line: &Line<'_>,
        off: usize,
    ) -> PResult<Option<Stmt>> {
        let t = &line.text[off..];
        let pos = line.pos(off);
        if t.starts_with('@') {
            return self.parse_command(line, off).map(Some);
        }
        let word = first_word(t);
        if is_reserved(word) {
            return match word {
                "let" => self.parse_let(line, off).map(Some),
                "break" | "continue" => {
                    let mut ep = ExprParser::new(line, off + word.len());
                    ep.expect_end()?;
                    Ok(Some(if word == "break" {
                        Stmt::Break(pos)
                    } else {
                        Stmt::Continue(pos)
                    }))
                }
                "else" => err(DiagCode::ElseWithoutIf, vec![], pos),
                "jump" => {
                    let mut ep = ExprParser::new(line, off + 4);
                    let target = parse_label_ref(&mut ep)?;
                    ep.expect_end()?;
                    Ok(Some(Stmt::Jump { target, pos }))
                }
                "call" => {
                    let mut ep = ExprParser::new(line, off + 4);
                    let call = parse_label_call(&mut ep, pos)?;
                    ep.expect_end()?;
                    Ok(Some(Stmt::Call(call)))
                }
                "return" => {
                    let mut ep = ExprParser::new(line, off + "return".len());
                    let value = if ep.at_end()? {
                        None
                    } else {
                        Some(ep.parse_expr()?)
                    };
                    ep.expect_end()?;
                    Ok(Some(Stmt::Return { value, pos }))
                }
                "end" => {
                    let mut ep = ExprParser::new(line, off + 3);
                    ep.expect_end()?;
                    Ok(Some(Stmt::End(pos)))
                }
                w if is_declaration(w) => err(
                    DiagCode::DeclarationNotAllowed,
                    vec![("what", DiagArg::Str(w.to_string()))],
                    pos,
                ),
                // Reserved words that do not start statements, such as `true`, `in` and `int`
                w => unexpected(w, pos),
            };
        }
        if !word.is_empty()
            && let Some(st) = parse_target_stmt(line, off, word, pos)?
        {
            return Ok(Some(st));
        }
        if let Some(q) = self.try_quote(line, off)? {
            return Ok(Some(q));
        }
        let (body_text, id) = split_stable_id(t, line, off)?;
        // A `\` at the start of a line followed by a character that is not an escape: a mark that forces a plain line (the `\`
        // is not kept in the body).
        let mut start = off;
        if let Some(rest) = body_text.strip_prefix('\\')
            && let Some(n) = rest.chars().next()
            && !matches!(n, '「' | '」' | '$' | '\\' | '=' | '#')
        {
            start += 1;
        }
        let parts = parse_body(line, start, off + body_text.len())?;
        // The line starts with `name「` and the name is not an identity: a plain line (warn if there is an identity with a close
        // spelling)
        let typo = {
            let word = first_word(t);
            (!word.is_empty()
                && t[word.len()..].starts_with('「')
                && !self.is_identity(&crate::ident::nfc(word)))
            .then(|| (crate::ident::normalize_name(word), pos))
        };
        Ok(Some(Stmt::Plain {
            parts,
            raw: body_text.to_string(),
            id,
            id_at: line.pos(off + body_text.len()),
            typo,
            pos,
        }))
    }

    /// When the body of an option is one statement, reads it (returned as a `Vec`).
    pub(super) fn parse_stmt_body(&mut self, jl: &Line<'_>, at: usize) -> PResult<Vec<Stmt>> {
        let mut body = Vec::new();
        body.extend(self.parse_stmt(jl, at)?);
        Ok(body)
    }

    /// Reads `{ stmt; stmt }` (a one-line block). `after` is the byte position right after `{`.
    pub(super) fn parse_inline_block(
        &mut self,
        line: &Line<'_>,
        after: usize,
        open: Pos,
    ) -> PResult<Vec<Stmt>> {
        let code = strip_comment(&line.text[after..]).trim_end_matches([' ', '\t']);
        let Some(inner) = code.strip_suffix('}') else {
            return err(DiagCode::UnterminatedBlock, vec![], open);
        };
        let mut body = Vec::new();
        for (a, b) in split_statements(inner) {
            let (start, end) = (after + a, after + b);
            let sl = line.truncated(end);
            let word = first_word(&sl.text[start..]);
            if is_block_statement(word) {
                // Statements that open blocks cannot be written in a one-line block
                self.push(unexpected::<()>(word, sl.pos(start)).unwrap_err());
                continue;
            }
            match self.parse_stmt(&sl, start) {
                Ok(Some(s)) => body.push(s),
                Ok(None) => {}
                Err(e) => self.push(e),
            }
        }
        Ok(body)
    }

    pub(super) fn parse_let(&mut self, line: &Line<'_>, off: usize) -> PResult<Stmt> {
        let pos = line.pos(off);
        let mut ep = ExprParser::new(line, off + 3);
        let (name, name_pos) = ep.expect_name()?;
        let rest = (|| {
            ep.skip_ws()?;
            let ty = if ep.peek() == Some(':') {
                ep.bump();
                Some(ep.parse_type_name()?)
            } else {
                None
            };
            ep.expect_char('=')?;
            let init = parse_value(&mut ep)?;
            ep.expect_end()?;
            Ok((ty, init))
        })();
        match rest {
            Ok((ty, init)) => Ok(Stmt::Let {
                name,
                name_pos,
                ty,
                init,
                pos,
            }),
            Err(e) => {
                self.out.poisoned.insert(name);
                Err(e)
            }
        }
    }

    /// Reads the header of `if` / `while` (the condition and `{`). If characters follow `{`, records an error and still reads
    /// the body. If the header cannot be read, `Err` (the caller skips the body).
    pub(super) fn parse_header(&mut self, line: &Line<'_>, after_kw: usize) -> PResult<Expr> {
        let mut ep = ExprParser::new_header(line, after_kw);
        let cond = ep.parse_expr()?;
        ep.expect_char('{')?;
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        Ok(cond)
    }

    pub(super) fn parse_while(&mut self, line: &Line<'_>, off: usize) -> PResult<Stmt> {
        let pos = line.pos(off);
        let cond = self.parse_header(line, off + "while".len())?;
        let (body, close) = self.parse_block(pos);
        if let Some(c) = close {
            self.check_tail(c);
        }
        Ok(Stmt::While { cond, body, pos })
    }

    /// Reads `for name in expr {` (characters after `{` are handled as for `while`).
    pub(super) fn parse_for(&mut self, line: &Line<'_>, off: usize) -> PResult<Stmt> {
        let pos = line.pos(off);
        let (key, var, var_pos, iter) = self.parse_for_header(line, off + "for".len())?;
        let (body, close) = self.parse_block(pos);
        if let Some(c) = close {
            self.check_tail(c);
        }
        Ok(Stmt::For {
            key,
            var,
            var_pos,
            iter,
            body,
            pos,
        })
    }

    /// Reads the header of `for` (`name in expr {`). The frame is large, so it is kept off the recursive path
    /// ([`Parser::parse_for`]).
    #[inline(never)]
    pub(super) fn parse_for_header(
        &mut self,
        line: &Line<'_>,
        after_kw: usize,
    ) -> PResult<LoopHeader> {
        let mut ep = ExprParser::new_header(line, after_kw);
        let (key, var, var_pos) = loop_names(&mut ep)?;
        match ep.ident()? {
            Some((w, _)) if w == "in" => {}
            Some((w, p)) => return unexpected(&w, p),
            None => return unexpected_char(ep.peek(), ep.here()),
        }
        let iter = ep.parse_expr()?;
        ep.expect_char('{')?;
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        Ok((key, var, var_pos, iter))
    }

    /// Reads an `if`. `kw` is the byte position of `if` in the line. Chains of `else if` are read with a loop (so the length
    /// of a chain does not deepen the recursion).
    pub(super) fn parse_if(&mut self, line: &Line<'_>, kw: usize) -> PResult<Stmt> {
        let pos = line.pos(kw);
        let cond = self.parse_header(line, kw + "if".len())?;
        let (body, close) = self.parse_block(pos);
        let mut arms = vec![(cond, body)];
        let mut else_body = Vec::new();
        let mut next = close;
        while let Some(c) = next {
            match self.parse_else_tail(c) {
                Tail::End => break,
                Tail::Else(body) => {
                    else_body = body;
                    break;
                }
                Tail::ElseIf(cond, body, close) => {
                    arms.push((cond, body));
                    next = close;
                }
            }
        }
        Ok(Stmt::If {
            arms,
            else_body,
            pos,
        })
    }

    /// Reads what follows the `}` that closed the body of an `if` (such as `else`). Errors are recorded, and it reads as far
    /// as it can.
    pub(super) fn parse_else_tail(&mut self, c: Close<'a>) -> Tail<'a> {
        match self.classify_else_tail(c) {
            TailKind::End => Tail::End,
            TailKind::Else(wp) => Tail::Else(self.parse_else_block(wp)),
            TailKind::ElseIf(cond, pos) => {
                let (body, close) = self.parse_block(pos);
                Tail::ElseIf(cond, body, close)
            }
        }
    }

    /// Whether what follows `}` is `else` or `else if` (the non-recursive part before reading the body). The frame is large,
    /// so it is separated from the recursive path ([`Parser::parse_else_tail`]).
    #[inline(never)]
    pub(super) fn classify_else_tail(&mut self, c: Close<'a>) -> TailKind {
        let cl = Line::new(c.raw, c.no);
        let mut ep = ExprParser::new(&cl, c.off);
        match ep.at_end() {
            Ok(true) => return TailKind::End,
            Ok(false) => {}
            Err(e) => {
                self.push(e);
                return TailKind::End;
            }
        }
        let word = match ep.ident() {
            Ok(w) => w,
            Err(e) => {
                self.push(e);
                return TailKind::End;
            }
        };
        let Some((w, wp)) = word else {
            // Characters that are not words (such as `} )`)
            if let Err(e) = ep.expect_end() {
                self.push(e);
            }
            if opens_block(c.raw) {
                self.skip_block();
            }
            return TailKind::End;
        };
        if w != "else" {
            self.push(unexpected::<()>(&w, wp).unwrap_err());
            if opens_block(c.raw) {
                self.skip_block();
            }
            return TailKind::End;
        }
        if let Err(e) = ep.skip_ws() {
            self.push(e);
            return TailKind::End;
        }
        if ep.peek() == Some('{') {
            ep.bump();
            if let Err(e) = ep.expect_end() {
                self.push(e);
            }
            return TailKind::Else(wp);
        }
        let at = ep.pos;
        if first_word(&c.raw[at..]) == "if" {
            let pos = cl.pos(at);
            return match self.parse_header(&cl, at + "if".len()) {
                Ok(cond) => TailKind::ElseIf(cond, pos),
                Err(e) => {
                    self.push(e);
                    if opens_block(c.raw) {
                        self.skip_block();
                    }
                    TailKind::End
                }
            };
        }
        // What follows `else` is neither `{` nor `if`. If the line ends with `{`, the body is still read.
        match ep.expect_end() {
            Err(e) => self.push(e),
            Ok(()) => self.push(unexpected_eol::<()>(ep.here()).unwrap_err()),
        }
        if opens_block(c.raw) {
            TailKind::Else(wp)
        } else {
            TailKind::End
        }
    }

    /// Reads the body of `else {` and checks what follows the closing `}`.
    pub(super) fn parse_else_block(&mut self, open: Pos) -> Vec<Stmt> {
        let (body, close) = self.parse_block(open);
        if let Some(c) = close {
            self.check_tail(c);
        }
        body
    }

    pub(super) fn parse_command(&mut self, line: &Line<'_>, off: usize) -> PResult<Stmt> {
        let mut ep = ExprParser::new(line, off);
        let mut call = ep.parse_command_call()?;
        call.id = read_stable_id(&mut ep)?;
        ep.expect_end()?;
        Ok(Stmt::Command(call))
    }
}

/// Whether the line looks like `….each { name =>` (a candidate for the `each` sugar; [`Parser::parse_each_header`]
/// checks it).
/// Reads the names of loop variables (`name` or `name1, name2`). With two, returns the first as `key`.
fn loop_names(ep: &mut ExprParser<'_>) -> PResult<LoopNames> {
    let (first, first_pos) = ep.expect_name()?;
    if ep.eat(",")? {
        let (var, var_pos) = ep.expect_name()?;
        return Ok((Some((first, first_pos)), var, var_pos));
    }
    Ok((None, first, first_pos))
}

pub(super) fn is_each_head(t: &str) -> bool {
    let head = t.split("//").next().unwrap_or(t).trim_end();
    head.ends_with("=>") && head.contains(".each")
}
