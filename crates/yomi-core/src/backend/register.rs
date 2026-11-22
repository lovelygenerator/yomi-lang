//! The register backend: turns the IR into register-based instructions ([`RegOp`]).
//!
//! Registers are the local variables (the first `locals`) followed by temporaries. Temporaries are reset to their starting
//! position for each statement, so they do not live across statements (`call` cannot be written in the middle of an
//! expression, so at a `call` or a stop point, temporaries are no longer in use). Sequences of values such as arguments
//! and elements are placed in consecutive temporaries.

use crate::ir::{
    Body, Builtin, Choice, ExprId, ExprKind, ForKind, Ir, Method, Operand, PatTest, StmtId,
    StmtKind, Stop, Tail, TextPiece,
};
use crate::program::{ArithOp, CallSite, PlaceRef, Reg, RegCode, RegOp, StopInfo};
use crate::syntax::ast::{BinOp, UnOp};
use crate::value::Value;

/// The maximum number of registers in one frame (numbers are `u16`).
pub(crate) const MAX_REGS: u32 = 1 << 16;

/// The register backend.
pub(crate) struct RegBackend;

/// The register-based code of one body, and the information tied to instruction positions.
pub(crate) struct RegOut {
    pub code: RegCode,
    pub lines: Vec<u32>,
    pub stops: Vec<StopInfo>,
    pub calls: Vec<CallSite>,
}

/// The number of registers exceeded the limit ([`MAX_REGS`]).
#[derive(Debug)]
pub(crate) struct TooManyRegisters;

impl super::Backend for RegBackend {
    type Code = RegOut;
    type Error = TooManyRegisters;

    fn lower(&mut self, body: &Body) -> Result<RegOut, TooManyRegisters> {
        let mut g = RegGen {
            ir: &body.ir,
            out: RegOut {
                code: RegCode::default(),
                lines: Vec::new(),
                stops: Vec::new(),
                calls: Vec::new(),
            },
            in_fn: body.in_fn,
            locals: body.locals,
            next: body.locals,
            max: body.locals,
            loops: Vec::new(),
        };
        g.body(body);
        if g.max > MAX_REGS {
            return Err(TooManyRegisters);
        }
        g.out.code.regs = g.max;
        g.out.code.vars = body.locals;
        Ok(g.out)
    }
}

/// The targets of `break` and `continue` for one loop.
struct LoopTargets {
    top: u32,
    breaks: Vec<usize>,
}

struct RegGen<'a> {
    ir: &'a Ir,
    out: RegOut,
    in_fn: bool,
    /// The number of local variables (registers after these are temporaries).
    locals: u32,
    /// The next temporary register to use.
    next: u32,
    /// The number of registers used.
    max: u32,
    loops: Vec<LoopTargets>,
}

/// Turns a number into a register number. Numbers over the limit are truncated (code that exceeds the limit is not used).
fn r(x: u32) -> Reg {
    x as Reg
}

