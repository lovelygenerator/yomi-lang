//! Golden tests.
//!
//! Reads `tests/golden/*.yomi` as scripts and compares the output with the `.out` of the same name.
//!
//! - If compilation passes, it runs and turns the sequence of stop points (and how it ended) into a string.
//! - If not, lists diagnostics in the form `line:col code arg=value...`. Warnings are prefixed with `警告 ` (when
//!   compilation passes, warnings are listed before the output of execution).
//! - The file name prefix decides what is checked (`run_`, `err_`, `diag_`).
//! - Lines `// @name: value` at the start of a script are headers. `// @registry: none` runs with a registry without sugar
//!   bindings. `// @registry: ignore` makes quote lines and choices "ignored". `// @registry: no-quote` / `no-choice`
//!   leave only that sugar unbound.
//! - `// @choose: 1 0 2` chooses, at choices (`ui.choose`), those numbers in order (numbers in the given list; 0 when they
//!   run out). Out-of-range numbers are passed as they are (resulting in a resume error).
//! - `// @seen: module.label#id ...` starts with those stop points already passed.
//! - `// @seed: number` is the random seed (the default is a fixed value).
//! - `// @packages: counter other mirror` registers test-only packages (`tests/common/mod.rs`) (not registered by
//!   default).
//! - `// @show: vars history seen slices` writes the state at the end (module variables, host-declared variables, the
//!   history of stop points (with `戻り=` if there is a resume value), passed stop points, and the state slice and
//!   persistent slice of `counter`).
//! - `// @rollback: K` runs to the end, goes back to the snapshot of the K-th (0-based) stop point, and runs again
//!   (`persistent var`s and passed stop points do not go back).
//! - `// @second-run: yes` runs to the end, then runs a second round from the start, carrying over the persistent data.
//! - `// @require-text-ids: yes` makes text lines without stable IDs compile errors (`CompileOptions::require_text_ids`).
//! - Text tables: if there is an `X.texts` with the same name as the script `X.yomi` (`table.texts` for directory cases),
//!   it is read with `TextTable::from_file` and set from the start with `Vm::set_texts`. If the table cannot be read or
//!   set, the error is written as `表の誤り ...` and it runs without a table (the original). `// @lang-at: K` sets the
//!   table at the K-th (0-based) stop point (after it is shown) and writes the notification rebuilt with `refresh_current`
//!   as `更新 ...` (earlier stop points are the original, later ones translations).
//! - `check` (the result of `TextTable::check` as `検査 ...`) and `render` (the arguments of each history entry
//!   re-rendered with the current table as `描画 ...`) can be added to `// @show:`.
//! - A directory is a script of multiple modules (each `<name>/*.yomi` file is a module, named by the file name without
//!   the extension, or by `// @module: name` on the first line if present). The expectation is `<name>/expected.out`. It
//!   runs from `main.start`. Diagnostics and runtime errors are prefixed with the module name (`main:3:5 code ...`).
//!
//! To rewrite the expectations: `BLESS=1 cargo test -p yomi-core --test golden` (after checking the output by eye).

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use yomi_core::{
    CompileOptions, DiagArg, DiagCode, Diagnostic, Identity, Input, Program, Registry, Severity,
    Sugar, TextIssue, TextIssueKind, TextTable, Value, Vm, VmConfig, Yield,
    compile_modules_with_options,
};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// One case (one script, or a directory of multiple modules).
struct Case {
    /// The name of the case (the file name without the extension, or the directory name).
    name: String,
    /// `(module name, source)`.
    sources: Vec<(String, String)>,
    /// Whether it is a directory case.
    multi: bool,
    /// The file of expectations.
    out_path: PathBuf,
    /// The contents of the text table file (if any).
    texts: Option<String>,
    /// `migrations.yomi` in a directory case.
    migrations: Option<String>,
    /// `(path, content)` of the files under `schema/` in a directory case.
    schemas: Vec<(String, String)>,
}

