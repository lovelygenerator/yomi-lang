//! Tests of the REPL core API (sections 7.2 to 7.8 of `spec/host/07-repl.md`).

#[path = "../common/mod.rs"]
mod common;

use yomi_core::{
    ApplyError, Completeness, DiagCode, Diagnostic, Input, InputKind, Program, Registry, Repl,
    ReplYield, Severity, Ty, Value, Vm, VmConfig, VmErrorKind, Yield, completeness,
};

/// A script (close to `game.yomi` of 7.1; uses a test package instead of `math`).
const GAME: &str = r#"import counter
identity 案内人 = "リオン"

save var favor: int = 3
save var items: list<str> = ["お面"]
persistent var best: int = 0

enum Mood { Happy, Normal, Sulky }
struct Prize { name: str, hp: int }

fn mood_of(level: int) -> Mood {
  if level >= 6 {
    return Mood.Happy
  }
  if level >= 3 {
    return Mood.Normal
  }
  return Mood.Sulky
}

label start {
  let gold = 100
  案内人「旅の人、遅い！」
  jump shop
}

label shop {
  choice {
    「買う」 => favor += 1
    「帰る」 => end
  }
}

label talk {
  let gold = 100
  let d = 5
  案内人「信頼度は${favor}、割ると${100 / d}」 #t1
  次の行 #t2
}

label greet(n: int) {
  案内人「やあ${n}」 #g1
}

label bye {
  さよなら
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

/// The script's program.
fn program(reg: &Registry) -> Program {
    yomi_core::compile_modules(&[("main", GAME)], reg).expect("コンパイルできる")
}

fn config() -> VmConfig {
    VmConfig::default().max_steps_per_run(Some(100_000))
}

/// Runs input and returns the result if there is no stop point (fails if it stops).
fn exec(repl: &mut Repl<'_>, input: &str) -> ReplYield {
    repl.submit(input)
        .unwrap_or_else(|d| panic!("{input:?} の診断: {d:?}"));
    let y = repl
        .run()
        .unwrap_or_else(|e| panic!("{input:?} の実行時エラー: {e:?}"));
    assert!(matches!(y, ReplYield::Done { .. }), "{input:?}: {y:?}");
    y
}

/// Evaluates an expression and returns the display form (`to_repl_text`).
fn eval(repl: &mut Repl<'_>, input: &str) -> String {
    match exec(repl, input) {
        ReplYield::Done {
            value: Some((v, _)),
            ..
        } => v.to_repl_text(),
        other => panic!("{input:?} は式ではない: {other:?}"),
    }
}

/// The list of diagnostic codes.
fn codes(d: &[Diagnostic]) -> Vec<&'static str> {
    d.iter().map(|d| d.code.as_str()).collect()
}

fn submit_err(repl: &mut Repl<'_>, input: &str) -> Vec<Diagnostic> {
    repl.submit(input)
        .expect_err(&format!("{input:?} は診断になる"))
}

fn text_of(args: &[Value]) -> String {
    args.last().map(Value::to_text).unwrap_or_default()
}

mod abort_limits;
mod attach_apply;
mod completeness;
mod determinism;
mod input_expr;
mod inspect;
mod not_allowed;
mod scope;
mod script_lines;
mod startup;
mod statements;