impl RegGen<'_> {
    fn emit(&mut self, op: RegOp, line: u32) -> usize {
        self.out.code.ops.push(op);
        self.out.lines.push(line);
        self.out.code.ops.len() - 1
    }

    fn here(&self) -> u32 {
        self.out.code.ops.len() as u32
    }

    /// Sets the target of the jump instruction at position `at` to `to`.
    fn patch(&mut self, at: usize, to: u32) {
        match &mut self.out.code.ops[at] {
            RegOp::Jump { to: t }
            | RegOp::JumpIfFalse { to: t, .. }
            | RegOp::JumpIfTrue { to: t, .. }
            | RegOp::JumpUnlessLtI { to: t, .. }
            | RegOp::JumpUnlessLeI { to: t, .. }
            | RegOp::JumpUnlessLtIK { to: t, .. }
            | RegOp::JumpUnlessLeIK { to: t, .. }
            | RegOp::JumpUnlessGtIK { to: t, .. }
            | RegOp::JumpUnlessGeIK { to: t, .. }
            | RegOp::JumpUnlessEqIK { to: t, .. }
            | RegOp::JumpUnlessNeIK { to: t, .. }
            | RegOp::ForNext { end: t, .. }
            | RegOp::ForNextAt { end: t, .. }
            | RegOp::ForMapNext { end: t, .. }
            | RegOp::ForRange { end: t, .. }
            | RegOp::MatchLit { next: t, .. }
            | RegOp::MatchSome { next: t, .. }
            | RegOp::MatchNone { next: t, .. }
            | RegOp::MatchVariant { next: t, .. } => *t = to,
            other => unreachable!("移り先を直せない命令: {other:?}"),
        }
    }

    /// Takes `n` temporaries in a row and returns the first number.
    /// Places an instruction that jumps if the condition `cond` is false, and returns its position (the target is patched
    /// later).
    ///
    /// If the condition is an integer comparison, the comparison instruction is replaced with one that compares and jumps.
    /// Short circuits (`&&`, `||`) are not replaced (the target of a short-circuit jump is the position right after the
    /// comparison).
    fn jump_unless(&mut self, cond: ExprId, line: u32) -> usize {
        let c = r(self.expr(cond, None));
        let compare = matches!(
            self.ir.exprs[cond].kind,
            ExprKind::Binary { op, .. } if !matches!(op, BinOp::And | BinOp::Or)
        );
        let fused = match self.out.code.ops.last() {
            Some(op) if compare => match *op {
                RegOp::LtI { dst, a, b } if dst == c => Some(RegOp::JumpUnlessLtI { a, b, to: 0 }),
                RegOp::LeI { dst, a, b } if dst == c => Some(RegOp::JumpUnlessLeI { a, b, to: 0 }),
                RegOp::LtIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessLtIK { a, k, to: 0 })
                }
                RegOp::LeIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessLeIK { a, k, to: 0 })
                }
                RegOp::GtIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessGtIK { a, k, to: 0 })
                }
                RegOp::GeIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessGeIK { a, k, to: 0 })
                }
                RegOp::EqIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessEqIK { a, k, to: 0 })
                }
                RegOp::NeIK { dst, a, k } if dst == c => {
                    Some(RegOp::JumpUnlessNeIK { a, k, to: 0 })
                }
                _ => None,
            },
            _ => None,
        };
        match fused {
            Some(op) => {
                let at = self.out.code.ops.len() - 1;
                self.out.code.ops[at] = op;
                self.out.lines[at] = line;
                at
            }
            None => self.emit(RegOp::JumpIfFalse { cond: c, to: 0 }, line),
        }
    }

    fn temps(&mut self, n: usize) -> u32 {
        let first = self.next;
        self.next += n as u32;
        self.max = self.max.max(self.next);
        first
    }

    fn temp(&mut self) -> u32 {
        self.temps(1)
    }

    /// Where to put the value (a new temporary if not specified).
    fn dst(&mut self, want: Option<u32>) -> u32 {
        want.unwrap_or_else(|| self.temp())
    }

    fn konst(&mut self, v: Value) -> crate::pool::Idx<Value> {
        self.out.code.consts.push(v)
    }

    // ---- Bodies and statements ----

    fn body(&mut self, body: &Body) {
        self.block(&body.stmts);
        let line = body.line;
        match &body.tail {
            Tail::Return if self.in_fn => {
                self.emit(RegOp::ReturnFn { src: None }, line);
            }
            Tail::Return => {
                self.emit(RegOp::Return { src: None }, line);
            }
            Tail::End => {
                self.emit(RegOp::End, line);
            }
            Tail::Repl(expr) => {
                let src = expr.map(|e| r(self.expr(e, None)));
                self.emit(RegOp::ReplDone { src }, line);
            }
        }
    }

    fn block(&mut self, ids: &[StmtId]) {
        for &id in ids {
            self.stmt(id);
        }
    }

    fn stmt(&mut self, id: StmtId) {
        let mark = self.next;
        self.stmt_inner(id);
        self.next = mark;
    }

    fn stmt_inner(&mut self, id: StmtId) {
        let ir = self.ir;
        let st = &ir.stmts[id];
        let line = st.line;
        match &st.kind {
            StmtKind::Eval(expr) => {
                self.expr(*expr, None);
            }
            StmtKind::Store { place, value } => self.store(*place, *value, line),
            StmtKind::Compound {
                place,
                op,
                ty,
                value,
            } => self.compound(*place, *op, *ty, *value, line),
            StmtKind::Mutate {
                place,
                path,
                indices,
                operands,
                action,
            } => {
                let start = self.temps(indices.len() + operands.len());
                for (i, &e) in indices.iter().chain(operands).enumerate() {
                    self.expr(e, Some(start + i as u32));
                }
                let plan = self.out.code.plans.push((path.clone(), *action));
                self.emit(
                    RegOp::Mutate {
                        place: *place,
                        plan,
                        start: r(start),
                    },
                    line,
                );
            }
            StmtKind::If { arms, else_body } => {
                let mut to_end = Vec::new();
                for (i, arm) in arms.iter().enumerate() {
                    let mark = self.next;
                    let jf = self.jump_unless(arm.cond, arm.line);
                    self.next = mark;
                    self.block(&arm.body);
                    if i + 1 < arms.len() || !else_body.is_empty() {
                        to_end.push(self.emit(RegOp::Jump { to: 0 }, arm.line));
                    }
                    let next = self.here();
                    self.patch(jf, next);
                }
                self.block(else_body);
                let end = self.here();
                for j in to_end {
                    self.patch(j, end);
                }
            }
            StmtKind::While { cond, body } => {
                let top = self.here();
                let mark = self.next;
                let jf = self.jump_unless(*cond, line);
                self.next = mark;
                self.loop_body(top, body, line);
                let end = self.here();
                self.patch(jf, end);
                self.close_loop(end);
            }
            StmtKind::For {
                iter,
                list,
                idx,
                var,
                key,
                kind,
                body,
            } => {
                let (list, idx, var) = (r(*list), r(*idx), r(*var));
                if let ForKind::Range { end } = kind {
                    self.expr(*iter, Some(u32::from(idx)));
                    self.expr(*end, Some(u32::from(list)));
                } else {
                    let v = self.expr(*iter, None);
                    let src = r(v);
                    self.emit(
                        if v >= self.locals {
                            RegOp::SetMove { dst: list, src }
                        } else {
                            RegOp::Set { dst: list, src }
                        },
                        line,
                    );
                    if *kind == ForKind::Map {
                        self.emit(RegOp::MapCompact { map: list }, line);
                    }
                    self.emit(RegOp::Int { dst: idx, n: 0 }, line);
                }
                let top = self.here();
                let op = match (kind, key) {
                    (ForKind::Range { .. }, _) => RegOp::ForRange {
                        idx,
                        limit: list,
                        var,
                        end: 0,
                    },
                    (ForKind::Map, Some(key)) => RegOp::ForMapNext {
                        map: list,
                        idx,
                        key: r(*key),
                        var,
                        end: 0,
                    },
                    (_, Some(key)) => RegOp::ForNextAt {
                        list,
                        idx,
                        var,
                        key: r(*key),
                        end: 0,
                    },
                    (_, None) => RegOp::ForNext {
                        list,
                        idx,
                        var,
                        end: 0,
                    },
                };
                let next = self.emit(op, line);
                self.loop_body(top, body, line);
                let end = self.here();
                self.patch(next, end);
                self.close_loop(end);
            }
            StmtKind::Match { scrutinee, arms } => {
                let s = r(self.expr(*scrutinee, None));
                let mut next: Option<usize> = None;
                let mut ends = Vec::new();
                for arm in arms {
                    if let Some(j) = next.take() {
                        let here = self.here();
                        self.patch(j, here);
                    }
                    if let Some((test, test_line)) = &arm.test {
                        let op = match test {
                            PatTest::Lit(v) => RegOp::MatchLit {
                                src: s,
                                k: self.konst(v.clone()),
                                next: 0,
                            },
                            PatTest::Some => RegOp::MatchSome { src: s, next: 0 },
                            PatTest::None => RegOp::MatchNone { src: s, next: 0 },
                            PatTest::Variant(name) => RegOp::MatchVariant {
                                src: s,
                                name: self.out.code.names.intern(name.clone()),
                                next: 0,
                            },
                        };
                        next = Some(self.emit(op, *test_line));
                    }
                    for &(payload, slot, bind_line) in &arm.binds {
                        let t = r(self.temp());
                        self.emit(
                            RegOp::Payload {
                                dst: t,
                                src: s,
                                i: payload,
                            },
                            bind_line,
                        );
                        self.emit(
                            RegOp::SetMove {
                                dst: r(slot),
                                src: t,
                            },
                            bind_line,
                        );
                    }
                    // If the subject is a temporary, discard it on entering the arm (do not leave the variable's value shared)
                    if u32::from(s) >= self.locals {
                        self.emit(RegOp::Clear { r: s }, arm.line);
                    }
                    self.block(&arm.body);
                    ends.push(self.emit(RegOp::Jump { to: 0 }, arm.line));
                }
                if let Some(j) = next {
                    let here = self.here();
                    self.patch(j, here);
                }
                if u32::from(s) >= self.locals {
                    self.emit(RegOp::Clear { r: s }, line);
                }
                let end = self.here();
                for j in ends {
                    self.patch(j, end);
                }
            }
            StmtKind::Choice(c) => self.choice(c, line),
            StmtKind::Break => {
                let j = self.emit(RegOp::Jump { to: 0 }, line);
                if let Some(l) = self.loops.last_mut() {
                    l.breaks.push(j);
                }
            }
            StmtKind::Continue => {
                let top = self.loops.last().map_or(0, |l| l.top);
                self.emit(RegOp::Jump { to: top }, line);
            }
            StmtKind::JumpLabel(label) => {
                self.emit(RegOp::JumpLabel { label: *label }, line);
            }
            StmtKind::End => {
                self.emit(RegOp::End, line);
            }
            StmtKind::Return(value) => {
                let src = value.map(|e| r(self.expr(e, None)));
                self.emit(
                    if self.in_fn {
                        RegOp::ReturnFn { src }
                    } else {
                        RegOp::Return { src }
                    },
                    line,
                );
            }
        }
    }

    fn loop_body(&mut self, top: u32, body: &[StmtId], line: u32) {
        self.loops.push(LoopTargets {
            top,
            breaks: Vec::new(),
        });
        self.block(body);
        self.emit(RegOp::Jump { to: top }, line);
    }

    fn close_loop(&mut self, end: u32) {
        if let Some(done) = self.loops.pop() {
            for b in done.breaks {
                self.patch(b, end);
            }
        }
    }

    /// Stores into a variable. Into local variables, with an instruction that counts the size. However, expressions that
    /// produce numbers are built directly into the local variable (the type is known and the size does not change, so it is
    /// not counted).
    fn store(&mut self, place: PlaceRef, value: ExprId, line: u32) {
        match place {
            PlaceRef::Local(x) if self.writes_scalar_last(value) => {
                self.expr(value, Some(x));
            }
            PlaceRef::Local(x) => {
                let v = self.expr(value, None);
                // From a temporary, move instead of copy (do not leave it shared)
                let op = if v >= self.locals {
                    RegOp::SetMove {
                        dst: r(x),
                        src: r(v),
                    }
                } else {
                    RegOp::Set {
                        dst: r(x),
                        src: r(v),
                    }
                };
                self.emit(op, line);
            }
            PlaceRef::Global(g) => {
                let v = self.expr(value, None);
                self.emit(RegOp::StoreGlobal { g, src: r(v) }, line);
            }
            PlaceRef::Host(h) => {
                let v = self.expr(value, None);
                self.emit(RegOp::StoreHost { h, src: r(v) }, line);
            }
        }
    }

    /// Whether the expression's result is an `int`, `float` or `bool` and it writes to its destination only once, at the end
    /// (it can be built directly into a local variable).
    fn writes_scalar_last(&self, id: ExprId) -> bool {
        match &self.ir.exprs[id].kind {
            ExprKind::Binary { op, .. } => !matches!(op, BinOp::And | BinOp::Or),
            ExprKind::Unary { .. } => true,
            ExprKind::Const(v) => matches!(v, Value::Int(_) | Value::Float(_) | Value::Bool(_)),
            ExprKind::Builtin { builtin, .. } => !matches!(builtin, Builtin::Range),
            ExprKind::Method { method, .. } => matches!(
                method,
                Method::Len | Method::IsEmpty | Method::ListContains | Method::MapHas
            ),
            _ => false,
        }
    }

    /// `variable op= value`. Evaluates the value first, then reads the variable.
    fn compound(&mut self, place: PlaceRef, op: ArithOp, ty: Operand, value: ExprId, line: u32) {
        // If the right-hand side is an integer constant, use an immediate instruction (constants do not affect evaluation order)
        let k = match (&self.ir.exprs[value].kind, ty) {
            (ExprKind::Const(Value::Int(k)), Operand::Int) => Some(*k),
            _ => None,
        };
        let v = match k {
            Some(_) => 0,
            None => self.expr(value, None),
        };
        let (var, store) = match place {
            PlaceRef::Local(x) => (x, None),
            PlaceRef::Global(g) => {
                let t = self.temp();
                self.emit(RegOp::LoadGlobal { dst: r(t), g }, line);
                (t, Some(RegOp::StoreGlobal { g, src: r(t) }))
            }
            PlaceRef::Host(h) => {
                let t = self.temp();
                self.emit(RegOp::LoadHost { dst: r(t), h }, line);
                (t, Some(RegOp::StoreHost { h, src: r(t) }))
            }
        };
        let (dst, a) = (r(var), r(var));
        let code = match (ty, k) {
            (Operand::Int, Some(k)) => int_k(op, dst, a, k),
            (Operand::Int, None) => int_op(op, dst, a, r(v)),
            _ => RegOp::ArithF {
                op,
                dst,
                a,
                b: r(v),
            },
        };
        self.emit(code, line);
        if let Some(store) = store {
            self.emit(store, line);
        }
    }

    /// `choice`. Builds the list of options, stops, and moves to the body of the chosen option.
    fn choice(&mut self, c: &Choice, line: u32) {
        let args_pc = self.here();
        self.emit(RegOp::ArgsBegin, line);
        self.emit(RegOp::ChoiceBegin, line);
        for (i, o) in c.options.iter().enumerate() {
            let mark = self.next;
            let mut skips = Vec::new();
            if let Some(key) = &o.once {
                let t = r(self.temp());
                let key = self.out.code.names.intern(key.clone());
                self.emit(RegOp::OnceAvailable { dst: t, key }, o.line);
                skips.push(self.emit(RegOp::JumpIfFalse { cond: t, to: 0 }, o.line));
            }
            if let Some(cond) = o.cond {
                let cr = r(self.expr(cond, None));
                skips.push(self.emit(RegOp::JumpIfFalse { cond: cr, to: 0 }, o.line));
            }
            self.emit(RegOp::TextBegin, o.line);
            let text = r(self.expr(o.text, None));
            let id = r(self.temp());
            let k = self.konst(o.id.clone());
            self.emit(RegOp::Const { dst: id, k }, o.line);
            self.emit(
                RegOp::ChoiceItem {
                    text,
                    id,
                    orig: i as u32,
                },
                o.line,
            );
            let next = self.here();
            for j in skips {
                self.patch(j, next);
            }
            self.next = mark;
        }
        let dst = r(self.temp());
        let ordinal = self.out.stops.len() as u32;
        let pc = self.here();
        self.emit(
            RegOp::ChoiceStop {
                dst,
                cmd: c.cmd,
                stop: ordinal,
            },
            line,
        );
        self.push_stop(&c.info, args_pc, pc);
        let table = self.out.code.tables.push(Vec::new());
        self.emit(RegOp::JumpTable { src: dst, table }, line);
        let mut targets = Vec::new();
        let mut ends = Vec::new();
        for o in &c.options {
            targets.push(self.here());
            if let Some(key) = &o.once {
                let key = self.out.code.names.intern(key.clone());
                self.emit(RegOp::MarkOnce { key }, o.line);
            }
            self.block(&o.body);
            ends.push(self.emit(RegOp::Jump { to: 0 }, o.line));
        }
        let end = self.here();
        for j in ends {
            self.patch(j, end);
        }
        self.out.code.tables[table] = targets;
    }

    fn push_stop(&mut self, info: &StopInfo, args_pc: u32, pc: u32) {
        let mut info = info.clone();
        info.args_pc = args_pc;
        info.pc = pc;
        self.out.stops.push(info);
    }

    // ---- Expressions ----

    /// Emits instructions that compute the value of an expression and returns the register holding the value. If `want` is
    /// given, puts it there. Otherwise, it may return a local variable as is (without copying).
    fn expr(&mut self, id: ExprId, want: Option<u32>) -> u32 {
        let ir = self.ir;
        let e = &ir.exprs[id];
        let line = e.line;
        match &e.kind {
            ExprKind::Error => self.dst(want),
            ExprKind::Const(Value::Int(n)) => {
                let d = self.dst(want);
                self.emit(RegOp::Int { dst: r(d), n: *n }, line);
                d
            }
            ExprKind::Const(v) => {
                let d = self.dst(want);
                let k = self.konst(v.clone());
                self.emit(RegOp::Const { dst: r(d), k }, line);
                d
            }
            ExprKind::Var(PlaceRef::Local(x)) => match want {
                Some(d) if d != *x => {
                    self.emit(
                        RegOp::Move {
                            dst: r(d),
                            src: r(*x),
                        },
                        line,
                    );
                    d
                }
                _ => *x,
            },
            ExprKind::Var(PlaceRef::Global(g)) => {
                let d = self.dst(want);
                self.emit(RegOp::LoadGlobal { dst: r(d), g: *g }, line);
                d
            }
            ExprKind::Var(PlaceRef::Host(h)) => {
                let d = self.dst(want);
                self.emit(RegOp::LoadHost { dst: r(d), h: *h }, line);
                d
            }
            ExprKind::Field { base, field } => self.field(*base, *field, want, line),
            ExprKind::IdField { base, name } => {
                let d = self.dst(want);
                let src = self.expr(*base, None);
                self.emit(
                    RegOp::IdField {
                        dst: r(d),
                        src: r(src),
                        name: *name,
                    },
                    line,
                );
                d
            }
            ExprKind::Unary { op, operand } => {
                let d = self.dst(want);
                let a = r(self.expr(*operand, None));
                let dst = r(d);
                self.emit(
                    match op {
                        UnOp::Neg => RegOp::Neg { dst, a },
                        UnOp::Not => RegOp::Not { dst, a },
                    },
                    line,
                );
                d
            }
            ExprKind::Binary { op, ty, lhs, rhs } => {
                self.binary(*op, *ty, (*lhs, *rhs), want, line)
            }
            ExprKind::List(items) => {
                let d = self.dst(want);
                let start = self.block_of(items);
                self.emit(
                    RegOp::MakeList {
                        dst: r(d),
                        start: r(start),
                        n: items.len() as u16,
                    },
                    line,
                );
                d
            }
            ExprKind::Map(entries) => {
                let d = self.dst(want);
                let start = self.temps(entries.len() * 2);
                for (i, &(k, v)) in entries.iter().enumerate() {
                    self.expr(k, Some(start + 2 * i as u32));
                    self.expr(v, Some(start + 2 * i as u32 + 1));
                }
                self.emit(
                    RegOp::MakeMap {
                        dst: r(d),
                        start: r(start),
                        n: entries.len() as u16,
                    },
                    line,
                );
                d
            }
            ExprKind::Some(inner) => {
                let d = self.dst(want);
                let a = r(self.expr(*inner, None));
                self.emit(RegOp::MakeSome { dst: r(d), a }, line);
                d
            }
            ExprKind::Struct { ty, order, fields } => {
                let d = self.dst(want);
                let start = self.block_of(fields);
                let order = self.out.code.orders.push(order.clone());
                self.emit(
                    RegOp::MakeStruct {
                        dst: r(d),
                        start: r(start),
                        ty: *ty,
                        order,
                    },
                    line,
                );
                d
            }
            ExprKind::Enum { ty, variant, args } => {
                let d = self.dst(want);
                let start = self.block_of(args);
                self.emit(
                    RegOp::MakeEnum {
                        dst: r(d),
                        start: r(start),
                        n: args.len() as u16,
                        ty: *ty,
                        variant: *variant,
                    },
                    line,
                );
                d
            }
            ExprKind::Index { base, index } => {
                let d = self.dst(want);
                let b = r(self.expr(*base, None));
                let i = r(self.expr(*index, None));
                self.emit(
                    RegOp::Index {
                        dst: r(d),
                        base: b,
                        index: i,
                    },
                    line,
                );
                d
            }
            ExprKind::Method { method, base, args } => {
                self.method(*method, *base, args, want, line)
            }
            ExprKind::Builtin { builtin, args } => self.builtin(*builtin, args, want, line),
            ExprKind::Text { parts, record } => self.text(parts, *record, want, line),
            ExprKind::CallFn { f, args, .. } => {
                let d = self.dst(want);
                let start = self.block_of(args);
                self.emit(
                    RegOp::CallFn {
                        dst: r(d),
                        start: r(start),
                        n: args.len() as u16,
                        f: *f,
                    },
                    line,
                );
                d
            }
            ExprKind::CallValue { callee, args } => {
                let d = self.dst(want);
                let c = self.temp();
                self.expr(*callee, Some(c));
                let start = self.block_of(args);
                self.emit(
                    RegOp::CallValue {
                        dst: r(d),
                        callee: r(c),
                        start: r(start),
                        n: args.len() as u16,
                    },
                    line,
                );
                d
            }
            ExprKind::MakeFn { f, captures } => {
                let d = self.dst(want);
                let start = self.block_of(captures);
                self.emit(
                    RegOp::MakeFn {
                        dst: r(d),
                        start: r(start),
                        n: captures.len() as u16,
                        f: *f,
                    },
                    line,
                );
                d
            }
            ExprKind::CallLabel {
                label, args, hash, ..
            } => self.call_label(*label, args, *hash, want, line),
            ExprKind::CallQuery { native, args } => {
                let d = self.dst(want);
                let start = self.block_of(args);
                self.emit(
                    RegOp::CallQuery {
                        dst: r(d),
                        start: r(start),
                        n: args.len() as u16,
                        native: *native,
                    },
                    line,
                );
                d
            }
            ExprKind::CallEffect { native, args, .. } => {
                let d = self.dst(want);
                let start = self.block_of(args);
                self.emit(
                    RegOp::CallEffect {
                        dst: r(d),
                        start: r(start),
                        n: args.len() as u16,
                        native: *native,
                    },
                    line,
                );
                d
            }
            ExprKind::Stop(stop) => self.stop(stop, want, line),
        }
    }

    /// `expr.field`. If it only follows fields from a variable, reads directly from the variable.
    fn field(&mut self, base: ExprId, field: u32, want: Option<u32>, line: u32) -> u32 {
        if let Some((place, mut path)) = self.var_path(base) {
            path.push(field);
            let d = self.dst(want);
            let path = self.out.code.paths.push(path.into_boxed_slice());
            self.emit(
                RegOp::LoadField {
                    dst: r(d),
                    place,
                    path,
                },
                line,
            );
            return d;
        }
        let d = self.dst(want);
        let src = self.expr(base, None);
        self.emit(
            RegOp::GetField {
                dst: r(d),
                src: r(src),
                field,
            },
            line,
        );
        d
    }

    fn method(
        &mut self,
        method: Method,
        base: ExprId,
        args: &[ExprId],
        want: Option<u32>,
        line: u32,
    ) -> u32 {
        let d = self.dst(want);
        let b = r(self.expr(base, None));
        let a: Vec<Reg> = args.iter().map(|&x| r(self.expr(x, None))).collect();
        let arg = |i: usize| a.get(i).copied().unwrap_or(0);
        let dst = r(d);
        let op = match method {
            Method::Len => RegOp::Len { dst, a: b },
            Method::IsEmpty => RegOp::IsEmpty { dst, a: b },
            Method::ListContains => RegOp::ListContains {
                dst,
                list: b,
                item: arg(0),
            },
            Method::ListSort => RegOp::ListSortBy {
                dst,
                list: b,
                keys: b,
            },
            Method::ListSortBy => RegOp::ListSortBy {
                dst,
                list: b,
                keys: arg(0),
            },
            Method::ListReverse => RegOp::ListReverse { dst, list: b },
            Method::ListSum | Method::ListSumFloat => RegOp::ListSum {
                dst,
                list: b,
                float: method == Method::ListSumFloat,
            },
            Method::ListMinBy | Method::ListMaxBy => RegOp::ListExtreme {
                dst,
                list: b,
                keys: arg(0),
                max: method == Method::ListMaxBy,
            },
            Method::MapHas => RegOp::MapHas {
                dst,
                map: b,
                key: arg(0),
            },
            Method::MapGet => RegOp::MapGet {
                dst,
                map: b,
                key: arg(0),
            },
            Method::MapKeys => RegOp::MapKeys { dst, map: b },
            Method::MapValues => RegOp::MapValues { dst, map: b },
        };
        self.emit(op, line);
        d
    }

    fn builtin(&mut self, builtin: Builtin, args: &[ExprId], want: Option<u32>, line: u32) -> u32 {
        let d = self.dst(want);
        let a: Vec<Reg> = args.iter().map(|&x| r(self.expr(x, None))).collect();
        let arg = |i: usize| a.get(i).copied().unwrap_or(0);
        let dst = r(d);
        let op = match builtin {
            Builtin::ToInt => RegOp::ToInt { dst, a: arg(0) },
            Builtin::ToFloat => RegOp::ToFloat { dst, a: arg(0) },
            Builtin::Seen => RegOp::Seen { dst, a: arg(0) },
            Builtin::Visited => RegOp::Visited { dst, a: arg(0) },
            Builtin::Random => RegOp::Random {
                dst,
                a: arg(0),
                b: arg(1),
            },
            Builtin::RandomFloat => RegOp::RandomFloat { dst },
            Builtin::Range => RegOp::Range {
                dst,
                a: arg(0),
                b: arg(1),
            },
        };
        self.emit(op, line);
        d
    }

    /// A `call` in an expression. Records the position of the call for resolving positions on load.
    fn call_label(
        &mut self,
        label: u32,
        args: &[ExprId],
        hash: u64,
        want: Option<u32>,
        line: u32,
    ) -> u32 {
        let d = self.dst(want);
        let start = self.block_of(args);
        let site = self.out.calls.len() as u32;
        let pc = self.emit(
            RegOp::Call {
                dst: r(d),
                start: r(start),
                n: args.len() as u16,
                label,
                site,
            },
            line,
        );
        self.out.calls.push(CallSite {
            pc: pc as u32,
            hash,
            target: label,
            line,
        });
        d
    }

    /// If the expression follows fields from a variable, the variable and the sequence of fields followed.
    fn var_path(&self, id: ExprId) -> Option<(PlaceRef, Vec<u32>)> {
        match &self.ir.exprs[id].kind {
            ExprKind::Var(place) => Some((*place, Vec::new())),
            ExprKind::Field { base, field } => {
                let (place, mut path) = self.var_path(*base)?;
                path.push(*field);
                Some((place, path))
            }
            _ => None,
        }
    }

    /// Places a sequence of expressions in consecutive temporaries, in the order written. Returns the first number.
    fn block_of(&mut self, ids: &[ExprId]) -> u32 {
        let start = self.temps(ids.len());
        for (i, &e) in ids.iter().enumerate() {
            self.expr(e, Some(start + i as u32));
        }
        start
    }

    fn binary(
        &mut self,
        op: BinOp,
        ty: Operand,
        (lhs, rhs): (ExprId, ExprId),
        want: Option<u32>,
        line: u32,
    ) -> u32 {
        if matches!(op, BinOp::And | BinOp::Or) {
            // Place the left value, and if it short-circuits, finish there
            let d = self.dst(want);
            self.expr(lhs, Some(d));
            let cond = r(d);
            let j = self.emit(
                if op == BinOp::And {
                    RegOp::JumpIfFalse { cond, to: 0 }
                } else {
                    RegOp::JumpIfTrue { cond, to: 0 }
                },
                line,
            );
            self.expr(rhs, Some(d));
            let end = self.here();
            self.patch(j, end);
            return d;
        }
        let d = self.dst(want);
        let a = r(self.expr(lhs, None));
        // If the right-hand side is an integer constant, use an immediate instruction (constants do not affect evaluation order)
        if ty == Operand::Int
            && let ExprKind::Const(Value::Int(k)) = &self.ir.exprs[rhs].kind
        {
            let k = *k;
            let dst = r(d);
            let code = match op {
                BinOp::Add => Some(int_k(ArithOp::Add, dst, a, k)),
                BinOp::Sub => Some(int_k(ArithOp::Sub, dst, a, k)),
                BinOp::Mul => Some(int_k(ArithOp::Mul, dst, a, k)),
                BinOp::Div => Some(int_k(ArithOp::Div, dst, a, k)),
                BinOp::Rem => Some(int_k(ArithOp::Rem, dst, a, k)),
                BinOp::Lt => Some(RegOp::LtIK { dst, a, k }),
                BinOp::Le => Some(RegOp::LeIK { dst, a, k }),
                BinOp::Gt => Some(RegOp::GtIK { dst, a, k }),
                BinOp::Ge => Some(RegOp::GeIK { dst, a, k }),
                BinOp::Eq => Some(RegOp::EqIK { dst, a, k }),
                BinOp::Ne => Some(RegOp::NeIK { dst, a, k }),
                BinOp::And | BinOp::Or => None,
            };
            if let Some(code) = code {
                self.emit(code, line);
                return d;
            }
        }
        let b = r(self.expr(rhs, None));
        let dst = r(d);
        let code = match (op, ty) {
            (BinOp::Eq, _) => RegOp::Eq { dst, a, b },
            (BinOp::Ne, _) => RegOp::Ne { dst, a, b },
            (BinOp::Add, Operand::Int) => int_op(ArithOp::Add, dst, a, b),
            (BinOp::Sub, Operand::Int) => int_op(ArithOp::Sub, dst, a, b),
            (BinOp::Mul, Operand::Int) => int_op(ArithOp::Mul, dst, a, b),
            (BinOp::Div, Operand::Int) => int_op(ArithOp::Div, dst, a, b),
            (BinOp::Rem, Operand::Int) => int_op(ArithOp::Rem, dst, a, b),
            (BinOp::Lt, Operand::Int) => RegOp::LtI { dst, a, b },
            (BinOp::Le, Operand::Int) => RegOp::LeI { dst, a, b },
            (BinOp::Gt, Operand::Int) => RegOp::LtI { dst, a: b, b: a },
            (BinOp::Ge, Operand::Int) => RegOp::LeI { dst, a: b, b: a },
            (BinOp::Add, _) => float_op(ArithOp::Add, dst, a, b),
            (BinOp::Sub, _) => float_op(ArithOp::Sub, dst, a, b),
            (BinOp::Mul, _) => float_op(ArithOp::Mul, dst, a, b),
            (BinOp::Div, _) => float_op(ArithOp::Div, dst, a, b),
            (BinOp::Rem, _) => float_op(ArithOp::Rem, dst, a, b),
            (BinOp::Lt, _) => RegOp::LtF { dst, a, b },
            (BinOp::Le, _) => RegOp::LeF { dst, a, b },
            (BinOp::Gt, _) => RegOp::LtF { dst, a: b, b: a },
            (BinOp::Ge, _) => RegOp::LeF { dst, a: b, b: a },
            (BinOp::And | BinOp::Or, _) => unreachable!("短絡は、先に扱った"),
        };
        self.emit(code, line);
        d
    }

    /// Joins string parts into one string. If `record`, adds the values of `${expr}` to the current record of the body.
    fn text(&mut self, parts: &[TextPiece], record: bool, want: Option<u32>, line: u32) -> u32 {
        if let [only] = parts {
            let v = match only {
                TextPiece::Lit(t) => {
                    let d = self.dst(want);
                    let k = self.konst(Value::Str(t.clone().into()));
                    self.emit(RegOp::Const { dst: r(d), k }, line);
                    return d;
                }
                TextPiece::Interp { expr, to_str } => {
                    let v = self.expr(*expr, None);
                    if *to_str {
                        let d = self.dst(want);
                        self.emit(RegOp::ToStr { dst: r(d), a: r(v) }, line);
                        d
                    } else {
                        v
                    }
                }
            };
            if record {
                self.emit(RegOp::Capture { src: r(v) }, line);
            }
            return match want {
                Some(d) if d != v => {
                    self.emit(
                        RegOp::Move {
                            dst: r(d),
                            src: r(v),
                        },
                        line,
                    );
                    d
                }
                _ => v,
            };
        }
        let d = self.dst(want);
        let start = self.temps(parts.len());
        for (i, p) in parts.iter().enumerate() {
            let slot = start + i as u32;
            match p {
                TextPiece::Lit(t) => {
                    let k = self.konst(Value::Str(t.clone().into()));
                    self.emit(RegOp::Const { dst: r(slot), k }, line);
                }
                TextPiece::Interp { expr, to_str } => {
                    self.expr(*expr, Some(slot));
                    if *to_str {
                        self.emit(
                            RegOp::ToStr {
                                dst: r(slot),
                                a: r(slot),
                            },
                            line,
                        );
                    }
                    if record {
                        self.emit(RegOp::Capture { src: r(slot) }, line);
                    }
                }
            }
        }
        self.emit(
            RegOp::Concat {
                dst: r(d),
                start: r(start),
                n: parts.len() as u16,
            },
            line,
        );
        d
    }

    /// A stop point. Starts the sequence of instructions that compute the arguments with `ArgsBegin`, then adds the stopping
    /// instruction and the stop point information.
    fn stop(&mut self, stop: &Stop, want: Option<u32>, line: u32) -> u32 {
        let d = self.dst(want);
        let start = self.temps(stop.args.len());
        let args_pc = self.here();
        self.emit(RegOp::ArgsBegin, line);
        if let Some(id) = &stop.mark_seen {
            let key = self.out.code.names.intern(id.clone());
            self.emit(RegOp::MarkSeen { key }, line);
        }
        for (i, a) in stop.args.iter().enumerate() {
            if a.text {
                self.emit(RegOp::TextBegin, self.ir.exprs[a.expr].line);
            }
            self.expr(a.expr, Some(start + i as u32));
        }
        let ordinal = self.out.stops.len() as u32;
        let pc = self.here();
        self.emit(
            RegOp::Stop {
                dst: r(d),
                start: r(start),
                n: stop.args.len() as u16,
                cmd: stop.cmd,
                stop: ordinal,
            },
            line,
        );
        self.push_stop(&stop.info, args_pc, pc);
        d
    }
}

