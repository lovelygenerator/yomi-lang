//! Expression parsing: type names.

use super::*;

impl<'a> ExprParser<'a> {
    /// Reads a type name (`int`, `list<int>`, `module.TypeName`). Nesting is limited the same as expressions (24).
    pub fn parse_type_name(&mut self) -> PResult<TyName> {
        let Some((mut name, pos)) = self.ident()? else {
            return unexpected_char(self.peek(), self.here());
        };
        if name == "fn" {
            return self.parse_fn_type(pos);
        }
        self.reject_versioned_name(&name, pos)?;
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
        if self.peek() == Some('.') && self.rest()[1..].chars().next().is_some_and(is_ident_start) {
            self.bump();
            let (member, _) = self.ident()?.expect("識別子の始まりを確かめた");
            name.push('.');
            name.push_str(&member);
        }
        self.skip_ws()?;
        let mut args = Vec::new();
        if self.peek() == Some('<') {
            self.nest_in(pos)?;
            self.bump();
            loop {
                let a = self.parse_type_name();
                match a {
                    Ok(a) => args.push(a),
                    Err(e) => {
                        self.nest -= 1;
                        return Err(e);
                    }
                }
                self.skip_ws()?;
                if self.peek() == Some(',') {
                    self.bump();
                } else {
                    break;
                }
            }
            self.nest -= 1;
            self.expect_char('>')?;
        }
        Ok(TyName { name, args, pos })
    }

    /// Reads what follows `fn` in a function type `fn(type, ...) -> type`. The return type cannot be omitted.
    fn parse_fn_type(&mut self, pos: Pos) -> PResult<TyName> {
        self.nest_in(pos)?;
        let r = self.parse_fn_type_inner(pos);
        self.nest -= 1;
        r
    }

    fn parse_fn_type_inner(&mut self, pos: Pos) -> PResult<TyName> {
        self.expect_char('(')?;
        let mut args = Vec::new();
        self.skip_ws()?;
        if self.peek() == Some(')') {
            self.bump();
        } else {
            loop {
                args.push(self.parse_type_name()?);
                self.skip_ws()?;
                match self.peek() {
                    Some(',') => {
                        self.bump();
                    }
                    Some(')') => {
                        self.bump();
                        break;
                    }
                    c => return unexpected_char(c, self.here()),
                }
            }
        }
        if !self.eat("->")? {
            return unexpected_char(self.peek(), self.here());
        }
        args.push(self.parse_type_name()?);
        Ok(TyName {
            name: "fn".to_string(),
            args,
            pos,
        })
    }
}
