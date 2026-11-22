//! Expression parsing: `match` patterns.

use super::*;

impl<'a> ExprParser<'a> {
    /// Reads a `match` pattern (up to before `=>`).
    pub fn parse_pattern(&mut self) -> PResult<Pattern> {
        self.skip_ws()?;
        let pos = self.here();
        match self.peek() {
            None => unexpected_eol(pos),
            Some('"') => match self.parse_string()? {
                Expr::Str(parts, p) => match parts.as_slice() {
                    [StrPart::Text(t, _)] => Ok(Pattern::Str(t.clone(), p)),
                    _ => err(DiagCode::NotConstant, vec![], p),
                },
                _ => unreachable!("文字列リテラルを読んだ"),
            },
            Some('-') => {
                self.bump();
                match self.parse_number()? {
                    Expr::Int(n, _) => match n.checked_neg() {
                        Some(n) => Ok(Pattern::Int(n, pos)),
                        None => err(
                            DiagCode::InvalidNumber,
                            vec![("text", DiagArg::Str(format!("-{n}")))],
                            pos,
                        ),
                    },
                    _ => unexpected("-", pos),
                }
            }
            Some(c) if c.is_ascii_digit() => {
                let start = self.pos;
                match self.parse_number()? {
                    Expr::Int(n, _) => Ok(Pattern::Int(n, pos)),
                    // `float` cannot be a pattern (compare with `==`)
                    _ => unexpected(&self.line.text[start..self.pos], pos),
                }
            }
            Some(c) if is_ident_start(c) => {
                let (name, p) = self.ident()?.expect("識別子の始まりを確かめた");
                match name.as_str() {
                    "_" => Ok(Pattern::Wild(p)),
                    "true" => Ok(Pattern::Bool(true, p)),
                    "false" => Ok(Pattern::Bool(false, p)),
                    "None" => Ok(Pattern::None(p)),
                    "Some" => {
                        let binds = self.parse_binders()?;
                        match <[_; 1]>::try_from(binds) {
                            Ok([b]) => Ok(Pattern::Some(b, p)),
                            Err(_) => unexpected("Some", p),
                        }
                    }
                    n if is_reserved(n) => unexpected(n, p),
                    _ => {
                        let binds = if self.peek() == Some('(') {
                            self.parse_binders()?
                        } else {
                            Vec::new()
                        };
                        Ok(Pattern::Variant {
                            name,
                            binds,
                            pos: p,
                        })
                    }
                }
            }
            Some(c) => unexpected(&c.to_string(), pos),
        }
    }

    /// Reads `(name, ...)` (`_` is the mark to discard). The `(` is at the current position.
    pub(super) fn parse_binders(&mut self) -> PResult<Vec<Option<(String, Pos)>>> {
        self.expect_char('(')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws()?;
            let at = self.here();
            let Some((n, p)) = self.ident()? else {
                return unexpected_char(self.peek(), at);
            };
            if n == "_" {
                out.push(None);
            } else if is_reserved(&n) {
                return unexpected(&n, p);
            } else {
                out.push(Some((n, p)));
            }
            self.skip_ws()?;
            if self.peek() == Some(',') {
                self.bump();
            } else {
                break;
            }
        }
        self.expect_char(')')?;
        Ok(out)
    }
}
