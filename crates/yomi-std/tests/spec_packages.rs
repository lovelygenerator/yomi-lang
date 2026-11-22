//! Checks that the package specs (`spec/packages/*.md`) match the implementation.
//!
//! - The tables of commands (names, parameter names and types; also return types for `text` and `input`) match the
//!   registry. Both something in a table but not registered, and something registered but not in a table, fail.
//! - The "error codes" tables (codes, arguments) match `yomi_std::error_codes()`.
//! - The script examples in the specs (`input.md`, `text.md`) compile, and the examples in `text.md` produce the output
//!   written.
//!
//! The spec files are relative to the workspace root (`CARGO_MANIFEST_DIR/../../spec/packages/…`).
//! If the files are missing (such as a public repository exported with only the spec), it finishes without checking
//! anything.

use std::collections::BTreeMap;
use std::path::PathBuf;

use yomi_core::{Identity, Input, Registry, Sugar, Value, Vm, VmConfig, Yield, compile};

const PACKAGES: &[&str] = &[
    "audio", "event", "flow", "image", "input", "json", "math", "screen", "text",
];

fn spec_text(package: &str) -> Option<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "spec",
        "packages",
        &format!("{package}.md"),
    ]
    .iter()
    .collect();
    std::fs::read_to_string(path).ok()
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    yomi_std::register_all(&mut r).unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// One command written in the spec (name, list of parameters as "name: type", return type (if written)).
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    params: Vec<String>,
    ret: Option<String>,
}