/// The list of cases (in name order).
fn all_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for e in fs::read_dir(golden_dir()).expect("golden ディレクトリを読める") {
        let p = e.expect("読める").path();
        let name = p.file_stem().and_then(|n| n.to_str()).unwrap().to_string();
        if p.is_dir() {
            let mut files: Vec<PathBuf> = fs::read_dir(&p)
                .expect("ケースのディレクトリを読める")
                .map(|e| e.expect("読める").path())
                .filter(|f| f.extension().is_some_and(|x| x == "yomi"))
                .filter(|f| f.file_name().is_some_and(|n| n != "migrations.yomi"))
                .collect();
            let mut schemas: Vec<(String, String)> = fs::read_dir(p.join("schema"))
                .into_iter()
                .flatten()
                .map(|e| e.expect("読める").path())
                .map(|f| {
                    let file = f.file_name().and_then(|n| n.to_str()).unwrap();
                    (format!("schema/{file}"), fs::read_to_string(&f).unwrap())
                })
                .collect();
            schemas.sort();
            files.sort();
            let sources = files
                .iter()
                .map(|f| {
                    let src = fs::read_to_string(f).unwrap();
                    // `// @module: name` on the first line can give a name different from the file name
                    let name = src
                        .lines()
                        .next()
                        .and_then(|l| l.strip_prefix("// @module: "))
                        .map(|n| n.trim().to_string())
                        .unwrap_or_else(|| {
                            f.file_stem().and_then(|n| n.to_str()).unwrap().to_string()
                        });
                    (name, src)
                })
                .collect();
            cases.push(Case {
                name: p.file_name().and_then(|n| n.to_str()).unwrap().to_string(),
                sources,
                multi: true,
                out_path: p.join("expected.out"),
                texts: fs::read_to_string(p.join("table.texts")).ok(),
                migrations: fs::read_to_string(p.join("migrations.yomi")).ok(),
                schemas,
            });
        } else if p.extension().is_some_and(|x| x == "yomi") {
            cases.push(Case {
                name,
                sources: vec![("main".to_string(), fs::read_to_string(&p).unwrap())],
                multi: false,
                out_path: p.with_extension("out"),
                texts: fs::read_to_string(p.with_extension("texts")).ok(),
                migrations: None,
                schemas: Vec::new(),
            });
        }
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}

/// The value of `// @name: value` at the start.
fn header<'a>(sources: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let prefix = format!("// @{name}:");
    sources
        .iter()
        .find_map(|(_, src)| {
            // Lines at the start that begin with `// @` are headers
            src.lines()
                .take_while(|l| l.starts_with("// @"))
                .find_map(|l| l.strip_prefix(prefix.as_str()))
        })
        .map(str::trim)
}

fn registry(sources: &[(String, String)]) -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(String, i64), ()>("game.give").unwrap();
    r.stop::<(i64,), i64>("game.roll").unwrap();
    r.stop::<(String,), String>("game.ask").unwrap();
    r.stop::<(String,), bool>("game.yes").unwrap();
    r.stop::<(), ()>("game.pause").unwrap();
    r.alias("game.old_give", "game.give").unwrap();
    // Named parameters and default values
    r.param_names("game.give", &["item", "count"]).unwrap();
    r.param_defaults("game.give", &[("count", Value::Int(1))])
        .unwrap();
    if let Some(names) = header(sources, "packages") {
        let names: Vec<&str> = names.split_whitespace().collect();
        common::register_test_packages(&mut r, &names);
    }
    match header(sources, "registry") {
        Some("none") => {}
        Some("ignore") => {
            r.bind(Sugar::Plain, "ui.say").unwrap();
            r.ignore(Sugar::Quote).unwrap();
            r.ignore(Sugar::Choice).unwrap();
        }
        Some("no-quote") => {
            r.bind(Sugar::Plain, "ui.say").unwrap();
            r.bind(Sugar::Choice, "ui.choose").unwrap();
        }
        Some("no-choice") => {
            r.bind(Sugar::Plain, "ui.say").unwrap();
            r.bind(Sugar::Quote, "ui.say_as").unwrap();
        }
        _ => {
            r.bind(Sugar::Plain, "ui.say").unwrap();
            r.bind(Sugar::Quote, "ui.say_as").unwrap();
            r.bind(Sugar::Choice, "ui.choose").unwrap();
        }
    }
    r
}

