//! Compilation: function values (references to `fn` and lambdas), their calls, and higher-order methods of `list`.

use super::*;
use crate::syntax::ast::Lambda;
use crate::value::FnValue;
use std::sync::Arc;

/// The name of a lambda's `fn` (appears as the location of runtime errors).
const LAMBDA_NAME: &str = "<lambda>";

/// Higher-order methods of `list`. The body is a hidden `fn` made by the compiler (one regardless of the element type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Hof {
    Map,
    Filter,
    Any,
    All,
    Find,
    Count,
    Fold,
    SortBy,
    MinBy,
    MaxBy,
}

impl Hof {
    pub(super) const NAMES: &'static [&'static str] = &[
        "map", "filter", "any", "all", "find", "count", "fold", "sort_by", "min_by", "max_by",
    ];

    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "map" => Hof::Map,
            "filter" => Hof::Filter,
            "any" => Hof::Any,
            "all" => Hof::All,
            "find" => Hof::Find,
            "count" => Hof::Count,
            "fold" => Hof::Fold,
            "sort_by" => Hof::SortBy,
            "min_by" => Hof::MinBy,
            "max_by" => Hof::MaxBy,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

/// The lambda literal of a function argument that can be written in the loop of the higher-order method: it has `count`
/// parameters and none is annotated (an annotation is checked against the expected type by the general path).
fn inline_lambda(e: &Expr, count: usize) -> Option<&Lambda> {
    match e {
        Expr::Lambda(l) if l.params.len() == count && l.params.iter().all(|p| p.ty.is_none()) => {
            Some(l)
        }
        _ => None,
    }
}

/// Whether values of the type can be ordered (a key of `sort_by`, `min_by` and `max_by`; an element of `sort`).
pub(super) fn is_orderable(ty: &Ty) -> bool {
    matches!(ty, Ty::Int | Ty::Float | Ty::Str | Ty::Bool)
}

/// A record for checking whether local variables of function types are used across stop points (for one label). `seq` is a
/// sequential number in the order written, given to reads, stop points, and entering and leaving loops.
#[derive(Debug, Default)]
pub(super) struct FnLive {
    seq: u32,
    /// Where a local variable of a function type was read (slot, number, block).
    uses: Vec<(u32, u32, u32)>,
    /// Where a local variable of a function type was assigned a whole value (slot, number, block).
    defs: Vec<(u32, u32, u32)>,
    /// Block parents (`blocks[b]` is the parent of block `b + 1`; 0 is outside the label).
    blocks: Vec<u32>,
    /// The current block.
    block: u32,
    stops: Vec<FnStop>,
    /// Loops (start number, end number, variables of function types in scope when entered).
    loops: Vec<(u32, u32, Vec<u32>)>,
    /// The loops currently entered (indices into `loops`).
    open: Vec<usize>,
    /// The slot and type name of the hidden `list` of each `for` currently entered that iterates a `list` containing function
    /// types.
    pub(super) hidden: Vec<(u32, String)>,
}

/// One stop point or `call`.
#[derive(Debug)]
struct FnStop {
    seq: u32,
    pos: Pos,
    what: &'static str,
    /// The variables of function types in scope at that point (slot, type name).
    cands: Vec<(u32, String)>,
    /// The loops it was in (indices into `FnLive::loops`).
    loops: Vec<usize>,
}

