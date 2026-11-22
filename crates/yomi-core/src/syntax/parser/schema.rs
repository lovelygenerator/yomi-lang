//! Parsing of the inputs of save migration: frozen schema files (`schema/vN.yomi`) and the `migrations` module.

use super::*;

/// A parsed frozen schema file.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SchemaFileAst {
    /// The version in the header.
    pub version: Option<u32>,
    /// The hash written in the header.
    pub declared_hash: Option<String>,
    /// The hash computed from the content after the header line.
    pub computed_hash: String,
    pub modules: Vec<SchemaModuleAst>,
}

/// One `module path { … }` block of a frozen schema.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SchemaModuleAst {
    pub path: String,
    pub pos: Pos,
    pub types: Vec<TypeDeclAst>,
    pub vars: Vec<SchemaVarAst>,
}

/// A variable of a frozen schema.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SchemaVarAst {
    pub persistent: bool,
    pub name: String,
    pub pos: Pos,
    pub ty: TyName,
}

/// A variable named in a `migrate` block (`v2::story::ch1.seen`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VarRef {
    pub version: Option<u32>,
    /// The module path joined with `::` (empty if written without a module).
    pub module: String,
    pub name: String,
    pub pos: Pos,
}

/// A statement of a `migrate` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MigrateStmt {
    Rename {
        old: VarRef,
        new: VarRef,
    },
    Convert {
        old: VarRef,
        old_ty: TyName,
        new: Option<VarRef>,
        new_ty: TyName,
        lambda: Box<Expr>,
    },
    Drop {
        old: VarRef,
    },
}

/// One `migrate N { … }` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MigrateAst {
    pub version: i64,
    pub pos: Pos,
    pub stmts: Vec<(Pos, MigrateStmt)>,
}

/// A parsed `migrations` module.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct MigrationsAst {
    /// `min_save_version N` (the value and the position).
    pub min_save_version: Vec<(i64, Pos)>,
    /// Whether `min_save_version` was written after another declaration.
    pub min_not_first: bool,
    pub blocks: Vec<MigrateAst>,
}

/// FNV-1a (64 bits) over bytes, without a separator after the end.
pub(crate) fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(super) fn split_lines(source: &str) -> Vec<&str> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

fn diag(module: &str, e: PErr) -> Diagnostic {
    Diagnostic {
        module: module.to_string(),
        line: e.pos.line,
        col: e.pos.col,
        severity: Severity::Error,
        code: e.code,
        args: e.args,
    }
}

fn invalid_file(module: &str, path: &str, line: u32) -> Diagnostic {
    Diagnostic {
        module: module.to_string(),
        line,
        col: 1,
        severity: Severity::Error,
        code: DiagCode::InvalidSchemaFile,
        args: vec![("path", DiagArg::Str(path.to_string()))],
    }
}

/// Reads a module path `a::b::c`.
fn parse_module_path(ep: &mut ExprParser<'_>) -> PResult<(String, Pos)> {
    let (mut path, pos) = ep.expect_name()?;
    while ep.rest().starts_with("::") {
        ep.pos += 2;
        let (seg, _) = ep.expect_name()?;
        path.push_str("::");
        path.push_str(&seg);
    }
    Ok((path, pos))
}