/// Compiles a case (with the header settings).
fn compile_case(
    case: &Case,
    reg: &Registry,
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    let sources: Vec<(&str, &str)> = case
        .sources
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let mut options = CompileOptions::default()
        .require_text_ids(header(&case.sources, "require-text-ids").is_some());
    if let Some(m) = &case.migrations {
        options = options.migrations(m);
    }
    for (path, src) in &case.schemas {
        options = options.frozen_schema_file(path, src);
    }
    compile_modules_with_options(&sources, reg, &options)
}

/// One line of the result of checking the text table.
fn show_issue(i: &TextIssue) -> String {
    let mark = if i.severity == Severity::Warning {
        "警告 "
    } else {
        "エラー "
    };
    let detail = match &i.kind {
        TextIssueKind::BadNumber { number, count } => format!(" number={number} count={count}"),
        _ => String::new(),
    };
    format!("{mark}{} key={:?}{detail}", i.kind.code(), i.key)
}

fn show_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{s:?}"),
        Value::Int(n) => n.to_string(),
        Value::Float(x) => format!("{x:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Unit => "()".to_string(),
        Value::Identity(i) => format!("Identity({}, {:?})", i.id, i.name),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(show_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Option(None) => "None".to_string(),
        Value::Option(Some(v)) => format!("Some({})", show_value(v)),
        other => format!("{other:?}"),
    }
}

fn show_diag(d: &Diagnostic, multi: bool) -> String {
    let mark = if d.severity == Severity::Warning {
        "警告 "
    } else {
        ""
    };
    let module = if multi {
        format!("{}:", d.module)
    } else {
        String::new()
    };
    let mut s = format!("{mark}{module}{}:{} {}", d.line, d.col, d.code.as_str());
    for (name, v) in &d.args {
        match v {
            DiagArg::Str(t) => write!(s, " {name}={t:?}").unwrap(),
            DiagArg::Int(n) => write!(s, " {name}={n}").unwrap(),
            DiagArg::List(v) => write!(s, " {name}={v:?}").unwrap(),
        }
    }
    s
}

