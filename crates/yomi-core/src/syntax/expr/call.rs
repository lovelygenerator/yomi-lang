//! Expression parsing: call arguments and command calls.

use super::*;

impl<'a> ExprParser<'a> {
    /// Reads the arguments and `)` from after `(`.
    pub fn parse_call_args(&mut self) -> PResult<Vec<Expr>> {
        let saved = std::mem::replace(&mut self.no_struct, false);
        let r = self.parse_call_args_inner();
        self.no_struct = saved;
        r
    }

    pub(super) fn parse_call_args_inner(&mut self) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        self.skip_ws()?;
        if self.peek() == Some(')') {
            self.bump();
            return Ok(args);
        }
        loop {
            args.push(self.parse_call_arg()?);
            self.skip_ws()?;
            if self.peek() == Some(',') {
                self.bump();
                self.skip_ws()?;
                // A trailing `,` is allowed
                if self.peek() == Some(')') {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_char(')')?;
        Ok(args)
    }

    /// Reads one call argument. `name: expr` is a named argument (`Expr::Named`).
    pub(super) fn parse_call_arg(&mut self) -> PResult<Expr> {
        self.skip_ws()?;
        let start = self.pos;
        if let Some((name, pos)) = self.ident()? {
            self.skip_ws()?;
            if self.peek() == Some(':') {
                self.bump();
                let value = self.parse_expr()?;
                return Ok(Expr::Named(name, Box::new(value), pos));
            }
        }
        self.pos = start;
        self.parse_expr()
    }

    /// Reads `@name(expr, ...)` (from the position of `@`; the stable ID is not read).
    pub fn parse_command_call(&mut self) -> PResult<CommandCall> {
        self.skip_ws()?;
        let pos = self.here();
        self.bump();
        let mut name = match self.ident()? {
            Some((n, _)) => n,
            None => return unexpected("@", pos),
        };
        while self.peek() == Some('.') {
            self.bump();
            match self.ident()? {
                Some((n, _)) => {
                    name.push('.');
                    name.push_str(&n);
                }
                None => return unexpected(".", self.here()),
            }
        }
        self.expect_char('(')?;
        let args = self.parse_call_args()?;
        Ok(CommandCall {
            name,
            args,
            id: None,
            id_at: self.here(),
            pos,
        })
    }
}
