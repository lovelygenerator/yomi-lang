//! `yomi test`: explores the branches of a script and compares what each path shows with golden files
//! (`spec/host/06-cli.md`, 6.17; `spec/host/08-test.md`).

use std::collections::BTreeSet;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use lingokit::{FluentArgs, Localizer};
use serde_json::{Value as Json, json};
use yomi_core::{
    DiagArg, ExploreConfig, ExploreReport, ExploreStep, FromValue, StopKind, Value, VmConfig,
    VmErrorKind, Yield, explore,
};
use yomi_std::event::Outcome;

use crate::diag::{error_line, vm_error_message};
use crate::modules::{self, SourceFile};
use crate::session::{self, Answer, RunOptions, expectation, parse_answer};
use crate::texts;

/// The first line of a golden file.
const GOLDEN_HEADER: &str = "yomi-golden 1";

/// The extension of golden files.
const GOLDEN_EXT: &str = "golden";

/// Settings of `yomi test`.
#[derive(Debug, Clone)]
pub struct Options {
    pub file: PathBuf,
    pub root: Option<PathBuf>,
    pub start: Option<String>,
    pub seeds: u64,
    pub max_states: usize,
    pub max_steps: Option<u64>,
    pub events: Vec<(String, String)>,
    pub golden: Option<PathBuf>,
    pub update: bool,
    pub require_coverage: bool,
    pub json: bool,
}

/// The line `yomi run --auto` reads for an answer, or `None` for a stop point that reads none.
fn answer_line(
    kind: StopKind,
    command: &str,
    option_id: Option<&str>,
    answer: &Value,
) -> Option<String> {
    match (kind, command, answer) {
        (StopKind::Choice, _, Value::Int(k)) => Some(match option_id {
            Some(id) => format!("#{id}"),
            None => (k + 1).to_string(),
        }),
        (_, "input.text", Value::Str(s)) => Some(s.to_string()),
        (_, "input.integer", Value::Int(n)) => Some(n.to_string()),
        (_, "input.confirm", Value::Bool(b)) => Some(if *b { "y" } else { "n" }.to_string()),
        (_, "event.wait", v) => Outcome::from_value(v).map(|o| match o {
            Outcome::Arrived(name, data) if data.is_empty() => name,
            Outcome::Arrived(name, data) => format!("{name} {data}"),
            Outcome::TimedOut => String::new(),
        }),
        _ => None,
    }
}

fn answer_lines(steps: &[ExploreStep]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|s| answer_line(s.kind, &s.command, s.option_id.as_deref(), &s.answer))
        .collect()
}

/// Whether `yomi run --auto` turns the line for `value` back into `value` (an empty line at `input.text` means the default,
/// for example, so the empty string cannot be entered there).
fn replayable(reg: &yomi_core::Registry, stop: &Yield, value: &Value) -> bool {
    let Yield::Stop { command, args, .. } = stop else {
        return false;
    };
    let name = reg.command(*command).name.as_str();
    let Some(line) = answer_line(StopKind::Command, name, None, value) else {
        return false;
    };
    if line.contains(['\n', '\r']) {
        return false;
    }
    let expect = expectation(reg, *command, args);
    matches!(parse_answer(&expect, &line), Ok(Answer::Return(v)) if &v == value)
}

/// What a golden file holds.
struct Golden {
    seed: u64,
    answers: Vec<String>,
    output: String,
}

fn render_golden(seed: u64, answers: &[String], output: &str) -> String {
    let mut text = format!("{GOLDEN_HEADER}\nseed {seed}\n");
    for a in answers {
        text.push_str("answer ");
        text.push_str(a);
        text.push('\n');
    }
    text.push_str("---\n");
    text.push_str(output);
    text
}

fn parse_golden(text: &str) -> Option<Golden> {
    let mut rest = text;
    let mut next_line = || -> Option<&str> {
        let (line, tail) = rest.split_once('\n')?;
        rest = tail;
        Some(line)
    };
    if next_line()? != GOLDEN_HEADER {
        return None;
    }
    let seed = next_line()?.strip_prefix("seed ")?.parse().ok()?;
    let mut answers = Vec::new();
    loop {
        let line = next_line()?;
        if line == "---" {
            break;
        }
        let a = line.strip_prefix("answer")?;
        answers.push(if a.is_empty() {
            String::new()
        } else {
            a.strip_prefix(' ')?.to_string()
        });
    }
    Some(Golden {
        seed,
        answers,
        output: rest.to_string(),
    })
}

/// What a script showed in the replay of a path.
struct Replay {
    code: i32,
    stdout: String,
}

