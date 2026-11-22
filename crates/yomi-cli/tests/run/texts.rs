//! Text replacement (localization)

use super::*;

/// The path of a sample file (copied into a directory separate from other samples).
pub(super) fn path_of(name: &str) -> String {
    fixture(name).to_str().unwrap().to_string()
}

#[test]
fn texts_table_prints_translated_bodies_and_options() {
    let table = path_of("texts_en.texts");
    let o = run("ja", "texts.yomi", &["--texts", &table], "\n\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "Day 3\nImouto「Hi, number 3」\n1: 「Yes」\n2: 「No」\nYes reply\nThe end\n（おわり）\n"
    );
}

#[test]
fn without_table_prints_original() {
    let o = run("ja", "texts.yomi", &[], "\n\n\n\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "朝 3 日目\n案内人「やあ、3 回目」\n1: 「はい」\n2: 「いいえ」\nはい返事\nおわり\n（おわり）\n"
    );
}

#[test]
fn unreadable_or_unsettable_texts_table_exits_1_without_running() {
    for (name, expect) in [
        ("texts_bad_syntax.texts", "texts_bad_syntax.texts:2: error:"),
        ("texts_no_tab.texts", "texts_no_tab.texts:2: error:"),
        ("texts_bad_number.texts", "texts.start#a"),
    ] {
        let table = path_of(name);
        // It ends before running, so standard input is empty
        let o = run("en", "texts.yomi", &["--texts", &table], "");
        assert_eq!(o.code, 1, "{name}");
        assert_eq!(o.stdout, "", "{name}");
        assert!(o.stderr.contains(expect), "{name}: {}", o.stderr);
    }
    let o = run("en", "texts.yomi", &["--texts", "/no/such/table"], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("/no/such/table"), "{}", o.stderr);
}

#[test]
fn texts_switches_language_while_stopped_and_rebuilds_current_stop_point() {
    let table = path_of("texts_en.texts");
    let input = format!(":texts {table}\n:texts off\n:texts {table}\n:quit\n");
    let o = run("ja", "texts.yomi", &[], &input);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        format!(
            "朝 3 日目\n差し替え表 {table} を使います\nDay 3\n原文に戻しました（off）\n朝 3 日目\n差し替え表 {table} を使います\nDay 3\n"
        )
    );
}

#[test]
fn texts_is_consistent_with_back_and_load_and_translates_from_next_stop_point() {
    let table = path_of("texts_en.texts");
    let dir = scratch("lang_save");
    let save = dir.join("s.sav");
    let save = save.to_str().unwrap();
    // Advance to the second with the original, save, switch to the translation, then :back and :load
    let input = format!(
        "\n:save {save}\n:texts {table}\n:back\n\n:load {save}\n:texts off\n:load {save}\n:quit\n"
    );
    let o = run("ja", "texts.yomi", &[], &input);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "朝 3 日目",
            "案内人「やあ、3 回目」",
            &format!("{save} に保存しました"),
            &format!("差し替え表 {table} を使います"),
            "Imouto「Hi, number 3」",
            // :back shows the previous stop point (morning) in the current language
            "Day 3",
            "Imouto「Hi, number 3」",
            // :load shows the saved stop point in the current language (the translation)
            &format!("{save} から読み込みました"),
            "Imouto「Hi, number 3」",
            "原文に戻しました（off）",
            "案内人「やあ、3 回目」",
            &format!("{save} から読み込みました"),
            "案内人「やあ、3 回目」",
        ]
    );
}

#[test]
fn unreadable_or_unsettable_texts_table_reports_to_stderr_and_waits_at_same_stop_point() {
    for name in ["texts_bad_number.texts", "texts_no_tab.texts"] {
        let table = path_of(name);
        let input = format!(":texts {table}\n\n:quit\n");
        let o = run("en", "texts.yomi", &[], &input);
        assert_eq!(o.code, 0, "{name}");
        // The table is not set, and it goes on to the next with the original
        assert_eq!(o.stdout, "朝 3 日目\n案内人「やあ、3 回目」\n", "{name}");
        assert!(
            o.stderr.contains("texts.start#a") || o.stderr.contains(name),
            "{}",
            o.stderr
        );
    }
    let o = run("ja", "texts.yomi", &[], ":texts\n:texts /no/such\n:quit\n");
    assert_eq!(o.code, 0);
    assert!(o.stderr.contains(":texts"), "{}", o.stderr);
    assert!(o.stderr.contains("/no/such"), "{}", o.stderr);
}

