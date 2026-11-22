//! Line parsing: top-level declarations of a module (import, identity, variables, labels, types).

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_module(&mut self) {
        while self.i < self.lines.len() {
            let raw = self.lines[self.i];
            let no = self.i as u32 + 1;
            self.i += 1;
            let off = indent_len(raw);
            let t = &raw[off..];
            if t.is_empty() || t.starts_with("//") {
                continue;
            }
            let word = first_word(t);
            let joined = if matches!(word, "save" | "persistent") {
                match self.join_statement(raw, no, off, off) {
                    Ok(j) => j,
                    Err(e) => {
                        self.push(e);
                        continue;
                    }
                }
            } else {
                None
            };
            let line = match &joined {
                Some(j) => Line::joined(&j.text, no, j.segs.clone()),
                None => Line::new(raw, no),
            };
            let res: PResult<()> = match word {
                "import" => self.parse_import(&line, off),
                "save" | "persistent" => {
                    self.seen_decl = true;
                    self.parse_var_decl(&line, off, word == "persistent")
                }
                "label" | "fn" => {
                    self.seen_decl = true;
                    self.parse_label(&line, off, word == "fn")
                }
                "identity" => {
                    self.seen_decl = true;
                    self.parse_identity(&line, off)
                }
                "enum" | "struct" => {
                    self.seen_decl = true;
                    self.parse_type_decl(&line, off, word == "enum")
                }
                _ if t.starts_with('}') => err(DiagCode::UnexpectedClose, vec![], line.pos(off)),
                _ => {
                    if starts_block(word) && opens_block(raw) {
                        self.skip_block();
                    }
                    err(DiagCode::StatementOutsideLabel, vec![], line.pos(off))
                }
            };
            if let Err(e) = res {
                self.push(e);
            }
        }
    }

    pub(super) fn parse_import(&mut self, line: &Line<'_>, off: usize) -> PResult<()> {
        let pos = line.pos(off);
        let mut ep = ExprParser::new(line, off + "import".len());
        let mut items = Vec::new();
        parse_import_tree(&mut ep, &mut Vec::new(), &mut items, 0)?;
        ep.expect_end()?;
        self.out.imports.extend(items);
        if self.seen_decl {
            return err(DiagCode::ImportNotFirst, vec![], pos);
        }
        Ok(())
    }

    pub(super) fn parse_identity(&mut self, line: &Line<'_>, off: usize) -> PResult<()> {
        let mut ep = ExprParser::new(line, off + "identity".len());
        let (name, pos) = ep.expect_name()?;
        ep.expect_char('=')?;
        ep.skip_ws()?;
        let value_pos = ep.here();
        let value = ep.parse_expr()?;
        ep.expect_end()?;
        // The display name is a string literal (without `${expr}`)
        let display = match value {
            Expr::Str(parts, _) if parts.iter().all(|p| matches!(p, StrPart::Text(..))) => parts
                .iter()
                .filter_map(|p| match p {
                    StrPart::Text(t, _) => Some(t.as_str()),
                    StrPart::Interp(_) => None,
                })
                .collect::<String>(),
            _ => return err(DiagCode::NotConstant, vec![], value_pos),
        };
        self.out
            .identities
            .push(IdentityDecl { name, display, pos });
        Ok(())
    }

    /// `save var name: type = constant` and `persistent var name: type = constant`.
    pub(super) fn parse_var_decl(
        &mut self,
        line: &Line<'_>,
        off: usize,
        persistent: bool,
    ) -> PResult<()> {
        let keyword = if persistent { "persistent" } else { "save" };
        let mut ep = ExprParser::new(line, off + keyword.len());
        match ep.ident()? {
            Some((w, _)) if w == "var" => {}
            Some((w, p)) => return unexpected(&w, p),
            None => return unexpected_eol(ep.here()),
        }
        let (name, pos) = ep.expect_name()?;
        let rest = (|| {
            ep.expect_char(':')?;
            let ty = ep.parse_type_name()?;
            ep.expect_char('=')?;
            let init = ep.parse_expr()?;
            ep.expect_end()?;
            Ok((ty, init))
        })();
        match rest {
            Ok((ty, init)) => {
                self.out.vars.push(VarDecl {
                    name,
                    ty,
                    init,
                    persistent,
                    pos,
                });
                Ok(())
            }
            Err(e) => {
                self.out.poisoned.insert(name);
                Err(e)
            }
        }
    }

    /// Reads `label` and `fn` declarations. A `fn` requires the `( )` of parameters (`fn name() { ... }`).
    pub(super) fn parse_label(&mut self, line: &Line<'_>, off: usize, is_fn: bool) -> PResult<()> {
        let keyword = if is_fn { "fn" } else { "label" };
        let mut ep = ExprParser::new(line, off + keyword.len());
        let (name, pos) = match ep.expect_name() {
            Ok(v) => v,
            Err(e) => {
                if opens_block(line.text) {
                    self.skip_block();
                }
                return Err(e);
            }
        };
        let header = (|| {
            let mut params = Vec::new();
            let mut ret = None;
            ep.skip_ws()?;
            if is_fn {
                ep.expect_char('(')?;
            }
            if is_fn || ep.peek() == Some('(') {
                if !is_fn {
                    ep.bump();
                }
                params = parse_params(&mut ep)?;
                ep.skip_ws()?;
                if ep.rest().starts_with("->") {
                    ep.pos += 2;
                    ret = Some(parse_type(&mut ep)?);
                }
            }
            ep.expect_char('{')?;
            Ok((params, ret))
        })();
        let (params, ret) = match header {
            Ok(h) => h,
            Err(e) => {
                self.out.poisoned.insert(name);
                if opens_block(line.text) {
                    self.skip_block();
                }
                return Err(e);
            }
        };
        // Even if characters follow `{`, the body is read (so errors do not cascade).
        if let Err(e) = ep.expect_end() {
            self.push(e);
        }
        let (body, close) = self.parse_block(pos);
        if let Some(c) = close {
            self.check_tail(c);
        }
        let decl = LabelDecl {
            name,
            params,
            ret,
            body,
            pos,
        };
        if is_fn {
            self.out.fns.push(decl);
        } else {
            self.out.labels.push(decl);
        }
        Ok(())
    }

    /// Reads a `struct` / `enum` declaration (multiple lines from `{` to the matching `}` together).
    pub(super) fn parse_type_decl(
        &mut self,
        line: &Line<'_>,
        off: usize,
        is_enum: bool,
    ) -> PResult<()> {
        let kw = if is_enum { "enum" } else { "struct" };
        let mut head = ExprParser::new(line, off + kw.len());
        let (name, pos) = match head.expect_name() {
            Ok(v) => v,
            Err(e) => {
                if opens_block(line.text) {
                    self.skip_block();
                }
                return Err(e);
            }
        };
        // Join lines up to the matching `}` (without comments; one space between lines)
        let first = strip_comment(line.text).trim_end_matches([' ', '\t']);
        let mut text = first.to_string();
        let mut segs = vec![Seg {
            start: 0,
            line: line.no,
            col_base: 0,
        }];
        let mut depth: i32 = brace_balance(first);
        let opened = first.contains('{');
        while opened && depth > 0 && self.i < self.lines.len() {
            let raw = self.lines[self.i];
            let no = self.i as u32 + 1;
            self.i += 1;
            let code = strip_comment(raw);
            let stripped = code.trim_start_matches([' ', '\t']);
            text.push(' ');
            segs.push(Seg {
                start: text.len(),
                line: no,
                col_base: (code.len() - stripped.len()) as u32,
            });
            text.push_str(stripped.trim_end_matches([' ', '\t']));
            depth += brace_balance(stripped);
        }
        if opened && depth > 0 {
            self.out.poisoned.insert(name);
            return err(DiagCode::UnterminatedBlock, vec![], pos);
        }
        let jl = Line::joined(&text, line.no, segs);
        let res = (|| {
            let mut ep = ExprParser::new(&jl, off + kw.len());
            ep.expect_name()?;
            ep.expect_char('{')?;
            let kind = if is_enum {
                TypeDeclKind::Enum(parse_variants(&mut ep)?)
            } else {
                TypeDeclKind::Struct(parse_fields(&mut ep)?)
            };
            ep.expect_end()?;
            Ok(kind)
        })();
        match res {
            Ok(kind) => {
                self.out.types.push(TypeDeclAst { name, pos, kind });
                Ok(())
            }
            Err(e) => {
                self.out.poisoned.insert(name);
                Err(e)
            }
        }
    }
}

