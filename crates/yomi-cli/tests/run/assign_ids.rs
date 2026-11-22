//! yomi assign-ids, yomi codes

use super::*;

pub(super) const AID_SCRIPT: &str = "import std::flow\nidentity 案内人 = \"案内人\"\nlabel start {\n  おはよう\n  案内人「やあ」 #hi\n  @flow.wait(1.0)\n  choice {\n    「はい」 => jump start\n    「いいえ」 #no => jump start\n  }\n}\n";

/// Compares with the ID parts (`#` + 6 alphanumerics) replaced by `#ID`.
pub(super) fn mask_ids(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let ok = b[i] == '#'
            && i > 0
            && b[i - 1] == ' '
            && i + 7 <= b.len()
            && b[i + 1..i + 7]
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && b.get(i + 7).is_none_or(|c| !c.is_ascii_alphanumeric());
        if ok {
            out.push_str("#ID");
            i += 7;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

#[test]
fn assign_ids_writes_missing_ids_and_second_run_changes_nothing() {
    let dir = scratch("aid_basic");
    let path = write_file(&dir, "a.yomi", AID_SCRIPT);
    let p = path.to_str().unwrap();
    let o = yomi(&["--lang", "ja", "assign-ids", p, "--seed", "5"], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "安定 ID を 3 件付けました（1 ファイル）\n");
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        mask_ids(&after),
        "import std::flow\nidentity 案内人 = \"案内人\"\nlabel start {\n  おはよう #ID\n  案内人「やあ」 #hi\n  @flow.wait(1.0) #ID\n  choice {\n    「はい」 #ID => jump start\n    「いいえ」 #no => jump start\n  }\n}\n"
    );
    // Second time: changes nothing
    let o2 = yomi(&["--lang", "ja", "assign-ids", p], "");
    assert_eq!(o2.code, 0);
    assert_eq!(o2.stdout, "付ける ID はありません\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    // The same seed gives the same IDs
    let other = write_file(&dir, "b.yomi", AID_SCRIPT);
    let other_dir = scratch("aid_basic_b");
    let other = other_dir.join(other.file_name().unwrap());
    std::fs::write(&other, AID_SCRIPT).unwrap();
    yomi(&["assign-ids", other.to_str().unwrap(), "--seed", "5"], "");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), after);
    // No temporary files are left
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn assign_ids_keeps_line_endings_and_bom() {
    let dir = scratch("aid_crlf");
    let src =
        "\u{feff}import std::flow\r\nlabel start {\r\n  おはよう\r\n  @flow.wait(1.0)\r\n}\r\n";
    let path = write_file(&dir, "a.yomi", src);
    let o = yomi(&["assign-ids", path.to_str().unwrap(), "--seed", "2"], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
    let after = String::from_utf8(bytes).unwrap();
    assert_eq!(after.matches("\r\n").count(), 5);
    assert_eq!(after.matches('\n').count(), 5);
    assert_eq!(
        mask_ids(&after),
        "\u{feff}import std::flow\r\nlabel start {\r\n  おはよう #ID\r\n  @flow.wait(1.0) #ID\r\n}\r\n"
    );
}

#[test]
fn assign_ids_dry_run_prints_ids_and_positions_without_writing() {
    let dir = scratch("aid_dry");
    let path = write_file(&dir, "a.yomi", AID_SCRIPT);
    let p = path.to_str().unwrap();
    let o = yomi(
        &["--lang", "ja", "assign-ids", p, "--dry-run", "--seed", "5"],
        "",
    );
    assert_eq!(o.code, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), AID_SCRIPT);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{}", o.stdout);
    assert!(
        lines[0].contains(":4:7: #") && lines[0].contains("素の行"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains(":6:18: #") && lines[1].contains("停止型コマンドの行"));
    assert!(lines[2].contains(":8:10: #") && lines[2].contains("選択肢の文"));
    assert_eq!(
        lines[3],
        "3 件に ID を付けます（1 ファイル。書き込みはしていません）"
    );
    // The IDs shown are the same as the IDs actually written
    yomi(&["assign-ids", p, "--seed", "5"], "");
    let after = std::fs::read_to_string(&path).unwrap();
    for l in &lines[..3] {
        let id = l.split("#").nth(1).unwrap().split(' ').next().unwrap();
        assert!(after.contains(&format!("#{id}")), "{id}");
    }
}

#[test]
fn assign_ids_check_returns_1_without_writing_when_lines_lack_ids() {
    let dir = scratch("aid_check");
    let path = write_file(&dir, "a.yomi", AID_SCRIPT);
    let p = path.to_str().unwrap();
    let o = yomi(&["--lang", "ja", "assign-ids", p, "--check"], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 3, "{}", o.stderr);
    assert!(o.stderr.contains("安定 ID がありません"));
    assert_eq!(o.stdout, "安定 ID のない行が 3 件あります（1 ファイル）\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), AID_SCRIPT);
    // 0 after adding
    yomi(&["assign-ids", p], "");
    let o = yomi(&["--lang", "ja", "assign-ids", p, "--check"], "");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "付ける ID はありません\n");
}

#[test]
fn assign_ids_leaves_all_files_on_compile_errors() {
    let dir = scratch("aid_error");
    let good = write_file(&dir, "good.yomi", "label start {\n  おはよう\n}\n");
    let bad = write_file(&dir, "bad.yomi", "label other {\n  あ\n  jump nowhere\n}\n");
    let o = yomi(
        &[
            "--lang",
            "ja",
            "assign-ids",
            good.to_str().unwrap(),
            bad.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("bad.yomi:3:"), "{}", o.stderr);
    assert!(o.stderr.contains("エラー"));
    assert_eq!(
        std::fs::read_to_string(&good).unwrap(),
        "label start {\n  おはよう\n}\n"
    );
    assert_eq!(
        std::fs::read_to_string(&bad).unwrap(),
        "label other {\n  あ\n  jump nowhere\n}\n"
    );
}

pub(super) const AID_ONCE_SCRIPT: &str = "import std::flow\nlabel start {\n  おはよう\n  choice {\n    「見る」 once => jump start\n    「待つ」 once if true => jump start\n    「帰る」 #bye => jump start\n  }\n}\n";

#[test]
fn assign_ids_adds_missing_once_ids_and_script_compiles() {
    let dir = scratch("aid_once");
    let path = write_file(&dir, "a.yomi", AID_ONCE_SCRIPT);
    let p = path.to_str().unwrap();
    // Before adding, `yomi check` fails with once_without_id
    let o = yomi(&["--lang", "ja", "check", p], "");
    assert_eq!(o.code, 1, "{}", o.stderr);
    let o = yomi(&["--lang", "ja", "assign-ids", p, "--seed", "5"], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "安定 ID を 3 件付けました（1 ファイル）\n");
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        mask_ids(&after),
        "import std::flow\nlabel start {\n  おはよう #ID\n  choice {\n    「見る」 once #ID => jump start\n    「待つ」 once if true #ID => jump start\n    「帰る」 #bye => jump start\n  }\n}\n"
    );
    // Compilation passes, and the second time changes nothing
    let o = yomi(&["--lang", "ja", "check", p], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = yomi(&["--lang", "ja", "assign-ids", p], "");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "付ける ID はありません\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
}

#[test]
fn assign_ids_check_and_dry_run_count_missing_once_ids_without_writing() {
    let dir = scratch("aid_once_check");
    let path = write_file(&dir, "a.yomi", AID_ONCE_SCRIPT);
    let p = path.to_str().unwrap();
    let o = yomi(&["--lang", "ja", "assign-ids", p, "--check"], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 3, "{}", o.stderr);
    assert!(o.stderr.contains(":5:") && o.stderr.contains("安定 ID がありません"));
    assert_eq!(o.stdout, "安定 ID のない行が 3 件あります（1 ファイル）\n");
    let o = yomi(
        &["--lang", "ja", "assign-ids", p, "--dry-run", "--seed", "5"],
        "",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout.lines().count(), 4, "{}", o.stdout);
    assert!(o.stdout.contains(":5:") && o.stdout.contains("選択肢の文"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), AID_ONCE_SCRIPT);
    // Even if both are given, nothing is written
    let o = yomi(&["assign-ids", p, "--check", "--dry-run"], "");
    assert_eq!(o.code, 1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), AID_ONCE_SCRIPT);
}

#[test]
fn assign_ids_writes_nothing_if_other_errors_besides_missing_once_ids() {
    let dir = scratch("aid_once_error");
    let src = "label start {\n  おはよう\n  choice {\n    「見る」 once => jump nowhere\n  }\n}\n";
    let path = write_file(&dir, "a.yomi", src);
    let o = yomi(&["--lang", "ja", "assign-ids", path.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("エラー"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), src);
}

#[test]
fn chosen_once_option_still_disappears_after_assign_ids_adds_once_ids() {
    let dir = scratch("aid_once_run");
    let src = "label start {\n  choice {\n    「見る」 once => { 見た; jump start }\n    「帰る」 #bye => 帰った\n  }\n}\n";
    let path = write_file(&dir, "a.yomi", src);
    let p = path.to_str().unwrap();
    assert_eq!(yomi(&["assign-ids", p, "--seed", "9"], "").code, 0);
    // The first time there are 2 options (choose 1); the second time the `once` one disappears and there is 1 option (go
    // home)
    let o = yomi(&["--lang", "ja", "run", p], "1\n\n1\n\n");
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(o.stdout.contains("見た"), "{}", o.stdout);
    assert_eq!(o.stdout.matches("「見る」").count(), 1, "{}", o.stdout);
}

#[test]
fn assign_ids_handles_files_and_directories_and_leaves_non_targets() {
    let dir = scratch("aid_multi");
    let a = write_file(
        &dir,
        "a.yomi",
        "import b\nlabel start {\n  あ\n  jump b.other\n}\n",
    );
    let b = write_file(&dir, "b.yomi", "label other {\n  い\n}\n");
    let c = write_file(&dir, "c.yomi", "label third {\n  う\n}\n");
    // Specify only a and b. c is read as a module but not rewritten
    let o = yomi(
        &[
            "assign-ids",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--seed",
            "9",
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(std::fs::read_to_string(&a).unwrap().contains("あ #"));
    assert!(std::fs::read_to_string(&b).unwrap().contains("い #"));
    assert_eq!(
        std::fs::read_to_string(&c).unwrap(),
        "label third {\n  う\n}\n"
    );
    // Specifying a directory covers everything under it
    let o = yomi(&["--lang", "ja", "assign-ids", dir.to_str().unwrap()], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "安定 ID を 1 件付けました（1 ファイル）\n");
    assert!(std::fs::read_to_string(&c).unwrap().contains("う #"));
}

#[test]
fn texts_template_lists_all_lines_after_assign_ids() {
    let dir = scratch("aid_texts");
    let path = write_file(&dir, "a.yomi", "label start {\n  あ\n  い\n}\n");
    let p = path.to_str().unwrap();
    let before = yomi(&["texts", "template", p], "");
    assert_eq!(before.code, 0);
    assert!(!before.stdout.contains("あ"));
    yomi(&["assign-ids", p], "");
    let after = yomi(&["texts", "template", p], "");
    assert!(
        after.stdout.contains("\tあ\n") && after.stdout.contains("\tい\n"),
        "{}",
        after.stdout
    );
}

#[test]
fn assign_ids_returns_1_without_script() {
    let dir = scratch("aid_none");
    let o = yomi(&["--lang", "ja", "assign-ids", dir.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("台本（.yomi）が見つかりません"),
        "{}",
        o.stderr
    );
}

#[test]
fn codes_prints_one_line_per_code_and_json_is_language_independent() {
    let ja = yomi(&["--lang", "ja", "codes"], "");
    let en = yomi(&["--lang", "en", "codes"], "");
    assert_eq!(ja.code, 0);
    assert_eq!(ja.stdout.lines().count(), en.stdout.lines().count());
    assert!(
        ja.stdout
            .lines()
            .any(|l| l.starts_with("diag\tunknown_name\tエラー\tname,suggestions\t"))
    );
    assert!(
        en.stdout
            .lines()
            .any(|l| l.starts_with("diag\tshadowed_builtin\twarning\tname\t"))
    );
    assert!(
        en.stdout
            .lines()
            .any(|l| l.starts_with("vm\tbad_input\terror\texpected,found\t"))
    );
    for l in ja.stdout.lines() {
        assert_eq!(l.split('\t').count(), 5, "{l}");
    }
    let j1 = yomi(&["--lang", "ja", "codes", "--json"], "");
    let j2 = yomi(&["--lang", "en", "codes", "--json"], "");
    assert_eq!(j1.stdout, j2.stdout);
    let root: serde_json::Value = serde_json::from_str(&j1.stdout).expect("JSON として読める");
    let missing = root["codes"]
        .as_array()
        .expect("codes は配列")
        .iter()
        .find(|c| c["code"] == "missing_text_id")
        .expect("missing_text_id がある");
    assert_eq!(missing["severity"], "error");
    assert_eq!(missing["args"], serde_json::json!(["kind"]));
    // The number of lines is the same as the number of JSON entries
    assert_eq!(
        j1.stdout.matches("\"group\"").count(),
        ja.stdout.lines().count()
    );
}

#[test]
fn codes_packages_appends_command_error_codes_and_output_without_it_is_unchanged() {
    for lang in ["ja", "en"] {
        let base = yomi(&["--lang", lang, "codes"], "");
        let with = yomi(&["--lang", lang, "codes", "--packages"], "");
        assert_eq!(with.code, 0);
        assert!(
            with.stdout.starts_with(&base.stdout),
            "{lang}: 先頭が変わった"
        );
        let extra: Vec<&str> = with.stdout[base.stdout.len()..].lines().collect();
        assert_eq!(extra.len(), 30 + 8);
        assert!(
            extra.iter().all(|l| l.split('\t').count() == 5),
            "{extra:?}"
        );
        assert!(extra[0].starts_with("package\tflow.negative_seconds\t"));
        assert!(
            extra
                .last()
                .unwrap()
                .starts_with("core\tcore.decl_only_function\t")
        );
        let jb = yomi(&["--lang", lang, "codes", "--json"], "");
        let jw = yomi(&["--lang", lang, "codes", "--json", "--packages"], "");
        assert!(!jb.stdout.contains("\"kind\": "));
        assert_eq!(
            jw.stdout.matches("\"kind\": ").count(),
            jw.stdout.matches("\"code\": \"").count()
        );
        assert_eq!(
            jw.stdout.matches("\"code\": \"").count(),
            jb.stdout.matches("\"code\": \"").count() + 30 + 8
        );
    }
    let a = yomi(&["--lang", "ja", "codes", "--json", "--packages"], "");
    let b = yomi(&["--lang", "en", "codes", "--json", "--packages"], "");
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn list_insert_inserts_and_out_of_range_is_runtime_error() {
    let o = run("ja", "m8d_insert.yomi", &[], "");
    assert_eq!(o.stdout, "[a, b, c]\n");
    assert_ne!(o.code, 0, "{}", o.stderr);
    assert!(o.stderr.contains("範囲外"), "{}", o.stderr);
}

#[test]
fn decomposed_identifiers_work_as_same_variables_and_bodies_print_as_is() {
    let o = run("ja", "m8d_nfc.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    // The "が" in the body stays decomposed (not normalized)
    assert_eq!(o.stdout, "本文\u{304B}\u{3099} 3\n（おわり）\n");
}

#[test]
fn invalid_identifier_chars_stop_as_unexpected_tokens() {
    let o = run("en", "m8d_bad_ident.yomi", &[], "");
    assert_ne!(o.code, 0);
    assert!(
        o.stderr.contains("2:8: error: unexpected token 🐱"),
        "{}",
        o.stderr
    );
}
