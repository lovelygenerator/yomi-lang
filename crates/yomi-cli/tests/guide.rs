//! Checks the code in the guide (`guide/*.md`) so that it cannot drift from the implementation.
//!
//! - A `yomi` block is a complete script, and `yomi check` accepts it without warnings.
//! - A `rust` block is a verbatim excerpt of the host example (`crates/yomi-std/examples/approval.rs`); indentation and
//!   blank lines at the ends are ignored.
//! - A link to another chapter points at a file that exists.
//!
//! If `guide/` is missing (such as when only the package is extracted), it finishes without checking anything.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn chapters() -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir(root().join("guide")) else {
        return Vec::new();
    };
    let mut out: Vec<_> = dir
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect();
    out.sort();
    out
}

/// The fenced blocks of a chapter: (language, body).
fn blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut open: Option<(String, Vec<&str>)> = None;
    for line in text.lines() {
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(lang)) => open = Some((lang.trim().to_string(), Vec::new())),
            (Some(_), Some("")) => {
                let (lang, body) = open.take().unwrap();
                out.push((lang, body.join("\n")));
            }
            (Some((_, body)), _) => body.push(line),
            _ => {}
        }
    }
    assert!(open.is_none(), "a code block is not closed");
    out
}

fn normalized(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).collect();
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[test]
fn yomi_blocks_pass_check() {
    let dir = tempfile::tempdir().unwrap();
    for (c, (name, text)) in chapters().into_iter().enumerate() {
        for (i, (lang, body)) in blocks(&text).iter().enumerate() {
            if lang != "yomi" {
                continue;
            }
            // One directory per block, with the entry file named `main` (its `start` label is the start label)
            let sub = dir.path().join(format!("ch{c}b{i}"));
            std::fs::create_dir(&sub).unwrap();
            let file = sub.join("main.yomi");
            std::fs::write(&file, body).unwrap();
            let out = Command::new(env!("CARGO_BIN_EXE_yomi"))
                .args(["check", file.to_str().unwrap()])
                .env("YOMI_CLI_LANG", "en")
                .output()
                .unwrap();
            assert!(
                out.status.success() && out.stderr.is_empty(),
                "{name}: block {} does not pass `yomi check` cleanly:\n{}",
                i + 1,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn rust_blocks_are_excerpts_of_the_host_example() {
    let chapters = chapters();
    if chapters.is_empty() {
        return;
    }
    let source =
        std::fs::read_to_string(root().join("crates/yomi-std/examples/approval.rs")).unwrap();
    let source = normalized(&source);
    for (name, text) in chapters {
        for (i, (lang, body)) in blocks(&text).iter().enumerate() {
            if lang != "rust" {
                continue;
            }
            let want = normalized(body);
            assert!(!want.is_empty(), "{name}: block {} is empty", i + 1);
            assert!(
                source.windows(want.len()).any(|w| w == want.as_slice()),
                "{name}: rust block {} is not an excerpt of examples/approval.rs:\n{body}",
                i + 1
            );
        }
    }
}

#[test]
fn links_between_chapters_exist() {
    for (name, text) in chapters() {
        for part in text.split("](").skip(1) {
            let target = part.split(')').next().unwrap();
            if target.starts_with("http") || target.starts_with('#') {
                continue;
            }
            let file = target.split('#').next().unwrap();
            assert!(
                root().join("guide").join(file).exists(),
                "{name}: link target {target} does not exist"
            );
        }
    }
}
