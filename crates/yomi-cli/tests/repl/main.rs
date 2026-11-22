//! Integration tests that run `yomi repl` and `:repl` of `yomi run` as actual commands (spec/host/07-repl.md).
//!
//! Standard input is fed through a pipe (it is not a terminal, so no prompts are shown).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The result of running.
struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

/// Copies a sample script to a directory separate from other samples and returns its path
/// (because all `.yomi` in the same directory as the entry file are read as modules).
fn fixture(name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let dir = std::env::temp_dir().join(format!(
        "yomi-cli-repl-fixture-{}-{}",
        std::process::id(),
        name.trim_end_matches(".yomi")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    // Tests that use the same sample run in parallel. Prepare samples one at a time so that other tests do not read files
    // being written or renamed (Windows sometimes cannot open a file while it is being renamed)
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !to.exists() {
        std::fs::copy(&from, &to).unwrap();
    }
    to
}

/// A working directory for each test.
fn scratch(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("yomi-cli-repl-test-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `yomi <args>` with standard input.
fn yomi(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .env_remove("YOMI_CLI_LANG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Some commands finish before reading input (argument errors and so on), so write failures are ignored
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    Output {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

/// `yomi repl <script>` (with English display).
fn repl(script: &str, stdin: &str) -> Output {
    let path = fixture(script);
    yomi(
        &[
            "--lang",
            "en",
            "repl",
            path.to_str().unwrap(),
            "--seed",
            "1",
        ],
        stdin,
    )
}

/// `yomi repl` without a script (with English display).
fn repl_empty(stdin: &str) -> Output {
    yomi(&["--lang", "en", "repl", "--seed", "1"], stdin)
}

/// `yomi run <script> …` (with English display).
fn run(script: &str, extra: &[&str], stdin: &str) -> Output {
    let path = fixture(script);
    let mut args = vec!["--lang", "en", "run", path.to_str().unwrap(), "--seed", "1"];
    args.extend_from_slice(extra);
    yomi(&args, stdin)
}

const GAME: &str = "repl_game.yomi";

mod commands;
mod errors;
mod expressions;
mod language;
mod run_repl;
mod script_lines;
mod startup;
