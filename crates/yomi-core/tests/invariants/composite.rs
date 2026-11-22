//! Compound values: list, Option, struct, enum, match

use super::*;

/// Declarations of compound values (types and variables). Placed at the start of the script.
pub(super) const COMPOSITE_HEAD: &str = "enum Dir { N, S, Item(int), Pair(int, str) }
struct Chara { name: str, hp: int, items: list<int> }
struct Party { lead: Chara, members: list<Chara>, best: Option<int> }
save var c: Chara = Chara { name: \"a\", hp: 3, items: [] }
save var d: Dir = Dir.N
save var xs: list<int> = [1, 2]
save var o: Option<int> = None
save var names: list<str> = []
save var p: Party = Party { lead: Chara { name: \"L\", hp: 1, items: [] }, members: [], best: None }
persistent var hist: list<int> = []
";

pub(super) fn gen_cvalue_int(rng: &mut Rng) -> String {
    match rng.below(5) {
        0 => rng.range(0, 9).to_string(),
        1 => "c.hp".to_string(),
        2 => "xs.len()".to_string(),
        3 => format!("({} + c.hp)", rng.range(0, 5)),
        _ => "p.lead.hp".to_string(),
    }
}

pub(super) fn gen_cdir(rng: &mut Rng) -> String {
    match rng.below(4) {
        0 => "Dir.N".to_string(),
        1 => "Dir.S".to_string(),
        2 => format!("Dir.Item({})", gen_cvalue_int(rng)),
        _ => format!(
            "Dir.Pair({}, \"{}\")",
            gen_cvalue_int(rng),
            rng.pick(&["x", "yy", "三"])
        ),
    }
}

pub(super) fn gen_cstmts(
    rng: &mut Rng,
    out: &mut String,
    indent: usize,
    depth: usize,
    ids: &mut usize,
) {
    let n = 1 + rng.below(if depth == 0 { 8 } else { 3 });
    for _ in 0..n {
        gen_cstmt(rng, out, indent, depth, ids);
    }
}

pub(super) fn gen_cstmt(
    rng: &mut Rng,
    out: &mut String,
    indent: usize,
    depth: usize,
    ids: &mut usize,
) {
    let pad = "  ".repeat(indent);
    let mut line = |s: &str| {
        out.push_str(&pad);
        out.push_str(s);
        out.push('\n');
    };
    let max = if depth >= 2 { 14 } else { 20 };
    match rng.below(max) {
        0 => line(&format!("c.hp += {}", rng.range(0, 4))),
        1 => line(&format!("c.items.push({})", gen_cvalue_int(rng))),
        2 => {
            line("if c.items.len() > 0 {");
            line("  c.items.remove(0)");
            line("}");
        }
        3 => line(&format!("xs.push({})", gen_cvalue_int(rng))),
        4 => {
            let i = rng.below(3);
            line(&format!("if xs.len() > {i} {{"));
            line(&format!("  xs[{i}] = {}", gen_cvalue_int(rng)));
            line("}");
        }
        5 => line(&format!("o = Some({})", gen_cvalue_int(rng))),
        6 => line("o = None"),
        7 => line(&format!("d = {}", gen_cdir(rng))),
        8 => line(&format!(
            "names.push(\"{}\")",
            rng.pick(&["a", "b", "案内人"])
        )),
        9 => line("p.lead.items.push(c.hp)"),
        10 => line(&format!(
            "p.members.push(Chara {{ name: \"m\", hp: {}, items: xs }})",
            gen_cvalue_int(rng)
        )),
        11 => {
            line("if p.members.len() > 0 {");
            line(&format!("  p.members[0].hp += {}", rng.range(1, 3)));
            line("}");
        }
        12 => {
            // Copies are independent
            *ids += 1;
            let t = format!("t{ids}");
            line(&format!("let {t} = c"));
            line(&format!("{t}.hp += 100"));
            line(&format!("{t}.items.push(1)"));
            line(&format!("${{c.hp}} ${{{t}.hp}} ${{c.items}} #s{ids}"));
        }
        13 => line("hist.push(c.hp)"),
        14 => {
            *ids += 1;
            let which = rng.below(6);
            let body = match which {
                0 => "${c}",
                1 => "${d}",
                2 => "${xs} ${o}",
                3 => "${p}",
                4 => "${names} ${hist}",
                _ => "${c.hp}",
            };
            line(&format!("{body} #s{ids}"));
        }
        15 | 16 => {
            // An exhaustive match
            line("match d {");
            line("  N => {");
            *ids += 1;
            line(&format!("    北 #s{ids}"));
            line("  }");
            line("  S => {");
            line("    c.hp += 1");
            line("  }");
            line("  Item(k) => {");
            line("    xs.push(k)");
            line("  }");
            line("  Pair(k, s) => {");
            *ids += 1;
            line(&format!("    ${{k}}${{s}} #s{ids}"));
            line("  }");
            line("}");
        }
        17 => {
            line("match o {");
            line("  Some(k) => {");
            line("    c.hp += k");
            line("  }");
            line("  None => {");
            line("    c.hp += 1");
            line("  }");
            line("}");
        }
        18 => {
            line("if c.hp < 12 {");
            gen_cstmts(rng, out, indent + 1, depth + 1, ids);
            out.push_str(&pad);
            out.push_str("} else {\n");
            gen_cstmts(rng, out, indent + 1, depth + 1, ids);
            out.push_str(&pad);
            out.push_str("}\n");
        }
        _ => {
            // A while with a fixed number of iterations
            *ids += 1;
            let i = format!("i{ids}");
            line(&format!("let {i} = 0"));
            line(&format!("while {i} < {} {{", rng.range(1, 4)));
            line(&format!("  c.items.push({i})"));
            line(&format!("  xs.push({i} * 2)"));
            line(&format!("  {i} += 1"));
            line("}");
        }
    }
}

/// Makes a valid script that uses compound values.
pub(super) fn gen_composite_script(rng: &mut Rng) -> String {
    let mut src = String::from(COMPOSITE_HEAD);
    src.push_str("label start {\n");
    let mut ids = 0;
    gen_cstmts(rng, &mut src, 1, 0, &mut ids);
    ids += 1;
    src.push_str(&format!("  ${{c}} ${{xs}} ${{p}} #s{ids}\n}}\n"));
    src
}

#[test]
fn compound_value_script_runs_to_end_and_has_stop_points() {
    let reg = registry();
    let (mut with_match, mut with_push, mut stops) = (0, 0, 0);
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_composite_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let t = trace_of(&prog, &reg, 2_000);
        // Indices are guarded, so no runtime errors occur
        assert!(
            matches!(t.last(), Some(Ok(Yield::End))),
            "seed={seed}: {:?}\n{src}",
            t.last()
        );
        stops += t.len();
        with_match += src.contains("match") as usize;
        with_push += src.contains("push") as usize;
        // Deterministic
        assert_eq!(t, trace_of(&prog, &reg, 2_000), "seed={seed}");
    }
    let n = case_count();
    assert!(with_match * 4 >= n && with_push * 2 >= n && stops >= n);
}

#[test]
fn compound_value_script_continues_identically_after_save_restore() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_composite_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let what = format!("composite seed={seed}");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}

#[test]
fn compound_value_script_rollback_matches_replay() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0x77);
        let src = gen_composite_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let what = format!("composite seed={seed}");
        let r = catch_unwind(AssertUnwindSafe(|| {
            let total = trace_of(&prog, &reg, 2_000)
                .iter()
                .filter(|s| matches!(s, Ok(Yield::Stop { .. })))
                .count();
            let stride = (total / 6).max(1);
            for index in (0..total).step_by(stride) {
                let mut probe = vm_at_stop(&prog, &reg, index);
                check_rollback(&prog, &reg, &mut probe, &mut rng, index, &what);
            }
        }));
        if let Err(p) = r {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}

