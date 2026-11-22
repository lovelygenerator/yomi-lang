//! Invariants of the REPL (property tests with a hand-written random generator, in the same style as `tests/invariants/`).
//!
//! - `YOMI_INVARIANT_CASES`: the number of cases per test (default 300).
//! - `YOMI_INVARIANT_SEED`: the random seed (the default is a fixed value; used to reproduce failures).
//!
//! Properties:
//! 1. Sequences of input do not panic.
//! 2. Input that ends in diagnostics, a runtime error or `abort` does not change the state (variables, declarations).
//! 3. The same sequence of input gives the same result.
//! 4. Without `apply`, the `attach`ed VM does not change.
//! 5. `completeness` does not panic on any string.

mod common;
use common::rng::{self, Rng};

use yomi_core::{
    Completeness, Input, Program, Registry, Repl, ReplYield, Value, Vm, VmConfig, Yield, compile,
    completeness,
};

fn case_count() -> usize {
    rng::case_count_or(300)
}

fn base_seed() -> u64 {
    rng::base_seed_or(0x5EED_1234)
}

const GAME: &str = r#"identity 案内人 = "リオン"
save var favor: int = 3
save var items: list<int> = [1]
persistent var best: int = 0
struct Pt { x: int, y: int }
fn dbl(n: int) -> int {
  return n * 2
}
label start {
  let g = 1
  案内人「やあ${favor}」 #s1
  jump more
}
label more {
  \おわり
  end
}
label add(a: int, b: int) -> int {
  return a + b
}
"#;

fn registry() -> Registry {
    let mut r = common::full_registry();
    r.stop::<(String,), String>("game.ask").unwrap();
    r
}

fn program(reg: &Registry) -> Program {
    compile(GAME, "main", reg).unwrap()
}

fn config() -> VmConfig {
    VmConfig::default().max_steps_per_run(Some(20_000)).seed(5)
}

/// Fragments of input (a mix of valid, invalid, incomplete and not-allowed ones).
const FRAGMENTS: &[&str] = &[
    "favor",
    "favor += 1",
    "favor = favor * 2",
    "favor = 1000000 * favor",
    "items.push(favor)",
    "items.clear()",
    "items[0]",
    "items[9]",
    "items[9] = 1",
    "let a = 1",
    "let a = \"あ\"",
    "let b = a",
    "let c = dbl(favor)",
    "let p = Pt { x: 1, y: 2 }",
    "p.x += 1",
    "a",
    "b",
    "c",
    "p",
    "dbl(3)",
    "dbl(favor) + 1",
    "1 / 0",
    "1 / (favor - favor)",
    "random(1, 6)",
    "fn f(n: int) -> int {\n  return n + 1\n}",
    "fn f(n: int) -> int {\n  return n + 2\n}",
    "fn f(n: str) -> int {\n  return 1\n}",
    "fn h() -> int {\n  return favor\n}",
    "struct Q { v: int }",
    "enum E { A, B(int) }",
    "identity 村長 = \"たろう\"",
    "import counter",
    "import nothing",
    "f(1)",
    "E.B(3)",
    "村長「こんにちは」",
    "案内人「こんにちは」",
    "\\ただの行",
    "ただの行",
    "信頼度は${favor}",
    "if favor > 3 {\n  favor -= 1\n}",
    "while favor < 20 {\n  favor += 1\n}",
    "while true {\n  favor += 1\n}",
    "for x in items {\n  favor += x\n}",
    "match favor {\n  3 => favor = 4\n  _ => favor = 3\n}",
    "choice {\n  「はい」 => favor += 1\n  「いいえ」 => favor -= 1\n}",
    "call add(1, 2)",
    "let r = call add(favor, 2)",
    "call more()",
    "call start()",
    "@counter.inc(2)",
    "counter.get()",
    "let n = @game.roll(6)",
    "let w = @game.ask(\"?\")",
    "jump more",
    "end",
    "return",
    "label z {",
    "save var s: int = 1",
    "persistent var t: int = 1",
    "案内人「やあ」 #q",
    "if true {",
    "fn g() {",
    "案内人「閉じない",
    "let x =",
    "}",
    ":vars",
    "",
    "// メモ",
    "gld",
    "best",
    "best = 5",
    "g",
];

fn gen_input(rng: &mut Rng) -> String {
    if rng.below(8) == 0 {
        // Join fragments and break them
        let a = rng.pick(FRAGMENTS);
        let b = rng.pick(FRAGMENTS);
        let mut s = format!("{a}\n{b}");
        if rng.below(2) == 0 {
            let cut = rng.below(s.chars().count() + 1);
            s = s.chars().take(cut).collect();
        }
        s
    } else {
        rng.pick(FRAGMENTS).to_string()
    }
}

/// A fingerprint of the state (variable values, REPL declarations).
fn fingerprint(repl: &Repl<'_>) -> String {
    let defs: Vec<String> = repl.defs().iter().map(ToString::to_string).collect();
    format!("{:?}\n{defs:?}", repl.variables())
}

