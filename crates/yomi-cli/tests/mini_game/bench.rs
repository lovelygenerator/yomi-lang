//! Load scripts (examples/bench)

use super::*;

/// Load scripts and their result lines. The results of the calculations are fixed regardless of time.
pub(super) const BENCHES: &[(&str, &str)] = &[
    ("loop_int", "結果: 45580\n"),
    ("list_hof", "結果: 3003727310\n"),
    ("list_hof_loop", "結果: 3003727310\n"),
    ("list_update", "要素数: 10000、合計: 100000000\n"),
    ("map_update", "要素数: 9000、合計: 135009000\n"),
    (
        "list_scale",
        "要素数: 40000、読み: 2399980000、合計: 1600000000\n",
    ),
    ("map_scale", "要素数: 40000、合計: 800020000\n"),
    (
        "text_join_split",
        "結果: 10000 個、全体 69999 文字、置換後 69999 文字\n",
    ),
    (
        "recursion",
        "fib(24) = 46368、深さ 200 の足し算 500 回の合計 = 10050000\n",
    ),
    ("struct_field", "結果: 150000、本文 100000 文字\n"),
    ("str_copy", "結果: 20000、100000 文字\n"),
    ("struct_copy", "結果: 80000\n"),
    ("label_call", "結果: 10000300000\n"),
];

/// Load scripts produce fixed result values. The run time is only printed to standard error and not used for pass/fail
/// (see it with `cargo test --release -p yomi-cli --test mini_game -- --nocapture 負荷用`).
#[test]
fn bench_scripts_run_with_fixed_results() {
    let dir = scratch("bench");
    for (name, expected) in BENCHES {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/bench")
            .join(name)
            .join("main.yomi");
        let started = std::time::Instant::now();
        let o = yomi(&dir, &["--lang", "ja", "run", script.to_str().unwrap()], "");
        let elapsed = started.elapsed();
        eprintln!("bench {name}: {:.3} 秒", elapsed.as_secs_f64());
        assert_eq!(o.code, 0, "{name}: {}", o.stderr);
        assert_eq!(o.stderr, "", "{name}");
        assert_eq!(o.stdout, format!("{expected}（おわり）\n"), "{name}");
    }
}

/// The load script with many stop points (`stops`) prints 20,000 lines of bodies and a total.
#[test]
fn stop_heavy_load_script_runs_and_prints_bodies_and_total() {
    let dir = scratch("bench_stops");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/bench/stops/main.yomi");
    let started = std::time::Instant::now();
    let o = yomi(&dir, &["--lang", "ja", "run", script.to_str().unwrap()], "");
    eprintln!("bench stops: {:.3} 秒", started.elapsed().as_secs_f64());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "");
    let mut expected: String = (0..20000).map(|i| format!("行 {i}\n")).collect();
    expected.push_str("合計: 199990000\n（おわり）\n");
    assert!(
        o.stdout == expected,
        "出力が違う（{} 行）",
        o.stdout.lines().count()
    );
}
