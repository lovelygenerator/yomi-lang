//! fn: random scripts containing fn

use super::*;

/// An integer expression. `vars` are the names of usable `int`s, `fns` is the number of callable `fn`s (`f0`…, 2
/// parameters).
pub(super) fn fn_int_expr(rng: &mut Rng, depth: u32, vars: &[&str], fns: usize) -> String {
    let leaf = depth == 0 || rng.below(3) == 0;
    if leaf {
        return match rng.below(4) {
            0 => format!("{}", rng.range(-5, 9)),
            1 | 2 if !vars.is_empty() => vars[rng.below(vars.len())].to_string(),
            _ => "xs.len()".to_string(),
        };
    }
    match rng.below(6) {
        0 => format!(
            "({} + {})",
            fn_int_expr(rng, depth - 1, vars, fns),
            fn_int_expr(rng, depth - 1, vars, fns)
        ),
        1 => format!(
            "({} - {})",
            fn_int_expr(rng, depth - 1, vars, fns),
            fn_int_expr(rng, depth - 1, vars, fns)
        ),
        2 => format!("({} * 2)", fn_int_expr(rng, depth - 1, vars, fns)),
        3 if fns > 0 => format!(
            "f{}({}, {})",
            rng.below(fns),
            fn_int_expr(rng, depth - 1, vars, fns),
            fn_int_expr(rng, depth - 1, vars, fns)
        ),
        4 => format!("random(1, {})", rng.range(1, 9)),
        _ => format!(
            "r({}, {})",
            rng.range(0, 6),
            fn_int_expr(rng, depth - 1, vars, fns)
        ),
    }
}

/// `fn f<i>(a: int, b: int) -> int`. Calls only `f0`…`f<i-1>`, `r` and `lst`, so it always terminates.
pub(super) fn gen_fn_def(rng: &mut Rng, i: usize) -> String {
    let vars = ["a", "b", "t"];
    let mut s = format!(
        "fn f{i}(a: int, b: int) -> int {{\n  let t = {}\n",
        fn_int_expr(rng, 2, &["a", "b"], i)
    );
    for u in 0..1 + rng.below(4) {
        match rng.below(6) {
            0 => {
                let c = fn_int_expr(rng, 1, &vars, i);
                let r = fn_int_expr(rng, 2, &vars, i);
                s.push_str(&format!("  if {c} > 3 {{\n    return {r}\n  }}\n"));
            }
            1 => {
                let e = fn_int_expr(rng, 2, &vars, i);
                s.push_str(&format!(
                    "  for k in range(0, 3) {{\n    t += {e} + k\n  }}\n"
                ));
            }
            2 => s.push_str(&format!(
                "  let w{u} = 0\n  while w{u} < 3 {{\n    w{u} += 1\n    t += w{u}\n  }}\n"
            )),
            3 => {
                let e = fn_int_expr(rng, 2, &vars, i);
                s.push_str(&format!(
                    "  match t % 3 {{\n    0 => return {e}\n    1 => {{\n      t += 1\n    }}\n    _ => {{\n    }}\n  }}\n"
                ));
            }
            4 => s.push_str(&format!(
                "  let ys{u} = lst(xs)\n  ys{u}.push(t)\n  t += ys{u}.len()\n"
            )),
            _ => {
                let e = fn_int_expr(rng, 2, &vars, i);
                s.push_str(&format!("  t = {e}\n"));
            }
        }
    }
    s.push_str(&format!(
        "  return {}\n}}\n\n",
        fn_int_expr(rng, 2, &vars, i)
    ));
    s
}

pub(super) fn gen_fn_script(rng: &mut Rng) -> String {
    let nfn = 2 + rng.below(4);
    let mut s = String::from(
        "save var acc: int = 0\nsave var xs: list<int> = [1, 2]\n\nfn r(n: int, a: int) -> int {\n  if n <= 0 {\n    return a\n  }\n  return r(n - 1, a + n)\n}\n\nfn lst(ys: list<int>) -> list<int> {\n  let o: list<int> = []\n  for y in ys {\n    o.push(y + 1)\n  }\n  return o\n}\n\n",
    );
    for i in 0..nfn {
        s.push_str(&gen_fn_def(rng, i));
    }
    s.push_str("label start {\n");
    for n in 0..4 + rng.below(6) {
        let e = fn_int_expr(rng, 3, &["acc"], nfn);
        match rng.below(5) {
            0 => s.push_str(&format!("  acc = {e}\n")),
            1 => s.push_str(&format!("  let v{n} = {e}\n  v{n} は ${{v{n}}} #t{n}\n")),
            2 => s.push_str(&format!("  if {e} > 0 {{\n    正 ${{{e}}} #p{n}\n  }}\n")),
            3 => s.push_str(&format!(
                "  xs = lst(xs)\n  xs.push({e})\n  列 ${{xs}} #l{n}\n"
            )),
            _ => s.push_str(&format!("  結果 ${{{e}}} ${{acc}} #s{n}\n")),
        }
    }
    s.push_str("}\n");
    s
}