struct Project<'a> {
    reg: &'a yomi_core::Registry,
    program: &'a yomi_core::Program,
    files: &'a [SourceFile],
    start: &'a str,
    /// The display language of golden files is fixed to English.
    en: Localizer,
}

impl Project<'_> {
    fn replay(&self, seed: u64, answers: &[String]) -> Replay {
        let mut input = String::new();
        for a in answers {
            input.push_str(a);
            input.push('\n');
        }
        let options = RunOptions {
            seed: Some(seed),
            auto: true,
            ..RunOptions::default()
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = session::run_compiled(
            &self.en,
            self.reg,
            self.program,
            self.files,
            self.start,
            &options,
            &mut Cursor::new(input.into_bytes()),
            &mut stdout,
            &mut stderr,
        );
        Replay {
            code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
        }
    }
}

/// The result of comparing with (or writing) golden files.
#[derive(Default)]
struct GoldenResult {
    /// The directory was not there (nothing was compared).
    missing_dir: bool,
    /// Name and the part of the difference to show.
    mismatched: Vec<(String, String)>,
    stale: Vec<String>,
    new: Vec<String>,
    unexplored: Vec<String>,
    written: Vec<String>,
    removed: Vec<String>,
    compared: usize,
}

/// The first line that differs, with 3 lines before and after (`-` is the golden, `+` is the replay).
fn diff_context(expected: &str, actual: &str) -> String {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let at = (0..e.len().max(a.len()))
        .find(|&i| e.get(i) != a.get(i))
        .unwrap_or(0);
    let mut out = String::new();
    for l in &e[at.saturating_sub(3)..at.min(e.len())] {
        out.push_str(&format!("    {l}\n"));
    }
    out.push_str(&format!("  - {}\n", e.get(at).copied().unwrap_or("")));
    out.push_str(&format!("  + {}\n", a.get(at).copied().unwrap_or("")));
    for l in e.iter().skip(at + 1).take(3) {
        out.push_str(&format!("    {l}\n"));
    }
    out
}

/// The names (without the extension) of the golden files in the directory, in order.
fn golden_names(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some(GOLDEN_EXT)
            && path.is_file()
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn golden_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{GOLDEN_EXT}"))
}

/// Compares the paths with the golden files in `dir` (6.17).
fn compare(
    project: &Project<'_>,
    report: &ExploreReport,
    dir: &Path,
) -> std::io::Result<GoldenResult> {
    let mut result = GoldenResult::default();
    let explored: BTreeSet<&str> = report.paths.iter().map(|p| p.name.as_str()).collect();
    let files = golden_names(dir)?;
    for name in &files {
        let text = std::fs::read_to_string(golden_path(dir, name))?;
        result.compared += 1;
        let Some(golden) = parse_golden(&text) else {
            result.stale.push(name.clone());
            continue;
        };
        let replay = project.replay(golden.seed, &golden.answers);
        if replay.code != 0 {
            result.stale.push(name.clone());
        } else if replay.stdout != golden.output {
            result
                .mismatched
                .push((name.clone(), diff_context(&golden.output, &replay.stdout)));
        }
        if !explored.contains(name.as_str()) {
            result.unexplored.push(name.clone());
        }
    }
    let have: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    result.new = report
        .paths
        .iter()
        .filter(|p| !have.contains(p.name.as_str()))
        .map(|p| p.name.clone())
        .collect();
    Ok(result)
}

/// Writes the golden files of the paths (`--update`). Files that hold the same content are left alone; `.golden` files of
/// paths that were not explored are removed.
fn update(
    project: &Project<'_>,
    report: &ExploreReport,
    dir: &Path,
    err: &mut dyn Write,
    loc: &Localizer,
) -> std::io::Result<GoldenResult> {
    let mut result = GoldenResult::default();
    std::fs::create_dir_all(dir)?;
    let mut keep = BTreeSet::new();
    for path in &report.paths {
        keep.insert(path.name.clone());
        let answers = answer_lines(&path.steps);
        let replay = project.replay(path.seed, &answers);
        if replay.code != 0 {
            let _ = writeln!(
                err,
                "{}",
                loc.fmt(
                    "test-golden-run-failed",
                    Some(&one("name", path.name.clone()))
                )
            );
            continue;
        }
        let text = render_golden(path.seed, &answers, &replay.stdout);
        let file = golden_path(dir, &path.name);
        if std::fs::read_to_string(&file).is_ok_and(|old| old == text) {
            continue;
        }
        std::fs::write(&file, text)?;
        result.written.push(path.name.clone());
    }
    for name in golden_names(dir)? {
        if !keep.contains(&name) {
            std::fs::remove_file(golden_path(dir, &name))?;
            result.removed.push(name);
        }
    }
    Ok(result)
}