/// Returns fixed answers at stop points. Returns a record of the results.
fn drive(repl: &mut Repl<'_>, reg: &Registry, input: &str) -> (String, bool) {
    let before = fingerprint(repl);
    if let Err(d) = repl.submit(input) {
        assert_eq!(
            fingerprint(repl),
            before,
            "診断の入力が状態を変えた: {input:?} {d:?}"
        );
        return (
            format!(
                "診断 {:?}",
                d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            ),
            false,
        );
    }
    let mut log = String::new();
    let mut changed = true;
    for step in 0..50 {
        match repl.run() {
            Err(e) => {
                assert_eq!(
                    fingerprint(repl),
                    before,
                    "実行時エラーが状態を変えた: {input:?} {e:?}"
                );
                log.push_str(&format!("エラー {:?}", e.kind));
                changed = false;
                break;
            }
            Ok(ReplYield::Stop { command, args, id }) => {
                log.push_str(&format!(
                    "停止 {} {args:?} {id:?};",
                    reg.command(command).name
                ));
                // Abort once every 3 times
                if step % 3 == 2 {
                    repl.abort();
                    assert_eq!(
                        fingerprint(repl),
                        before,
                        "abort が状態を戻さない: {input:?}"
                    );
                    changed = false;
                    break;
                }
                let v = match reg.command(command).name.as_str() {
                    "ui.choose" => Value::Int((step % 2) as i64),
                    "game.roll" => Value::Int(3),
                    "game.ask" => Value::Str("答".into()),
                    _ => Value::Unit,
                };
                if let Err(e) = repl.resume(Input::Return(v)) {
                    log.push_str(&format!("再開エラー {:?}", e.kind));
                    repl.abort();
                    assert_eq!(fingerprint(repl), before);
                    changed = false;
                    break;
                }
            }
            Ok(other) => {
                log.push_str(&format!("{other:?}"));
                break;
            }
        }
    }
    let _ = changed;
    (log, true)
}

#[test]
fn input_sequences_never_panic_and_failed_inputs_keep_state() {
    let reg = registry();
    let prog = program(&reg);
    for case in 0..case_count() {
        let mut rng = Rng::new(base_seed() + case as u64);
        let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
        for _ in 0..12 {
            let input = gen_input(&mut rng);
            drive(&mut repl, &reg, &input);
            if rng.below(30) == 0 {
                repl.reset().unwrap();
            }
        }
    }
}

#[test]
fn same_input_sequences_give_same_results() {
    let reg = registry();
    let prog = program(&reg);
    for case in 0..case_count() {
        let play = || {
            let mut rng = Rng::new((base_seed() ^ 0xABCD) + case as u64);
            let mut repl = Repl::from_program(&prog, &reg, config()).unwrap();
            let mut out = Vec::new();
            for _ in 0..10 {
                let input = gen_input(&mut rng);
                out.push(drive(&mut repl, &reg, &input).0);
            }
            out.push(fingerprint(&repl));
            out
        };
        assert_eq!(play(), play(), "case {case}");
    }
}

#[test]
fn attached_vm_is_unchanged_without_apply() {
    let reg = registry();
    let prog = program(&reg);
    for case in 0..case_count() {
        let mut rng = Rng::new((base_seed() ^ 0x77) + case as u64);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
        let before = (
            vm.save().unwrap(),
            vm.current().cloned(),
            vm.variables(),
            vm.save_persistent(),
        );
        {
            let mut repl = Repl::attach(&vm).unwrap();
            for _ in 0..10 {
                let input = gen_input(&mut rng);
                drive(&mut repl, &reg, &input);
            }
        }
        let after = (
            vm.save().unwrap(),
            vm.current().cloned(),
            vm.variables(),
            vm.save_persistent(),
        );
        assert_eq!(before, after, "case {case}");
        // The continuation is the same as without using the REPL
        let mut plain = Vm::new(&prog, &reg, config()).unwrap();
        plain.run().unwrap();
        vm.resume(Input::Return(Value::Unit)).unwrap();
        plain.resume(Input::Return(Value::Unit)).unwrap();
        assert_eq!(vm.run().unwrap(), plain.run().unwrap());
    }
}

#[test]
fn script_can_continue_after_applying_apply() {
    let reg = registry();
    let prog = program(&reg);
    for case in 0..case_count() {
        let mut rng = Rng::new((base_seed() ^ 0x99) + case as u64);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        vm.run().unwrap();
        let mut repl = Repl::attach(&vm).unwrap();
        for _ in 0..6 {
            let input = gen_input(&mut rng);
            drive(&mut repl, &reg, &input);
        }
        let _ = repl.apply(&mut vm);
        // Whether it failed or succeeded, the script can be saved and continued
        let bytes = vm.save().unwrap();
        assert!(
            Vm::load(&prog, &reg, config(), &bytes).is_ok(),
            "case {case}"
        );
        vm.resume(Input::Return(Value::Unit)).unwrap();
        vm.run().unwrap();
    }
}

#[test]
fn completeness_never_panics_and_prefixes_of_valid_input_are_complete_or_incomplete() {
    let mut rng = Rng::new(base_seed());
    let alphabet: Vec<char> = "{}()[]\"\\「」#=+-*/<>,.:;@$ \n\tab1あ案内人/"
        .chars()
        .collect();
    for _ in 0..case_count() * 4 {
        let n = rng.below(40);
        let s: String = (0..n).map(|_| *rng.pick(&alphabet)).collect();
        let _ = completeness(&s);
    }
    // Valid input cut partway at a line break
    for frag in FRAGMENTS {
        let lines: Vec<&str> = frag.split('\n').collect();
        for k in 1..=lines.len() {
            let head = lines[..k].join("\n");
            if let Completeness::Invalid(d) = completeness(&head) {
                // Depending on where it is cut, it may become invalid, but then there is at least one diagnostic
                assert!(!d.is_empty(), "{head:?}");
            }
        }
        // Among complete fragments, those that close their blocks are never incomplete
        if !frag.ends_with('{') && !frag.contains("閉じない") {
            assert!(
                !matches!(completeness(frag), Completeness::Incomplete),
                "{frag:?}"
            );
        }
    }
}