/// Parses a frozen schema file. `module` is the name shown in diagnostics (`schema/v2`), `path` the file path.
pub(crate) fn parse_schema_file(
    source: &str,
    module: &str,
    path: &str,
) -> (SchemaFileAst, Vec<Diagnostic>) {
    let lines = split_lines(source);
    let mut out = SchemaFileAst {
        computed_hash: format!("{:016x}", fnv1a_bytes(lines[1..].join("\n").as_bytes())),
        ..SchemaFileAst::default()
    };
    let mut diags = Vec::new();
    let header: Vec<&str> = lines[0].split_whitespace().collect();
    let unquoted = |t: &str| {
        t.strip_prefix('"')
            .and_then(|t| t.strip_suffix('"'))
            .map(str::to_string)
    };
    match header.as_slice() {
        ["schema", n, "hash", h] if unquoted(h).is_some() => {
            out.version = n
                .parse::<u32>()
                .ok()
                .filter(|v| *v >= 1 && !n.starts_with('0'));
            out.declared_hash = unquoted(h);
        }
        _ => {}
    }
    if out.version.is_none() || out.declared_hash.is_none() {
        diags.push(invalid_file(module, path, 1));
        return (out, diags);
    }
    let mut i = 1;
    let mut current: Option<SchemaModuleAst> = None;
    while i < lines.len() {
        let raw = lines[i];
        let no = i as u32 + 1;
        i += 1;
        let off = indent_len(raw);
        let t = &raw[off..];
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let line = Line::new(raw, no);
        let word = if t.starts_with('}') {
            "}"
        } else {
            first_word(t)
        };
        let res: PResult<()> = (|| match (word, &mut current) {
            ("module", None) => {
                let mut ep = ExprParser::new(&line, off + word.len());
                let (path, pos) = parse_module_path(&mut ep)?;
                ep.expect_char('{')?;
                ep.expect_end()?;
                current = Some(SchemaModuleAst {
                    path,
                    pos,
                    types: Vec::new(),
                    vars: Vec::new(),
                });
                Ok(())
            }
            ("}", Some(_)) => {
                let mut ep = ExprParser::new(&line, off + 1);
                ep.expect_end()?;
                out.modules.push(current.take().expect("開いている"));
                Ok(())
            }
            ("struct" | "enum", Some(m)) => {
                let is_enum = word == "enum";
                let mut ep = ExprParser::new(&line, off + word.len());
                let (name, pos) = ep.expect_name()?;
                ep.expect_char('{')?;
                let kind = if is_enum {
                    TypeDeclKind::Enum(parse_variants(&mut ep)?)
                } else {
                    TypeDeclKind::Struct(parse_fields(&mut ep)?)
                };
                ep.expect_end()?;
                m.types.push(TypeDeclAst { name, pos, kind });
                Ok(())
            }
            ("save" | "persistent", Some(m)) => {
                let mut ep = ExprParser::new(&line, off + word.len());
                match ep.ident()? {
                    Some((kw, _)) if kw == "var" => {}
                    _ => return unexpected_char(ep.peek(), ep.here()),
                }
                let (name, pos) = ep.expect_name()?;
                ep.expect_char(':')?;
                let ty = ep.parse_type_name()?;
                ep.expect_end()?;
                m.vars.push(SchemaVarAst {
                    persistent: word == "persistent",
                    name,
                    pos,
                    ty,
                });
                Ok(())
            }
            _ => unexpected(word, line.pos(off)),
        })();
        if let Err(e) = res {
            diags.push(diag(module, e));
        }
    }
    if let Some(m) = current {
        diags.push(diag(
            module,
            PErr {
                code: DiagCode::UnterminatedBlock,
                args: vec![],
                pos: m.pos,
            },
        ));
    }
    (out, diags)
}

/// Reads a variable name in a `migrate` block: `[v2::][a::b.]name`.
pub(super) fn parse_var_ref(ep: &mut ExprParser<'_>) -> PResult<VarRef> {
    let (first, pos) = ep.expect_name()?;
    let mut version = None;
    let mut head = first;
    if let Some(digits) = head.strip_prefix('v')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && ep.rest().starts_with("::")
    {
        version = digits.parse::<u32>().ok();
        ep.pos += 2;
        head = ep.expect_name()?.0;
    }
    let mut segs = vec![head];
    while ep.rest().starts_with("::") {
        ep.pos += 2;
        segs.push(ep.expect_name()?.0);
    }
    if ep.peek() == Some('.') {
        ep.bump();
        let (name, _) = ep.expect_name()?;
        Ok(VarRef {
            version,
            module: segs.join("::"),
            name,
            pos,
        })
    } else if segs.len() == 1 {
        Ok(VarRef {
            version,
            module: String::new(),
            name: segs.remove(0),
            pos,
        })
    } else {
        unexpected_char(ep.peek(), ep.here())
    }
}

/// The net count of opening brackets in the code part of a line.
pub(super) fn bracket_balance(s: &str) -> i32 {
    let mut n = 0;
    let mut in_str = false;
    let mut esc = false;
    for c in strip_comment(s).chars() {
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
            '(' | '[' | '{' => n += 1,
            ')' | ']' | '}' => n -= 1,
            _ => {}
        }
    }
    n
}