/// Collects the outer names that appear in the body of a lambda (names not bound by parameters; in the order written, no
/// duplicates).
fn free_names(e: &Expr, bound: &mut Vec<String>, out: &mut Vec<String>) {
    let mut add = |n: &str, bound: &Vec<String>| {
        if !bound.iter().any(|b| b == n) && !out.iter().any(|o| o == n) {
            out.push(n.to_string());
        }
    };
    match e {
        Expr::Name(n, _) | Expr::Qualified(n, _, _) => add(n, bound),
        Expr::Call(n, args, _) | Expr::QueryCall(n, _, args, _) => {
            add(n, bound);
            args.iter().for_each(|a| free_names(a, bound, out));
        }
        Expr::List(xs, _) => xs.iter().for_each(|x| free_names(x, bound, out)),
        Expr::Map(kvs, _) => kvs.iter().for_each(|(k, v)| {
            free_names(k, bound, out);
            free_names(v, bound, out);
        }),
        Expr::Str(parts, _) => parts.iter().for_each(|p| {
            if let StrPart::Interp(x) = p {
                free_names(x, bound, out);
            }
        }),
        Expr::Some(x, _) | Expr::Unary(_, x, _) | Expr::Named(_, x, _) | Expr::Field(x, _, _) => {
            free_names(x, bound, out)
        }
        Expr::Index(a, b, _) | Expr::Binary(_, a, b, _) => {
            free_names(a, bound, out);
            free_names(b, bound, out);
        }
        Expr::Method(x, _, args, _) => {
            free_names(x, bound, out);
            args.iter().for_each(|a| free_names(a, bound, out));
        }
        Expr::StructLit(_, inits, _) => inits.iter().for_each(|i| free_names(&i.value, bound, out)),
        Expr::LabelCall(c) => c.args.iter().for_each(|a| free_names(a, bound, out)),
        Expr::Command(c) | Expr::MisplacedCommand(c) => {
            c.args.iter().for_each(|a| free_names(a, bound, out))
        }
        Expr::Lambda(l) => {
            let n = bound.len();
            bound.extend(l.params.iter().map(|p| p.name.clone()));
            free_names(&l.body, bound, out);
            bound.truncate(n);
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::None(_) | Expr::Default(..) => {}
    }
}

impl Compiler<'_> {
    /// Records that a local variable of a function type was read (or, if `def`, assigned a whole value) (used for the liveness
    /// check).
    pub(super) fn fn_access(&mut self, slot: u32, def: bool) {
        if let Some(c) = &mut self.cur {
            let l = &mut c.live;
            l.seq += 1;
            let at = (slot, l.seq, l.block);
            if def {
                l.defs.push(at);
            } else {
                l.uses.push(at);
            }
        }
    }

    pub(super) fn fn_use(&mut self, slot: u32) {
        self.fn_access(slot, false);
    }

    /// Enters a block (the return value, the outer block, is passed when leaving).
    pub(super) fn fn_block_enter(&mut self) -> u32 {
        let Some(c) = &mut self.cur else {
            return 0;
        };
        let l = &mut c.live;
        let outer = l.block;
        l.blocks.push(outer);
        l.block = l.blocks.len() as u32;
        outer
    }

    pub(super) fn fn_block_exit(&mut self, outer: u32) {
        if let Some(c) = &mut self.cur {
            c.live.block = outer;
        }
    }

    /// The slots and type names of local variables of function types now in scope (including hidden `list`s of `for`).
    fn fn_locals_in_scope(&self) -> Vec<(u32, String)> {
        let Some(c) = &self.cur else {
            return Vec::new();
        };
        let mut out: Vec<(u32, String)> = c
            .scopes
            .iter()
            .flat_map(|sc| sc.values())
            .filter_map(|(slot, t)| {
                t.as_ref()
                    .filter(|t| t.contains_fn())
                    .map(|t| (*slot, t.name()))
            })
            .chain(c.live.hidden.iter().cloned())
            .collect();
        out.sort();
        out
    }

    /// Records a stop point or `call` (`what`). Local variables go into saves, but function values cannot, so if variables
    /// of function types in scope at that point are used later, it becomes an error when the body has been read
    /// ([`Compiler::fn_live_check`]).
    pub(super) fn fn_stop(&mut self, pos: Pos, what: &'static str) {
        let cands = self.fn_locals_in_scope();
        if cands.is_empty() {
            return;
        }
        let c = self.ctx();
        let l = &mut c.live;
        l.seq += 1;
        l.stops.push(FnStop {
            seq: l.seq,
            pos,
            what,
            cands,
            loops: l.open.clone(),
        });
    }

    /// Enters a loop (`while` including its condition; `for` its body).
    pub(super) fn fn_loop_enter(&mut self) {
        let in_scope: Vec<u32> = self
            .fn_locals_in_scope()
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        if let Some(c) = &mut self.cur {
            let l = &mut c.live;
            l.seq += 1;
            l.open.push(l.loops.len());
            l.loops.push((l.seq, u32::MAX, in_scope));
        }
    }

    /// Leaves a loop.
    pub(super) fn fn_loop_exit(&mut self) {
        if let Some(c) = &mut self.cur {
            let l = &mut c.live;
            l.seq += 1;
            let k = l.open.pop().expect("ループの中");
            l.loops[k].1 = l.seq;
        }
    }

    /// Makes errors of local variables of function types that are used after a stop point or `call` (read later, or, inside a
    /// loop containing a stop point, a variable declared outside the loop is read).
    pub(super) fn fn_live_check(&mut self) {
        let Some(c) = &mut self.cur else {
            return;
        };
        let live = std::mem::take(&mut c.live);
        // Whether block `a` is block `b` (or outside it)
        let encloses = |a: u32, mut b: u32| loop {
            if a == b {
                return true;
            }
            if b == 0 {
                return false;
            }
            b = live.blocks[b as usize - 1];
        };
        // Whether, after `from` and before `u`, there is a whole-value assignment in a block enclosing `u`'s block (`u` cannot be
        // reached without going through it)
        let redefined = |slot: u32, from: u32, u: u32, ub: u32| {
            live.defs
                .iter()
                .any(|&(s, d, db)| s == slot && from < d && d < u && encloses(db, ub))
        };
        // Whether it is read without being reassigned after `from` up to and including `to`
        let used_in = |slot: u32, from: u32, to: u32| {
            live.uses.iter().any(|&(s, u, ub)| {
                s == slot && from < u && u <= to && !redefined(slot, from, u, ub)
            })
        };
        for st in &live.stops {
            let found = st.cands.iter().find(|(slot, _)| {
                used_in(*slot, st.seq, u32::MAX)
                    || st.loops.iter().any(|&k| {
                        let (from, to, outer) = &live.loops[k];
                        outer.contains(slot) && used_in(*slot, *from, *to)
                    })
            });
            if let Some((_, ty)) = found {
                self.error(
                    st.pos,
                    DiagCode::FnNotSaveable,
                    vec![("type", s(ty)), ("what", s(st.what))],
                );
            }
        }
    }

    /// Turns the name of a `fn` into a function value (without captures). A `fn` without a return value cannot be a value.
    pub(super) fn fn_ref(&mut self, flat: u32, written: &str, pos: Pos) -> Typed {
        if self.impure_use(pos, "fn") {
            return self.failed();
        }
        let sig = self.fn_sigs[flat as usize].clone();
        let ret = match sig.ret {
            RetTy::Ty(t) => t,
            RetTy::None => {
                self.name_error(pos, DiagCode::CallWithoutValue, written);
                return self.failed();
            }
            RetTy::Bad => return self.failed(),
        };
        let v = Value::Fn(Arc::new(FnValue {
            f: flat,
            captures: Box::new([]),
        }));
        let id = self.node(ExprKind::Const(v), pos.line);
        let params: Option<Vec<Ty>> = sig.params.into_iter().collect();
        (params.map(|p| Ty::Fn(p, Box::new(ret))), id)
    }

    /// Calls a local variable of a function type with `name(args, ...)`. Named arguments and defaults cannot be used.
    pub(super) fn call_value(
        &mut self,
        callee: Typed,
        written: &str,
        args: &[Expr],
        pos: Pos,
    ) -> Typed {
        let unnamed = self.reject_named(written, args);
        let args = unnamed.as_slice();
        let (Some(Ty::Fn(params, ret)), callee) = callee else {
            self.discard_exprs(args);
            return self.failed();
        };
        if params.len() != args.len() {
            self.discard_exprs(args);
            self.arg_count_error(written, params.len(), args.len(), pos);
            return (Some(*ret), self.error_node());
        }
        let mut ids = Vec::new();
        for (a, w) in args.iter().zip(&params) {
            let (got, id) = self.expr_ir(a, Some(w));
            self.expect_ty(a, got, w.clone());
            ids.push(id);
        }
        let id = self.node(ExprKind::CallValue { callee, args: ids }, pos.line);
        (Some(*ret), id)
    }

    /// A lambda. Parameter types are decided by annotations or by the expected function type (`hint_params`). The body becomes
    /// a hidden `fn`. Outer local variables used by the body (and REPL variables) are captured with their values at creation
    /// (by value).
    pub(super) fn lambda_expr(
        &mut self,
        l: &Lambda,
        hint_params: Option<&[Ty]>,
        hint_ret: Option<&Ty>,
    ) -> Typed {
        let count_ok = hint_params.is_none_or(|h| h.len() == l.params.len());
        if let Some(h) = hint_params
            && !count_ok
        {
            self.arg_count_error("|…|", h.len(), l.params.len(), l.pos);
        }
        let mut ptys: Vec<Option<Ty>> = Vec::new();
        for (i, p) in l.params.iter().enumerate() {
            let hinted = hint_params.and_then(|h| h.get(i));
            let t = match (&p.ty, hinted) {
                (Some(tn), _) => {
                    let t = self.ty_from_name(tn);
                    if let (Some(t), Some(h)) = (&t, hinted)
                        && t != h
                    {
                        self.type_mismatch(p.pos, &h.name(), &t.name());
                    }
                    t
                }
                (None, Some(h)) => Some(h.clone()),
                (None, None) => {
                    self.error(p.pos, DiagCode::CannotInferType, vec![]);
                    None
                }
            };
            ptys.push(t);
        }
        let caps = self.captures_of(l);
        let f = self.fn_total + self.lambdas.len() as u32;
        self.lambdas.push(None);
        let outer = self.cur.replace(LabelCtx {
            ir: Ir::default(),
            locals: Vec::new(),
            scopes: vec![HashMap::new()],
            ids: HashSet::new(),
            refs: LabelRefs::default(),
            loop_depth: 0,
            name: LAMBDA_NAME.to_string(),
            ret: RetTy::None,
            in_fn: true,
            live: Default::default(),
            lets: Default::default(),
        });
        let named = l
            .params
            .iter()
            .map(|p| (p.name.as_str(), Some(p.pos)))
            .zip(ptys.iter().cloned())
            .chain(caps.iter().map(|(n, _, t)| ((n.as_str(), None), t.clone())));
        for ((name, pos), ty) in named {
            if let Some(pos) = pos {
                if self.ctx().scopes[0].contains_key(name) {
                    self.name_error(pos, DiagCode::DuplicateName, name);
                }
                self.warn_if_builtin(name, pos);
            }
            let c = self.ctx();
            let slot = c.locals.len() as u32;
            c.locals.push(LocalInfo {
                name: name.to_string(),
                ty: ty.clone().unwrap_or(Ty::Int),
            });
            c.scopes[0].insert(name.to_string(), (slot, ty));
        }
        let (mut bt, body) = self.expr_ir(&l.body, hint_ret);
        if let Some(want) = hint_ret {
            self.expect_ty(&l.body, bt.clone(), want.clone());
            // Type errors in the body were reported here (do not also report a type mismatch of the whole expression)
            if bt.is_some() {
                bt = Some(want.clone());
            }
        }
        let ctx = std::mem::replace(&mut self.cur, outer).expect("ラムダ式の途中");
        let code = self.lambda_code(
            ctx,
            body,
            l.body.pos(),
            l.params.len() as u32,
            caps.len() as u32,
            bt.clone(),
        );
        self.lambdas[(f - self.fn_total) as usize] = Some(code);
        let line = l.pos.line;
        let id = if caps.is_empty() {
            let v = Value::Fn(Arc::new(FnValue {
                f,
                captures: Box::new([]),
            }));
            self.node(ExprKind::Const(v), line)
        } else {
            let captures = caps
                .iter()
                .map(|(_, place, _)| self.node(ExprKind::Var(*place), line))
                .collect();
            self.node(ExprKind::MakeFn { f, captures }, line)
        };
        let ty = match (ptys.into_iter().collect::<Option<Vec<_>>>(), bt) {
            (Some(ps), Some(r)) if count_ok => Some(Ty::Fn(ps, Box::new(r))),
            _ => None,
        };
        (ty, id)
    }

    /// The names a lambda captures (name, source, type). Only those not visible from a `fn` body (outer local variables and
    /// REPL variables). Module variables are read with their values at the time of the call, the same as for `fn`.
    fn captures_of(&mut self, l: &Lambda) -> Vec<(String, PlaceRef, Option<Ty>)> {
        let mut bound: Vec<String> = l.params.iter().map(|p| p.name.clone()).collect();
        let mut free = Vec::new();
        free_names(&l.body, &mut bound, &mut free);
        let caps: Vec<_> = free
            .into_iter()
            .filter_map(|n| match self.lookup(&n)? {
                (true, slot, ty) => Some((n, PlaceRef::Local(slot), ty)),
                (false, _, _) if self.md().global_map.contains_key(&n) => None,
                (false, slot, ty) => Some((n, PlaceRef::Global(slot), ty)),
            })
            .collect();
        for (_, place, ty) in &caps {
            if let PlaceRef::Local(slot) = place {
                self.mark_read(*slot);
            }
            if let (PlaceRef::Local(slot), Some(t)) = (place, ty)
                && t.contains_fn()
            {
                self.fn_use(*slot);
            }
        }
        caps
    }

    /// A parameter that receives a function value. For a lambda, the parameter types are decided from `params` (if `ret` is
    /// given, the type of the body is also checked). If the type is not `fn(params) -> ret`, emits a diagnostic. Returns the
    /// return type (`None` if it cannot be decided) and the expression.
    fn fn_arg(&mut self, e: &Expr, params: &[Ty], ret: Option<&Ty>) -> (Option<Ty>, ExprId) {
        let (got, id) = match e {
            Expr::Lambda(l) => self.lambda_expr(l, Some(params), ret),
            _ => {
                let hint = ret.map(|r| Ty::Fn(params.to_vec(), Box::new(r.clone())));
                self.expr_ir(e, hint.as_ref())
            }
        };
        let Some(got) = got else {
            return (None, id);
        };
        let got_ret = match &got {
            Ty::Fn(ps, r) if ps.as_slice() == params && ret.is_none_or(|w| **r == *w) => {
                return (Some((**r).clone()), id);
            }
            Ty::Fn(_, r) => (**r).clone(),
            _ => ret.cloned().unwrap_or(Ty::Unit),
        };
        let want = Ty::Fn(params.to_vec(), Box::new(ret.cloned().unwrap_or(got_ret)));
        self.type_mismatch(e.pos(), &want.name(), &got.name());
        (None, id)
    }

    /// The error for a type that a `list` method cannot order or add up.
    pub(super) fn unsupported_element(&mut self, method: &str, ty: &Ty, pos: Pos) {
        self.error(
            pos,
            DiagCode::UnsupportedElementType,
            vec![("method", s(method)), ("type", s(&ty.name()))],
        );
    }

    /// Higher-order methods of `list` such as `xs.map(f)`. `base` is the evaluated `list` expression.
    pub(super) fn list_hof(
        &mut self,
        hof: Hof,
        (base, elem): (ExprId, &Ty),
        args: &[Expr],
        pos: Pos,
    ) -> Typed {
        let want = if hof == Hof::Fold { 2 } else { 1 };
        if args.len() != want {
            self.discard_exprs(args);
            self.arg_count_error(hof.name(), want, args.len(), pos);
            return self.failed();
        }
        let elem = elem.clone();
        let mut ids = vec![base];
        let mut inlined = None;
        let ty = match hof {
            Hof::Map | Hof::SortBy | Hof::MinBy | Hof::MaxBy => {
                let (r, f) = match inline_lambda(&args[0], 1) {
                    Some(l) => {
                        let (r, id) =
                            self.inline_hof(hof, l, ids.clone(), std::slice::from_ref(&elem), None);
                        inlined = Some(id);
                        (r, id)
                    }
                    None => self.fn_arg(&args[0], std::slice::from_ref(&elem), None),
                };
                if inlined.is_none() {
                    ids.push(f);
                }
                match r {
                    Some(r) if hof == Hof::Map => Some(Ty::List(Box::new(r))),
                    Some(k) if hof != Hof::Map && !is_orderable(&k) => {
                        self.unsupported_element(hof.name(), &k, pos);
                        None
                    }
                    Some(_) if hof == Hof::SortBy => Some(Ty::List(Box::new(elem))),
                    Some(_) if hof != Hof::Map => Some(Ty::Option(Box::new(elem))),
                    _ => None,
                }
            }
            Hof::Filter | Hof::Any | Hof::All | Hof::Find | Hof::Count => {
                let hint = std::slice::from_ref(&elem);
                match inline_lambda(&args[0], 1) {
                    Some(l) => {
                        let (_, id) = self.inline_hof(hof, l, ids.clone(), hint, Some(&Ty::Bool));
                        inlined = Some(id);
                    }
                    None => {
                        let (_, f) = self.fn_arg(&args[0], hint, Some(&Ty::Bool));
                        ids.push(f);
                    }
                }
                Some(match hof {
                    Hof::Filter => Ty::List(Box::new(elem)),
                    Hof::Find => Ty::Option(Box::new(elem)),
                    Hof::Count => Ty::Int,
                    _ => Ty::Bool,
                })
            }
            Hof::Fold => {
                let (acc, init) = self.expr_ir(&args[0], None);
                ids.push(init);
                match acc {
                    Some(acc) => {
                        let hints = [acc.clone(), elem];
                        match inline_lambda(&args[1], 2) {
                            Some(l) => {
                                let (_, id) =
                                    self.inline_hof(hof, l, ids.clone(), &hints, Some(&acc));
                                inlined = Some(id);
                            }
                            None => {
                                let (_, f) = self.fn_arg(&args[1], &hints, Some(&acc));
                                ids.push(f);
                            }
                        }
                        Some(acc)
                    }
                    None => {
                        self.discard_exprs(&args[1..]);
                        None
                    }
                }
            }
        };
        if let Some(id) = inlined {
            return (ty, id);
        }
        let f = self.hof_fn(hof);
        let id = self.node(ExprKind::CallFn { f, args: ids }, pos.line);
        (ty, id)
    }

    /// The index into `Program::fns` of the `fn` that is the body of a higher-order method (one per program; made on first
    /// use).
    fn hof_fn(&mut self, hof: Hof) -> u32 {
        if let Some(&f) = self.hofs.get(&hof) {
            return f;
        }
        let f = self.fn_total + self.lambdas.len() as u32;
        self.lambdas.push(None);
        self.hofs.insert(hof, f);
        let code = self.hof_code(hof);
        self.lambdas[(f - self.fn_total) as usize] = Some(code);
        f
    }

    /// The body of `sort_by`, `min_by` and `max_by`: computes the keys with the body of `map`, then picks with one instruction.
    fn keyed_hof_code(&mut self, hof: Hof) -> FnCode {
        let map = self.hof_fn(Hof::Map);
        let mut ir = Ir::default();
        let line = 0;
        let var = |ir: &mut Ir, slot: u32| ir.expr(ExprKind::Var(PlaceRef::Local(slot)), line);
        let (xs, f, xs2) = (var(&mut ir, 0), var(&mut ir, 1), var(&mut ir, 0));
        let keys = ir.expr(
            ExprKind::CallFn {
                f: map,
                args: vec![xs2, f],
            },
            line,
        );
        let method = match hof {
            Hof::SortBy => Method::ListSortBy,
            Hof::MinBy => Method::ListMinBy,
            _ => Method::ListMaxBy,
        };
        let value = ir.expr(
            ExprKind::Method {
                method,
                base: xs,
                args: vec![keys],
            },
            line,
        );
        let stmts = vec![ir.stmt(StmtKind::Return(Some(value)), line)];
        let locals: Vec<LocalInfo> = ["xs", "f"]
            .iter()
            .map(|n| LocalInfo {
                name: (*n).to_string(),
                ty: Ty::Unit,
            })
            .collect();
        let body = Body {
            ir,
            stmts,
            tail: Tail::Return,
            in_fn: true,
            locals: locals.len() as u32,
            line,
        };
        let code = self.lower_body(&body, Pos { line, col: 0 });
        FnCode {
            module: 0,
            name: format!("<{}>", hof.name()),
            reg: code.reg,
            lines: code.lines,
            locals,
            params: 2,
            captures: 0,
            ret: None,
        }
    }

    /// The body of a higher-order method. The parameters are `xs` and `f` (`xs`, `init` and `f` for `fold`). It does not
    /// depend on the value types, so local variables have no types (all are given values before use).
    fn hof_code(&mut self, hof: Hof) -> FnCode {
        if matches!(hof, Hof::SortBy | Hof::MinBy | Hof::MaxBy) {
            return self.keyed_hof_code(hof);
        }
        let s = HofSlots::new(hof, 1);
        let mut ir = Ir::default();
        let stmts = hof_stmts(&mut ir, hof, &s, Callee::Value);
        let mut names: Vec<&str> = vec!["xs"];
        if hof == Hof::Fold {
            names.push("acc");
        }
        names.push("f");
        if s.has_out {
            names.push("out");
        }
        names.extend(["$list", "$idx", "x"]);
        let locals: Vec<LocalInfo> = names
            .iter()
            .map(|n| LocalInfo {
                name: (*n).to_string(),
                ty: Ty::Unit,
            })
            .collect();
        let line = 0;
        let body = Body {
            ir,
            stmts,
            tail: Tail::Return,
            in_fn: true,
            locals: locals.len() as u32,
            line,
        };
        let code = self.lower_body(&body, Pos { line, col: 0 });
        FnCode {
            module: 0,
            name: format!("<{}>", hof.name()),
            reg: code.reg,
            lines: code.lines,
            locals,
            params: s.params,
            captures: 0,
            ret: None,
        }
    }

    /// A higher-order method whose function argument is the lambda literal `l`: one hidden `fn` per call site with the body
    /// of the lambda written in the loop (no function value is made and no call through one is done). The captured values
    /// are passed as extra arguments after `ids` (the arguments before the function). `hints` are the types of the lambda's
    /// parameters and `ret` the type the body must have. Returns the type of the body and the call.
    fn inline_hof(
        &mut self,
        hof: Hof,
        l: &Lambda,
        mut ids: Vec<ExprId>,
        hints: &[Ty],
        ret: Option<&Ty>,
    ) -> (Option<Ty>, ExprId) {
        let caps = self.captures_of(l);
        let s = HofSlots::new(hof, caps.len() as u32);
        let f = self.fn_total + self.lambdas.len() as u32;
        self.lambdas.push(None);
        let mut locals = vec![LocalInfo {
            name: "xs".to_string(),
            ty: Ty::Unit,
        }];
        if hof == Hof::Fold {
            locals.push(LocalInfo {
                name: "acc".to_string(),
                ty: hints[0].clone(),
            });
        }
        let mut scope: HashMap<String, (u32, Option<Ty>)> = HashMap::new();
        for (i, (name, _, ty)) in caps.iter().enumerate() {
            scope.insert(name.clone(), (s.f + i as u32, ty.clone()));
            locals.push(LocalInfo {
                name: name.clone(),
                ty: ty.clone().unwrap_or(Ty::Int),
            });
        }
        let hidden = |n: &str| LocalInfo {
            name: n.to_string(),
            ty: Ty::Unit,
        };
        if s.has_out {
            locals.push(hidden("out"));
        }
        locals.extend(["$list", "$idx"].map(hidden));
        locals.push(LocalInfo {
            name: "x".to_string(),
            ty: hints[hints.len() - 1].clone(),
        });
        // The parameters of the lambda: the accumulator (`fold`) and the element.
        let slots: Vec<u32> = if hof == Hof::Fold {
            vec![s.acc, s.x]
        } else {
            vec![s.x]
        };
        let mut seen: Vec<&str> = Vec::new();
        for ((p, slot), ty) in l.params.iter().zip(&slots).zip(hints) {
            if seen.contains(&p.name.as_str()) {
                self.name_error(p.pos, DiagCode::DuplicateName, &p.name);
            }
            seen.push(&p.name);
            self.warn_if_builtin(&p.name, p.pos);
            scope.insert(p.name.clone(), (*slot, Some(ty.clone())));
        }
        let outer = self.cur.replace(LabelCtx {
            ir: Ir::default(),
            locals,
            scopes: vec![scope],
            ids: HashSet::new(),
            refs: LabelRefs::default(),
            loop_depth: 0,
            name: LAMBDA_NAME.to_string(),
            ret: RetTy::None,
            in_fn: true,
            live: Default::default(),
            lets: Default::default(),
        });
        let (mut bt, body) = self.expr_ir(&l.body, ret);
        if let Some(want) = ret {
            self.expect_ty(&l.body, bt.clone(), want.clone());
            if bt.is_some() {
                bt = Some(want.clone());
            }
        }
        let mut ctx = std::mem::replace(&mut self.cur, outer).expect("ラムダ式の途中");
        let line = l.pos.line;
        let stmts = hof_stmts(&mut ctx.ir, hof, &s, Callee::Inline(body));
        let nlocals = ctx.locals.len() as u32;
        let code = Body {
            ir: std::mem::take(&mut ctx.ir),
            stmts,
            tail: Tail::Return,
            in_fn: true,
            locals: nlocals,
            line,
        };
        let pos = l.body.pos();
        let lowered = self.lower_body(&code, pos);
        self.lambdas[(f - self.fn_total) as usize] = Some(FnCode {
            module: self.cur_mod as u32,
            name: ctx.name,
            reg: lowered.reg,
            lines: lowered.lines,
            locals: ctx.locals,
            params: s.params,
            captures: 0,
            ret: None,
        });
        for (_, place, _) in &caps {
            ids.push(self.node(ExprKind::Var(*place), line));
        }
        let id = self.node(ExprKind::CallFn { f, args: ids }, line);
        (bt, id)
    }
}

