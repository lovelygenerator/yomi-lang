//! Line parsing: `match` and `choice` (arms and options).

use super::*;

impl<'a> Parser<'a> {
    /// Reads from `match expr {` to the matching `}`.
    pub(super) fn parse_match(&mut self, line: &Line<'_>, off: usize) -> PResult<Stmt> {
        let pos = line.pos(off);
        let scrutinee = self.parse_match_header(line, off)?;
        if self.depth >= MAX_BLOCK_DEPTH {
            return Ok(self.match_too_deep(pos, scrutinee));
        }
        self.depth += 1;
        let arms = self.parse_arms(pos);
        self.depth -= 1;
        Ok(Stmt::Match {
            scrutinee,
            arms,
            pos,
        })
    }

    /// The header of `match` (the subject expression and `{`). If characters follow `{`, records an error and still reads the
    /// body.
    #[inline(never)]
    pub(super) fn parse_match_header(&mut self, line: &Line<'_>, off: usize) -> PResult<Expr> {
        let mut ep = ExprParser::new_header(line, off + "match".len());
        let scrutinee = ep.parse_expr()?;
        ep.expect_char('{')?;
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        Ok(scrutinee)
    }

    /// A `match` nested too deeply: emits an error and reads no further.
    #[inline(never)]
    pub(super) fn match_too_deep(&mut self, pos: Pos, scrutinee: Expr) -> Stmt {
        self.push(PErr {
            code: DiagCode::BlockTooDeep,
            args: vec![("limit", DiagArg::Int(i64::from(MAX_BLOCK_DEPTH)))],
            pos,
        });
        self.i = self.lines.len();
        self.aborted = true;
        Stmt::Match {
            scrutinee,
            arms: Vec::new(),
            pos,
        }
    }

