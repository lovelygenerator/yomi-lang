//! Display language

use super::*;

#[test]
fn diagnostics_errors_and_help_appear_in_japanese_display() {
    let path = fixture(GAME);
    let o = yomi(
        &["--lang", "ja", "repl", path.to_str().unwrap()],
        "gld\nlet xs = [1]\nxs[3]\n:foo\n:help\n:save a\n",
    );
    assert_eq!(o.code, 0);
    let err: Vec<&str> = o.stderr.lines().collect();
    assert!(err[0].starts_with("<repl>:1:1: エラー: "), "{}", err[0]);
    assert!(err[1].starts_with("<repl>: エラー: "), "{}", err[1]);
    assert!(
        err[2].starts_with("知らないコマンドです: :foo"),
        "{}",
        err[2]
    );
    assert!(
        err[3].starts_with(":save は REPL の中では使えません"),
        "{}",
        err[3]
    );
    assert!(o.stdout.contains(":help            コマンドの一覧を出す"));
}

#[test]
fn help_describes_repl_in_en_and_ja() {
    for (lang, word) in [("en", "REPL"), ("ja", "REPL")] {
        let o = yomi(&["--lang", lang, "repl", "--help"], "");
        assert_eq!(o.code, 0);
        assert!(o.stdout.contains(word), "{}", o.stdout);
        assert!(o.stdout.contains("--persistent"));
    }
    let o = yomi(&["--lang", "en", "--help"], "");
    assert!(o.stdout.contains("repl"), "{}", o.stdout);
}