fn split_params(s: &str) -> Vec<String> {
    s.split(", ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Reads command descriptions from a table. The first column is in the form `name(args)` (`math`, `text`) or the full
/// name (others).
fn parse(package: &str, text: &str) -> BTreeMap<String, Doc> {
    let mut out = BTreeMap::new();
    let mut in_errors = false;
    let mut in_host = false;
    for line in text.lines() {
        if line.starts_with('#') {
            in_errors = line.contains("エラーコード");
            in_host = line.contains("ホスト（Rust）");
        }
        // The "error codes" table is not a table of commands (the `error_codes` test reads it).
        // The table of "functions used from the host (Rust)" is not script commands either
        if in_errors || in_host || !line.trim_start().starts_with('|') {
            continue;
        }
        let c = cells(line);
        if c.len() < 2 || c[0].starts_with("---") || c[0] == "関数" || c[0] == "コマンド" {
            continue;
        }
        let first = backticked(&c[0]);
        if first.is_empty() {
            continue;
        }
        if first[0].contains('(') {
            // One or two `name(args)`. The return value is in the second column in the form `int` / `float`. `同じ型` is not compared
            let rets: Vec<String> = c[1].split(" / ").map(|r| r.replace('`', "")).collect();
            for (i, seg) in first.iter().enumerate() {
                let (name, rest) = seg.split_once('(').expect("括弧がある");
                let params = split_params(rest.trim_end_matches(')'));
                let ret = if package == "text" {
                    rets.get(i).or(rets.first()).cloned()
                } else {
                    None
                };
                out.insert(format!("{package}.{name}"), Doc { params, ret });
            }
        } else if first[0].starts_with(&format!("{package}.")) && first.len() == 1 {
            let mut params = None;
            let mut ret = None;
            for cell in &c[1..] {
                let segs = backticked(cell);
                if params.is_none()
                    && (cell == "なし"
                        || (!segs.is_empty() && segs.iter().all(|s| s.contains(": "))))
                {
                    params = Some(if cell == "なし" { vec![] } else { segs });
                } else if package == "input" && params.is_some() && ret.is_none() && segs.len() == 1
                {
                    ret = Some(segs[0].clone());
                }
            }
            if let Some(params) = params {
                out.insert(first[0].clone(), Doc { params, ret });
            }
        }
    }
    out
}

#[test]
fn spec_command_tables_match_registry() {
    let reg = registry();
    let mut problems: Vec<String> = Vec::new();
    for package in PACKAGES {
        let Some(text) = spec_text(package) else {
            eprintln!("spec/packages/{package}.md がないので、確かめない");
            continue;
        };
        let docs = parse(package, &text);
        assert!(!docs.is_empty(), "{package}.md からコマンドの表を読めない");
        let decls: BTreeMap<String, _> = reg
            .commands()
            .into_iter()
            .filter(|d| d.package.as_deref() == Some(package))
            .map(|d| (d.name.clone(), d))
            .collect();
        for name in docs.keys() {
            if !decls.contains_key(name) {
                problems.push(format!("{name}: 仕様書にあって、登録にない"));
            }
        }
        for (name, d) in &decls {
            let Some(doc) = docs.get(name) else {
                problems.push(format!("{name}: 登録にあって、仕様書にない"));
                continue;
            };
            let actual: Vec<String> = d
                .args
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let n = d.params.get(i).cloned().flatten().unwrap_or_default();
                    match d.defaults.get(i).cloned().flatten() {
                        Some(v) => format!("{n}: {} = {}", t.name(), default_text(&v)),
                        None => format!("{n}: {}", t.name()),
                    }
                })
                .collect();
            if actual != doc.params {
                problems.push(format!(
                    "{name}: 引数が違う（仕様書 {:?}、登録 {actual:?}）",
                    doc.params
                ));
            }
            if let Some(r) = &doc.ret
                && *r != d.ret.name()
            {
                problems.push(format!(
                    "{name}: 戻り値が違う（仕様書 {r}、登録 {}）",
                    d.ret.name()
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "パッケージの仕様書が実装と合わない:\n{}",
        problems.join("\n")
    );
}

/// How a default value is written in the spec (in the form of a script literal; floating point numbers have a decimal
/// point).
fn default_text(v: &Value) -> String {
    match v {
        Value::Float(x) if x.fract() == 0.0 => format!("{x:.1}"),
        Value::Float(x) => x.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Str(t) => format!("{t:?}"),
        Value::Option(None) => "None".to_string(),
        other => format!("{other:?}"),
    }
}

/// Extracts script examples (code blocks) in the spec that start with `import`.
fn script_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        match (&mut cur, line.starts_with("```")) {
            (None, true) if line == "```" => cur = Some(String::new()),
            (Some(_), true) => {
                let body = cur.take().unwrap();
                if body.starts_with("import ") {
                    out.push(body);
                }
            }
            (Some(b), false) => {
                b.push_str(line);
                b.push('\n');
            }
            _ => {}
        }
    }
    out
}

#[test]
fn spec_script_examples_compile() {
    let reg = registry();
    let mut checked = 0;
    for package in PACKAGES {
        let Some(text) = spec_text(package) else {
            continue;
        };
        for (i, block) in script_blocks(&text).iter().enumerate() {
            if let Err(diags) = compile(block, "main", &reg) {
                panic!(
                    "{package}.md の {} 番目の例がコンパイルできない: {diags:?}",
                    i + 1
                );
            }
            checked += 1;
        }
    }
    if PACKAGES.iter().any(|p| spec_text(p).is_some()) {
        assert!(checked >= 2, "確かめた例が少なすぎる: {checked}");
    }
}

#[test]
fn spec_text_examples_produce_written_output() {
    let Some(text) = spec_text("text") else {
        return;
    };
    let reg = registry();
    let block = script_blocks(&text).into_iter().next().expect("例がある");
    let program = compile(&block, "main", &reg).expect("コンパイルできる");
    let mut vm = Vm::new(&program, &reg, VmConfig::default()).unwrap();
    let mut shown: Vec<String> = Vec::new();
    loop {
        match vm.run().expect("動く") {
            Yield::Stop { args, .. } => {
                if let Some(Value::Str(s)) = args.first() {
                    shown.push(s.to_string());
                }
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            Yield::End => break,
            _ => unreachable!(),
        }
    }
    // The spec text: "表示は、順に `ゆみ・ひろ・さく`・`007`・`いう`・`ーいー`"
    let want: Vec<String> = text
        .lines()
        .find_map(|l| l.strip_prefix("表示は、順に "))
        .expect("表示の行がある")
        .trim_end_matches('。')
        .split("`・`")
        .map(|s| s.trim_matches('`').to_string())
        .collect();
    assert_eq!(shown, want);
}

/// Reads codes and argument names from the table in the "error codes" section.
fn parse_error_codes(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut in_errors = false;
    for line in text.lines() {
        if line.starts_with('#') {
            in_errors = line.contains("エラーコード");
        }
        if !in_errors || !line.trim_start().starts_with('|') {
            continue;
        }
        let c = cells(line);
        let first = backticked(&c[0]);
        if c.len() < 3 || first.len() != 1 {
            continue;
        }
        let args = if c[1] == "—" {
            Vec::new()
        } else {
            backticked(&c[1])
        };
        assert!(
            out.insert(first[0].clone(), args).is_none(),
            "{}: 表で重複",
            first[0]
        );
    }
    out
}

#[test]
fn spec_error_code_tables_match_error_codes() {
    let mut checked = 0;
    for package in PACKAGES {
        let actual: BTreeMap<String, Vec<String>> = yomi_std::error_codes()
            .iter()
            .filter(|c| c.code.split_once('.').map(|(p, _)| p) == Some(*package))
            .map(|c| {
                (
                    c.code.to_string(),
                    c.args.iter().map(|a| a.to_string()).collect(),
                )
            })
            .collect();
        let Some(text) = spec_text(package) else {
            continue;
        };
        let doc = parse_error_codes(&text);
        assert_eq!(doc, actual, "{package}.md のエラーコードの表が合わない");
        checked += actual.len();
    }
    if PACKAGES.iter().any(|p| spec_text(p).is_some()) {
        assert_eq!(checked, yomi_std::error_codes().len());
    }
}

#[test]
fn error_codes_are_package_error_name_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for c in yomi_std::error_codes() {
        let (package, name) = c.code.split_once('.').expect("パッケージ名.エラー名");
        assert!(PACKAGES.contains(&package), "{}", c.code);
        assert!(
            !name.is_empty() && name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
            "{}",
            c.code
        );
        assert!(seen.insert(c.code), "{} が重複", c.code);
    }
}
