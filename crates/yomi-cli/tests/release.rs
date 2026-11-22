//! `yomi release` (section 6.16 of `spec/host/06-cli.md`).

use std::process::{Command, Output};

const GOOD: &str = "save var hp: int = 1\nlabel start {\n    one #a1\n    end\n}\n";

fn yomi(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .current_dir(dir)
        .env("YOMI_CLI_LANG", "en")
        .output()
        .expect("yomi を起動できる")
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("UTF-8")
}

#[test]
fn release_freezes_when_every_check_passes_and_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), GOOD).unwrap();
    let out = yomi(dir.path(), &["release", "--dry-run"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(!dir.path().join("schema").exists());
    let out = yomi(dir.path(), &["release"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(dir.path().join("schema/v1.yomi").exists());
    assert!(text(&out.stdout).contains("ready for release"));
    // running again is fine (the frozen schema is up to date)
    assert_eq!(yomi(dir.path(), &["release"]).status.code(), Some(0));
}

#[test]
fn release_stops_without_writing_on_missing_ids_or_formatting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.yomi"),
        "label start {\n    one\n    end\n}\n",
    )
    .unwrap();
    let out = yomi(dir.path(), &["release"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.path().join("schema").exists());
    std::fs::write(
        dir.path().join("main.yomi"),
        "label start {\n  one #a1\n  end\n}\n",
    )
    .unwrap();
    let out = yomi(dir.path(), &["release"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("needs formatting"));
    assert!(!dir.path().join("schema").exists());
}

#[test]
fn release_checks_that_saves_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), GOOD).unwrap();
    std::fs::create_dir(dir.path().join("saves")).unwrap();
    std::fs::write(dir.path().join("saves/bad.sav"), b"garbage").unwrap();
    let out = yomi(dir.path(), &["release", "--saves", "saves"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("bad.sav"));
    assert!(!dir.path().join("schema").exists());
}

#[test]
fn saves_and_decls_cannot_be_used_together() {
    let dir = tempfile::tempdir().unwrap();
    let out = yomi(dir.path(), &["release", "--saves", "x", "--decls", "y"]);
    assert_eq!(out.status.code(), Some(2));
}