/// Compiles and runs a script and turns it into an output string.
fn render(case: &Case) -> String {
    let reg = registry(&case.sources);
    let multi = case.multi;
    let mut out = String::new();
    let prog = match compile_case(case, &reg) {
        Ok((p, warnings)) => {
            for d in &warnings {
                writeln!(out, "{}", show_diag(d, multi)).unwrap();
            }
            p
        }
        Err(diags) => {
            for d in &diags {
                writeln!(out, "{}", show_diag(d, multi)).unwrap();
            }
            return out;
        }
    };
    let start = if multi { "main.start" } else { "start" };
    let choices: Vec<i64> = header(&case.sources, "choose")
        .map(|v| {
            v.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let seen: Vec<&str> = header(&case.sources, "seen")
        .map(|v| v.split_whitespace().collect())
        .unwrap_or_default();
    let seed: u64 = header(&case.sources, "seed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(yomi_core::DEFAULT_SEED);
    let show: Vec<&str> = header(&case.sources, "show")
        .map(|v| v.split_whitespace().collect())
        .unwrap_or_default();
    let second_run = header(&case.sources, "second-run").is_some();
    let rollback_at: Option<usize> = header(&case.sources, "rollback").and_then(|v| v.parse().ok());
    let cfg = VmConfig::default()
        .max_steps_per_run(Some(100_000))
        .seen_points(seen)
        .seed(seed)
        .start(start);
    let mut vm = match Vm::new(&prog, &reg, cfg.clone()) {
        Ok(vm) => vm,
        Err(e) => {
            writeln!(out, "実行時エラー {} (VM を作れない)", e.kind.code()).unwrap();
            return out;
        }
    };
    // The text table (if any, read and check it)
    let table = match &case.texts {
        Some(src) => match TextTable::from_file(src) {
            Ok(t) => {
                if show.contains(&"check") {
                    for i in t.check(&prog) {
                        writeln!(out, "検査 {}", show_issue(&i)).unwrap();
                    }
                }
                Some(t)
            }
            Err(e) => {
                writeln!(out, "表の誤り {e}").unwrap();
                None
            }
        },
        None => None,
    };
    let lang_at: Option<usize> = header(&case.sources, "lang-at").and_then(|v| v.parse().ok());
    let mut pending = None;
    if let Some(t) = table {
        if lang_at.is_some() {
            pending = Some(t);
        } else if let Err(errors) = vm.set_texts(Some(t)) {
            for i in &errors {
                writeln!(out, "表の誤り {}", show_issue(i)).unwrap();
            }
        }
    }
    let player = Player {
        reg: &reg,
        multi,
        lang_at,
    };
    let mut snapshot = None;
    let ended = player.play(
        &mut vm,
        &choices,
        rollback_at,
        &mut snapshot,
        &mut pending,
        &mut out,
    );
    show_state(&vm, &show, &mut out);
    // Rollback: after running to the end, go back to the snapshot of the given stop point and run again.
    // `persistent var`s and passed stop points do not go back.
    if ended && let Some((snap, used)) = snapshot {
        writeln!(
            out,
            "== ロールバック（停止点 {} へ戻る）",
            rollback_at.unwrap()
        )
        .unwrap();
        vm.restore(&snap).expect("戻せる");
        player.play(
            &mut vm,
            &choices[used.min(choices.len())..],
            None,
            &mut None,
            &mut None,
            &mut out,
        );
        show_state(&vm, &show, &mut out);
    }
    // Second round: carry over the persistent data (`persistent var`s and passed stop points) and start from the beginning
    if ended && second_run {
        writeln!(out, "== 2 周目（永続データを引き継ぐ）").unwrap();
        let mut vm2 =
            Vm::new(&prog, &reg, cfg.persistent_data(vm.save_persistent())).expect("作れる");
        player.play(&mut vm2, &choices, None, &mut None, &mut None, &mut out);
        show_state(&vm2, &show, &mut out);
    }
    out
}

/// Runs the script and turns the stop points into a string.
struct Player<'a> {
    reg: &'a Registry,
    multi: bool,
    /// The number of the stop point at which to set the text table (`// @lang-at`).
    lang_at: Option<usize>,
}

impl Player<'_> {
    /// Runs to the end (or to a runtime error). `true` if it ended normally.
    ///
    /// A VM that is already stopped (after a rollback) starts from that stop point. At the `snap_at`-th (0-based) stop point,
    /// puts the snapshot and the number of choices used so far into `snapshot`.
    fn play(
        &self,
        vm: &mut Vm<'_>,
        choices: &[i64],
        snap_at: Option<usize>,
        snapshot: &mut Option<(yomi_core::Snapshot, usize)>,
        pending: &mut Option<TextTable>,
        out: &mut String,
    ) -> bool {
        let reg = self.reg;
        let mut chosen: std::collections::VecDeque<i64> = choices.iter().copied().collect();
        let mut stops = 0usize;
        for _ in 0..1_000 {
            let y = match vm.current() {
                Some(y) if vm.is_stopped() => Ok(y.clone()),
                _ => vm.run(),
            };
            match y {
                Ok(Yield::Stop { command, args, id }) => {
                    let name = &reg.command(command).name;
                    let shown: Vec<String> = args.iter().map(show_value).collect();
                    write!(out, "停止 {name}({})", shown.join(", ")).unwrap();
                    if let Some(id) = &id {
                        write!(out, " #{id}").unwrap();
                    }
                    out.push('\n');
                    if snap_at == Some(stops) {
                        *snapshot =
                            Some((vm.snapshot().expect("停止中"), choices.len() - chosen.len()));
                    }
                    // The table is set after this stop point is shown. Rebuild the current stop point; translations from then on
                    if self.lang_at == Some(stops)
                        && let Some(t) = pending.take()
                    {
                        match vm.set_texts(Some(t)) {
                            Ok(()) => {
                                let y = vm.refresh_current().expect("停止中");
                                if let Yield::Stop { args, .. } = y {
                                    let shown: Vec<String> = args.iter().map(show_value).collect();
                                    writeln!(out, "更新 {name}({})", shown.join(", ")).unwrap();
                                }
                            }
                            Err(errors) => {
                                for i in &errors {
                                    writeln!(out, "表の誤り {}", show_issue(i)).unwrap();
                                }
                            }
                        }
                    }
                    stops += 1;
                    let ret = match name.as_str() {
                        // Returns twice the argument
                        "game.roll" => match &shown[0].parse::<i64>() {
                            Ok(n) => Value::Int(n.wrapping_mul(2)),
                            Err(_) => Value::Int(0),
                        },
                        // Returns the argument with "答:" prepended
                        "game.ask" => match args.first() {
                            Some(Value::Str(t)) => Value::Str(format!("答:{t}").into()),
                            _ => Value::Str("".into()),
                        },
                        // True if the number of characters of the argument is even
                        "game.yes" => match args.first() {
                            Some(Value::Str(t)) => Value::Bool(t.chars().count() % 2 == 0),
                            _ => Value::Bool(false),
                        },
                        // Take the numbers to choose in order from the header
                        "ui.choose" => {
                            let k = chosen.pop_front().unwrap_or(0);
                            writeln!(out, "選択 {k}").unwrap();
                            Value::Int(k)
                        }
                        _ => Value::Unit,
                    };
                    if let Err(e) = vm.resume(Input::Return(ret)) {
                        writeln!(out, "再開できない {}", e.kind.code()).unwrap();
                        return false;
                    }
                }
                Ok(Yield::End) => {
                    out.push_str("終了\n");
                    return true;
                }
                Ok(other) => {
                    writeln!(out, "未知の停止 {other:?}").unwrap();
                    return false;
                }
                Err(e) => {
                    write!(out, "実行時エラー {}", e.kind.code()).unwrap();
                    if let Some(l) = &e.label {
                        match &e.module {
                            Some(m) if self.multi => write!(out, " label={m}.{l}").unwrap(),
                            _ => write!(out, " label={l}").unwrap(),
                        }
                    }
                    if let Some(n) = e.line {
                        write!(out, " line={n}").unwrap();
                    }
                    out.push('\n');
                    return false;
                }
            }
        }
        out.push_str("停止点が多すぎる（1000 を超えた）\n");
        false
    }
}

/// Writes the state at the end specified by `// @show: vars history seen`.
fn show_state(vm: &Vm<'_>, show: &[&str], out: &mut String) {
    if show.contains(&"vars") {
        for v in vm.variables().iter().filter(|v| !v.local) {
            let mark = if v.persistent { " (永続)" } else { "" };
            writeln!(out, "変数 {} = {}{mark}", v.name, show_value(&v.value)).unwrap();
        }
    }
    if show.contains(&"slices") {
        if let Some(s) = vm.package_state::<common::Counter>() {
            writeln!(out, "スライス counter {s:?}").unwrap();
        }
        if let Some(s) = vm.package_persistent::<common::Counter>() {
            writeln!(out, "永続スライス counter {s:?}").unwrap();
        }
        if let Some(s) = vm.package_state::<common::Mirror>() {
            writeln!(out, "スライス mirror {s:?}").unwrap();
        }
        if let Some(s) = vm.package_state::<common::Other>() {
            writeln!(out, "スライス other {s:?}").unwrap();
        }
    }
    if show.contains(&"seen") {
        writeln!(out, "通過済み {:?}", vm.seen_points()).unwrap();
    }
    if show.contains(&"render") {
        for r in vm.history() {
            let args: Vec<String> = vm.render(r).iter().map(show_value).collect();
            writeln!(
                out,
                "描画 {} {}({}) 本文={:?}",
                r.key.as_deref().unwrap_or("-"),
                r.command,
                args.join(", "),
                vm.history_text(r)
            )
            .unwrap();
        }
    }
    if show.contains(&"history") {
        for r in vm.history() {
            let args: Vec<String> = r.args.iter().map(show_value).collect();
            write!(
                out,
                "履歴 {} {}({}) 値={:?}",
                r.key.as_deref().unwrap_or("-"),
                r.command,
                args.join(", "),
                r.values
            )
            .unwrap();
            // Resume return values (not written for `()` or for stop points not yet resumed)
            if let Some(ret) = &r.ret {
                write!(out, " 戻り={}", show_value(ret)).unwrap();
            }
            out.push('\n');
        }
    }
}

/// Makes a line-by-line diff readable by people.
fn diff(expected: &str, actual: &str) -> String {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    for i in 0..e.len().max(a.len()) {
        let (le, la) = (e.get(i), a.get(i));
        if le == la {
            writeln!(out, "  {:>3} | {}", i + 1, le.unwrap()).unwrap();
        } else {
            if let Some(l) = le {
                writeln!(out, "- {:>3} | {l}", i + 1).unwrap();
            }
            if let Some(l) = la {
                writeln!(out, "+ {:>3} | {l}", i + 1).unwrap();
            }
        }
    }
    out
}

fn bless() -> bool {
    std::env::var_os("BLESS").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Compares all scripts whose prefix matches with their expectations.
fn check_group(prefix: &str) {
    let mut failures = String::new();
    let mut n = 0;
    for case in all_cases()
        .into_iter()
        .filter(|c| c.name.starts_with(prefix))
    {
        n += 1;
        let name = &case.name;
        let actual = render(&case);
        if bless() {
            fs::write(&case.out_path, &actual).unwrap();
            continue;
        }
        match fs::read_to_string(&case.out_path) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => {
                writeln!(
                    failures,
                    "== {name} の出力が違う\n{}",
                    diff(&expected, &actual)
                )
                .unwrap();
            }
            Err(_) => {
                writeln!(
                    failures,
                    "== {name} の期待ファイルがない（BLESS=1 で作る）。実際の出力:\n{actual}"
                )
                .unwrap();
            }
        }
    }
    assert!(n > 0, "接頭辞 {prefix} の台本がない");
    assert!(failures.is_empty(), "\n{failures}");
}

#[test]
fn scripts_with_plain_lines_variables_and_branches_run_as_expected() {
    check_group("run_");
}

#[test]
fn runtime_error_scripts_stop_with_expected_errors() {
    check_group("err_");
}

#[test]
fn diagnostic_scripts_return_expected_diagnostics_in_position_order() {
    check_group("diag_");
}

#[test]
fn every_diagnostic_code_appears_in_at_least_one_script() {
    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    for case in all_cases() {
        match compile_case(&case, &registry(&case.sources)) {
            Ok((_, warnings)) => seen.extend(warnings.iter().map(|d| d.code.as_str())),
            Err(diags) => seen.extend(diags.iter().map(|d| d.code.as_str())),
        }
    }
    let missing: Vec<&str> = DiagCode::ALL
        .iter()
        .map(|c| c.as_str())
        // `repl_not_allowed` occurs only with REPL input (not when compiling scripts; `tests/repl/` checks it).
        // `too_many_registers` occurs only with very large labels (unit tests check it)
        .filter(|c| !matches!(*c, "repl_not_allowed" | "too_many_registers"))
        .filter(|c| !seen.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "ゴールデンに出てこないコード: {missing:?}"
    );
}

#[test]
fn diagnostic_code_names_are_all_distinct() {
    let names: BTreeSet<&str> = DiagCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), DiagCode::ALL.len());
}

