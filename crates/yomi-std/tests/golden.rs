//! Golden tests of `yomi-std`.
//!
//! Reads `tests/golden/*.yomi` as scripts and compares the output with the `.out` of the same name (the same idea as the
//! golden tests of `yomi-core`; here it runs with a registry with `yomi_std::register_all` registered).
//!
//! - If compilation fails, lists diagnostics in the form `line:col code arg=value...`.
//! - If it passes, runs and lists the stop points (`停止 command(args) #ID`) and how it ended (`終了`,
//!   `実行時エラー code ...`). At stop points, it resumes right away without doing anything (without waiting).
//! - At stop points (and the end), if there are cues of `audio`, `image` or `screen`, adds lines `  合図 音 ...`,
//!   `  合図 画像 ...`, `  合図 画面 ...`. Before the end, prints the state of sound, images and the screen (sounds of
//!   channels kept playing, images shown, the screen cover), if any.
//! - At `input.*` stop points, if there is a `.in` with the same name, its lines (`true`, `false`, integers, otherwise
//!   strings) are returned in order as return values. Otherwise, the default value (the `default` argument, `true`) is
//!   returned. Other stop points resume with `()`.
//! - The prefixes `run_`, `err_` and `diag_` classify file names (the output format is common).
//!
//! To rewrite the expectations: `BLESS=1 cargo test -p yomi-std --test golden` (after checking the output by eye).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use yomi_core::{
    DiagArg, Identity, Input, Registry, Severity, Sugar, Value, Vm, VmConfig, VmError, Yield,
    compile_modules_with_warnings,
};
use yomi_std::audio::Audio;
use yomi_std::image::Image;
use yomi_std::screen::Screen;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
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

fn show_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{s:?}"),
        Value::Int(n) => n.to_string(),
        Value::Float(x) => format!("{x:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Unit => "()".to_string(),
        other => format!("{other:?}"),
    }
}

/// Turns one line of `.in` into a return value.
fn parse_input(line: &str) -> Value {
    match line {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => match line.parse::<i64>() {
            Ok(n) => Value::Int(n),
            Err(_) => Value::Str(line.into()),
        },
    }
}

/// The return value used to resume a stop point. For `input.*`, the next line of `.in` or the default value.
fn return_value(name: &str, args: &[Value], inputs: &mut std::vec::IntoIter<Value>) -> Value {
    if !name.starts_with("input.") {
        return Value::Unit;
    }
    if let Some(v) = inputs.next() {
        return v;
    }
    match (name, args) {
        ("input.text", [_, default, _]) | ("input.integer", [_, default, _, _]) => default.clone(),
        _ => Value::Bool(true),
    }
}

/// Adds the sound and image cues as lines.
fn write_cues(out: &mut String, vm: &Vm<'_>) {
    if let Some(a) = vm.package_state::<Audio>() {
        for c in &a.cues {
            writeln!(out, "  合図 音 {c:?}").unwrap();
        }
    }
    if let Some(i) = vm.package_state::<Image>() {
        for c in &i.cues {
            writeln!(out, "  合図 画像 {c:?}").unwrap();
        }
    }
    if let Some(s) = vm.package_state::<Screen>() {
        for c in &s.cues {
            writeln!(out, "  合図 画面 {c:?}").unwrap();
        }
    }
}

/// Before the end, prints the state of sound and images only when it differs from the default.
fn write_state(out: &mut String, vm: &Vm<'_>) {
    if let Some(a) = vm.package_state::<Audio>()
        && !a.channels.is_empty()
    {
        writeln!(out, "  状態 チャンネル {:?}", a.channels).unwrap();
    }
    if let Some(i) = vm.package_state::<Image>()
        && !i.items.is_empty()
    {
        writeln!(out, "  状態 画像 {:?}", i.items).unwrap();
    }
    if let Some(s) = vm.package_state::<Screen>()
        && s.cover.is_some()
    {
        writeln!(out, "  状態 画面 {:?}", s.cover).unwrap();
    }
}