/// Parses a `migrations` module.
pub(crate) fn parse_migrations(source: &str, module: &str) -> (MigrationsAst, Vec<Diagnostic>) {
    let lines = split_lines(source);
    let mut out = MigrationsAst::default();
    let mut diags = Vec::new();
    let mut seen_block = false;
    let mut i = 0;
    let mut open: Option<MigrateAst> = None;
    while i < lines.len() {
        let raw = lines[i];
        let no = i as u32 + 1;
        i += 1;
        let off = indent_len(raw);
        let t = &raw[off..];
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let line = Line::new(raw, no);
        let word = if t.starts_with('}') {
            "}"
        } else {
            first_word(t)
        };
        let res: PResult<()> = (|| {
            match (word, open.is_some()) {
                ("min_save_version", false) => {
                    let mut ep = ExprParser::new(&line, off + word.len());
                    ep.skip_ws()?;
                    let start = ep.pos;
                    while ep.peek().is_some_and(|c| c.is_ascii_digit()) {
                        ep.bump();
                    }
                    let value = ep.line.text[start..ep.pos].parse::<i64>().unwrap_or(-1);
                    if start == ep.pos {
                        return unexpected_char(ep.peek(), ep.here());
                    }
                    ep.expect_end()?;
                    if seen_block {
                        out.min_not_first = true;
                    }
                    out.min_save_version.push((value, line.pos(off)));
                    Ok(())
                }
                ("migrate", false) => {
                    let mut ep = ExprParser::new(&line, off + word.len());
                    ep.skip_ws()?;
                    let start = ep.pos;
                    while ep.peek().is_some_and(|c| c.is_ascii_digit()) {
                        ep.bump();
                    }
                    if start == ep.pos {
                        return unexpected_char(ep.peek(), ep.here());
                    }
                    let version = ep.line.text[start..ep.pos]
                        .parse::<i64>()
                        .unwrap_or(i64::MAX);
                    ep.expect_char('{')?;
                    ep.expect_end()?;
                    seen_block = true;
                    open = Some(MigrateAst {
                        version,
                        pos: line.pos(off),
                        stmts: Vec::new(),
                    });
                    Ok(())
                }
                ("}", true) => {
                    let mut ep = ExprParser::new(&line, off + 1);
                    ep.expect_end()?;
                    out.blocks.push(open.take().expect("開いている"));
                    Ok(())
                }
                ("rename" | "drop" | "convert", true) => {
                    let pos = line.pos(off);
                    let stmt = if word == "convert" {
                        // The lambda continues to the following lines while it is unfinished
                        let mut text = strip_comment(raw).trim_end().to_string();
                        let mut segs = vec![Seg {
                            start: 0,
                            line: no,
                            col_base: 0,
                        }];
                        let mut depth = bracket_balance(&text);
                        while (text.ends_with('=') || depth > 0) && i < lines.len() {
                            let next = lines[i];
                            let nno = i as u32 + 1;
                            i += 1;
                            let code = strip_comment(next);
                            let stripped = code.trim_start_matches([' ', '\t']);
                            text.push(' ');
                            segs.push(Seg {
                                start: text.len(),
                                line: nno,
                                col_base: (code.len() - stripped.len()) as u32,
                            });
                            text.push_str(stripped.trim_end_matches([' ', '\t']));
                            depth += bracket_balance(stripped);
                        }
                        let jl = Line::joined(&text, no, segs);
                        let mut ep = ExprParser::new(&jl, off + word.len());
                        ep.schema_names = true;
                        let old = parse_var_ref(&mut ep)?;
                        ep.expect_char(':')?;
                        let old_ty = ep.parse_type_name()?;
                        if !ep.eat("->")? {
                            return unexpected_char(ep.peek(), ep.here());
                        }
                        ep.skip_ws()?;
                        let save = ep.pos;
                        let new = match parse_var_ref(&mut ep) {
                            Ok(v) if ep.peek() == Some(':') => {
                                ep.bump();
                                Some(v)
                            }
                            _ => {
                                ep.pos = save;
                                None
                            }
                        };
                        let new_ty = ep.parse_type_name()?;
                        if !ep.eat("=")? {
                            return unexpected_char(ep.peek(), ep.here());
                        }
                        ep.skip_ws()?;
                        let (start, first) = (ep.here(), ep.peek());
                        let lambda = ep.parse_expr()?;
                        if !matches!(lambda, Expr::Lambda(_)) {
                            return unexpected_char(first, start);
                        }
                        ep.expect_end()?;
                        MigrateStmt::Convert {
                            old,
                            old_ty,
                            new,
                            new_ty,
                            lambda: Box::new(lambda),
                        }
                    } else {
                        let mut ep = ExprParser::new(&line, off + word.len());
                        let old = parse_var_ref(&mut ep)?;
                        if word == "rename" {
                            if !ep.eat("->")? {
                                return unexpected_char(ep.peek(), ep.here());
                            }
                            let new = parse_var_ref(&mut ep)?;
                            ep.expect_end()?;
                            MigrateStmt::Rename { old, new }
                        } else {
                            ep.expect_end()?;
                            MigrateStmt::Drop { old }
                        }
                    };
                    open.as_mut().expect("開いている").stmts.push((pos, stmt));
                    Ok(())
                }
                _ => {
                    if word == "label" || word == "fn" || is_declaration(word) {
                        return err(
                            DiagCode::UnexpectedToken,
                            vec![("found", DiagArg::Str(word.to_string()))],
                            line.pos(off),
                        );
                    }
                    unexpected(word, line.pos(off))
                }
            }
        })();
        if let Err(e) = res {
            diags.push(diag(module, e));
        }
    }
    if let Some(b) = open {
        diags.push(diag(
            module,
            PErr {
                code: DiagCode::UnterminatedBlock,
                args: vec![],
                pos: b.pos,
            },
        ));
    }
    (out, diags)
}