#[test]
fn scripts_with_fn_continue_identically_after_save_and_restore() {
    let reg = registry();
    let mut stops = 0usize;
    for case in 0..case_count() {
        let seed = base_seed() + 0x1000 + case as u64;
        let src = gen_fn_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        stops += trace_of(&prog, &reg, 2_000)
            .iter()
            .filter(|s| matches!(s, Ok(Yield::Stop { .. })))
            .count();
        let what = format!("fn seed={seed}");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
    assert!(stops > 0, "停止点のある台本が作られていない");
}

#[test]
fn scripts_with_fn_give_same_stop_points_for_same_input() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + 0x2000 + case as u64;
        let src = gen_fn_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let a = trace_of(&prog, &reg, 2_000);
        let b = trace_of(&prog, &reg, 2_000);
        assert_eq!(a.len(), b.len(), "seed={seed}\n{src}");
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(format!("{x:?}"), format!("{y:?}"), "seed={seed}\n{src}");
        }
        // Saving, loading and running gives the same sequence of stop points (from the first stop point)
        assert!(
            a.iter().all(|s| s.is_ok()),
            "生成した台本が実行時エラー: seed={seed}\n{src}\n{a:?}"
        );
    }
}

#[test]
fn generated_fn_scripts_contain_fn_calls() {
    let mut calls = 0usize;
    for case in 0..case_count() {
        let src = gen_fn_script(&mut Rng::new(base_seed() + 0x1000 + case as u64));
        if src.contains("f0(") || src.contains("f1(") {
            calls += 1;
        }
    }
    assert!(calls * 2 > case_count(), "fn の呼び出しが少ない: {calls}");
}

/// Randomly combines `fn`s containing never-ending recursion and loops.
pub(super) fn gen_runaway_script(rng: &mut Rng) -> String {
    let mut s = String::new();
    // Shapes of recursion: self-recursion, mutual recursion, growing arguments, fattening lists, recursion inside loops
    match rng.below(5) {
        0 => s.push_str("fn go(n: int) -> int {\n  return go(n + 1)\n}\n"),
        1 => s.push_str("fn go(n: int) -> int {\n  return back(n + 1) + 1\n}\nfn back(n: int) -> int {\n  return go(n * 2)\n}\n"),
        2 => s.push_str("fn go(xs: list<int>) -> int {\n  xs.push(1)\n  return go(xs)\n}\n"),
        3 => s.push_str("fn go(n: int) -> int {\n  let t = 0\n  for i in range(0, 3) {\n    t += go(n + i)\n  }\n  return t\n}\n"),
        _ => s.push_str("fn go(n: int) -> int {\n  let t = 0\n  while true {\n    t += n\n  }\n  return t\n}\n"),
    }
    let arg = if s.contains("xs: list<int>") {
        "[1]"
    } else {
        "0"
    };
    s.push_str(&format!("label start {{\n  ${{go({arg})}}\n}}\n"));
    s
}

#[test]
fn fn_recursion_and_infinite_loops_always_stop_at_some_limit() {
    let reg = registry();
    for case in 0..case_count() {
        let mut rng = Rng::new(base_seed() + 0x3000 + case as u64);
        let src = gen_runaway_script(&mut rng);
        let prog = compile_ok(&src, &reg);
        let depth = [1u32, 2, 50, 256, 5_000][rng.below(5)];
        let steps = [100u64, 10_000, 200_000][rng.below(3)];
        let total = [None, Some(2_000usize), Some(1_000_000)][rng.below(3)];
        let cfg = config()
            .max_call_depth(depth)
            .max_steps_per_run(Some(steps))
            .max_total_bytes(total);
        let mut vm = Vm::new(&prog, &reg, cfg).unwrap();
        let kind = vm
            .run()
            .expect_err(&format!("終わらないはずの台本が終わった\n{src}"))
            .kind;
        assert!(
            matches!(
                kind,
                VmErrorKind::CallDepthExceeded
                    | VmErrorKind::StepLimitExceeded
                    | VmErrorKind::MemoryLimitExceeded
                    | VmErrorKind::IntegerOverflow
            ),
            "{kind:?}\n{src}"
        );
        // After an error, it cannot continue
        assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
    }
}
