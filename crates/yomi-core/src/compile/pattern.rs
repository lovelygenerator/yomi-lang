//! Compilation: `match`, and checking exhaustiveness of patterns.

use super::*;

impl Compiler<'_> {
    /// The `match` statement. Tests the arms from top to bottom, extracts the bindings of the matching arm, and runs its body.
    /// If not exhaustive, `non_exhaustive_match`.
    pub(super) fn match_stmt(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        pos: Pos,
        out: &mut Vec<StmtId>,
    ) {
        let (sty, sid) = self.expr(scrutinee);
        let decl = match &sty {
            Some(Ty::Enum(n)) => find_type(&self.types, n).cloned(),
            _ => None,
        };
        let mut cov = Coverage::default();
        let mut ir_arms = Vec::new();
        for arm in arms {
            self.ctx().scopes.push(HashMap::new());
            if sty.is_some() && cov.complete(sty.as_ref(), decl.as_ref()) {
                self.warn(arm.pattern.pos(), DiagCode::UnreachablePattern, vec![]);
            }
            let (test, binds) =
                self.arm_pattern(&arm.pattern, sty.as_ref(), decl.as_ref(), &mut cov);
            let mut ir_binds = Vec::new();
            for (i, bind, bty) in binds {
                if let Some((n, bp)) = bind {
                    let slot = self.declare_local(&n, bp, bty);
                    ir_binds.push((i, slot, bp.line));
                }
            }
            let body = self.block(&arm.body);
            self.ctx().scopes.pop();
            ir_arms.push(IrArm {
                test,
                binds: ir_binds,
                body,
                line: arm.pos.line,
            });
        }
        self.push_stmt(
            out,
            StmtKind::Match {
                scrutinee: sid,
                arms: ir_arms,
            },
            pos.line,
        );
        if let Some(t) = &sty {
            let missing = cov.missing(t, decl.as_ref());
            if !missing.is_empty() {
                self.error(
                    pos,
                    DiagCode::NonExhaustiveMatch,
                    vec![("missing", DiagArg::List(missing))],
                );
            }
        }
    }

    /// Checks the pattern of an arm. Returns the condition to enter the arm (`None` for `_`) and its line, and the bindings
    /// (content number, name, type).
    pub(super) fn arm_pattern(
        &mut self,
        pat: &Pattern,
        sty: Option<&Ty>,
        decl: Option<&TypeDecl>,
        cov: &mut Coverage,
    ) -> (Option<(PatTest, u32)>, Vec<ArmBinding>) {
        let mismatch = |this: &mut Self, found: &str| {
            if let Some(t) = sty {
                this.error(
                    pat.pos(),
                    DiagCode::TypeMismatch,
                    vec![("expected", s(&t.name())), ("found", s(found))],
                );
            }
        };
        let line = pat.pos().line;
        match pat {
            Pattern::Wild(_) => {
                cov.catch_all = true;
            }
            Pattern::Int(n, p) => {
                if sty.is_none_or(|t| *t == Ty::Int) {
                    if !cov.lits.insert(format!("i{n}")) && sty.is_some() {
                        self.warn(*p, DiagCode::UnreachablePattern, vec![]);
                    }
                    return (Some((PatTest::Lit(Value::Int(*n)), line)), Vec::new());
                } else {
                    mismatch(self, "int");
                }
            }
            Pattern::Str(t, p) => {
                if sty.is_none_or(|ty| *ty == Ty::Str) {
                    if !cov.lits.insert(format!("s{t}")) && sty.is_some() {
                        self.warn(*p, DiagCode::UnreachablePattern, vec![]);
                    }
                    let value = Value::Str(t.clone().into());
                    return (Some((PatTest::Lit(value), line)), Vec::new());
                } else {
                    mismatch(self, "str");
                }
            }
            Pattern::Bool(b, p) => {
                if sty.is_none_or(|ty| *ty == Ty::Bool) {
                    let seen = &mut cov.bools[usize::from(*b)];
                    if *seen && sty.is_some() {
                        self.warn(*p, DiagCode::UnreachablePattern, vec![]);
                    }
                    *seen = true;
                    return (Some((PatTest::Lit(Value::Bool(*b)), line)), Vec::new());
                } else {
                    mismatch(self, "bool");
                }
            }
            Pattern::None(p) => {
                if sty.is_none_or(|ty| matches!(ty, Ty::Option(_))) {
                    if cov.none && sty.is_some() {
                        self.warn(*p, DiagCode::UnreachablePattern, vec![]);
                    }
                    cov.none = true;
                    return (Some((PatTest::None, line)), Vec::new());
                } else {
                    mismatch(self, "Option<_>");
                }
            }
            Pattern::Some(bind, p) => {
                if sty.is_none_or(|ty| matches!(ty, Ty::Option(_))) {
                    if cov.some && sty.is_some() {
                        self.warn(*p, DiagCode::UnreachablePattern, vec![]);
                    }
                    cov.some = true;
                    let inner = match sty {
                        Some(Ty::Option(t)) => Some((**t).clone()),
                        _ => None,
                    };
                    return (Some((PatTest::Some, line)), vec![(0, bind.clone(), inner)]);
                }
                mismatch(self, "Option<_>");
            }
            Pattern::Variant { name, binds, pos } => {
                let Some(TypeDecl {
                    name: tname,
                    kind: TypeKind::Enum { variants },
                }) = decl
                else {
                    if sty.is_some() {
                        mismatch(self, "enum");
                    }
                    return (
                        None,
                        binds
                            .iter()
                            .enumerate()
                            .map(|(i, b)| (i as u32, b.clone(), None))
                            .collect(),
                    );
                };
                let Some(v) = variants.iter().find(|v| v.name == *name) else {
                    let found = suggest(name, variants.iter().map(|v| v.name.as_str()));
                    let mut args = vec![("name", s(name)), ("type", s(tname))];
                    if !found.is_empty() {
                        args.push(("suggestions", DiagArg::List(found)));
                    }
                    self.error(*pos, DiagCode::UnknownVariant, args);
                    return (
                        None,
                        binds
                            .iter()
                            .enumerate()
                            .map(|(i, b)| (i as u32, b.clone(), None))
                            .collect(),
                    );
                };
                if v.args.len() != binds.len() {
                    self.arg_count_error(
                        &format!("{tname}.{name}"),
                        v.args.len(),
                        binds.len(),
                        *pos,
                    );
                    return (None, Vec::new());
                }
                if !cov.variants.insert(name.clone()) {
                    self.warn(*pos, DiagCode::UnreachablePattern, vec![]);
                }
                let binds = binds
                    .iter()
                    .zip(&v.args)
                    .enumerate()
                    .map(|(i, (b, t))| (i as u32, b.clone(), Some(t.clone())))
                    .collect();
                return (Some((PatTest::Variant(name.clone()), line)), binds);
            }
        }
        (None, Vec::new())
    }
}