#[test]
fn corrupting_compound_value_saves_does_not_panic_on_load() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0xC0DE);
        let src = gen_composite_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        for bytes in some_saves(&prog, &reg, &mut rng, 2) {
            // A 1-bit flip is always caught by the checksum
            for _ in 0..4 {
                let mut broken = bytes.clone();
                let pos = rng.below(broken.len());
                broken[pos] ^= 1 << rng.below(8);
                assert!(
                    Vm::load(&prog, &reg, config(), &broken).is_err(),
                    "seed={seed} 位置 {pos}"
                );
            }
            // Break the body and recompute the checksum. When it can be read, continuing to run does not panic
            for _ in 0..24 {
                let mut body = bytes[..bytes.len() - 4].to_vec();
                let pos = 8 + rng.below(body.len() - 8);
                body[pos] = body[pos].wrapping_add(1 + rng.below(255) as u8);
                let crc = crc32(&body);
                body.extend_from_slice(&crc.to_le_bytes());
                let r = catch_unwind(AssertUnwindSafe(|| {
                    if let Ok((mut vm, _)) = Vm::load(&prog, &reg, config(), &body) {
                        let _ = vm.variables();
                        let _ = vm.save();
                        let _ = trace(&mut vm, &reg, 300);
                    }
                }));
                assert!(
                    r.is_ok(),
                    "seed={seed}: 壊れたセーブでパニック（位置 {pos}）"
                );
            }
        }
    }
}

