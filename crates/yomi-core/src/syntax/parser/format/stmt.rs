//! The formatter: statements, `match` arms and `choice` options.

use super::block::is_each_head;
use super::continuation::is_block_header;
use super::layout;
use super::*;

/// What follows `=>` in an option or an arm.
struct Body {
    text: String,
    opens: bool,
}

impl<'a> Formatter<'a> {
    pub(super) fn statement_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        let t = &raw[off..];
        let word = first_word(t);
        let joined = syntax(self.p.join_statement(raw, no, off, off))?;
        let mut comment = comment_of(self.p.lines[self.p.i - 1]);
        let marked;
        let mut pair = None;
        if let Some(j) = &joined
            && let Some(l) = layout::Layout::new(j, is_block_header(t))
        {
            let (m, used) = l.marked_text();
            marked = m;
            comment = None;
            pair = Some((l, used));
        } else {
            marked = String::new();
        }
        let line = match (&joined, &pair) {
            (Some(_), Some(_)) => Line::new(&marked, no),
            (Some(j), None) => Line::joined(&j.text, no, j.segs.clone()),
            _ => Line::new(raw, no),
        };
        self.layout = pair;
        match word {
            "if" | "while" => {
                let cond = syntax(self.header_cond(&line, off + word.len()))?;
                self.open(
                    format!("{word} {} {{", canon::expr(&cond)),
                    comment,
                    Ctx::Block,
                );
            }
            "match" => {
                let cond = syntax(self.header_cond(&line, off + word.len()))?;
                self.open(
                    format!("match {} {{", canon::expr(&cond)),
                    comment,
                    Ctx::Match,
                );
            }
            "for" => {
                let (key, var, _, iter) = syntax(self.p.parse_for_header(&line, off + 3))?;
                let names = names_text(&key, &var);
                self.open(
                    format!("for {names} in {} {{", canon::expr(&iter)),
                    comment,
                    Ctx::Block,
                );
            }
            "choice" => {
                let g = self.groups;
                self.groups += 1;
                self.open("choice {".to_string(), comment, Ctx::Choice(g));
            }
            _ if is_each_head(t) => match syntax(self.p.parse_each_header(&line, off))? {
                Some((key, var, _, iter)) => {
                    let names = names_text(&key, &var);
                    self.open(
                        format!("{}.each {{ {names} =>", canon::expr(&iter)),
                        comment,
                        Ctx::Block,
                    );
                }
                None => self.simple_line(raw, &line, off, comment)?,
            },
            _ => self.simple_line(raw, &line, off, comment)?,
        }
        Ok(())
    }

    fn open(&mut self, text: String, comment: Option<String>, ctx: Ctx) {
        self.line(with_comment(text, comment));
        self.ctx.push(ctx);
    }

    /// A statement that does not open a block, a quote line or a plain line.
    fn simple_line(
        &mut self,
        raw: &'a str,
        line: &Line<'_>,
        off: usize,
        comment: Option<String>,
    ) -> FResult<()> {
        let first_i = self.p.i;
        let st = syntax(self.p.parse_simple_stmt(line, off))?;
        let Some(st) = st else {
            return Err(FormatError::Unverified);
        };
        let d = self.depth();
        match &st {
            Stmt::Plain { raw: body, id, .. } => {
                self.out.push(Out::Text(d, with_id(body.clone(), id)));
            }
            Stmt::Quote { id, .. } => {
                let consumed = self.p.i - first_i;
                self.quote_lines(raw, off, consumed, id);
            }
            other => {
                let text = stmt_text(other);
                self.line(with_comment(text, comment));
            }
        }
        Ok(())
    }

    /// A quote line over `1 + consumed` physical lines. The text is kept as written (the spaces at the ends of the lines
    /// too), the later lines are indented one level deeper.
    fn quote_lines(&mut self, raw: &str, off: usize, consumed: usize, id: &Option<StableId>) {
        let d = self.depth();
        let first_no = self.p.i - consumed - 1;
        if consumed == 0 {
            self.out
                .push(Out::Text(d, with_id(raw[off..].to_string(), id)));
            return;
        }
        self.out.push(Out::Text(d, raw[off..].to_string()));
        for k in 1..=consumed {
            let l = self.p.lines[first_no + k];
            let stripped = l.trim_start_matches([' ', '\t']);
            if k == consumed {
                self.out
                    .push(Out::Text(d + 1, with_id(stripped.to_string(), id)));
            } else {
                self.out.push(Out::Keep(d + 1, stripped.to_string()));
            }
        }
    }

    /// The canonical text of one statement that does not open a block (the part of `line` from `at`).
    fn simple_text(&mut self, line: &Line<'_>, at: usize) -> FResult<String> {
        let st = syntax(self.p.parse_stmt(line, at))?;
        Ok(match st {
            Some(Stmt::Plain { raw, id, .. }) => with_id(raw, &id),
            Some(Stmt::Quote { id, .. }) => with_id(line.text[at..].to_string(), &id),
            Some(other) => stmt_text(&other),
            None => line.text[at..].trim_end().to_string(),
        })
    }

    /// The body after `=>` (an option or an arm): `{` (a block opens on this line), `{ a; b }`, or one statement.
    fn body_text(&mut self, jl: &Line<'_>, body_at: usize) -> FResult<Body> {
        Ok(match syntax(classify_option_body(jl, body_at))? {
            OptionBody::Block(_) => Body {
                text: "{".to_string(),
                opens: true,
            },
            OptionBody::Inline(after, _) => {
                let code = strip_comment(&jl.text[after..]).trim_end_matches([' ', '\t']);
                let inner = code.strip_suffix('}').unwrap_or(code);
                let mut parts = Vec::new();
                for (a, b) in split_statements(inner) {
                    let sl = jl.truncated(after + b);
                    parts.push(self.simple_text(&sl, after + a)?);
                }
                let text = if parts.is_empty() {
                    "{}".to_string()
                } else {
                    format!("{{ {} }}", parts.join("; "))
                };
                Body { text, opens: false }
            }
            OptionBody::Stmt(at) => Body {
                text: self.simple_text(jl, at)?,
                opens: false,
            },
        })
    }

    pub(super) fn arm_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        if raw[off..].starts_with('}') {
            return self.close_line(raw, no, off);
        }
        let line = Line::new(raw, no);
        let comment = comment_of(raw);
        let mut ep = ExprParser::new(&line, off);
        syntax(ep.parse_pattern())?;
        let pattern = line.text[off..ep.pos].trim().to_string();
        let (_, body_at) = syntax(parse_arm_head(&line, off))?;
        let code = strip_comment(line.text).trim_end_matches([' ', '\t']);
        let end = code.strip_suffix(',').map_or(code.len(), str::len);
        let jl = line.truncated(end.max(body_at));
        let body = self.body_text(&jl, body_at)?;
        let text = with_comment(format!("{pattern} => {}", body.text), comment);
        self.line(text);
        if body.opens {
            self.ctx.push(Ctx::Block);
        }
        Ok(())
    }

    pub(super) fn option_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        if raw[off..].starts_with('}') {
            return self.close_line(raw, no, off);
        }
        let joined_line = syntax(self.p.join_option(raw, no, off))?;
        let line = match &joined_line {
            Some(j) => Line::joined(&j.text, no, j.segs.clone()),
            None => Line::new(raw, no),
        };
        let first_i = self.p.i;
        let following = &self.p.lines[self.p.i.min(self.p.lines.len())..];
        let Some(joined) = join_quote(&line, off, following, self.p.i as u32 + 1) else {
            return Err(FormatError::Unverified);
        };
        self.p.i += joined.consumed;
        let jl = Line::joined(&joined.text, line.no, joined.segs.clone());
        let mut ep = ExprParser::new(&jl, joined.end);
        syntax(ep.skip_ws())?;
        let mut head_rest = String::new();
        if first_word(ep.rest()) == "once" {
            ep.pos += "once".len();
            head_rest.push_str(" once");
        }
        syntax(ep.skip_ws())?;
        if first_word(ep.rest()) == "if" {
            ep.pos += "if".len();
            let cond = syntax(ep.parse_expr())?;
            head_rest.push_str(&format!(" if {}", canon::expr(&cond)));
        }
        if let Some(id) = syntax(read_stable_id(&mut ep))? {
            head_rest.push_str(&format!(" #{}", id.id));
        }
        syntax(ep.skip_ws())?;
        if !ep.rest().starts_with("=>") {
            return Err(FormatError::Unverified);
        }
        ep.pos += 2;
        syntax(ep.skip_ws())?;
        let body_at = ep.pos;
        // A `//` inside the text of the quote is not a comment
        let tail = &jl.text[joined.end..];
        let comment = {
            let rest = &tail[strip_comment(tail).len()..];
            (!rest.is_empty()).then(|| rest.trim_end().to_string())
        };
        let body = self.body_text(&jl, body_at)?;
        let d = self.depth();
        let body_text = with_comment(body.text.clone(), comment);
        let Some(Ctx::Choice(group)) = self.ctx.last() else {
            return Err(FormatError::Unverified);
        };
        let group = *group;
        if joined.consumed == 0 {
            let head = format!("{}{head_rest}", &jl.text[off..joined.end]);
            let width = display_width(&head);
            self.out.push(Out::Opt {
                depth: d,
                group,
                head,
                width,
                body: body_text,
            });
        } else {
            let first_no = first_i - 1;
            self.out.push(Out::Text(d, raw[off..].to_string()));
            for k in 1..=joined.consumed {
                let l = self.p.lines[first_no + k];
                let stripped = l.trim_start_matches([' ', '\t']);
                if k == joined.consumed {
                    let seg_start = joined.segs.last().map_or(0, |s| s.start);
                    let upto = &stripped[..joined.end - seg_start];
                    self.out.push(Out::Text(
                        d + 1,
                        format!("{upto}{head_rest} => {body_text}"),
                    ));
                } else {
                    self.out.push(Out::Keep(d + 1, stripped.to_string()));
                }
            }
        }
        if body.opens {
            self.ctx.push(Ctx::Block);
        }
        Ok(())
    }
}

