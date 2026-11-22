//! Integration tests that run `yomi run` as an actual command.

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
/// (because `yomi run` reads all `.yomi` in the same directory as the entry file as modules).
fn fixture(name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let dir = std::env::temp_dir().join(format!(
        "yomi-cli-fixture-{}-{}",
        std::process::id(),
        name.trim_end_matches(".yomi")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    // Tests using the same sample run in parallel. Only one of them may put the file there: replacing it while another
    // test's `yomi` lists the directory makes the entry file vanish for a moment (seen on Windows)
    static CREATING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CREATING.lock().unwrap_or_else(|e| e.into_inner());
    // Rename from a temporary file so that readers do not see a partially written file
    if !to.exists() {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp =
            std::env::temp_dir().join(format!("yomi-cli-fixture-tmp-{}-{n}", std::process::id()));
        std::fs::copy(&from, &tmp).unwrap();
        std::fs::rename(&tmp, &to).unwrap();
    }
    to
}

/// A working directory for each test (holds save files).
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yomi-cli-test-{}-{test}", std::process::id()));
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    Output {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

/// Runs a script file with a given language.
fn run(lang: &str, script: &str, extra: &[&str], stdin: &str) -> Output {
    let path = fixture(script);
    let mut args = vec!["--lang", lang, "run", path.to_str().unwrap()];
    args.extend_from_slice(extra);
    yomi(&args, stdin)
}

mod assign_ids;
mod auto;
mod basic;
mod dap_cmd;
mod debug_cmd;
mod decls;
mod event;
mod multi_module;
mod persistence;
mod reload;
mod test_cmd;
mod texts;

use decls::*;
use texts::*;