    /// Reads arms up to the `}` line.
    pub(super) fn parse_arms(&mut self, open: Pos) -> Vec<MatchArm> {
        let mut arms = Vec::new();
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
                self.check_tail(Close {
                    raw,
                    no,
                    off: off + 1,
                });
                return arms;
            }
            let line = Line::new(raw, no);
            match self.parse_arm(&line, off) {
                Ok(a) => arms.push(a),
                Err(e) => {
                    // If the line opens the block of a body, skip it (so errors do not cascade)
                    if opens_block(self.lines[self.i - 1]) {
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
        arms
    }

    /// Reads one arm (`pattern => body`).
    pub(super) fn parse_arm(&mut self, line: &Line<'_>, off: usize) -> PResult<MatchArm> {
        let pos = line.pos(off);
        let (pattern, body_at) = parse_arm_head(line, off)?;
        let body = self.parse_arm_body(line, body_at)?;
        Ok(MatchArm { pattern, body, pos })
    }

    /// Reads the body of an arm (after `=>`; a `{ }` block or one statement; a trailing `,` is allowed).
    pub(super) fn parse_arm_body(&mut self, line: &Line<'_>, body_at: usize) -> PResult<Vec<Stmt>> {
        let code = strip_comment(line.text).trim_end_matches([' ', '\t']);
        let end = code.strip_suffix(',').map_or(code.len(), str::len);
        let jl = line.truncated(end.max(body_at));
        match classify_option_body(&jl, body_at)? {
            OptionBody::Block(open) => {
                let (body, close) = self.parse_block(open);
                if let Some(c) = close {
                    self.check_arm_tail(c);
                }
                Ok(body)
            }
            OptionBody::Inline(after, open) => self.parse_inline_block(&jl, after, open),
            OptionBody::Stmt(at) => self.parse_stmt_body(&jl, at),
        }
    }

    /// After the `}` that closed the block of an arm, only `,` can be written.
    pub(super) fn check_arm_tail(&mut self, c: Close<'a>) {
        if strip_comment(&c.raw[c.off..]).trim() == "," {
            return;
        }
        self.check_tail(c);
    }

    /// Reads from `choice {` to the matching `}` (the position of `choice` is `pos`).
    pub(super) fn parse_choice(&mut self, line: &Line<'_>, off: usize, pos: Pos) -> PResult<Stmt> {
        self.parse_choice_open(line, off)?;
        if self.depth >= MAX_BLOCK_DEPTH {
            return Ok(self.choice_too_deep(pos));
        }
        self.depth += 1;
        let options = self.parse_options(pos);
        self.depth -= 1;
        Ok(Stmt::Choice { options, pos })
    }

    /// Checks the `{` of the `choice` line and what follows it.
    #[inline(never)]
    pub(super) fn parse_choice_open(&mut self, line: &Line<'_>, off: usize) -> PResult<()> {
        let mut ep = ExprParser::new(line, off + "choice".len());
        ep.expect_char('{')?;
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        Ok(())
    }

    /// A `choice` nested too deeply: emits an error and reads no further.
    #[inline(never)]
    pub(super) fn choice_too_deep(&mut self, pos: Pos) -> Stmt {
        self.push(PErr {
            code: DiagCode::BlockTooDeep,
            args: vec![("limit", DiagArg::Int(i64::from(MAX_BLOCK_DEPTH)))],
            pos,
        });
        self.i = self.lines.len();
        self.aborted = true;
        Stmt::Choice {
            options: Vec::new(),
            pos,
        }
    }

    /// Reads options up to the `}` line.
    pub(super) fn parse_options(&mut self, open: Pos) -> Vec<ChoiceOpt> {
        let mut options = Vec::new();
        // The number of lines that were attempted as options (including those that could not be read)
        let mut attempted = 0;
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
                self.check_tail(Close {
                    raw,
                    no,
                    off: off + 1,
                });
                if attempted == 0 {
                    self.push(PErr {
                        code: DiagCode::EmptyChoice,
                        args: vec![],
                        pos: open,
                    });
                }
                return options;
            }
            attempted += 1;
            let joined = match self.join_option(raw, no, off) {
                Ok(j) => j,
                Err(e) => {
                    self.push(e);
                    continue;
                }
            };
            let line = match &joined {
                Some(j) => Line::joined(&j.text, no, j.segs.clone()),
                None => Line::new(raw, no),
            };
            match self.parse_option(&line, off) {
                Ok(o) => options.push(o),
                Err(e) => {
                    // If the line opens the block of a body, skip it (so errors do not cascade)
                    if opens_block(self.lines[self.i - 1]) {
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
        options
    }

    /// Reads one option (`「text」 [once] [if cond] [#id] => body`).
    ///
    /// To keep the frame on each round of recursion small (a body can contain `choice` and the like), it is split into the
    /// head up to `=>` ([`Parser::parse_option_head`]) and the body ([`Parser::parse_option_body`]).
    pub(super) fn parse_option(&mut self, line: &Line<'_>, off: usize) -> PResult<ChoiceOpt> {
        let mut opt = ChoiceOpt::empty(line.pos(off));
        let (joined, body_at) = self.parse_option_head(line, off, &mut opt)?;
        opt.body = self.parse_option_body(&joined, line.no, body_at)?;
        Ok(opt)
    }

    /// Reads an option up to `=>`.
    #[inline(never)]
    pub(super) fn parse_option_head(
        &mut self,
        line: &Line<'_>,
        off: usize,
        out: &mut ChoiceOpt,
    ) -> PResult<(Joined, usize)> {
        let t = &line.text[off..];
        let pos = line.pos(off);
        if !t.starts_with('「') {
            return Err(ExprParser::new(line, off)
                .expect_end()
                .expect_err("空でない行"));
        }
        let following = &self.lines[self.i.min(self.lines.len())..];
        let Some(joined) = join_quote(line, off, following, self.i as u32 + 1) else {
            return err(DiagCode::UnterminatedQuote, vec![], pos);
        };
        self.i += joined.consumed;
        let jl = Line::joined(&joined.text, line.no, joined.segs.clone());
        let parts = parse_body(&jl, off, joined.end)?;
        let mut ep = ExprParser::new(&jl, joined.end);
        ep.skip_ws()?;
        let once = if first_word(ep.rest()) == "once" {
            let p = ep.here();
            ep.pos += "once".len();
            Some(p)
        } else {
            None
        };
        ep.skip_ws()?;
        let cond = if first_word(ep.rest()) == "if" {
            ep.pos += "if".len();
            Some(ep.parse_expr()?)
        } else {
            None
        };
        let id = read_stable_id(&mut ep)?;
        ep.skip_ws()?;
        let id_at = ep.here();
        if ep.rest().starts_with("=>") {
            ep.pos += 2;
        } else if ep.rest().is_empty() {
            return unexpected_eol(ep.here());
        } else {
            return unexpected(&ep.found_here(), ep.here());
        }
        ep.skip_ws()?;
        let body_at = ep.pos;
        *out = ChoiceOpt {
            parts,
            once,
            cond,
            id,
            id_at,
            body: Vec::new(),
            pos,
        };
        Ok((joined, body_at))
    }

    /// Reads the body of an option (after `=>`; a `{ }` block or one statement). `joined` is the line with the option text
    /// joined, and `body_at` is the starting byte position of the body.
    pub(super) fn parse_option_body(
        &mut self,
        joined: &Joined,
        no: u32,
        body_at: usize,
    ) -> PResult<Vec<Stmt>> {
        let jl = Line::joined(&joined.text, no, joined.segs.clone());
        match classify_option_body(&jl, body_at)? {
            OptionBody::Block(open) => {
                // A multi-line block
                let (body, close) = self.parse_block(open);
                if let Some(c) = close {
                    self.check_tail(c);
                }
                Ok(body)
            }
            OptionBody::Inline(after, open) => self.parse_inline_block(&jl, after, open),
            OptionBody::Stmt(at) => self.parse_stmt_body(&jl, at),
        }
    }
}