fn names_text(key: &Option<(String, Pos)>, var: &str) -> String {
    match key {
        Some((k, _)) => format!("{k}, {var}"),
        None => var.to_string(),
    }
}

fn with_id(text: String, id: &Option<StableId>) -> String {
    match id {
        Some(id) => {
            let t = text.trim_end_matches([' ', '\t']);
            let t = t.strip_suffix(&format!("#{}", id.id)).unwrap_or(t);
            format!("{} #{}", t.trim_end_matches([' ', '\t']), id.id)
        }
        None => text.trim_end_matches([' ', '\t']).to_string(),
    }
}

fn value_text(e: &Expr) -> String {
    match e {
        Expr::Command(c) => match &c.id {
            Some(id) => format!("{} #{}", canon::expr(e), id.id),
            None => canon::expr(e),
        },
        _ => canon::expr(e),
    }
}

fn target_text(module: &Option<(String, Pos)>, name: &str, path: &[PathElem]) -> String {
    let mut s = String::new();
    if let Some((m, _)) = module {
        s.push_str(m);
        s.push('.');
    }
    s.push_str(name);
    for e in path {
        match e {
            PathElem::Field(f, _) => {
                s.push('.');
                s.push_str(f);
            }
            PathElem::Index(i, _) => {
                s.push('[');
                s.push_str(&canon::expr(i));
                s.push(']');
            }
        }
    }
    s
}