fn int_op(op: ArithOp, dst: Reg, a: Reg, b: Reg) -> RegOp {
    match op {
        ArithOp::Add => RegOp::AddI { dst, a, b },
        ArithOp::Sub => RegOp::SubI { dst, a, b },
        ArithOp::Mul => RegOp::MulI { dst, a, b },
        ArithOp::Div => RegOp::DivI { dst, a, b },
        ArithOp::Rem => RegOp::RemI { dst, a, b },
    }
}

fn int_k(op: ArithOp, dst: Reg, a: Reg, k: i64) -> RegOp {
    match op {
        ArithOp::Add => RegOp::AddIK { dst, a, k },
        ArithOp::Sub => RegOp::SubIK { dst, a, k },
        ArithOp::Mul => RegOp::MulIK { dst, a, k },
        ArithOp::Div => RegOp::DivIK { dst, a, k },
        ArithOp::Rem => RegOp::RemIK { dst, a, k },
    }
}

fn float_op(op: ArithOp, dst: Reg, a: Reg, b: Reg) -> RegOp {
    RegOp::ArithF { op, dst, a, b }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagArg, DiagCode, Registry, compile_modules};

    #[test]
    fn instructions_fit_in_16_bytes() {
        assert!(
            std::mem::size_of::<RegOp>() <= 16,
            "{}",
            std::mem::size_of::<RegOp>()
        );
    }

    #[test]
    fn label_exceeding_register_limit_is_error() {
        // The local variables alone reach the limit, and the temporaries of an expression that builds a `list` exceed it
        let mut src = String::from("label start {\n");
        for i in 0..MAX_REGS - 1 {
            src.push_str(&format!("  let v{i} = {i}\n"));
        }
        src.push_str("  let w = [1]\n}\n");
        let Err(diags) = compile_modules(&[("main", &src)], &Registry::new()) else {
            panic!("上限を超えるはず");
        };
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagCode::TooManyRegisters);
        assert_eq!(
            diags[0].args,
            vec![("limit", DiagArg::Int(i64::from(MAX_REGS)))]
        );
        // The position of the label name
        assert_eq!((diags[0].line, diags[0].col), (1, 7));
    }

    #[test]
    fn label_with_exactly_max_registers_compiles() {
        let mut src = String::from("label start {\n");
        for i in 0..MAX_REGS - 1 {
            src.push_str(&format!("  let v{i} = 0\n"));
        }
        src.push_str("}\n");
        let p = compile_modules(&[("main", &src)], &Registry::new()).expect("コンパイルできる");
        assert_eq!(p.labels[0].reg.regs, MAX_REGS - 1);
    }
}
