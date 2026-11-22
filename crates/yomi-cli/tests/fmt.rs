//! `yomi fmt` (section 6.13 of `spec/host/06-cli.md`).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const MESSY: &str = "label start {\n  jump  start\n}\n";
const TIDY: &str = "label start {\n    jump start\n}\n";

fn yomi(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
        .args(args)
        .env("YOMI_CLI_LANG", "en")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("yomi を起動できる");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .expect("標準入力")
            .write_all(text.as_bytes())
            .expect("書ける");
    }
    child.wait_with_output().expect("終わる")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 のパス")
}

#[test]
fn check_lists_files_without_writing_and_write_makes_them_clean() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.yomi");
    std::fs::write(&file, MESSY).unwrap();

    let out = yomi(&["fmt", "--check", path(&file)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("main.yomi: needs formatting"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), MESSY);

    let out = yomi(&["fmt", path(&file)], None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), TIDY);

    let out = yomi(&["fmt", "--check", path(&file)], None);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn stdout_prints_the_result_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.yomi");
    std::fs::write(&file, MESSY).unwrap();
    let out = yomi(&["fmt", "--stdout", path(&file)], None);
    assert_eq!(text(&out.stdout), TIDY);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), MESSY);
}

#[test]
fn stdin_is_formatted_as_the_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.yomi");
    std::fs::write(&file, TIDY).unwrap();
    let out = yomi(&["fmt", "-", "--stdin-path", path(&file)], Some(MESSY));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), TIDY);
}

#[test]
fn nothing_is_written_when_one_file_has_a_syntax_error() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("a.yomi");
    let bad = dir.path().join("b.yomi");
    std::fs::write(&good, MESSY).unwrap();
    std::fs::write(&bad, "label start {\n    let x = (1 +\n}\n").unwrap();
    let out = yomi(&["fmt", path(dir.path())], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("b.yomi"));
    assert_eq!(std::fs::read_to_string(&good).unwrap(), MESSY);
}

#[test]
fn frozen_schema_files_are_not_formatted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), TIDY).unwrap();
    std::fs::create_dir(dir.path().join("schema")).unwrap();
    let frozen = dir.path().join("schema").join("v1.yomi");
    std::fs::write(&frozen, "not   formatted\n").unwrap();
    let out = yomi(&["fmt", path(dir.path())], None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(&frozen).unwrap(),
        "not   formatted\n"
    );
}
