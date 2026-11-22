//! Checks that the CLI spec (`spec/host/06-cli.md`) matches the options of the actual `yomi`.
//!
//! - The set of options written in each subcommand's usage code block (those starting with `yomi …`) matches the options of
//!   `yomi <subcommand> --help` (excluding `--help` and `--lang`). Both something in the spec but not implemented, and
//!   something implemented but not in the spec, fail.
//! - All subcommands of `yomi --help` appear in the spec.
//!
//! The spec file is relative to the workspace root (`CARGO_MANIFEST_DIR/../../spec/host/06-cli.md`).
//! If the file is missing (such as a public repository exported with only the spec), it finishes without checking
//! anything.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn spec_text() -> Option<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "spec",
        "host",
        "06-cli.md",
    ]
    .iter()
    .collect();
    std::fs::read_to_string(path).ok()
}

/// The standard output of `yomi <args>` (with English display).
fn yomi_stdout(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .env("YOMI_CLI_LANG", "en")
        .output()
        .expect("yomi を起動できる");
    assert!(out.status.success(), "yomi {args:?} が失敗した");
    String::from_utf8(out.stdout).expect("UTF-8")
}

/// The names of long options in the `Options:` section of `--help` (excluding `--help` and `--lang`).
fn help_options(args: &[&str]) -> BTreeSet<String> {
    let mut full = args.to_vec();
    full.push("--help");
    let help = yomi_stdout(&full);
    let mut in_options = false;
    let mut found = BTreeSet::new();
    for line in help.lines() {
        if line.trim_end() == "Options:" {
            in_options = true;
            continue;
        }
        if !in_options {
            continue;
        }
        // Look only at lines starting with `-`, to exclude wrapped description lines
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        if let Some(name) = trimmed
            .split(|c: char| c == ',' || c.is_whitespace())
            .find_map(|w| w.strip_prefix("--"))
            .filter(|n| !n.is_empty())
        {
            found.insert(name.to_string());
        }
    }
    found.remove("help");
    found.remove("lang");
    found
}

/// Returns the usage code blocks (those starting with `yomi subcommand …`) as the sequence of subcommands → the text of
/// the block.
///
/// The sequence of subcommands is the words after `yomi` that do not start with `-`, `<` or `[` (`run`, `texts check`
/// and so on).
fn usage_blocks(spec: &str) -> Vec<(Vec<String>, String)> {
    let mut blocks = Vec::new();
    let mut lines = spec.lines();
    while let Some(line) = lines.next() {
        if !line.starts_with("```") {
            continue;
        }
        let body: Vec<&str> = lines
            .by_ref()
            .take_while(|l| !l.starts_with("```"))
            .collect();
        let Some(first) = body.first() else { continue };
        let Some(rest) = first.strip_prefix("yomi ") else {
            continue;
        };
        let names: Vec<String> = rest
            .split_whitespace()
            .take_while(|w| w.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
            .map(str::to_string)
            .collect();
        if names.is_empty() {
            continue;
        }
        blocks.push((names, body.join("\n")));
    }
    blocks
}

/// Collects the `--name`s in a block.
fn block_options(block: &str) -> BTreeSet<String> {
    block
        .split(|c: char| c.is_whitespace() || c == '[' || c == ']')
        .filter_map(|w| w.strip_prefix("--"))
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the subcommand is in this build (`lsp` and `mcp` can be removed with cargo features).
fn built_in(command: &str) -> bool {
    let optional = [
        ("lsp", cfg!(feature = "lsp")),
        ("mcp", cfg!(feature = "mcp")),
    ];
    optional
        .iter()
        .all(|&(name, enabled)| name != command || enabled)
}

#[test]
fn spec_usage_options_match_actual_help() {
    let Some(spec) = spec_text() else { return };
    let blocks = usage_blocks(&spec);
    let covered: BTreeSet<String> = blocks.iter().map(|(names, _)| names.join(" ")).collect();
    for expected in [
        "run",
        "check",
        "assign-ids",
        "texts check",
        "texts template",
        "decls",
        "codes",
        "debug",
        "dap",
        "version",
    ] {
        assert!(
            covered.contains(expected),
            "仕様書に `yomi {expected}` の使い方のブロックがない"
        );
    }
    for (names, block) in blocks.iter().filter(|(names, _)| built_in(&names[0])) {
        let args: Vec<&str> = names.iter().map(String::as_str).collect();
        let actual = help_options(&args);
        let written = block_options(block);
        assert_eq!(
            written,
            actual,
            "`yomi {}` のオプションが、仕様書と --help で食い違う（仕様書: {written:?}、実装: {actual:?}）",
            names.join(" ")
        );
    }
}

#[test]
fn all_options_and_subcommands_appear_in_spec_text() {
    let Some(spec) = spec_text() else { return };
    // Top-level subcommands
    let top = yomi_stdout(&["--help"]);
    let commands: Vec<&str> = top
        .lines()
        .skip_while(|l| l.trim_end() != "Commands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| *name != "help")
        .collect();
    assert!(
        commands.len() >= 7,
        "サブコマンドを読めていない: {commands:?}"
    );
    for name in &commands {
        assert!(
            spec.contains(&format!("yomi {name}")),
            "仕様書に `yomi {name}` が出てこない"
        );
    }
    // The display language option is also written
    assert!(spec.contains("--lang"));
    // Under `texts`
    for sub in ["check", "template"] {
        for option in help_options(&["texts", sub]) {
            assert!(
                spec.contains(&format!("`--{option}")),
                "仕様書に `--{option}`（texts {sub}）が出てこない"
            );
        }
    }
}
