//! VM: the execution loop of register-based instructions.
//!
//! Registers include the temporaries that follow the declared count in the frame's array of local variables (`locals`).

use super::exec::Step;
use super::*;
use crate::program::{Reg, RegCode};

/// Takes the value of a register, leaving `()` (uses a temporary without copying).
#[inline(always)]
fn take_value(v: &mut Value) -> Value {
    std::mem::replace(v, Value::Unit)
}

/// If the operand is a temporary (at or after `vars`), takes it (discarded after use). For a local variable, `None`
/// (borrow and read). Leaving a copy of a variable's value in a temporary would keep `list`s and `map`s shared, causing a
/// copy on every update.
#[inline(always)]
fn own(regs: &mut [Value], r: Reg, vars: usize) -> Option<Value> {
    ((r as usize) >= vars).then(|| take_value(&mut regs[r as usize]))
}

#[inline(always)]
fn is_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Unit | Value::Int(_) | Value::Float(_) | Value::Bool(_)
    )
}

/// Writes a value to a register. If the previous value is a number, boolean or `()`, the drop is not called.
#[inline(always)]
fn put(slot: &mut Value, v: Value) {
    if is_scalar(slot) {
        std::mem::forget(std::mem::replace(slot, v));
    } else {
        *slot = v;
    }
}

/// The number of `fn` local variable arrays kept for reuse (so unused arrays are not kept after deep recursion).
const SPARE_LOCALS: usize = 16;

/// Writes a number or boolean to a register. If the previous value is the same kind, only overwrites it (the drop is not
/// called).
#[inline(always)]
fn put_int(slot: &mut Value, n: i64) {
    match slot {
        Value::Int(x) => *x = n,
        _ => put(slot, Value::Int(n)),
    }
}

/// The order of two sort keys (both `int`, `float`, `str` or `bool`; floats are always finite).
fn compare_keys(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        other => unreachable!("同じ型の並べ替えのキーのはず: {other:?}"),
    }
}

/// The total of a list of numbers. Overflow is a runtime error, the same as for `+`.
fn sum_values(items: &ListData, float: bool) -> Result<Value, VmErrorKind> {
    let mut total = if float {
        Value::Float(0.0)
    } else {
        Value::Int(0)
    };
    for v in items.iter() {
        total = arith(ArithOp::Add, total, v.clone())?;
    }
    Ok(total)
}

#[inline(always)]
fn put_bool(slot: &mut Value, b: bool) {
    match slot {
        Value::Bool(x) => *x = b,
        _ => put(slot, Value::Bool(b)),
    }
}

#[inline(always)]
fn put_float(slot: &mut Value, f: f64) {
    match slot {
        Value::Float(x) => *x = f,
        _ => put(slot, Value::Float(f)),
    }
}

/// The integer in a register (the compiler guarantees the type).
#[inline(always)]
fn int(regs: &[Value], r: Reg) -> i64 {
    match regs[r as usize] {
        Value::Int(n) => n,
        ref other => unreachable!("整数のはず: {other:?}"),
    }
}

#[inline(always)]
fn float(regs: &[Value], r: Reg) -> f64 {
    match regs[r as usize] {
        Value::Float(x) => x,
        ref other => unreachable!("小数のはず: {other:?}"),
    }
}

#[inline(always)]
fn boolean(regs: &[Value], r: Reg) -> bool {
    match regs[r as usize] {
        Value::Bool(b) => b,
        ref other => unreachable!("真偽のはず: {other:?}"),
    }
}

/// The number of instructions between checks of the interrupt token.
const INTERRUPT_CHECK_INTERVAL: u64 = 1024;

impl<'a> Vm<'a> {
    /// Takes the values of the `n` registers from `start` (they are temporaries, so they are not copied).
    fn take_regs(&mut self, start: Reg, n: u16) -> Vec<Value> {
        let at = start as usize;
        self.locals[at..at + n as usize]
            .iter_mut()
            .map(take_value)
            .collect()
    }

    /// Puts a built value into a register after checking the size limit.
    fn put_checked(&mut self, dst: Reg, v: Value) -> Result<(), VmErrorKind> {
        if v.approx_bytes() > self.cfg.max_value_bytes {
            return Err(VmErrorKind::ValueTooLarge);
        }
        self.locals[dst as usize] = v;
        Ok(())
    }

