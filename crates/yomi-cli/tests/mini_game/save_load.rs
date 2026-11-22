//! (e) Save, load and rollback

use super::*;

/// The line number of the `n`-th non-empty line among the input lines (including empty lines).
pub(super) fn nth_answer_line(lines: &[&str], n: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.is_empty())
        .nth(n)
        .map(|(i, _)| i)
        .unwrap()
}

/// The output when stopping with `:quit` after the first `n` lines of input (up to showing the `n`-th stop point).
pub(super) fn probe(dir: &Path, lines: &[&str], n: usize) -> String {
    let mut input = lines[..n].join("\n");
    input.push_str("\n:quit\n");
    let main = main_yomi();
    let o = yomi(dir, &run_args(&main, "", None), &input);
    assert_eq!(o.code, 0, "{}", o.stderr);
    o.stdout
}

pub(super) fn join_lines(parts: &[&[&str]]) -> String {
    let mut s = String::new();
    for part in parts {
        for l in *part {
            s.push_str(l);
            s.push('\n');
        }
    }
    s
}

/// "The output after the `n`-th stop point" (the original output without the output up to the `n`-th stop point).
pub(super) fn tail_after(dir: &Path, original: &str, lines: &[&str], n: usize) -> String {
    let head = probe(dir, lines, n);
    original
        .strip_prefix(head.as_str())
        .unwrap_or_else(|| panic!("元の出力が、途中までの出力で始まらない:\n{head}"))
        .to_string()
}

#[test]
fn saving_and_reloading_midway_reach_same_ending() {
    let play = &PLAYS[0];
    let dir = scratch("save_load");
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let lines: Vec<&str> = input.lines().collect();
    let main = main_yomi();
    let original = yomi(&dir, &run_args(&main, "", None), &input);
    assert_eq!(original.code, 0, "{}", original.stderr);
    // Save at the first choice in the village square (the 5th answer) and go back at the last choice (after the archery range
    // and the forest)
    let k = nth_answer_line(&lines, 4);
    let m = lines.iter().rposition(|l| !l.is_empty()).unwrap();
    let tail = tail_after(&dir, &original.stdout, &lines, k);
    assert!(
        tail.contains("ダメージ！"),
        "弓の練習場を含む区間でなければならない"
    );

    let stdin = join_lines(&[
        &lines[..k],
        &[":save s.sav"],
        &lines[k..=m],
        &[":load s.sav"],
        &lines[k..],
    ]);
    let o = yomi(&dir, &run_args(&main, "", None), &stdin);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "");
    assert!(o.stdout.contains("s.sav に保存しました"));
    assert!(o.stdout.contains("s.sav から読み込みました"));
    // The output after going back (including the archery rolls) is the same as what was played before going back. The random
    // state goes back too
    assert!(o.stdout.ends_with(&tail), "戻ったあとが違う:\n{}", o.stdout);
    let hits = |s: &str| s.matches("ダメージ！").count();
    assert_eq!(hits(&o.stdout), 2 * hits(&original.stdout));
}

#[test]
fn save_file_loaded_in_another_process_reaches_same_ending() {
    let play = &PLAYS[1];
    let dir = scratch("save_load_process");
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let lines: Vec<&str> = input.lines().collect();
    let main = main_yomi();
    let original = yomi(&dir, &run_args(&main, "", None), &input);
    assert_eq!(original.code, 0, "{}", original.stderr);
    // Save at the 3rd choice in the village square and end
    let k = nth_answer_line(&lines, 6);
    let first = join_lines(&[&lines[..k], &[":save s.sav", ":quit"]]);
    let o1 = yomi(&dir, &run_args(&main, "", None), &first);
    assert_eq!(o1.code, 0, "{}", o1.stderr);
    assert!(o1.stdout.contains("s.sav に保存しました"));
    // A new process does :load at the first stop point and plays the rest
    let second = join_lines(&[&[":load s.sav"], &lines[k..]]);
    let o2 = yomi(&dir, &run_args(&main, "", None), &second);
    assert_eq!(o2.code, 0, "{}", o2.stderr);
    assert_eq!(o2.stderr, "");
    let tail = tail_after(&dir, &original.stdout, &lines, k);
    assert!(o2.stdout.ends_with(&tail), "{}", o2.stdout);
    assert!(o2.stdout.contains(play.ending_ja));
}

#[test]
fn redoing_same_choice_after_back_reaches_same_ending() {
    let play = &PLAYS[1];
    let dir = scratch("back");
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let lines: Vec<&str> = input.lines().collect();
    let main = main_yomi();
    let original = yomi(&dir, &run_args(&main, "", None), &input);
    assert_eq!(original.code, 0, "{}", original.stderr);
    // Advance 3 from the first choice in the village square, and go back 3 times
    let j = nth_answer_line(&lines, 4);
    let tail = tail_after(&dir, &original.stdout, &lines, j);
    let stdin = join_lines(&[&lines[..j + 3], &[":back", ":back", ":back"], &lines[j..]]);
    let o = yomi(&dir, &run_args(&main, "", None), &stdin);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "");
    assert!(o.stdout.ends_with(&tail), "{}", o.stdout);
}

#[test]
fn save_and_load_mid_english_play_reach_same_ending() {
    let play = &PLAYS[1];
    let dir = scratch("save_load_en");
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let lines: Vec<&str> = input.lines().collect();
    let main = main_yomi();
    let table = texts_en();
    let args = run_args(&main, "", Some(&table));
    let original = yomi(&dir, &args, &input);
    assert_eq!(original.code, 0, "{}", original.stderr);
    let k = nth_answer_line(&lines, 5);
    let m = lines.iter().rposition(|l| !l.is_empty()).unwrap();
    let head = {
        let mut s = lines[..k].join("\n");
        s.push_str("\n:quit\n");
        yomi(&dir, &args, &s).stdout
    };
    let tail = original
        .stdout
        .strip_prefix(head.as_str())
        .unwrap()
        .to_string();
    let stdin = join_lines(&[
        &lines[..k],
        &[":save e.sav"],
        &lines[k..=m],
        &[":load e.sav"],
        &lines[k..],
    ]);
    let o = yomi(&dir, &args, &stdin);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with(&tail), "{}", o.stdout);
    assert!(o.stdout.contains(play.ending_en));
}