#[test]
fn texts_check_warns_missing_and_extra_translations_and_exits_0() {
    let script = path_of("texts.yomi");
    let table = path_of("texts_partial.texts");
    let o = yomi(&["--lang", "en", "texts", "check", &script, &table], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let e: Vec<&str> = o.stderr.lines().collect();
    assert_eq!(
        e,
        vec![
            format!("{table}: warning: no translation for identity:texts.案内人"),
            format!("{table}: warning: no translation for texts.start#b"),
            format!("{table}: warning: no translation for texts.start#yes"),
            format!("{table}: warning: no translation for texts.start#no"),
            format!("{table}: warning: no translation for texts.start#r1"),
            format!("{table}: warning: no translation for texts.start#r2"),
            format!("{table}: warning: no translation for texts.start#z"),
            format!(
                "{table}: warning: the translation for texts.start#gone is not used (the script has no such line)"
            ),
        ]
    );
    assert_eq!(o.stdout, "text table check: 0 errors, 8 warnings\n");
}

#[test]
fn texts_check_makes_number_errors_errors_and_exits_1_ja() {
    let script = path_of("texts.yomi");
    let table = path_of("texts_bad_number.texts");
    let o = yomi(&["--lang", "ja", "texts", "check", &script, &table], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains(&format!(
            "{table}: エラー: texts.start#a の訳が ${{2}} を使っていますが、原文の埋め込みは 1 個です"
        )),
        "{}",
        o.stderr
    );
    assert!(o.stdout.contains("エラー 1 件"), "{}", o.stdout);
}

#[test]
fn texts_check_reports_0_without_problems_and_1_with_line_for_table_errors() {
    let script = path_of("texts.yomi");
    let table = path_of("texts_en.texts");
    let o = yomi(&["--lang", "en", "texts", "check", &script, &table], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "");
    assert_eq!(o.stdout, "the text table matches the script\n");
    for (name, expect) in [
        ("texts_no_tab.texts", ":2: error: the line has no TAB"),
        (
            "texts_bad_syntax.texts",
            ":2: error: the translation of texts.start#a has an embed that is not ${n}",
        ),
    ] {
        let table = path_of(name);
        let o = yomi(&["--lang", "en", "texts", "check", &script, &table], "");
        assert_eq!(o.code, 1, "{name}");
        assert!(o.stderr.contains(expect), "{name}: {}", o.stderr);
        assert_eq!(o.stdout, "");
    }
}

#[test]
fn texts_check_reports_compile_errors_and_unreadable_tables_with_1() {
    let table = path_of("texts_en.texts");
    let broken = path_of("broken.yomi");
    let o = yomi(&["--lang", "en", "texts", "check", &broken, &table], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("broken.yomi:"), "{}", o.stderr);
    let script = path_of("texts.yomi");
    let o = yomi(&["--lang", "en", "texts", "check", &script, "/no/such"], "");
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("/no/such"), "{}", o.stderr);
}

#[test]
fn texts_template_prints_table_of_original_id_bodies_that_passes_check() {
    let script = path_of("texts.yomi");
    let o = yomi(
        &[
            "--lang",
            "en",
            "texts",
            "template",
            &script,
            "--text-language",
            "en",
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        concat!(
            "@lang en\n",
            "identity:texts.案内人\t案内人\n",
            "texts.start#a\t朝 ${1} 日目\n",
            "texts.start#b\t「やあ、${1} 回目」\n",
            "texts.start#yes\t「はい」\n",
            "texts.start#no\t「いいえ」\n",
            "texts.start#r1\tはい返事\n",
            "texts.start#r2\tいいえ返事\n",
            "texts.start#z\tおわり\n",
        )
    );
    // Write it out and pass it through the check
    let dir = scratch("template");
    let file = dir.join("t.texts");
    std::fs::write(&file, &o.stdout).unwrap();
    let c = yomi(
        &[
            "--lang",
            "en",
            "texts",
            "check",
            &script,
            file.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(c.code, 0, "{}", c.stderr);
    assert_eq!(c.stderr, "");
    // If the language name is omitted, no @lang line is printed
    let o = yomi(&["--lang", "en", "texts", "template", &script], "");
    assert!(
        o.stdout.starts_with("identity:texts.案内人\t"),
        "{}",
        o.stdout
    );
}