    /// The code being run.
    fn reg_code<'p>(&self, program: &'p Program) -> &'p RegCode {
        match self.cur_fn {
            Some(f) => &program.fns[f].reg,
            None => &program.labels[self.label].reg,
        }
    }

    /// Runs register-based instructions. If `until` is given, stops on reaching that position.
    pub(super) fn exec(&mut self, until: Option<usize>) -> Result<Inner, VmError> {
        let program_ref = self.program.clone();
        let program: &Program = &program_ref;
        let mut steps: u64 = 0;
        // The step limit and the interrupt are checked at one comparison: `next_check` is the nearer of the two
        let limit_at = self
            .cfg
            .max_steps_per_run
            .map_or(u64::MAX, |m| m.saturating_add(1));
        let single = self.dbg_single;
        // In single-step mode, the first instruction runs and the check before the second one returns
        let mut next_check = if single {
            2
        } else {
            limit_at.min(INTERRUPT_CHECK_INTERVAL)
        };
        let until = until.unwrap_or(usize::MAX);
        let mut code = self.reg_code(program);
        let mut vars = code.vars as usize;
        macro_rules! fail {
            ($e:expr) => {
                return Err(self.err($e))
            };
        }
        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(k) => fail!(k),
                }
            };
        }
        if self.interrupt.is_interrupted() {
            fail!(VmErrorKind::Interrupted);
        }
        loop {
            // Inside a `fn`, do not look at the marker position (`fn` instruction numbers are separate from those of labels)
            if self.pc == until && self.cur_fn.is_none() {
                return Ok(Inner::Reached);
            }
            steps += 1;
            if steps >= next_check {
                if single {
                    return Ok(Inner::Stepped);
                }
                if steps >= limit_at {
                    fail!(VmErrorKind::StepLimitExceeded);
                }
                if self.interrupt.is_interrupted() {
                    fail!(VmErrorKind::Interrupted);
                }
                next_check = limit_at.min(steps + INTERRUPT_CHECK_INTERVAL);
            }
            let op = &code.ops[self.pc];
            self.pc += 1;
            let regs = &mut self.locals;
            // The values of the operands (taken if a temporary, borrowed if a local variable)
            macro_rules! val {
                ($own:expr, $r:expr) => {
                    match &$own {
                        Some(v) => v,
                        None => &regs[$r as usize],
                    }
                };
            }
            match op {
                RegOp::Int { dst, n } => regs[*dst as usize] = Value::Int(*n),
                RegOp::Const { dst, k } => regs[*dst as usize] = code.consts[*k].clone(),
                RegOp::Move { dst, src } => {
                    let o = own(regs, *src, vars);
                    let v = val!(o, *src).clone();
                    regs[*dst as usize] = v;
                }
                RegOp::Clear { r } => regs[*r as usize] = Value::Unit,
                RegOp::Set { dst, src } => {
                    let v = regs[*src as usize].clone();
                    tri!(self.set_local(*dst, v));
                }
                RegOp::SetMove { dst, src } => {
                    let v = take_value(&mut regs[*src as usize]);
                    tri!(self.set_local(*dst, v));
                }
                RegOp::LoadGlobal { dst, g } => {
                    regs[*dst as usize] = self.globals[*g as usize].clone();
                }
                RegOp::StoreGlobal { g, src } => {
                    let v = own(regs, *src, vars).unwrap_or_else(|| regs[*src as usize].clone());
                    let grow = v.approx_bytes();
                    let shrink = self.globals[*g as usize].approx_bytes();
                    self.globals[*g as usize] = v;
                    tri!(self.account(shrink, grow));
                }
                RegOp::LoadHost { dst, h } => {
                    let id = self.host_var_ids[*h as usize];
                    regs[*dst as usize] = self.host_vars[id].clone();
                }
                RegOp::StoreHost { h, src } => {
                    let v = own(regs, *src, vars).unwrap_or_else(|| regs[*src as usize].clone());
                    let id = self.host_var_ids[*h as usize];
                    let grow = v.approx_bytes();
                    let shrink = self.host_vars[id].approx_bytes();
                    self.host_vars[id] = v;
                    tri!(self.account(shrink, grow));
                }
                RegOp::LoadField { dst, place, path } => {
                    let mut v = match place {
                        PlaceRef::Local(i) => &regs[*i as usize],
                        PlaceRef::Global(i) => &self.globals[*i as usize],
                        PlaceRef::Host(i) => &self.host_vars[self.host_var_ids[*i as usize]],
                    };
                    for &f in code.paths[*path].iter() {
                        let Value::Struct(st) = v else {
                            unreachable!("struct のはず: {v:?}")
                        };
                        v = &st.fields[f as usize].1;
                    }
                    let v = v.clone();
                    self.locals[*dst as usize] = v;
                }
                RegOp::GetField { dst, src, field } => {
                    let Value::Struct(st) =
                        own(regs, *src, vars).unwrap_or_else(|| regs[*src as usize].clone())
                    else {
                        unreachable!("struct のはず")
                    };
                    // Take it without copying if not otherwise shared
                    let (_, v) = Arc::unwrap_or_clone(st).fields.swap_remove(*field as usize);
                    regs[*dst as usize] = v;
                }
                RegOp::IdField { dst, src, name } => {
                    let o = own(regs, *src, vars);
                    let Value::Identity(i) = val!(o, *src) else {
                        unreachable!("Identity のはず")
                    };
                    let s = if *name { i.name.clone() } else { i.id.clone() };
                    regs[*dst as usize] = Value::Str(s.into());
                }
                RegOp::AddI { dst, a, b } => {
                    let v = tri!(int_arith(ArithOp::Add, int(regs, *a), int(regs, *b)));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::SubI { dst, a, b } => {
                    let v = tri!(int_arith(ArithOp::Sub, int(regs, *a), int(regs, *b)));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::MulI { dst, a, b } => {
                    let v = tri!(int_arith(ArithOp::Mul, int(regs, *a), int(regs, *b)));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::DivI { dst, a, b } => {
                    let v = tri!(int_arith(ArithOp::Div, int(regs, *a), int(regs, *b)));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::RemI { dst, a, b } => {
                    let v = tri!(int_arith(ArithOp::Rem, int(regs, *a), int(regs, *b)));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::AddIK { dst, a, k } => {
                    let v = tri!(int_arith(ArithOp::Add, int(regs, *a), *k));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::SubIK { dst, a, k } => {
                    let v = tri!(int_arith(ArithOp::Sub, int(regs, *a), *k));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::MulIK { dst, a, k } => {
                    let v = tri!(int_arith(ArithOp::Mul, int(regs, *a), *k));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::DivIK { dst, a, k } => {
                    let v = tri!(int_arith(ArithOp::Div, int(regs, *a), *k));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::RemIK { dst, a, k } => {
                    let v = tri!(int_arith(ArithOp::Rem, int(regs, *a), *k));
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::LtI { dst, a, b } => {
                    let v = int(regs, *a) < int(regs, *b);
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::LeI { dst, a, b } => {
                    let v = int(regs, *a) <= int(regs, *b);
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::LtIK { dst, a, k } => {
                    let v = int(regs, *a) < *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::LeIK { dst, a, k } => {
                    let v = int(regs, *a) <= *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::GtIK { dst, a, k } => {
                    let v = int(regs, *a) > *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::GeIK { dst, a, k } => {
                    let v = int(regs, *a) >= *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::EqIK { dst, a, k } => {
                    let v = int(regs, *a) == *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::NeIK { dst, a, k } => {
                    let v = int(regs, *a) != *k;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::ArithF { op, dst, a, b } => {
                    let v = tri!(arith(
                        *op,
                        Value::Float(float(regs, *a)),
                        Value::Float(float(regs, *b))
                    ));
                    self.locals[*dst as usize] = v;
                }
                RegOp::LtF { dst, a, b } => {
                    let v = float(regs, *a) < float(regs, *b);
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::LeF { dst, a, b } => {
                    let v = float(regs, *a) <= float(regs, *b);
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::Eq { dst, a, b } | RegOp::Ne { dst, a, b } => {
                    let (oa, ob) = (own(regs, *a, vars), own(regs, *b, vars));
                    let eq = val!(oa, *a) == val!(ob, *b);
                    let ne = matches!(op, RegOp::Ne { .. });
                    let v = eq != ne;
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::Neg { dst, a } => {
                    let v = match regs[*a as usize] {
                        Value::Int(n) => n
                            .checked_neg()
                            .map(Value::Int)
                            .ok_or_else(|| VmErrorKind::IntegerOverflow),
                        // Normalize `-0.0` to `0.0`
                        Value::Float(x) => Ok(Value::Float(finite_float(-x).unwrap_or(0.0))),
                        ref other => unreachable!("数のはず: {other:?}"),
                    };
                    let v = tri!(v);
                    self.locals[*dst as usize] = v;
                }
                RegOp::Not { dst, a } => {
                    let v = !boolean(regs, *a);
                    put_bool(&mut regs[*dst as usize], v);
                }
                RegOp::ToInt { dst, a } => {
                    let x = float(regs, *a);
                    // 2^63 is outside the range of i64. `NaN` is not in range either.
                    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&x) {
                        let v = x.trunc() as i64;
                        put_int(&mut regs[*dst as usize], v);
                    } else {
                        fail!(VmErrorKind::FloatOutOfRange);
                    }
                }
                RegOp::ToFloat { dst, a } => {
                    let v = int(regs, *a) as f64;
                    put_float(&mut regs[*dst as usize], v);
                }
                RegOp::ToStr { dst, a } => {
                    let o = own(regs, *a, vars);
                    let s = val!(o, *a).to_text();
                    regs[*dst as usize] = Value::Str(s.into());
                }
                RegOp::Concat { dst, start, n } => {
                    let max = self.cfg.max_value_bytes;
                    let mut out = String::new();
                    let at = *start as usize;
                    let mut too_large = false;
                    for p in &mut regs[at..at + *n as usize] {
                        out.push_str(&take_value(p).to_text());
                        if out.len() > max {
                            too_large = true;
                            break;
                        }
                    }
                    if too_large {
                        fail!(VmErrorKind::ValueTooLarge);
                    }
                    self.locals[*dst as usize] = Value::Str(out.into());
                }
                RegOp::Range { dst, a, b } => {
                    let (lo, hi) = (int(regs, *a), int(regs, *b));
                    let n = i128::from(hi) - i128::from(lo);
                    let v = if n <= 0 {
                        Value::List(ListData::new())
                    } else {
                        // Each element is 8 bytes (`Value::approx_bytes`). Check the size before building
                        let bytes = 24u128.saturating_add((n as u128).saturating_mul(8));
                        if bytes > self.cfg.max_value_bytes as u128 {
                            fail!(VmErrorKind::ValueTooLarge);
                        }
                        Value::List(ListData::range(lo, hi))
                    };
                    self.locals[*dst as usize] = v;
                }
                RegOp::Random { dst, a, b } => {
                    let (min, max) = (int(regs, *a), int(regs, *b));
                    if min > max {
                        fail!(VmErrorKind::InvalidRandomRange { min, max });
                    }
                    let v = self.rng.range_inclusive(min, max);
                    put_int(&mut self.locals[*dst as usize], v);
                }
                RegOp::RandomFloat { dst } => {
                    let v = self.rng.unit_f64();
                    put_float(&mut regs[*dst as usize], v);
                }
                RegOp::Jump { to } => self.pc = *to as usize,
                RegOp::JumpIfFalse { cond, to } => {
                    if !boolean(regs, *cond) {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessLtI { a, b, to } => {
                    if int(regs, *a) >= int(regs, *b) {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessLeI { a, b, to } => {
                    if int(regs, *a) > int(regs, *b) {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessLtIK { a, k, to } => {
                    if int(regs, *a) >= *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessLeIK { a, k, to } => {
                    if int(regs, *a) > *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessGtIK { a, k, to } => {
                    if int(regs, *a) <= *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessGeIK { a, k, to } => {
                    if int(regs, *a) < *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessEqIK { a, k, to } => {
                    if int(regs, *a) != *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpUnlessNeIK { a, k, to } => {
                    if int(regs, *a) == *k {
                        self.pc = *to as usize;
                    }
                }
                RegOp::JumpIfTrue { cond, to } => {
                    if boolean(regs, *cond) {
                        self.pc = *to as usize;
                    }
                }
                RegOp::MakeList { dst, start, n } => {
                    let items = self.take_regs(*start, *n);
                    tri!(self.put_checked(*dst, Value::List(items.into())));
                }
                RegOp::MakeSome { dst, a } => {
                    let inner = own(regs, *a, vars).unwrap_or_else(|| regs[*a as usize].clone());
                    tri!(self.put_checked(*dst, Value::Option(Some(Box::new(inner)))));
                }
                RegOp::MakeStruct {
                    dst,
                    start,
                    ty,
                    order,
                } => {
                    let order = &code.orders[*order];
                    let given = self.take_regs(*start, order.len() as u16);
                    let v = build::make_struct(program, *ty, order, given);
                    tri!(self.put_checked(*dst, v));
                }
                RegOp::MakeEnum {
                    dst,
                    start,
                    n,
                    ty,
                    variant,
                } => {
                    let args = self.take_regs(*start, *n);
                    let v = build::make_enum(program, *ty, *variant, args);
                    tri!(self.put_checked(*dst, v));
                }
                RegOp::MakeMap { dst, start, n } => {
                    let flat = self.take_regs(*start, *n * 2);
                    tri!(self.put_checked(*dst, build::make_map(flat)));
                }
                RegOp::Index { dst, base, index } => {
                    let (ob, oi) = (own(regs, *base, vars), own(regs, *index, vars));
                    let v = build::index(val!(ob, *base), val!(oi, *index));
                    let v = tri!(v);
                    self.locals[*dst as usize] = v;
                }
                RegOp::MapGet { dst, map, key } => {
                    let (om, ok) = (own(regs, *map, vars), own(regs, *key, vars));
                    let Value::Map(entries) = val!(om, *map) else {
                        unreachable!("map のはず")
                    };
                    let found = entries.get(val!(ok, *key)).map(|v| Box::new(v.clone()));
                    regs[*dst as usize] = Value::Option(found);
                }
                RegOp::MapHas { dst, map, key } => {
                    let (om, ok) = (own(regs, *map, vars), own(regs, *key, vars));
                    let Value::Map(entries) = val!(om, *map) else {
                        unreachable!("map のはず")
                    };
                    let has = entries.contains_key(val!(ok, *key));
                    put_bool(&mut regs[*dst as usize], has);
                }
                RegOp::MapKeys { dst, map } | RegOp::MapValues { dst, map } => {
                    let om = own(regs, *map, vars);
                    let Value::Map(entries) = val!(om, *map) else {
                        unreachable!("map のはず")
                    };
                    let keys = matches!(op, RegOp::MapKeys { .. });
                    let items = entries
                        .iter()
                        .map(|(k, v)| if keys { k.clone() } else { v.clone() })
                        .collect();
                    regs[*dst as usize] = Value::List(items);
                }
                RegOp::Len { dst, a } => {
                    let o = own(regs, *a, vars);
                    let n = match val!(o, *a) {
                        Value::List(items) => items.len(),
                        Value::Map(entries) => entries.len(),
                        other => unreachable!("list か map のはず: {other:?}"),
                    };
                    let v = n as i64;
                    put_int(&mut regs[*dst as usize], v);
                }
                RegOp::IsEmpty { dst, a } => {
                    let o = own(regs, *a, vars);
                    let empty = match val!(o, *a) {
                        Value::List(items) => items.is_empty(),
                        Value::Map(entries) => entries.is_empty(),
                        other => unreachable!("list か map のはず: {other:?}"),
                    };
                    put_bool(&mut regs[*dst as usize], empty);
                }
                RegOp::ListContains { dst, list, item } => {
                    let (ol, oi) = (own(regs, *list, vars), own(regs, *item, vars));
                    let Value::List(items) = val!(ol, *list) else {
                        unreachable!("list のはず")
                    };
                    let has = items.contains(val!(oi, *item));
                    put_bool(&mut regs[*dst as usize], has);
                }
                RegOp::ListSortBy { dst, list, keys } => {
                    // A plain `sort` has the list itself as the keys (taking a temporary register twice would find it empty)
                    let same = list == keys;
                    let ol = own(regs, *list, vars);
                    let ok = if same { None } else { own(regs, *keys, vars) };
                    let Value::List(items) = val!(ol, *list) else {
                        unreachable!("list のはず")
                    };
                    let ks = if same {
                        items
                    } else {
                        let Value::List(ks) = val!(ok, *keys) else {
                            unreachable!("list のはず")
                        };
                        ks
                    };
                    let mut order: Vec<usize> = (0..items.len()).collect();
                    order.sort_by(|&a, &b| compare_keys(&ks[a], &ks[b]));
                    let sorted: ListData = order.into_iter().map(|i| items[i].clone()).collect();
                    regs[*dst as usize] = Value::List(sorted);
                }
                RegOp::ListReverse { dst, list } => {
                    let ol = own(regs, *list, vars);
                    let Value::List(items) = val!(ol, *list) else {
                        unreachable!("list のはず")
                    };
                    let reversed: ListData = items.iter().rev().cloned().collect();
                    regs[*dst as usize] = Value::List(reversed);
                }
                RegOp::ListSum { dst, list, float } => {
                    let ol = own(regs, *list, vars);
                    let Value::List(items) = val!(ol, *list) else {
                        unreachable!("list のはず")
                    };
                    let total = sum_values(items, *float);
                    let v = tri!(total);
                    regs[*dst as usize] = v;
                }
                RegOp::ListExtreme {
                    dst,
                    list,
                    keys,
                    max,
                } => {
                    let (ol, ok) = (own(regs, *list, vars), own(regs, *keys, vars));
                    let (Value::List(items), Value::List(ks)) = (val!(ol, *list), val!(ok, *keys))
                    else {
                        unreachable!("list のはず")
                    };
                    // The first element wins ties
                    let mut best: Option<usize> = None;
                    for i in 0..ks.len() {
                        let better = match best {
                            None => true,
                            Some(b) => {
                                let o = compare_keys(&ks[i], &ks[b]);
                                if *max { o.is_gt() } else { o.is_lt() }
                            }
                        };
                        if better {
                            best = Some(i);
                        }
                    }
                    let found = best.map(|i| Box::new(items[i].clone()));
                    regs[*dst as usize] = Value::Option(found);
                }
                RegOp::MatchLit { src, k, next } => {
                    if regs[*src as usize] != code.consts[*k] {
                        self.pc = *next as usize;
                    }
                }
                RegOp::MatchSome { src, next } => {
                    if !matches!(regs[*src as usize], Value::Option(Some(_))) {
                        self.pc = *next as usize;
                    }
                }
                RegOp::MatchNone { src, next } => {
                    if !matches!(regs[*src as usize], Value::Option(None)) {
                        self.pc = *next as usize;
                    }
                }
                RegOp::MatchVariant { src, name, next } => {
                    let hit = matches!(
                        &regs[*src as usize],
                        Value::Enum(e) if *e.variant == *code.names[*name]
                    );
                    if !hit {
                        self.pc = *next as usize;
                    }
                }
                RegOp::Payload { dst, src, i } => {
                    let v = match &regs[*src as usize] {
                        Value::Option(Some(inner)) => (**inner).clone(),
                        Value::Enum(e) => e.args[*i as usize].clone(),
                        other => unreachable!("中身のある値のはず: {other:?}"),
                    };
                    regs[*dst as usize] = v;
                }
                RegOp::ForNext {
                    list,
                    idx,
                    var,
                    end,
                } => {
                    let i = int(regs, *idx);
                    let item = match &regs[*list as usize] {
                        Value::List(items) => usize::try_from(i).ok().and_then(|i| items.get(i)),
                        other => unreachable!("反復の対象は list のはず: {other:?}"),
                    };
                    match item.cloned() {
                        Some(v) => {
                            let grow = v.approx_bytes();
                            let shrink = regs[*var as usize].approx_bytes();
                            regs[*var as usize] = v;
                            let v = i + 1;
                            put_int(&mut regs[*idx as usize], v);
                            tri!(self.account(shrink, grow));
                        }
                        None => self.pc = *end as usize,
                    }
                }
                RegOp::ForNextAt {
                    list,
                    idx,
                    var,
                    key,
                    end,
                } => {
                    if !tri!(self.for_next_at(*list, *idx, *var, *key)) {
                        self.pc = *end as usize;
                    }
                }
                RegOp::MapCompact { map } => match &mut regs[*map as usize] {
                    Value::Map(m) => m.compact_for_iteration(),
                    other => unreachable!("map のはず: {other:?}"),
                },
                RegOp::ForMapNext {
                    map,
                    idx,
                    key,
                    var,
                    end,
                } => {
                    if !tri!(self.for_map_next(*map, *idx, *key, *var)) {
                        self.pc = *end as usize;
                    }
                }
                RegOp::ForRange {
                    idx,
                    limit,
                    var,
                    end,
                } => {
                    let i = int(regs, *idx);
                    if i < int(regs, *limit) {
                        put_int(&mut regs[*var as usize], i);
                        // Since `i < limit`, it does not overflow
                        put_int(&mut regs[*idx as usize], i + 1);
                    } else {
                        self.pc = *end as usize;
                    }
                }
                RegOp::Mutate { place, plan, start } => {
                    let (path, action) = &code.plans[*plan];
                    let n_idx = mutate::index_count(path);
                    let n_ops = match action {
                        Action::Clear | Action::Shuffle => 0,
                        Action::Insert => 2,
                        _ => 1,
                    };
                    // Take them out without building an array (`idxs` is not allocated when there are no indices)
                    let at = *start as usize;
                    let mut idxs = std::mem::take(&mut self.scratch_idxs);
                    idxs.extend(self.locals[at..at + n_idx].iter_mut().map(take_value));
                    let operand =
                        (n_ops > 0).then(|| take_value(&mut self.locals[at + n_idx + n_ops - 1]));
                    let insert_at = (n_ops == 2).then(|| int(&self.locals, (at + n_idx) as Reg));
                    let result =
                        self.mutate_with(*place, (path, *action), &idxs, insert_at, operand);
                    idxs.clear();
                    self.scratch_idxs = idxs;
                    let (shrink, grow) = tri!(result);
                    tri!(self.account(shrink, grow));
                }
                RegOp::ArgsBegin => {
                    self.args_rng.clone_from(&self.rng);
                    self.cap.clear();
                }
                RegOp::TextBegin => self.cap.push(Vec::new()),
                RegOp::Capture { src } => {
                    if let (Value::Str(text), Some(unit)) =
                        (&self.locals[*src as usize], self.cap.last_mut())
                    {
                        unit.push(text.to_string());
                    }
                }
                RegOp::CallEffect {
                    dst,
                    start,
                    n,
                    native,
                }
                | RegOp::CallQuery {
                    dst,
                    start,
                    n,
                    native,
                } => {
                    let args = self.take_regs(*start, *n);
                    if let Some(v) = tri!(self.call_native(*native as usize, &args)) {
                        self.locals[*dst as usize] = v;
                    }
                }
                RegOp::Seen { dst, a } => {
                    let Value::Str(text) = &regs[*a as usize] else {
                        unreachable!("文字列のはず")
                    };
                    let text = text.to_string();
                    let r = self.has_seen(&text);
                    put_bool(&mut self.locals[*dst as usize], r);
                }
                RegOp::Visited { dst, a } => {
                    let Value::Str(text) = &regs[*a as usize] else {
                        unreachable!("文字列のはず")
                    };
                    let text = text.to_string();
                    let r = self.has_visited(&text);
                    put_bool(&mut self.locals[*dst as usize], r);
                }
                RegOp::OnceAvailable { dst, key } => {
                    let done = self
                        .once_done
                        .contains(&(self.label as u32, code.names[*key].clone()));
                    let v = !done;
                    put_bool(&mut self.locals[*dst as usize], v);
                }
                RegOp::MarkSeen { key } => {
                    let k = seen_key(program, self.label, &code.names[*key]);
                    self.seen.insert(k);
                }
                RegOp::MarkOnce { key } => {
                    self.once_done
                        .insert((self.label as u32, code.names[*key].clone()));
                }
                RegOp::ChoiceBegin => self.choice = ChoiceBuild::default(),
                RegOp::ChoiceItem { text, id, orig } => {
                    let text =
                        own(regs, *text, vars).unwrap_or_else(|| regs[*text as usize].clone());
                    let id = own(regs, *id, vars).unwrap_or_else(|| regs[*id as usize].clone());
                    self.choice.texts.push(text);
                    self.choice.ids.push(id);
                    self.choice.orig.push(*orig);
                }
                RegOp::JumpTable { src, table } => {
                    let n = int(regs, *src);
                    match usize::try_from(n)
                        .ok()
                        .and_then(|n| code.tables[*table].get(n))
                    {
                        Some(t) => self.pc = *t as usize,
                        None => fail!(VmErrorKind::NoChoiceAvailable),
                    }
                }
                _ => match self.reg_flow(program, op) {
                    Ok(Step::Next) => {
                        code = self.reg_code(program);
                        vars = code.vars as usize;
                    }
                    Ok(Step::Yield(inner)) => return Ok(inner),
                    Err(kind) => fail!(kind),
                },
            }
        }
    }

    /// Enters a `fn` (including lambdas). The arguments are the `n` registers from `start` (moved from the caller's
    /// temporaries), and `captures` are the captured values placed after the arguments.
    fn enter_fn(
        &mut self,
        program: &Program,
        f: u32,
        start: Reg,
        n: u16,
        captures: &[Value],
    ) -> Result<(), VmErrorKind> {
        // The call depth counts label `call`s and `fn`s together
        if self.frames.len() + self.fn_frames.len() >= self.cfg.max_call_depth as usize {
            return Err(VmErrorKind::CallDepthExceeded);
        }
        let fc = &program.fns[f as usize];
        let regs = fc.reg.regs as usize;
        // Reused arrays had everything other than numbers, booleans and `()` cleared on return, so overwriting does not run drops
        let mut locals = self.spare_locals.pop().unwrap_or_default();
        if locals.len() > regs {
            locals.truncate(regs);
        } else {
            locals.resize_with(regs, || Value::Unit);
        }
        let at = start as usize;
        let n = n as usize;
        let declared = fc.locals.len();
        let (vars, temps) = locals.split_at_mut(declared);
        for slot in temps {
            put(slot, Value::Unit);
        }
        let (args, rest) = vars.split_at_mut(n);
        for (slot, arg) in args.iter_mut().zip(&mut self.locals[at..at + n]) {
            put(slot, take_value(arg));
        }
        for (i, slot) in rest.iter_mut().enumerate() {
            let v = match captures.get(i) {
                Some(c) => c.clone(),
                None => fc.locals[n + i].ty.default_in(&program.types),
            };
            put(slot, v);
        }
        let grow = Self::locals_bytes(&locals[..declared]);
        self.fn_frames.push(FnFrame {
            caller: self.cur_fn,
            ret_pc: self.pc,
            locals: std::mem::replace(&mut self.locals, locals),
        });
        self.cur_fn = Some(f as usize);
        self.pc = 0;
        self.account(0, grow)
    }

    /// Returns from a `fn` (the return value goes to the `dst` of the call instruction).
    fn return_fn(&mut self, program: &Program, src: Option<Reg>) {
        let n = program.fns[self.cur_fn.expect("fn の中のはず")]
            .locals
            .len();
        let gone = Self::locals_bytes(&self.locals[..n]);
        let v = src.map(|r| take_value(&mut self.locals[r as usize]));
        let fr = self
            .fn_frames
            .pop()
            .expect("fn の中のはず（コンパイラの不具合）");
        let mut done = std::mem::replace(&mut self.locals, fr.locals);
        if self.spare_locals.len() < SPARE_LOCALS {
            for v in &mut done {
                if !is_scalar(v) {
                    *v = Value::Unit;
                }
            }
            self.spare_locals.push(done);
        }
        self.cur_fn = fr.caller;
        self.pc = fr.ret_pc;
        if let Some(v) = v {
            let dst = match &self.reg_code(program).ops[self.pc - 1] {
                RegOp::CallFn { dst, .. } | RegOp::CallValue { dst, .. } => *dst,
                other => unreachable!("fn の呼び出しのはず: {other:?}"),
            };
            put(&mut self.locals[dst as usize], v);
        }
        // It only decreases, so the limit is not checked
        let _ = self.account(gone, 0);
    }

    /// One step of `for i, x in xs`. If there is an element, puts it into `var` and `key` and returns `true`.
    /// Not inlined, to keep the execution loop small (avoiding slowing down other instructions).
    #[inline(never)]
    fn for_next_at(
        &mut self,
        list: Reg,
        idx: Reg,
        var: Reg,
        key: Reg,
    ) -> Result<bool, VmErrorKind> {
        let regs = &mut self.locals;
        let i = int(regs, idx);
        let item = match &regs[list as usize] {
            Value::List(items) => usize::try_from(i).ok().and_then(|i| items.get(i)),
            other => unreachable!("反復の対象は list のはず: {other:?}"),
        };
        let Some(v) = item.cloned() else {
            return Ok(false);
        };
        let grow = v.approx_bytes();
        let shrink = regs[var as usize].approx_bytes();
        regs[var as usize] = v;
        put_int(&mut regs[key as usize], i);
        put_int(&mut regs[idx as usize], i + 1);
        self.account(shrink, grow)?;
        Ok(true)
    }

    /// One step of `for k, v in m`. If there is a pair, puts it into `key` and `var` and returns `true`. Not inlined for the
    /// same reason as `for_next_at`.
    #[inline(never)]
    fn for_map_next(
        &mut self,
        map: Reg,
        idx: Reg,
        key: Reg,
        var: Reg,
    ) -> Result<bool, VmErrorKind> {
        let regs = &mut self.locals;
        let i = int(regs, idx);
        let pair = match &regs[map as usize] {
            Value::Map(m) => usize::try_from(i).ok().and_then(|i| m.entry_at(i)),
            other => unreachable!("反復の対象は map のはず: {other:?}"),
        };
        let Some((k, v)) = pair.cloned() else {
            return Ok(false);
        };
        let grow = k.approx_bytes() + v.approx_bytes();
        let shrink = regs[key as usize].approx_bytes() + regs[var as usize].approx_bytes();
        regs[key as usize] = k;
        regs[var as usize] = v;
        put_int(&mut regs[idx as usize], i + 1);
        self.account(shrink, grow)?;
        Ok(true)
    }

    /// Puts into a local variable and counts the size.
    fn set_local(&mut self, dst: Reg, v: Value) -> Result<(), VmErrorKind> {
        let grow = v.approx_bytes();
        let shrink = self.locals[dst as usize].approx_bytes();
        self.locals[dst as usize] = v;
        self.account(shrink, grow)
    }

    /// Instructions for calls, returns, stop points and the end (the code being run may change).
    fn reg_flow(&mut self, program: &Program, op: &RegOp) -> Result<Step, VmErrorKind> {
        match op {
            RegOp::JumpLabel { label } => {
                let shrink = Self::locals_bytes(self.label_locals());
                self.label = *label as usize;
                self.pc = 0;
                self.locals = default_locals(program, &program.labels[self.label]);
                self.visited.insert(*label);
                self.account(shrink, Self::locals_bytes(self.label_locals()))?;
            }
            RegOp::Call {
                start, n, label, ..
            } => {
                if self.frames.len() >= self.cfg.max_call_depth as usize {
                    return Err(VmErrorKind::CallDepthExceeded);
                }
                let target = *label as usize;
                let code = &program.labels[target];
                // Move the arguments as they are from the caller's temporaries (allocating the array only once)
                let mut locals: Vec<Value> = Vec::with_capacity(frame_len(code));
                let at = *start as usize;
                for (i, l) in code.locals.iter().enumerate() {
                    locals.push(if i < *n as usize {
                        take_value(&mut self.locals[at + i])
                    } else {
                        l.ty.default_in(&program.types)
                    });
                }
                let grow = Self::locals_bytes(&locals);
                locals.resize(frame_len(code), Value::Unit);
                self.frames.push(Frame {
                    label: self.label,
                    ret_pc: self.pc,
                    locals: std::mem::replace(&mut self.locals, locals),
                });
                self.label = target;
                self.pc = 0;
                self.visited.insert(*label);
                self.account(0, grow)?;
            }
            RegOp::CallFn { start, n, f, .. } => self.enter_fn(program, *f, *start, *n, &[])?,
            RegOp::CallValue {
                callee, start, n, ..
            } => {
                let Value::Fn(fv) = take_value(&mut self.locals[*callee as usize]) else {
                    unreachable!("関数の値のはず（コンパイラの不具合）");
                };
                self.enter_fn(program, fv.f, *start, *n, &fv.captures)?;
            }
            RegOp::MakeFn { dst, start, n, f } => {
                let at = *start as usize;
                let captures: Box<[Value]> = self.locals[at..at + *n as usize]
                    .iter_mut()
                    .map(take_value)
                    .collect();
                let v = Value::Fn(std::sync::Arc::new(crate::value::FnValue {
                    f: *f,
                    captures,
                }));
                self.put_checked(*dst, v)?;
            }
            RegOp::ReturnFn { src } => self.return_fn(program, *src),
            RegOp::Return { src } => {
                let gone = Self::locals_bytes(self.label_locals());
                let v = src.map(|r| take_value(&mut self.locals[r as usize]));
                match self.frames.pop() {
                    // No caller: same as `end`
                    None => {
                        self.state = State::Ended;
                        return Ok(Step::Yield(Inner::Yield(Yield::End)));
                    }
                    Some(f) => {
                        self.label = f.label;
                        self.pc = f.ret_pc;
                        self.locals = f.locals;
                        if let Some(v) = v {
                            let dst = match &self.reg_code(program).ops[self.pc - 1] {
                                RegOp::Call { dst, .. } => *dst,
                                other => unreachable!("ラベルの呼び出しのはず: {other:?}"),
                            };
                            self.locals[dst as usize] = v;
                        }
                        // It only decreases, so the limit is not checked
                        let _ = self.account(gone, 0);
                    }
                }
            }
            RegOp::Stop {
                start,
                n,
                cmd,
                stop,
                ..
            } => {
                let args = self.take_regs(*start, *n);
                let id = program.labels[self.label].stops[*stop as usize].id.clone();
                let command = self.cmd_ids[*cmd as usize];
                // Stop commands with validation (such as `flow.wait`) check their arguments before stopping
                if let Some(check) = self.registry.stop_check(command)
                    && let Err(e) = check.run(&args)
                {
                    return Err(e.into_vm_kind());
                }
                let y = Yield::Stop { command, args, id };
                self.state = State::Stopped { ordinal: *stop };
                return Ok(Step::Yield(Inner::Yield(self.arrive(y, *stop))));
            }
            RegOp::ChoiceStop { cmd, stop, .. } => {
                if self.choice.orig.is_empty() {
                    return Err(VmErrorKind::NoChoiceAvailable);
                }
                let y = self.choice_yield(*cmd);
                self.state = State::Stopped { ordinal: *stop };
                return Ok(Step::Yield(Inner::Yield(self.arrive(y, *stop))));
            }
            RegOp::ReplDone { src } => {
                // The end of REPL input. The value of an expression is put on `stack` (the REPL takes it)
                if let Some(r) = src {
                    let v = self.locals[*r as usize].clone();
                    self.stack.push(v);
                }
                self.state = State::Ended;
                return Ok(Step::Yield(Inner::Yield(Yield::End)));
            }
            RegOp::End => {
                // Even if something is on the call stack, finish without returning
                self.frames.clear();
                self.stack.clear();
                self.state = State::Ended;
                return Ok(Step::Yield(Inner::Yield(Yield::End)));
            }
            other => unreachable!("流れの命令ではない: {other:?}"),
        }
        Ok(Step::Next)
    }

    /// The local variables of the current label's frame (excluding temporaries).
    fn label_locals(&self) -> &[Value] {
        let n = self.program.labels[self.label].locals.len();
        &self.locals[..n.min(self.locals.len())]
    }
}
