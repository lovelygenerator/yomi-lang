//! Compilation: generating code for the bodies of labels and functions.

use super::*;

impl Compiler<'_> {
    /// Compiles a label. `flat` is the sequential number across the whole program. Returns the code and whether it may reach
    /// the end.
    pub(super) fn label(&mut self, l: &LabelDecl, flat: u32) -> (LabelCode, bool, LabelRefs) {
        let sig = self.sigs[flat as usize].clone();
        let (mut ctx, code, falls) = self.body(l, &sig, false);
        let refs = std::mem::take(&mut ctx.refs);
        let code = LabelCode {
            module: self.cur_mod as u32,
            name: l.name.clone(),
            reg: code.reg,
            lines: code.lines,
            locals: ctx.locals,
            params: l.params.len() as u32,
            ret: match sig.ret {
                RetTy::Ty(t) => Some(t),
                _ => None,
            },
            stops: code.stops,
            calls: code.calls,
        };
        (code, falls, refs)
    }

    /// Compiles a `fn`. `flat` is an index into `Program::fns`.
    pub(super) fn fn_code(&mut self, f: &LabelDecl, flat: u32) -> FnCode {
        let sig = self.fn_sigs[flat as usize].clone();
        let (ctx, code, _) = self.body(f, &sig, true);
        FnCode {
            module: self.cur_mod as u32,
            name: f.name.clone(),
            reg: code.reg,
            lines: code.lines,
            locals: ctx.locals,
            params: f.params.len() as u32,
            captures: 0,
            ret: match sig.ret {
                RetTy::Ty(t) => Some(t),
                _ => None,
            },
        }
    }

    /// Records the `let` variables of the body that are never read.
    fn record_unused_variables(&mut self) {
        let m = self.cur_mod;
        let c = self.ctx();
        let unused: Vec<(usize, Pos, String)> = c
            .lets
            .decls
            .iter()
            .filter(|(slot, _, _)| !c.lets.read.contains(slot))
            .map(|(_, name, pos)| (m, *pos, name.clone()))
            .collect();
        self.unused_vars.extend(unused);
    }

    /// Compiles the body of a label or `fn`. Returns the compiler state, the code, and whether it may reach the end.
    pub(super) fn body(
        &mut self,
        l: &LabelDecl,
        sig: &Sig,
        in_fn: bool,
    ) -> (LabelCtx, Lowered, bool) {
        self.cur = Some(LabelCtx {
            ir: Ir::default(),
            locals: Vec::new(),
            scopes: vec![HashMap::new()],
            ids: HashSet::new(),
            refs: LabelRefs::default(),
            loop_depth: 0,
            name: l.name.clone(),
            ret: sig.ret.clone(),
            in_fn,
            live: Default::default(),
            lets: Default::default(),
        });
        // Parameters are the first local variables
        for (p, ty) in l.params.iter().zip(&sig.params) {
            if self.lookup(&p.name).is_some() || self.is_import(&p.name) {
                self.name_error(p.pos, DiagCode::DuplicateName, &p.name);
            }
            self.warn_if_builtin(&p.name, p.pos);
            let c = self.ctx();
            let slot = c.locals.len() as u32;
            c.locals.push(LocalInfo {
                name: p.name.clone(),
                ty: ty.clone().unwrap_or(Ty::Int),
            });
            c.scopes[0].insert(p.name.clone(), (slot, ty.clone()));
        }
        let stmts = self.block(&l.body);
        self.fn_live_check();
        if !self.repl_is_input_label(&l.name) {
            self.record_unused_variables();
        }
        let falls = completes(&l.body);
        if falls && let RetTy::Ty(_) = sig.ret {
            self.name_error(l.pos, DiagCode::MissingReturn, &l.name);
        }
        // End: return to the caller (for a label with no caller, same as `end`). Labels with a return type and `fn`s do not reach
        // the end (if they might, a diagnostic was emitted). REPL input, if it is an expression, finishes keeping the value.
        let tail = if !in_fn && self.repl_is_input_label(&l.name) {
            Tail::Repl(self.repl_input_tail())
        } else if in_fn || matches!(sig.ret, RetTy::None) {
            Tail::Return
        } else {
            Tail::End
        };
        let mut ctx = self.cur.take().expect("ラベルの途中");
        let body = Body {
            ir: std::mem::take(&mut ctx.ir),
            stmts,
            tail,
            in_fn,
            locals: ctx.locals.len() as u32,
            line: l.pos.line,
        };
        let code = self.lower_body(&body, l.pos);
        (ctx, code, falls)
    }

    /// A hidden label that evaluates `expr` and leaves its value on the stack (like the end of REPL input). The first `params`
    /// locals of `ctx` are its parameters.
    pub(super) fn expr_label(
        &mut self,
        mut ctx: LabelCtx,
        expr: ExprId,
        name: String,
        pos: Pos,
        params: u32,
    ) -> LabelCode {
        let body = Body {
            ir: std::mem::take(&mut ctx.ir),
            stmts: Vec::new(),
            tail: Tail::Repl(Some(expr)),
            in_fn: false,
            locals: ctx.locals.len() as u32,
            line: pos.line,
        };
        let code = self.lower_body(&body, pos);
        LabelCode {
            module: self.cur_mod as u32,
            name,
            reg: code.reg,
            lines: code.lines,
            locals: ctx.locals,
            params,
            ret: None,
            stops: code.stops,
            calls: code.calls,
        }
    }

    /// The code of a lambda body (a `fn` that only returns the expression `body`). `params` is the number of parameters,
    /// `captures` the number of captured values.
    pub(super) fn lambda_code(
        &mut self,
        mut ctx: LabelCtx,
        body: ExprId,
        pos: Pos,
        params: u32,
        captures: u32,
        ret: Option<Ty>,
    ) -> FnCode {
        let stmt = ctx.ir.stmt(StmtKind::Return(Some(body)), pos.line);
        let body = Body {
            ir: std::mem::take(&mut ctx.ir),
            stmts: vec![stmt],
            tail: Tail::Return,
            in_fn: true,
            locals: ctx.locals.len() as u32,
            line: pos.line,
        };
        let code = self.lower_body(&body, pos);
        FnCode {
            module: self.cur_mod as u32,
            name: ctx.name,
            reg: code.reg,
            lines: code.lines,
            locals: ctx.locals,
            params,
            captures,
            ret,
        }
    }

    /// Turns a body into register-based code.
    pub(super) fn lower_body(&mut self, body: &Body, pos: Pos) -> Lowered {
        match RegBackend.lower(body) {
            Ok(c) => Lowered {
                reg: Box::new(c.code),
                lines: c.lines,
                stops: c.stops,
                calls: c.calls,
            },
            Err(TooManyRegisters) => {
                // If there are no front-end diagnostics, make it an error later
                let m = self.cur_mod;
                self.reg_overflow.push((m, pos));
                Lowered::default()
            }
        }
    }
}

/// The code of one body made by the backend.
#[derive(Default)]
pub(super) struct Lowered {
    pub(super) reg: Box<RegCode>,
    pub(super) lines: Vec<u32>,
    stops: Vec<StopInfo>,
    calls: Vec<CallSite>,
}