fn one(key: &'static str, value: impl Into<String>) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    a.set(key, value.into());
    a
}

fn answers_text(loc: &Localizer, steps: &[ExploreStep]) -> String {
    let lines = answer_lines(steps);
    if lines.is_empty() {
        loc.fmt("test-no-answers", None)
    } else {
        lines.join(", ")
    }
}

/// The code and arguments of a runtime error, for JSON.
fn error_code_args(kind: &VmErrorKind) -> (String, Json) {
    if let Some((code, args)) = kind.command_error() {
        let map: serde_json::Map<String, Json> = args
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    DiagArg::Str(s) => json!(s),
                    DiagArg::Int(n) => json!(n),
                    DiagArg::List(l) => json!(l),
                };
                ((*k).to_string(), v)
            })
            .collect();
        return (code.to_string(), Json::Object(map));
    }
    let args = match kind {
        VmErrorKind::KeyNotFound { key } => json!({ "key": key }),
        VmErrorKind::IndexOutOfRange { index, len } => json!({ "index": index, "len": len }),
        VmErrorKind::InvalidRandomRange { min, max } => json!({ "min": min, "max": max }),
        VmErrorKind::ChoiceOutOfRange { index, count } => json!({ "index": index, "count": count }),
        VmErrorKind::BadInput { expected, found } => {
            json!({ "expected": expected.name(), "found": found.name() })
        }
        VmErrorKind::StartLabelNotFound(name)
        | VmErrorKind::CommandMissing(name)
        | VmErrorKind::UnknownVariable(name)
        | VmErrorKind::SliceSerialize(name) => json!({ "name": name }),
        _ => json!({}),
    };
    (kind.code().to_string(), args)
}

/// Runs `yomi test`. Returns the exit code.
pub fn run<O: Write, E: Write>(loc: &Localizer, opts: &Options, out: &mut O, err: &mut E) -> i32 {
    let name = opts.file.display().to_string();
    let (files, entry) = match modules::collect(&opts.file, opts.root.as_deref()) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                let _ = writeln!(err, "{}", texts::collect_error_line(loc, &name, e));
            }
            return 1;
        }
    };
    let start = modules::start_label(&files, entry, opts.start.clone());
    let reg = session::registry();
    let Some(program) = session::compile_files(loc, &reg, &files, &start, err) else {
        return 1;
    };

    let mut vm = VmConfig::default().start(&start);
    if let Some(steps) = opts.max_steps {
        vm = vm.max_steps_per_run(Some(steps));
    }
    let mut config = ExploreConfig::default();
    config.vm = vm;
    config.seeds = (0..opts.seeds).collect();
    config.max_states = opts.max_states;
    let report = match explore(&program, &reg, &config, &mut |y: &Yield| {
        let mut c = yomi_std::test_answers_with_events(&reg, y, &opts.events)?;
        c.retain(|v| replayable(&reg, y, v));
        Some(c)
    }) {
        Ok(r) => r,
        Err(e) => {
            let file = modules::path_of(&files, e.module.as_deref().unwrap_or(""));
            let _ = writeln!(
                err,
                "{}",
                error_line(loc, &file, &vm_error_message(loc, &e))
            );
            return 1;
        }
    };

    let project = Project {
        reg: &reg,
        program: &program,
        files: &files,
        start: &start,
        en: Localizer::only(&crate::LOCALES, "en"),
    };
    let dir = opts.golden.clone().unwrap_or_else(|| {
        opts.root
            .clone()
            .or_else(|| opts.file.parent().map(Path::to_path_buf))
            .unwrap_or_default()
            .join("tests")
            .join("golden")
    });
    let golden = if opts.update {
        update(&project, &report, &dir, err, loc)
    } else if dir.is_dir() {
        compare(&project, &report, &dir)
    } else {
        Ok(GoldenResult {
            missing_dir: true,
            ..GoldenResult::default()
        })
    };
    let golden = match golden {
        Ok(g) => g,
        Err(e) => {
            let mut args = one("dir", dir.display().to_string());
            args.set("reason", e.to_string());
            let _ = writeln!(err, "{}", loc.fmt("test-golden-io", Some(&args)));
            return 1;
        }
    };

    let coverage_gap = !report.unreached_stops.is_empty()
        || !report.unchosen_options.is_empty()
        || !report.unreached_labels.is_empty()
        || report.truncated;
    let failed = !report.errors.is_empty()
        || !report.unanswered.is_empty()
        || !golden.mismatched.is_empty()
        || !golden.stale.is_empty()
        || (opts.require_coverage && coverage_gap);

    if opts.json {
        let doc = json_report(&report, &golden);
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&doc).expect("JSON can be written")
        );
        mismatch_details(loc, &golden, &dir, err);
    } else {
        human_report(loc, opts, &report, &golden, &dir, &files, out);
        mismatch_details(loc, &golden, &dir, out);
    }
    i32::from(failed)
}

