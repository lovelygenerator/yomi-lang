//! Expression parsing: struct, map, list, number and string literals.

use super::*;

impl<'a> ExprParser<'a> {
    /// Reads the fields of a struct literal and `}` from after `{`.
    pub(super) fn parse_struct_fields(&mut self) -> PResult<Vec<FieldInit>> {
        let mut fields = Vec::new();
        self.skip_ws()?;
        if self.peek() == Some('}') {
            self.bump();
            return Ok(fields);
        }
        loop {
            let (name, pos) = self.expect_name()?;
            self.expect_char(':')?;
            let value = self.parse_expr()?;
            fields.push(FieldInit { name, pos, value });
            self.skip_ws()?;
            if self.peek() == Some(',') {
                self.bump();
                self.skip_ws()?;
                // A trailing `,` is allowed
                if self.peek() == Some('}') {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_char('}')?;
        Ok(fields)
    }

    /// If `{` follows a type name, reads a struct literal (not inside header expressions).
    pub(super) fn try_struct_literal(&mut self, ty: TypeRef, start: Pos) -> PResult<Option<Expr>> {
        if self.no_struct {
            return Ok(None);
        }
        self.skip_ws()?;
        if self.peek() != Some('{') {
            return Ok(None);
        }
        self.node(start)?;
        self.nest_in(start)?;
        self.bump();
        let saved = std::mem::replace(&mut self.no_struct, false);
        let fields = self.parse_struct_fields();
        self.no_struct = saved;
        self.nest -= 1;
        Ok(Some(Expr::StructLit(ty, fields?, start)))
    }

    /// Reads the `key: value` entries of a `map` literal and `}` from after `{` (a trailing `,` is allowed).
    pub(super) fn parse_map_entries(&mut self) -> PResult<Vec<(Expr, Expr)>> {
        let mut entries = Vec::new();
        self.skip_ws()?;
        if self.peek() == Some('}') {
            self.bump();
            return Ok(entries);
        }
        loop {
            let key = self.parse_expr()?;
            self.expect_char(':')?;
            let value = self.parse_expr()?;
            entries.push((key, value));
            self.skip_ws()?;
            if self.peek() == Some(',') {
                self.bump();
                self.skip_ws()?;
                if self.peek() == Some('}') {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_char('}')?;
        Ok(entries)
    }

    /// Reads the elements of a list and `]` from after `[`.
    pub(super) fn parse_list_items(&mut self) -> PResult<Vec<Expr>> {
        let mut items = Vec::new();
        self.skip_ws()?;
        if self.peek() == Some(']') {
            self.bump();
            return Ok(items);
        }
        loop {
            items.push(self.parse_expr()?);
            self.skip_ws()?;
            if self.peek() == Some(',') {
                self.bump();
                self.skip_ws()?;
                // A trailing `,` is allowed
                if self.peek() == Some(']') {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_char(']')?;
        Ok(items)
    }

    pub(super) fn parse_number(&mut self) -> PResult<Expr> {
        let start = self.pos;
        let pos = self.here();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let text = &self.line.text[start..self.pos];
        if self.peek() == Some('.')
            && self.rest()[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            // Floating point: integer part `.` fraction part (no exponent notation; section 1.7 of `spec/language/01-lexical.md`)
            self.bump();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.bump();
                } else {
                    break;
                }
            }
            let text = &self.line.text[start..self.pos];
            let invalid = || {
                err(
                    DiagCode::InvalidNumber,
                    vec![("text", DiagArg::Str(text.to_string()))],
                    pos,
                )
            };
            // Digit separators at the edges, as in `1_.5` and `1.5_`, are errors
            if text.ends_with('_') || text.contains("_.") {
                return invalid();
            }
            let digits: String = text.chars().filter(|c| *c != '_').collect();
            return match digits.parse::<f64>().ok().and_then(finite_float) {
                Some(x) => Ok(Expr::Float(x, pos)),
                None => invalid(),
            };
        }
        let invalid = || {
            err(
                DiagCode::InvalidNumber,
                vec![("text", DiagArg::Str(text.to_string()))],
                pos,
            )
        };
        if text.ends_with('_') {
            return invalid();
        }
        let digits: String = text.chars().filter(|c| *c != '_').collect();
        match digits.parse::<i64>() {
            Ok(n) => Ok(Expr::Int(n, pos)),
            Err(_) => invalid(),
        }
    }

    pub(super) fn parse_string(&mut self) -> PResult<Expr> {
        let pos = self.here();
        self.bump(); // "
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut written = String::new();
        loop {
            let at = self.here();
            match self.bump() {
                None => return err(DiagCode::UnterminatedString, vec![], pos),
                Some('"') => break,
                Some('\\') => match self.bump() {
                    Some(c @ ('"' | '\\' | 'n' | 't' | '$')) => {
                        written.push('\\');
                        written.push(c);
                        text.push(match c {
                            'n' => '\n',
                            't' => '\t',
                            c => c,
                        });
                    }
                    Some(c) => {
                        return err(
                            DiagCode::InvalidEscape,
                            vec![("found", DiagArg::Str(format!("\\{c}")))],
                            at,
                        );
                    }
                    None => return err(DiagCode::UnterminatedString, vec![], pos),
                },
                Some('$') if self.peek() == Some('{') => {
                    self.bump();
                    if !text.is_empty() {
                        parts.push(StrPart::Text(
                            std::mem::take(&mut text),
                            std::mem::take(&mut written),
                        ));
                    }
                    self.node(at)?;
                    self.nest_in(at)?;
                    let e = self.parse_expr();
                    self.nest -= 1;
                    let e = e?;
                    self.expect_char('}')?;
                    parts.push(StrPart::Interp(e));
                }
                Some(c) => {
                    text.push(c);
                    written.push(c);
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(StrPart::Text(text, written));
        }
        Ok(Expr::Str(parts, pos))
    }
}