/// The maximum nesting depth of `{ }` (so deeply nested input on one line does not exhaust the stack).
const IMPORT_MAX_DEPTH: usize = 16;

/// Reads the tree of an `import`.
///
/// ```text
/// import_tree = "{" import_tree { "," import_tree } [ "," ] "}"
///             | path [ "as" identifier ]
///             | path "::" "{" import_tree { "," import_tree } [ "," ] "}" ;
/// path        = identifier { "::" identifier } ;
/// ```
///
/// `prefix` is the first part of the path already read. For each leaf (one name, including `as`), adds an `Import` to
/// `out`.
fn parse_import_tree(
    ep: &mut ExprParser<'_>,
    prefix: &mut Vec<String>,
    out: &mut Vec<Import>,
    depth: usize,
) -> PResult<()> {
    ep.skip_ws()?;
    if ep.peek() == Some('{') {
        return parse_import_group(ep, prefix, out, depth);
    }
    let base = prefix.len();
    let result = parse_import_path(ep, prefix, out, depth);
    prefix.truncate(base);
    result
}

/// Reads one path of an `import` (`a::b::c`, an `as` alias, `{…}` groups) and adds it to `out`. `prefix` is the levels so
/// far.
fn parse_import_path(
    ep: &mut ExprParser<'_>,
    prefix: &mut Vec<String>,
    out: &mut Vec<Import>,
    depth: usize,
) -> PResult<()> {
    // Attach diagnostics to the start of this item (the first word of the path)
    let mut item_pos = None;
    loop {
        let (seg, seg_pos) = ep.expect_name()?;
        let item_pos = *item_pos.get_or_insert(seg_pos);
        prefix.push(seg.clone());
        if ep.eat("::")? {
            ep.skip_ws()?;
            if ep.peek() == Some('{') {
                return parse_import_group(ep, prefix, out, depth);
            }
            continue;
        }
        // A leaf. Read `as alias` if present
        ep.skip_ws()?;
        let (local, aliased) = match ep.rest().strip_prefix("as") {
            Some(after) if !after.chars().next().is_some_and(is_ident_continue) => {
                ep.bump();
                ep.bump();
                let (alias, _) = ep.expect_name()?;
                (alias, true)
            }
            _ => (seg, false),
        };
        out.push(Import {
            path: prefix.join("::"),
            local,
            aliased,
            pos: item_pos,
        });
        return Ok(());
    }
}

/// Reads a `{…}` group of an `import` (`a::{b, c}`).
fn parse_import_group(
    ep: &mut ExprParser<'_>,
    prefix: &mut Vec<String>,
    out: &mut Vec<Import>,
    depth: usize,
) -> PResult<()> {
    if depth >= IMPORT_MAX_DEPTH {
        return unexpected_char(ep.peek(), ep.here());
    }
    ep.expect_char('{')?;
    loop {
        parse_import_tree(ep, prefix, out, depth + 1)?;
        if ep.eat(",")? {
            // A trailing comma is allowed
            ep.skip_ws()?;
            if ep.peek() == Some('}') {
                break;
            }
            continue;
        }
        break;
    }
    ep.expect_char('}')
}