/// The golden files whose replay differs, with the first differing line and its surroundings.
fn mismatch_details(loc: &Localizer, golden: &GoldenResult, dir: &Path, w: &mut dyn Write) {
    for (name, diff) in &golden.mismatched {
        let file = golden_path(dir, name).display().to_string();
        let _ = writeln!(
            w,
            "{}",
            loc.fmt("test-golden-mismatch", Some(&one("file", file)))
        );
        let _ = write!(w, "{diff}");
    }
}

fn human_report<O: Write>(
    loc: &Localizer,
    opts: &Options,
    report: &ExploreReport,
    golden: &GoldenResult,
    dir: &Path,
    files: &[SourceFile],
    out: &mut O,
) {
    let say = |out: &mut O, key: &str, args: Option<&FluentArgs<'_>>| {
        let _ = writeln!(out, "{}", loc.fmt(key, args));
    };
    say(
        out,
        "test-states",
        Some(&one("n", report.states.to_string())),
    );
    say(
        out,
        "test-paths",
        Some(&one("n", report.paths.len().to_string())),
    );
    if report.truncated {
        say(
            out,
            "test-truncated",
            Some(&one("max", opts.max_states.to_string())),
        );
    }
    for e in &report.errors {
        let mut a = one("seed", e.seed.to_string());
        a.set("answers", answers_text(loc, &e.steps));
        say(out, "test-error-path", Some(&a));
        let file = modules::path_of(files, e.error.module.as_deref().unwrap_or(""));
        let _ = writeln!(
            out,
            "  {}",
            error_line(loc, &file, &vm_error_message(loc, &e.error))
        );
    }
    for s in &report.unanswered {
        let mut a = one("seed", s.seed.to_string());
        a.set("answers", answers_text(loc, &s.steps));
        a.set("stop", s.stop.clone());
        a.set("command", s.command.clone());
        say(out, "test-stuck", Some(&a));
    }
    for (key, list) in [
        ("test-unreached-stops", &report.unreached_stops),
        ("test-unchosen-options", &report.unchosen_options),
        ("test-unreached-labels", &report.unreached_labels),
    ] {
        if list.is_empty() {
            continue;
        }
        say(out, key, None);
        for item in list {
            let _ = writeln!(out, "  {item}");
        }
    }
    if golden.missing_dir {
        say(
            out,
            "test-golden-none",
            Some(&one("dir", dir.display().to_string())),
        );
    }
    if opts.update {
        let mut a = one("written", golden.written.len().to_string());
        a.set("removed", golden.removed.len().to_string());
        a.set("dir", dir.display().to_string());
        say(out, "test-golden-written", Some(&a));
    }
    for (key, list) in [
        ("test-golden-stale", &golden.stale),
        ("test-golden-new", &golden.new),
        ("test-golden-unexplored", &golden.unexplored),
    ] {
        for name in list {
            say(out, key, Some(&one("name", name.clone())));
        }
    }
}

fn json_report(report: &ExploreReport, golden: &GoldenResult) -> Json {
    let paths: Vec<Json> = report
        .paths
        .iter()
        .map(|p| json!({ "name": p.name, "seed": p.seed, "answers": answer_lines(&p.steps) }))
        .collect();
    let errors: Vec<Json> = report
        .errors
        .iter()
        .map(|e| {
            let (code, args) = error_code_args(&e.error.kind);
            json!({
                "seed": e.seed,
                "answers": answer_lines(&e.steps),
                "code": code,
                "args": args,
                "module": e.error.module,
                "label": e.error.label,
                "line": e.error.line,
            })
        })
        .collect();
    let unanswered: Vec<Json> = report
        .unanswered
        .iter()
        .map(|s| {
            json!({
                "seed": s.seed,
                "answers": answer_lines(&s.steps),
                "stop": s.stop,
                "command": s.command,
            })
        })
        .collect();
    json!({
        "states": report.states,
        "truncated": report.truncated,
        "paths": paths,
        "errors": errors,
        "unanswered": unanswered,
        "unreached_stops": report.unreached_stops,
        "unchosen_options": report.unchosen_options,
        "unreached_labels": report.unreached_labels,
        "golden": {
            "mismatched": golden.mismatched.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            "stale": golden.stale,
            "new": golden.new,
            "unexplored": golden.unexplored,
            "written": golden.written,
            "removed": golden.removed,
        },
    })
}
