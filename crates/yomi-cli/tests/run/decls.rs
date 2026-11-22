//! Declaration files (yomi decls, yomi check)

use super::*;

/// Writes a file to the working directory.
pub(super) fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn decls_prints_builtin_registry_stably_and_matches_golden() {
    let a = yomi(&["decls"], "");
    let b = yomi(&["decls"], "");
    assert_eq!(a.code, 0);
    assert_eq!(a.stderr, "");
    assert_eq!(a.stdout, b.stdout);
    let golden = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/std.decls.json"),
    )
    .unwrap();
    assert_eq!(
        a.stdout, golden,
        "tests/fixtures/std.decls.json と違う（意図した変更なら書き直す）"
    );
    assert!(a.stdout.contains("\"format\": \"yomi-decls\""));
}

#[test]
fn check_validates_without_running_and_prints_diagnostics_to_stderr() {
    let dir = scratch("check_plain");
    let ok = write_file(&dir, "ok.yomi", "label start {\n  おはよう\n}\n");
    let o = yomi(&["--lang", "ja", "check", ok.to_str().unwrap()], "");
    assert_eq!(
        (o.code, o.stdout.as_str(), o.stderr.as_str()),
        (0, "問題は見つかりませんでした\n", "")
    );
    let dir = scratch("check_bad");
    let bad = write_file(
        &dir,
        "bad.yomi",
        "import nothing\nlabel start {\n  x = 1\n}\n",
    );
    let o = yomi(&["--lang", "en", "check", bad.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("bad.yomi:1:") && o.stderr.contains("error"),
        "{}",
        o.stderr
    );
    assert!(o.stdout.contains("errors"), "{}", o.stdout);
}

#[test]
fn check_with_decls_gives_same_diagnostics_without_host_rust() {
    let dir = scratch("check_decls");
    let decls = write_file(&dir, "std.decls.json", &yomi(&["decls"], "").stdout);
    let script = write_file(
        &dir,
        "a.yomi",
        "import std::audio\nlabel start {\n  @audio.play(\"bgm\", \"x\", fade: 1)\n  @audio.nope()\n  @audio.se(1)\n}\n",
    );
    let with = yomi(
        &[
            "--lang",
            "en",
            "check",
            script.to_str().unwrap(),
            "--decls",
            decls.to_str().unwrap(),
        ],
        "",
    );
    let without = yomi(&["--lang", "en", "check", script.to_str().unwrap()], "");
    assert_eq!(with.code, 1);
    assert_eq!(with.stderr, without.stderr);
    assert_eq!(with.stdout, without.stdout);
    assert!(with.stderr.contains("audio.nope"), "{}", with.stderr);
}

#[test]
fn check_with_unreadable_decls_reports_in_ja_and_en_and_returns_1() {
    let dir = scratch("check_decls_bad");
    let script = write_file(&dir, "a.yomi", "label start {\n  あ\n}\n");
    for (content, ja, en) in [
        ("{", "JSON として読めません", "not valid JSON"),
        (
            "{\"format\":\"x\",\"version\":1}",
            "宣言ファイルではありません",
            "not a yomi declaration file",
        ),
        (
            "{\"format\":\"yomi-decls\",\"version\":9}",
            "読めない版数",
            "unsupported declaration file version 9",
        ),
        (
            "{\"format\":\"yomi-decls\",\"version\":1,\"commands\":[{\"name\":\"a.b\",\"kind\":\"stop\",\"params\":[{\"type\":\"zzz\"}]}]}",
            "commands[0].params[0].type",
            "invalid type name: zzz",
        ),
    ] {
        let decls = write_file(&dir, "d.json", content);
        for (lang, want) in [("ja", ja), ("en", en)] {
            let o = yomi(
                &[
                    "--lang",
                    lang,
                    "check",
                    script.to_str().unwrap(),
                    "--decls",
                    decls.to_str().unwrap(),
                ],
                "",
            );
            assert_eq!(o.code, 1, "{content}");
            assert!(o.stderr.contains(want), "{lang} {content}: {}", o.stderr);
            assert!(o.stderr.contains("d.json"), "{}", o.stderr);
        }
    }
    let o = yomi(
        &[
            "check",
            script.to_str().unwrap(),
            "--decls",
            dir.join("none.json").to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(o.code, 1);
}