#[test]
fn diagnostics_have_lines_and_columns_at_least_1_ordered_by_module_and_position() {
    for case in all_cases() {
        let all = match compile_case(&case, &registry(&case.sources)) {
            Ok((_, warnings)) => warnings,
            Err(diags) => diags,
        };
        // Modules not in the sources (frozen schema files, then `migrations`) come after them
        let order = |m: &str| {
            let idx = case.sources.iter().position(|(n, _)| n == m);
            (
                idx.unwrap_or(usize::MAX),
                idx.is_none() && m == "migrations",
                if idx.is_none() {
                    m.to_string()
                } else {
                    String::new()
                },
            )
        };
        for d in &all {
            assert!(
                d.line >= 1 && d.col >= 1,
                "{}: {}",
                case.name,
                show_diag(d, true)
            );
        }
        let keys: Vec<_> = all
            .iter()
            .map(|d| (order(&d.module), d.line, d.col))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "{}: 位置の順でない", case.name);
    }
}

#[test]
fn no_script_has_only_expectation_file_left() {
    let names: BTreeSet<String> = all_cases().into_iter().map(|c| c.name).collect();
    for e in fs::read_dir(golden_dir()).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "out") {
            let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
            assert!(names.contains(&stem), "{stem}.out に対応する .yomi がない");
        }
        if p.is_dir() {
            assert!(
                p.join("expected.out").exists(),
                "{} に expected.out がない",
                p.display()
            );
        }
    }
}
