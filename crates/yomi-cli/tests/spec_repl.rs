//! Checks that the examples and tables of the REPL spec (`spec/host/07-repl.md`) match the actual `yomi repl`.
//!
//! - The script `game.yomi` of 7.1 passes `yomi check`.
//! - The interaction examples in the spec (blocks containing lines starting with `yomi> `) are fed to
//!   `yomi repl game.yomi` exactly as their input, and the output lines written in the example appear on standard output
//!   (lines starting with `<repl>` on standard error), in the order written.
//! - The table of interactive commands in 7.6 matches the list of `:help` (`:repl` cannot be used inside the REPL, so it
//!   is not in the list).
//!
//! If the spec file is missing (such as a public repository exported with only the spec), it finishes without checking
//! anything.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn spec_text() -> Option<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "spec",
        "host",
        "07-repl.md",
    ]
    .iter()
    .collect();
    std::fs::read_to_string(path).ok()
}

/// Returns only the blocks without a language, skipping blocks with a language.
fn script_and_transcripts(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix("```") else {
            continue;
        };
        let mut body = Vec::new();
        for l in lines.by_ref() {
            if l.starts_with("```") {
                break;
            }
            body.push(l);
        }
        if rest.trim().is_empty() {
            blocks.push(body.join("\n"));
        }
    }
    blocks
}

struct Output {
    stdout: String,
    stderr: String,
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yomi-spec-repl-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn yomi(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .env_remove("YOMI_CLI_LANG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    Output {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// Each line of `expected` appears in `actual` in this order (other lines may come in between).
fn is_subsequence(expected: &[String], actual: &str) -> Result<(), String> {
    let mut lines = actual.lines();
    for want in expected {
        if !lines.by_ref().any(|l| l == want) {
            return Err(want.clone());
        }
    }
    Ok(())
}

/// Extracts, from an interaction example, the standard input (lines of `yomi> ` and `...> `, empty lines, and numbers
/// answering choices) and the expected output lines.
fn split_transcript(block: &str) -> (String, Vec<String>, Vec<String>) {
    let mut stdin = String::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut prev = "";
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("yomi> ") {
            stdin.push_str(rest);
            stdin.push('\n');
        } else if let Some(rest) = line.strip_prefix("...>") {
            stdin.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            stdin.push('\n');
        } else if line.trim().is_empty() {
            // Input of only Enter while waiting at a stop point
            stdin.push('\n');
        } else if line.bytes().all(|b| b.is_ascii_digit())
            && prev
                .split_once(": ")
                .is_some_and(|(n, _)| n.bytes().all(|b| b.is_ascii_digit()))
        {
            // Answers to choices
            stdin.push_str(line);
            stdin.push('\n');
        } else if line.starts_with("<repl>") {
            err.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
        prev = line;
    }
    (stdin, out, err)
}

#[test]
fn spec_script_and_interaction_examples_match_actual_yomi_repl() {
    let Some(spec) = spec_text() else {
        return;
    };
    let blocks = script_and_transcripts(&spec);
    let game = blocks
        .iter()
        .find(|b| b.contains("label start") && b.contains("label shop"))
        .expect("7.1 の台本 game.yomi がある");
    let dir = scratch("game");
    let path = dir.join("game.yomi");
    std::fs::write(&path, format!("{game}\n")).unwrap();
    let path = path.to_str().unwrap();

    let checked = yomi(&["--lang", "en", "check", path], "");
    assert!(
        checked.stderr.is_empty(),
        "仕様書の台本が検査を通らない: {}",
        checked.stderr
    );

    let transcripts: Vec<&String> = blocks.iter().filter(|b| b.contains("yomi> ")).collect();
    assert!(
        transcripts.len() >= 3,
        "対話の例が 3 つ以上ある（7.4・7.5.2・7.7）: {}",
        transcripts.len()
    );
    for block in transcripts {
        let (stdin, want_out, want_err) = split_transcript(block);
        let got = yomi(&["--lang", "en", "repl", path, "--seed", "1"], &stdin);
        if let Err(line) = is_subsequence(&want_out, &got.stdout) {
            panic!(
                "仕様書の例の出力の行が、標準出力にない: {line:?}\n入力:\n{stdin}\n標準出力:\n{}\n標準エラー:\n{}",
                got.stdout, got.stderr
            );
        }
        if let Err(line) = is_subsequence(&want_err, &got.stderr) {
            panic!(
                "仕様書の例のエラーの行が、標準エラーにない: {line:?}\n入力:\n{stdin}\n標準出力:\n{}\n標準エラー:\n{}",
                got.stdout, got.stderr
            );
        }
    }
}

#[test]
fn spec_interactive_command_table_matches_help_list() {
    let Some(spec) = spec_text() else {
        return;
    };
    // The first column of the table in 7.6 (`:name` or `:name <args>`)
    let start = spec.find("## 7.6 ").expect("7.6 がある");
    let section = &spec[start..];
    let end = section.find("### `yomi run`").expect("7.6 の表の終わり");
    let mut documented: Vec<String> = Vec::new();
    for line in section[..end].lines() {
        let Some(rest) = line.strip_prefix("| `:") else {
            continue;
        };
        let name = rest.split(['`', ' ', '（']).next().unwrap();
        documented.push(format!(":{name}"));
    }
    assert!(documented.len() >= 10, "表を読めた: {documented:?}");

    let got = yomi(&["--lang", "en", "repl"], ":help\n");
    let mut listed: Vec<String> = got
        .stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| w.starts_with(':'))
        .map(|w| w.to_string())
        .collect();
    listed.sort();
    // `:repl` can be used only inside `yomi run`, so it does not appear in the REPL's `:help`
    let mut want: Vec<String> = documented.into_iter().filter(|c| c != ":repl").collect();
    want.sort();
    assert_eq!(listed, want, "`:help` の一覧と、7.6 の表の対話コマンド");
}