// --- Agreement between the exhaustiveness check of match and execution ---

/// The shape of a pattern for the check.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Pat {
    /// A variable (`_`)
    Any,
    /// The number (0-based) of an enumerator, `Some`/`None`, a boolean or an integer
    Case(usize),
}

/// The type of the value to scan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Scrut {
    /// `E { A, B(int), C(int, str), D }`
    Enum,
    /// `Option<int>`: 0 is `Some`, 1 is `None`
    Opt,
    /// `bool`: 0 is `true`, 1 is `false`
    Bool,
    /// `int`: values are literals 0..=2. Exhaustiveness needs `_`
    Int,
}

impl Scrut {
    pub(super) fn cases(self) -> usize {
        match self {
            Scrut::Enum => 4,
            Scrut::Opt | Scrut::Bool => 2,
            Scrut::Int => 3,
        }
    }

    pub(super) fn pattern_text(self, case: usize) -> String {
        match (self, case) {
            (Scrut::Enum, 0) => "A".into(),
            (Scrut::Enum, 1) => "B(n)".into(),
            (Scrut::Enum, 2) => "C(n, s)".into(),
            (Scrut::Enum, _) => "D".into(),
            (Scrut::Opt, 0) => "Some(n)".into(),
            (Scrut::Opt, _) => "None".into(),
            (Scrut::Bool, 0) => "true".into(),
            (Scrut::Bool, _) => "false".into(),
            (Scrut::Int, k) => k.to_string(),
        }
    }

    /// The expression of a value and which case it is. When the case's binding has a value, how the binding is shown.
    pub(super) fn value_text(self, case: usize) -> String {
        match (self, case) {
            (Scrut::Enum, 0) => "E.A".into(),
            (Scrut::Enum, 1) => "E.B(7)".into(),
            (Scrut::Enum, 2) => "E.C(8, \"z\")".into(),
            (Scrut::Enum, _) => "E.D".into(),
            (Scrut::Opt, 0) => "Some(5)".into(),
            (Scrut::Opt, _) => "None".into(),
            (Scrut::Bool, 0) => "true".into(),
            (Scrut::Bool, _) => "false".into(),
            (Scrut::Int, k) => k.to_string(),
        }
    }

    pub(super) fn binding_text(self, case: usize) -> &'static str {
        match (self, case) {
            (Scrut::Enum, 1) => ":${n}",
            (Scrut::Enum, 2) => ":${n}${s}",
            (Scrut::Opt, 0) => ":${n}",
            _ => "",
        }
    }

    pub(super) fn binding_expect(self, case: usize) -> &'static str {
        match (self, case) {
            (Scrut::Enum, 1) => ":7",
            (Scrut::Enum, 2) => ":8z",
            (Scrut::Opt, 0) => ":5",
            _ => "",
        }
    }
}

/// A plan for one `match`.
pub(super) struct MatchPlan {
    pub(super) scrut: Scrut,
    pub(super) arms: Vec<Pat>,
    pub(super) value: usize,
}

impl MatchPlan {
    pub(super) fn generate(rng: &mut Rng) -> Self {
        let scrut = *rng.pick(&[Scrut::Enum, Scrut::Opt, Scrut::Bool, Scrut::Int]);
        let n = 1 + rng.below(6);
        let arms = (0..n)
            .map(|_| {
                if rng.chance(1, 5) {
                    Pat::Any
                } else {
                    Pat::Case(rng.below(scrut.cases()))
                }
            })
            .collect();
        MatchPlan {
            scrut,
            arms,
            value: rng.below(scrut.cases()),
        }
    }

    /// Whether it is exhaustive (has `_`, or all cases appear).
    pub(super) fn exhaustive(&self) -> bool {
        self.arms.contains(&Pat::Any)
            || (0..self.scrut.cases()).all(|c| self.arms.contains(&Pat::Case(c)))
            // Integer literals alone are not exhaustive
            && self.scrut != Scrut::Int
    }

    /// The number of unreachable arms (after `_`, or repeats of cases that already appeared).
    pub(super) fn unreachable(&self) -> usize {
        let mut seen_any = false;
        let mut seen = vec![false; self.scrut.cases()];
        let mut count = 0;
        for a in &self.arms {
            match a {
                _ if seen_any => count += 1,
                Pat::Any => seen_any = true,
                Pat::Case(c) => {
                    if seen[*c] {
                        count += 1;
                    }
                    seen[*c] = true;
                }
            }
            // Arms after all cases have appeared are unreachable (only for exhaustive enumerations)
            if !seen_any && self.scrut != Scrut::Int && seen.iter().all(|s| *s) {
                seen_any = true;
            }
        }
        count
    }

