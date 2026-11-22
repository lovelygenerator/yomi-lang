//! (d) The English text table

use super::*;

#[test]
fn english_table_reaches_all_endings_with_changed_output() {
    let table = texts_en();
    for play in PLAYS {
        let ja = run_play(play, None);
        let en = run_play(play, Some(&table));
        assert_eq!(en.code, 0, "{}: {}", play.name, en.stderr);
        assert_eq!(en.stderr, "", "{}", play.name);
        assert_ne!(ja.stdout, en.stdout, "{}", play.name);
        assert!(
            en.stdout.contains(play.ending_en),
            "{} が {} に着いていない:\n{}",
            play.name,
            play.ending_en,
            en.stdout
        );
        assert!(!en.stdout.contains(play.ending_ja), "{}", play.name);
        // Sound and image cues do not change with the table (the order of stop points is the same too)
        let cues = |s: &str| -> Vec<String> {
            s.lines()
                .filter(|l| l.starts_with("[audio]") || l.starts_with("[image]"))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(cues(&ja.stdout), cues(&en.stdout), "{}", play.name);
        // Input prompts and speaker names (identity display names) are also in English, with no Japanese left (finding 3)
        assert!(en.stdout.contains("[input] Your name?"), "{}", play.name);
        assert!(!en.stdout.contains("名前は？"), "{}", play.name);
        for ja_name in ["リオン", "道具屋の主人", "村の長老", "ギルドの受付"] {
            assert!(!en.stdout.contains(&format!("{ja_name}\"")), "{ja_name}");
        }
        assert_eq!(
            ja.stdout.lines().count(),
            en.stdout.lines().count(),
            "{}",
            play.name
        );
        check_golden(&playthrough_file(play.name, "en.out"), &en.stdout);
    }
}

#[test]
fn texts_check_reports_no_missing_or_extra_translations() {
    let dir = scratch("texts_check");
    let main = main_yomi();
    let table = texts_en();
    let o = yomi(&dir, &["--lang", "ja", "texts", "check", &main, &table], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "", "警告（足りない訳・余った訳）が出ている");
}

#[test]
fn texts_template_keys_match_english_table_keys() {
    let dir = scratch("texts_template");
    let main = main_yomi();
    let o = yomi(&dir, &["--lang", "ja", "texts", "template", &main], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let keys = |text: &str| -> Vec<String> {
        let mut k: Vec<String> = text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
            .map(|l| l.split('\t').next().unwrap().to_string())
            .collect();
        k.sort();
        k
    };
    let table = std::fs::read_to_string(texts_en()).unwrap();
    assert_eq!(keys(&o.stdout), keys(&table));
}