fn stmt_text(st: &Stmt) -> String {
    match st {
        Stmt::Let { name, ty, init, .. } => match ty {
            Some(t) => format!("let {name}: {} = {}", canon::ty(t), value_text(init)),
            None => format!("let {name} = {}", value_text(init)),
        },
        Stmt::Assign {
            module,
            name,
            path,
            op,
            value,
            ..
        } => {
            let op = match op {
                AssignOp::Set => "=",
                AssignOp::Add => "+=",
                AssignOp::Sub => "-=",
                AssignOp::Mul => "*=",
                AssignOp::Div => "/=",
                AssignOp::Rem => "%=",
            };
            format!(
                "{} {op} {}",
                target_text(module, name, path),
                value_text(value)
            )
        }
        Stmt::ListMethod {
            module,
            name,
            path,
            method,
            args,
            ..
        } => format!(
            "{}.{method}({})",
            target_text(module, name, path),
            canon::args(args)
        ),
        Stmt::Break(_) => "break".to_string(),
        Stmt::Continue(_) => "continue".to_string(),
        Stmt::End(_) => "end".to_string(),
        Stmt::Jump { target, .. } => format!("jump {}", target.written()),
        Stmt::Call(c) => format!("call {}({})", c.target.written(), canon::args(&c.args)),
        Stmt::Return { value, .. } => match value {
            Some(v) => format!("return {}", canon::expr(v)),
            None => "return".to_string(),
        },
        Stmt::Command(c) => match &c.id {
            Some(id) => format!("@{}({}) #{}", c.name, canon::args(&c.args), id.id),
            None => format!("@{}({})", c.name, canon::args(&c.args)),
        },
        _ => String::new(),
    }
}
