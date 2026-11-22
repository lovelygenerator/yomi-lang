//! The formatter: declarations.

use super::*;

impl<'a> Formatter<'a> {
    /// A line outside labels: a declaration.
    pub(super) fn top_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        let t = &raw[off..];
        let word = first_word(t);
        if t.starts_with('}') {
            return Err(FormatError::Unverified);
        }
        let joined = if matches!(word, "save" | "persistent") {
            syntax(self.p.join_statement(raw, no, off, off))?
        } else {
            None
        };
        let comment = comment_of(self.p.lines[self.p.i - 1]);
        let line = match &joined {
            Some(j) => Line::joined(&j.text, no, j.segs.clone()),
            None => Line::new(raw, no),
        };
        match word {
            "import" => {
                let code = strip_comment(raw).trim();
                let text = import_text(&code["import".len()..])?;
                self.line(with_comment(format!("import {text}"), comment));
            }
            "identity" => {
                let mut ep = ExprParser::new(&line, off + "identity".len());
                let (name, _) = syntax(ep.expect_name())?;
                syntax(ep.expect_char('='))?;
                syntax(ep.skip_ws())?;
                let value = syntax(ep.parse_expr())?;
                self.line(with_comment(
                    format!("identity {name} = {}", canon::expr(&value)),
                    comment,
                ));
            }
            "save" | "persistent" => {
                let mut ep = ExprParser::new(&line, off + word.len());
                syntax(ep.ident())?;
                let (name, _) = syntax(ep.expect_name())?;
                syntax(ep.expect_char(':'))?;
                let ty = syntax(ep.parse_type_name())?;
                syntax(ep.expect_char('='))?;
                let init = syntax(ep.parse_expr())?;
                self.line(with_comment(
                    format!(
                        "{word} var {name}: {} = {}",
                        canon::ty(&ty),
                        canon::expr(&init)
                    ),
                    comment,
                ));
            }
            "label" | "fn" => self.label_header(&line, off, word == "fn", comment)?,
            "struct" | "enum" => self.type_decl(raw, no, off, word == "enum")?,
            _ => return Err(FormatError::Unverified),
        }
        Ok(())
    }

    fn label_header(
        &mut self,
        line: &Line<'_>,
        off: usize,
        is_fn: bool,
        comment: Option<String>,
    ) -> FResult<()> {
        let kw = if is_fn { "fn" } else { "label" };
        let mut ep = ExprParser::new(line, off + kw.len());
        let (name, _) = syntax(ep.expect_name())?;
        let mut params = Vec::new();
        let mut ret = None;
        let mut parens = is_fn;
        syntax(ep.skip_ws())?;
        if is_fn {
            syntax(ep.expect_char('('))?;
        }
        if is_fn || ep.peek() == Some('(') {
            if !is_fn {
                ep.bump();
            }
            parens = true;
            params = syntax(parse_params(&mut ep))?;
            syntax(ep.skip_ws())?;
            if ep.rest().starts_with("->") {
                ep.pos += 2;
                ret = Some(syntax(parse_type(&mut ep))?);
            }
        }
        syntax(ep.expect_char('{'))?;
        let mut text = format!("{kw} {name}");
        if parens && (is_fn || !params.is_empty() || ret.is_some()) {
            let ps: Vec<String> = params
                .iter()
                .map(|p| match &p.default {
                    Some(d) => format!("{}: {} = {}", p.name, canon::ty(&p.ty), canon::expr(d)),
                    None => format!("{}: {}", p.name, canon::ty(&p.ty)),
                })
                .collect();
            text.push_str(&format!("({})", ps.join(", ")));
            if let Some(r) = &ret {
                text.push_str(&format!(" -> {}", canon::ty(r)));
            }
        }
        text.push_str(" {");
        self.line(with_comment(text, comment));
        self.ctx.push(Ctx::Block);
        Ok(())
    }

    /// A `struct` or `enum`. One line stays one line. Over several lines, each field or variant gets a line, and the
    /// types of the fields of a `struct` are aligned (8.5, 8.6).
    fn type_decl(&mut self, raw: &'a str, no: u32, off: usize, is_enum: bool) -> FResult<()> {
        let kw = if is_enum { "enum" } else { "struct" };
        let mut phys = vec![raw];
        let first = strip_comment(raw).trim_end_matches([' ', '\t']);
        let mut depth: i32 = brace_balance(first);
        let mut text = first.to_string();
        let mut segs = vec![Seg {
            start: 0,
            line: no,
            col_base: 0,
        }];
        while first.contains('{') && depth > 0 && self.p.i < self.p.lines.len() {
            let l = self.p.lines[self.p.i];
            let n = self.p.i as u32 + 1;
            self.p.i += 1;
            let code = strip_comment(l);
            let stripped = code.trim_start_matches([' ', '\t']);
            text.push(' ');
            segs.push(Seg {
                start: text.len(),
                line: n,
                col_base: (code.len() - stripped.len()) as u32,
            });
            text.push_str(stripped.trim_end_matches([' ', '\t']));
            depth += brace_balance(stripped);
            phys.push(l);
        }
        let jl = Line::joined(&text, no, segs);
        let mut ep = ExprParser::new(&jl, off + kw.len());
        let (name, _) = syntax(ep.expect_name())?;
        syntax(ep.expect_char('{'))?;
        // The items: (line number, text without the separator, width of the name part for a struct)
        let items: Vec<(u32, String, String)> = if is_enum {
            syntax(parse_variants(&mut ep))?
                .into_iter()
                .map(|v| {
                    let t = if v.args.is_empty() {
                        v.name.clone()
                    } else {
                        let a: Vec<String> = v.args.iter().map(canon::ty).collect();
                        format!("{}({})", v.name, a.join(", "))
                    };
                    (v.pos.line, String::new(), t)
                })
                .collect()
        } else {
            syntax(parse_fields(&mut ep))?
                .into_iter()
                .map(|(n, pos, ty)| (pos.line, format!("{n}:"), canon::ty(&ty)))
                .collect()
        };
        let comment = comment_of(phys[phys.len() - 1]);
        let has_comment_lines = phys.iter().skip(1).any(|l| comment_of(l).is_some());
        if phys.len() == 1 || (items.is_empty() && !has_comment_lines) {
            let body: Vec<String> = items
                .iter()
                .map(|(_, k, t)| {
                    if k.is_empty() {
                        t.clone()
                    } else {
                        format!("{k} {t}")
                    }
                })
                .collect();
            let t = if body.is_empty() {
                format!("{kw} {name} {{}}")
            } else {
                format!("{kw} {name} {{ {} }}", body.join(", "))
            };
            self.line(with_comment(t, comment));
            return Ok(());
        }
        let width = items
            .iter()
            .map(|(_, k, _)| display_width(k))
            .max()
            .unwrap_or(0);
        self.line(with_comment(
            format!("{kw} {name} {{"),
            comment_of(phys[0]).filter(|_| phys.len() > 1 && !phys[0].trim_end().ends_with('}')),
        ));
        let d = self.depth() + 1;
        let last = phys.len() - 1;
        for (k, l) in phys.iter().enumerate() {
            let n = no + k as u32;
            let code = strip_comment(l).trim();
            let cmt = comment_of(l);
            if k > 0 && l.trim().is_empty() {
                self.out.push(Out::Blank);
                continue;
            }
            if k > 0 && code.is_empty() {
                self.out.push(Out::Line(d, l.trim().to_string()));
                continue;
            }
            let here: Vec<&(u32, String, String)> = items.iter().filter(|i| i.0 == n).collect();
            let closes = k == last && code.ends_with('}');
            for (j, (_, key, ty)) in here.iter().enumerate() {
                let body = if key.is_empty() {
                    format!("{ty},")
                } else {
                    format!("{key}{} {ty},", " ".repeat(width - display_width(key)))
                };
                let c = if j + 1 == here.len() && !closes && k > 0 {
                    cmt.clone()
                } else {
                    None
                };
                self.out.push(Out::Line(d, with_comment(body, c)));
            }
        }
        let d0 = self.depth();
        let close_comment = if last > 0 && strip_comment(phys[last]).trim().ends_with('}') {
            comment_of(phys[last])
        } else {
            None
        };
        self.out
            .push(Out::Line(d0, with_comment("}".to_string(), close_comment)));
        Ok(())
    }
}