/// How a higher-order method calls its function argument.
#[derive(Clone, Copy)]
enum Callee {
    /// Through the function value in the slot `f`.
    Value,
    /// The expression written in place (the body of a lambda).
    Inline(ExprId),
}

/// The local variable slots of the body of a higher-order method. Slot 0 is `xs`, followed by `acc` (`fold`), then the
/// function value or the captured values.
struct HofSlots {
    acc: u32,
    f: u32,
    out: u32,
    has_out: bool,
    list: u32,
    idx: u32,
    x: u32,
    params: u32,
}

impl HofSlots {
    fn new(hof: Hof, extra: u32) -> Self {
        let acc = 1;
        let f = if hof == Hof::Fold { 2 } else { 1 };
        let out = f + extra;
        let has_out = !matches!(hof, Hof::Any | Hof::All | Hof::Find | Hof::Fold);
        let list = out + u32::from(has_out);
        HofSlots {
            acc,
            f,
            out,
            has_out,
            list,
            idx: list + 1,
            x: list + 2,
            params: out,
        }
    }
}

/// The statements of the body of a higher-order method.
fn hof_stmts(ir: &mut Ir, hof: Hof, s: &HofSlots, callee: Callee) -> Vec<StmtId> {
    let line = 0;
    let (out, list, idx, x, f) = (s.out, s.list, s.idx, s.x, s.f);
    let var = |ir: &mut Ir, slot: u32| ir.expr(ExprKind::Var(PlaceRef::Local(slot)), line);
    let konst = |ir: &mut Ir, v: Value| ir.expr(ExprKind::Const(v), line);
    let ret = |ir: &mut Ir, v: ExprId| ir.stmt(StmtKind::Return(Some(v)), line);
    let call = |ir: &mut Ir, args: Vec<u32>| match callee {
        Callee::Inline(e) => e,
        Callee::Value => {
            let callee = var(ir, f);
            let args = args.into_iter().map(|a| var(ir, a)).collect();
            ir.expr(ExprKind::CallValue { callee, args }, line)
        }
    };
    let when = |ir: &mut Ir, cond: ExprId, body: Vec<StmtId>| {
        ir.stmt(
            StmtKind::If {
                arms: vec![IfArm { cond, body, line }],
                else_body: Vec::new(),
            },
            line,
        )
    };
    let push = |ir: &mut Ir, v: ExprId| {
        ir.stmt(
            StmtKind::Mutate {
                place: PlaceRef::Local(out),
                path: Vec::new(),
                indices: Vec::new(),
                operands: vec![v],
                action: Action::Push,
            },
            line,
        )
    };
    let mut stmts = Vec::new();
    let init = match hof {
        Hof::Count => Some(Value::Int(0)),
        _ if s.has_out => Some(Value::List(Vec::new().into())),
        _ => None,
    };
    if let Some(v) = init {
        let value = konst(ir, v);
        stmts.push(ir.stmt(
            StmtKind::Store {
                place: PlaceRef::Local(out),
                value,
            },
            line,
        ));
    }
    let body = match hof {
        Hof::Map | Hof::SortBy | Hof::MinBy | Hof::MaxBy => {
            let c = call(ir, vec![x]);
            vec![push(ir, c)]
        }
        Hof::Filter => {
            let c = call(ir, vec![x]);
            let v = var(ir, x);
            let p = push(ir, v);
            vec![when(ir, c, vec![p])]
        }
        Hof::Count => {
            let c = call(ir, vec![x]);
            let one = konst(ir, Value::Int(1));
            let add = ir.stmt(
                StmtKind::Compound {
                    place: PlaceRef::Local(out),
                    op: ArithOp::Add,
                    ty: Operand::Int,
                    value: one,
                },
                line,
            );
            vec![when(ir, c, vec![add])]
        }
        Hof::Any | Hof::All | Hof::Find => {
            let mut c = call(ir, vec![x]);
            if hof == Hof::All {
                c = ir.expr(
                    ExprKind::Unary {
                        op: UnOp::Not,
                        operand: c,
                    },
                    line,
                );
            }
            let v = match hof {
                Hof::Find => {
                    let v = var(ir, x);
                    ir.expr(ExprKind::Some(v), line)
                }
                _ => konst(ir, Value::Bool(hof == Hof::Any)),
            };
            let r = ret(ir, v);
            vec![when(ir, c, vec![r])]
        }
        Hof::Fold => {
            let value = call(ir, vec![s.acc, x]);
            vec![ir.stmt(
                StmtKind::Store {
                    place: PlaceRef::Local(s.acc),
                    value,
                },
                line,
            )]
        }
    };
    let iter = var(ir, 0);
    stmts.push(ir.stmt(
        StmtKind::For {
            iter,
            list,
            idx,
            var: x,
            key: None,
            kind: ForKind::List,
            body,
        },
        line,
    ));
    let result = match hof {
        Hof::Map | Hof::Filter | Hof::Count => var(ir, out),
        Hof::Fold => var(ir, s.acc),
        Hof::Find => konst(ir, Value::Option(None)),
        Hof::Any | Hof::All => konst(ir, Value::Bool(hof == Hof::All)),
        Hof::SortBy | Hof::MinBy | Hof::MaxBy => {
            let method = match hof {
                Hof::SortBy => Method::ListSortBy,
                Hof::MinBy => Method::ListMinBy,
                _ => Method::ListMaxBy,
            };
            let base = var(ir, 0);
            let keys = var(ir, out);
            ir.expr(
                ExprKind::Method {
                    method,
                    base,
                    args: vec![keys],
                },
                line,
            )
        }
    };
    stmts.push(ret(ir, result));
    stmts
}