    /// The number of the first arm that matches when run.
    pub(super) fn expected_arm(&self) -> Option<usize> {
        self.arms.iter().position(|a| match a {
            Pat::Any => true,
            Pat::Case(c) => *c == self.value,
        })
    }
}

#[test]
fn match_exhaustiveness_check_agrees_with_execution() {
    let reg = registry();
    let head = "enum E { A, B(int), C(int, str), D }\n";
    let (mut exhaustive, mut not_exhaustive, mut with_unreachable) = (0, 0, 0);
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0x3A7C);
        let k = 1 + rng.below(3);
        let plans: Vec<MatchPlan> = (0..k).map(|_| MatchPlan::generate(&mut rng)).collect();
        let mut src = String::from(head);
        src.push_str("label start {\n");
        for (m, plan) in plans.iter().enumerate() {
            let ty = match plan.scrut {
                Scrut::Enum => "E",
                Scrut::Opt => "Option<int>",
                Scrut::Bool => "bool",
                Scrut::Int => "int",
            };
            writeln!(
                src,
                "  let x{m}: {ty} = {}",
                plan.scrut.value_text(plan.value)
            )
            .unwrap();
            writeln!(src, "  match x{m} {{").unwrap();
            for (i, arm) in plan.arms.iter().enumerate() {
                let (pat, bind) = match arm {
                    Pat::Any => ("_".to_string(), ""),
                    Pat::Case(c) => (plan.scrut.pattern_text(*c), plan.scrut.binding_text(*c)),
                };
                writeln!(src, "    {pat} => m{m}a{i}{bind}").unwrap();
            }
            src.push_str("  }\n");
        }
        src.push_str("  end\n}\n");

        let all_ok = plans.iter().all(MatchPlan::exhaustive);
        let expected_unreachable: usize = plans.iter().map(MatchPlan::unreachable).sum();
        match compile_with_warnings(&src, "main", &reg) {
            Ok((prog, warnings)) => {
                assert!(all_ok, "seed={seed}: 網羅していないのに通った\n{src}");
                exhaustive += 1;
                let unreachable = warnings
                    .iter()
                    .filter(|w| w.code.as_str() == "unreachable_pattern")
                    .count();
                assert_eq!(unreachable, expected_unreachable, "seed={seed}\n{src}");
                with_unreachable += (unreachable > 0) as usize;
                // Execution: only the body of the first matching arm is output
                let got: Vec<String> = trace_of(&prog, &reg, 100)
                    .iter()
                    .filter_map(|s| match s {
                        Ok(Yield::Stop { args, .. }) => args.last().map(Value::to_text),
                        _ => None,
                    })
                    .collect();
                let want: Vec<String> = plans
                    .iter()
                    .enumerate()
                    .map(|(m, p)| {
                        let i = p.expected_arm().expect("網羅していれば合うアームがある");
                        let extra = match p.arms[i] {
                            Pat::Any => "",
                            Pat::Case(c) => p.scrut.binding_expect(c),
                        };
                        format!("m{m}a{i}{extra}")
                    })
                    .collect();
                assert_eq!(got, want, "seed={seed}\n{src}");
            }
            Err(all_diags) => {
                let diags: Vec<_> = all_diags
                    .into_iter()
                    .filter(|d| d.severity == Severity::Error)
                    .collect();
                assert!(
                    !all_ok,
                    "seed={seed}: 網羅しているのに断られた: {}\n{src}",
                    show_diags(&diags)
                );
                not_exhaustive += 1;
                let missing = plans.iter().filter(|p| !p.exhaustive()).count();
                assert!(
                    diags
                        .iter()
                        .all(|d| d.code.as_str() == "non_exhaustive_match"),
                    "seed={seed}: {}\n{src}",
                    show_diags(&diags)
                );
                assert_eq!(diags.len(), missing, "seed={seed}\n{src}");
            }
        }
    }
    let n = case_count();
    assert!(
        exhaustive * 5 >= n,
        "網羅した match が少ない: {exhaustive}/{n}"
    );
    assert!(
        not_exhaustive * 5 >= n,
        "網羅しない match が少ない: {not_exhaustive}/{n}"
    );
    assert!(
        with_unreachable * 20 >= n,
        "到達しないアームの例が少ない: {with_unreachable}/{n}"
    );
}
