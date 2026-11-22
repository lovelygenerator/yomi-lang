//! (a) Playthroughs

use super::*;

#[test]
fn input_files_match_input_built_from_answers() {
    for play in PLAYS {
        input_of(play);
    }
}

#[test]
fn all_endings_are_reached_with_fixed_input_and_seed_and_output_matches() {
    for play in PLAYS {
        let o = run_play(play, None);
        assert_eq!(o.code, 0, "{}: {}", play.name, o.stderr);
        // No warnings or diagnostics
        assert_eq!(o.stderr, "", "{}", play.name);
        assert!(
            o.stdout.contains(play.ending_ja),
            "{} が {} に着いていない:\n{}",
            play.name,
            play.ending_ja,
            o.stdout
        );
        assert!(o.stdout.ends_with("（おわり）\n"), "{}", play.name);
        check_golden(&playthrough_file(play.name, "out"), &o.stdout);
    }
}

#[test]
fn same_seed_gives_same_output_and_different_seed_changes_archery_results() {
    let play = &PLAYS[0];
    let a = run_play(play, None);
    let b = run_play(play, None);
    assert_eq!(a.stdout, b.stdout);
    // Run with another seed (the same input; even if the order of stop points changes, it always ends)
    let dir = scratch("seed");
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let main = main_yomi();
    let o = yomi(
        &dir,
        &["--lang", "ja", "run", &main, "--seed", "123456789"],
        &input,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let damage = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| l.contains("ダメージ！"))
            .map(str::to_string)
            .collect()
    };
    assert!(!damage(&a.stdout).is_empty());
    assert_ne!(damage(&a.stdout), damage(&o.stdout));
}

#[test]
fn persistent_data_carries_over_to_next_play() {
    // After seeing the Night of the Seal, the opening text grows, and the same choices lead to another ending
    let first = run_play(&PLAYS[0], None);
    let second = run_play(&PLAYS[3], None);
    assert!(!first.stdout.contains("この景色を前にも見た"));
    assert!(second.stdout.contains("この景色を前にも見た"));
    assert!(second.stdout.contains("あの夜の祠の光を、まだ覚えている"));
    assert!(
        second
            .stdout
            .contains("見たエンディング: 封印の夜、次の依頼も")
    );
}