fn write_error(out: &mut String, e: &VmError) {
    write!(out, "実行時エラー {}", e.kind.code()).unwrap();
    if let Some(l) = &e.label {
        write!(out, " label={l}").unwrap();
    }
    if let Some(n) = e.line {
        write!(out, " line={n}").unwrap();
    }
    out.push('\n');
}

fn render(src: &str, inputs: Vec<Value>) -> String {
    let mut inputs = inputs.into_iter();
    let reg = registry();
    let mut out = String::new();
    let prog = match compile_modules_with_warnings(&[("main", src)], &reg) {
        Ok((p, warnings)) => {
            for d in &warnings {
                writeln!(out, "警告 {}:{} {}", d.line, d.col, d.code.as_str()).unwrap();
            }
            p
        }
        Err(diags) => {
            for d in &diags {
                let mark = if d.severity == Severity::Warning {
                    "警告 "
                } else {
                    ""
                };
                write!(out, "{mark}{}:{} {}", d.line, d.col, d.code.as_str()).unwrap();
                for (name, v) in &d.args {
                    match v {
                        DiagArg::Str(t) => write!(out, " {name}={t:?}").unwrap(),
                        DiagArg::Int(n) => write!(out, " {name}={n}").unwrap(),
                        DiagArg::List(v) => write!(out, " {name}={v:?}").unwrap(),
                    }
                }
                out.push('\n');
            }
            return out;
        }
    };
    let cfg = VmConfig::default()
        .max_steps_per_run(Some(100_000))
        .start("main.start");
    let mut vm = Vm::new(&prog, &reg, cfg).expect("VM を作れる");
    for _ in 0..1_000 {
        match vm.run() {
            Ok(Yield::Stop { command, args, id }) => {
                write_cues(&mut out, &vm);
                let name = reg.command(command).name.clone();
                let shown: Vec<String> = args.iter().map(show_value).collect();
                write!(out, "停止 {name}({})", shown.join(", ")).unwrap();
                if let Some(id) = id {
                    write!(out, " #{id}").unwrap();
                }
                out.push('\n');
                let value = return_value(&name, &args, &mut inputs);
                if name.starts_with("input.") {
                    writeln!(out, "  入力 {}", show_value(&value)).unwrap();
                }
                if let Err(e) = vm.resume(Input::Return(value)) {
                    write_error(&mut out, &e);
                    return out;
                }
            }
            Ok(Yield::End) => {
                write_cues(&mut out, &vm);
                write_state(&mut out, &vm);
                out.push_str("終了\n");
                return out;
            }
            Ok(other) => {
                writeln!(out, "未知の停止 {other:?}").unwrap();
                return out;
            }
            Err(e) => {
                write_error(&mut out, &e);
                return out;
            }
        }
    }
    out.push_str("停止点が多すぎる\n");
    out
}

#[test]
fn golden_output_matches_expectations() {
    let bless = std::env::var_os("BLESS").is_some();
    let mut files: Vec<PathBuf> = fs::read_dir(golden_dir())
        .expect("golden ディレクトリを読める")
        .map(|e| e.expect("読める").path())
        .filter(|p| p.extension().is_some_and(|x| x == "yomi"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "ゴールデンの台本がある");
    let mut failures = Vec::new();
    for f in files {
        let src = fs::read_to_string(&f).unwrap();
        let inputs = fs::read_to_string(f.with_extension("in"))
            .map(|t| t.lines().map(parse_input).collect())
            .unwrap_or_default();
        let got = render(&src, inputs);
        let out_path = f.with_extension("out");
        if bless {
            fs::write(&out_path, &got).unwrap();
            continue;
        }
        let want = fs::read_to_string(&out_path)
            .unwrap_or_else(|_| panic!("{} がない（BLESS=1 で作る）", out_path.display()));
        if got != want {
            failures.push(format!(
                "--- {}\n期待:\n{want}実際:\n{got}",
                f.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
