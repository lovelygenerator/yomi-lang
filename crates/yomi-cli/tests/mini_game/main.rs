//! Integration tests of `examples/mini-game` (a sample script that goes through multiple modules).
//!
//! Runs `yomi run` as an actual command and, with fixed input (`playthroughs/*.in`) and a fixed random seed, compares the
//! output up to each ending (`playthroughs/*.out`, and `*.en.out` for English).
//!
//! How the input files are made: `yomi run` reads one line of standard input at each stop point (each line of body), so
//! before the answers to choices and input, as many empty lines as there are bodies are needed. To avoid counting by hand,
//! lines are sent one at a time from `*.answers` (only the answers) to a `yomi run` kept running (the sync signal is
//! `:sync`), making `*.in` with empty lines. If `*.answers` and `*.in` disagree, the test fails.
//!
//! To rewrite the expectations: `BLESS=1 cargo test -p yomi-cli --test mini_game` (after checking the output by eye).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The result of running.
struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

/// One playthrough.
struct Play {
    /// The file name (`playthroughs/<name>.answers` and so on).
    name: &'static str,
    /// A play to do first with the same persistent data file.
    after: Option<&'static str>,
    /// The heading of the ending that appears in the Japanese output.
    ending_ja: &'static str,
    /// The heading of the ending that appears in the English output.
    ending_en: &'static str,
}

/// All plays (those listed in `after` come first).
const PLAYS: &[Play] = &[
    Play {
        name: "end_good",
        after: None,
        ending_ja: "【封印の夜】",
        ending_en: "[The Night of the Seal]",
    },
    Play {
        name: "end_normal",
        after: None,
        ending_ja: "【帰り道】",
        ending_en: "[The Way Back]",
    },
    Play {
        name: "end_lost",
        after: None,
        ending_ja: "【霧の森で】",
        ending_en: "[Lost in the Fog]",
    },
    Play {
        name: "end_promise",
        after: Some("end_good"),
        ending_ja: "【次の依頼も】",
        ending_en: "[The Next Quest, Too]",
    },
];

/// The random seed (fixed for all plays).
const SEED: &str = "7";

fn game_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mini-game")
}

fn main_yomi() -> String {
    game_dir().join("main.yomi").to_str().unwrap().to_string()
}

fn texts_en() -> String {
    game_dir().join("en.texts").to_str().unwrap().to_string()
}

fn playthrough_file(name: &str, ext: &str) -> PathBuf {
    game_dir()
        .join("playthroughs")
        .join(format!("{name}.{ext}"))
}

fn bless() -> bool {
    std::env::var_os("BLESS").is_some_and(|v| v != "0" && !v.is_empty())
}

/// A working directory for each test (holds saves and persistent data; a new one on each call).
fn scratch(test: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("yomi-mini-game-{}-{test}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn yomi_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yomi"));
    cmd.args(args).env_remove("YOMI_CLI_LANG").current_dir(dir);
    cmd
}

/// Runs `yomi <args>` in the working directory with standard input.
fn yomi(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = yomi_command(dir, args)
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

/// The arguments of `yomi run` (Japanese display, fixed seed). `persistent` is a file name inside the working directory.
fn run_args<'a>(main: &'a str, persistent: &'a str, texts: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec!["--lang", "ja", "run", main, "--seed", SEED];
    if !persistent.is_empty() {
        args.extend(["--persistent", persistent]);
    }
    if let Some(t) = texts {
        args.extend(["--texts", t]);
    }
    args
}

mod answers;
mod auto;
mod bench;
mod checks;
mod english;
mod playthrough;
mod repl;
mod save_load;

use answers::*;
