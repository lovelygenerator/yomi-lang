//! (b)(c)(f) Checks, IDs, declaration files

use super::*;

#[test]
fn check_prints_no_warnings_or_diagnostics() {
    let dir = scratch("check");
    let main = main_yomi();
    let o = yomi(&dir, &["--lang", "ja", "check", &main], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "問題は見つかりませんでした\n");
    assert_eq!(o.stderr, "");
}

#[test]
fn assign_ids_check_succeeds_when_all_lines_have_ids() {
    let dir = scratch("assign");
    let root = game_dir();
    let o = yomi(
        &dir,
        &[
            "--lang",
            "ja",
            "assign-ids",
            "--check",
            root.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "");
}

#[test]
fn copy_without_once_ids_gets_them_from_assign_ids_and_japanese_play_is_same() {
    let dir = scratch("once_strip");
    let copy = dir.join("game");
    std::fs::create_dir_all(&copy).unwrap();
    let mut stripped = 0;
    for entry in std::fs::read_dir(game_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "yomi") {
            continue;
        }
        let mut src = std::fs::read_to_string(&path).unwrap();
        for id in ["archery", "elder", "fishing", "amulet"] {
            let before = src.clone();
            src = src.replace(&format!(" #{id}"), "");
            stripped += usize::from(src != before);
        }
        std::fs::write(copy.join(path.file_name().unwrap()), src).unwrap();
    }
    assert_eq!(stripped, 4);
    let root = copy.to_str().unwrap();
    // Removing them makes it fail to compile (`once` options need IDs)
    let o = yomi(
        &dir,
        &["--lang", "ja", "check", &format!("{root}/main.yomi")],
        "",
    );
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert!(o.stderr.contains("once"), "{}", o.stderr);
    // `--check` also counts 4 and returns 1
    let o = yomi(&dir, &["--lang", "ja", "assign-ids", "--check", root], "");
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert_eq!(o.stdout, "安定 ID のない行が 4 件あります（2 ファイル）\n");
    // Adding them (with a fixed seed) makes compilation pass, and the second time changes nothing
    let o = yomi(
        &dir,
        &["--lang", "ja", "assign-ids", root, "--seed", "20261118"],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "安定 ID を 4 件付けました（2 ファイル）\n");
    let main = format!("{root}/main.yomi");
    let o = yomi(&dir, &["--lang", "ja", "check", &main], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "問題は見つかりませんでした\n");
    let o = yomi(
        &dir,
        &["--lang", "ja", "assign-ids", root, "--seed", "1"],
        "",
    );
    assert_eq!(o.stdout, "付ける ID はありません\n");
    // Even if the ID values change, the Japanese play (including how `once` options disappear) gives the same output
    for play in PLAYS.iter().filter(|p| p.after.is_none()) {
        let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
        let o = yomi(&dir, &run_args(&main, "", None), &input);
        assert_eq!(o.code, 0, "{}: {}", play.name, o.stderr);
        let expected = std::fs::read_to_string(playthrough_file(play.name, "out")).unwrap();
        assert_eq!(o.stdout, expected, "{}", play.name);
    }
}

#[test]
fn check_passes_with_declaration_file_alone() {
    let dir = scratch("decls");
    let decls = yomi(&dir, &["decls"], "");
    assert_eq!(decls.code, 0);
    std::fs::write(dir.join("std.decls.json"), &decls.stdout).unwrap();
    let main = main_yomi();
    let o = yomi(
        &dir,
        &["--lang", "ja", "check", &main, "--decls", "std.decls.json"],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "問題は見つかりませんでした\n");
}