/// The standard form of an `import` tree (`a::b`, `a::{b, c}`, `a as b`).
fn import_text(code: &str) -> FResult<String> {
    let mut toks: Vec<String> = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == ':' {
            chars.next();
            chars.next();
            toks.push("::".to_string());
        } else if matches!(c, '{' | '}' | ',') {
            chars.next();
            toks.push(c.to_string());
        } else {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || matches!(c, ':' | '{' | '}' | ',') {
                    break;
                }
                w.push(c);
                chars.next();
            }
            toks.push(w);
        }
    }
    let mut i = 0;
    let s = import_tree(&toks, &mut i);
    if i != toks.len() {
        return Err(FormatError::Unverified);
    }
    Ok(s)
}

fn import_tree(toks: &[String], i: &mut usize) -> String {
    if toks.get(*i).map(String::as_str) == Some("{") {
        return import_group(toks, i);
    }
    let mut s = String::new();
    while *i < toks.len() {
        let t = toks[*i].as_str();
        match t {
            "::" => {
                *i += 1;
                s.push_str("::");
                if toks.get(*i).map(String::as_str) == Some("{") {
                    s.push_str(&import_group(toks, i));
                    return s;
                }
            }
            "as" => {
                *i += 1;
                s.push_str(" as ");
                if let Some(a) = toks.get(*i) {
                    s.push_str(a);
                    *i += 1;
                }
                return s;
            }
            "{" | "}" | "," => return s,
            w => {
                s.push_str(w);
                *i += 1;
            }
        }
    }
    s
}

fn import_group(toks: &[String], i: &mut usize) -> String {
    *i += 1;
    let mut items = Vec::new();
    while *i < toks.len() && toks[*i] != "}" {
        if toks[*i] == "," {
            *i += 1;
            continue;
        }
        items.push(import_tree(toks, i));
    }
    *i += 1;
    format!("{{{}}}", items.join(", "))
}
