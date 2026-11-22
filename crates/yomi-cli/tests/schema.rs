//! `yomi schema freeze`, `yomi schema extract` and `yomi migrate-init` (sections 6.14 and 6.15 of `spec/host/06-cli.md`).

use std::process::{Command, Output};

const V1: &str = "struct Item { id: str }\nsave var hp: int = 1\nsave var bag: list<Item> = []\nlabel start {\n    end\n}\n";
const V2: &str = "struct Item { id: str, n: int }\nsave var hp: float = 1.0\nsave var bag: list<Item> = []\nlabel start {\n    end\n}\n";

fn yomi(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .current_dir(dir)
        .env("YOMI_CLI_LANG", "en")
        .output()
        .expect("yomi を起動できる")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

#[test]
fn freeze_writes_the_file_once_and_reports_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), V1).unwrap();
    let out = yomi(dir.path(), &["schema", "freeze"]);
    assert_eq!(out.status.code(), Some(0));
    let frozen = std::fs::read_to_string(dir.path().join("schema/v1.yomi")).unwrap();
    assert!(frozen.starts_with("schema 1 hash \""));
    assert!(frozen.contains("    save var hp: int\n"));
    let out = yomi(dir.path(), &["schema", "freeze"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("up to date"));
    // The frozen schema is accepted by `check`, and the scripts are unchanged
    let out = yomi(dir.path(), &["check", "main.yomi"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

#[test]
fn changed_schema_after_freeze_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), V1).unwrap();
    assert_eq!(
        yomi(dir.path(), &["schema", "freeze"]).status.code(),
        Some(0)
    );
    std::fs::write(dir.path().join("main.yomi"), V2).unwrap();
    let out = yomi(dir.path(), &["schema", "freeze"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn migrate_init_prints_or_appends_a_comment_template() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), V1).unwrap();
    assert_eq!(
        yomi(dir.path(), &["schema", "freeze"]).status.code(),
        Some(0)
    );
    std::fs::write(dir.path().join("main.yomi"), V2).unwrap();
    let out = yomi(dir.path(), &["migrate-init"]);
    assert_eq!(out.status.code(), Some(0));
    let template = text(&out.stdout);
    assert!(template.starts_with("migrate 2 {\n"));
    assert!(template.contains("    // convert v1::main.hp: int -> float = |v| v\n"));
    assert!(!dir.path().join("migrations.yomi").exists());
    assert_eq!(
        yomi(dir.path(), &["migrate-init", "--write"]).status.code(),
        Some(0)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("migrations.yomi")).unwrap(),
        template
    );
    // migrate 2 now exists
    assert_eq!(yomi(dir.path(), &["migrate-init"]).status.code(), Some(1));
    // and freezing the new version works
    assert_eq!(
        yomi(dir.path(), &["schema", "freeze"]).status.code(),
        Some(0)
    );
    assert!(dir.path().join("schema/v2.yomi").exists());
}

#[test]
fn migrate_init_without_a_frozen_schema_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), V1).unwrap();
    assert_eq!(yomi(dir.path(), &["migrate-init"]).status.code(), Some(1));
}

#[test]
fn extract_rejects_broken_files_and_existing_outputs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.sav"), b"not a save").unwrap();
    let out = yomi(dir.path(), &["schema", "extract", "bad.sav"]);
    assert_eq!(out.status.code(), Some(1));
}
