//! Automatic play (`yomi run --auto`; answering with the stable IDs of options)

use super::*;

/// The input of automatic play (`playthroughs/<name>.auto`). It is the standard input of `--auto` itself, containing only
/// choice numbers, `#ID`s, answers to `input.*` and interactive commands (no empty lines for bodies are needed). Lines
/// starting with `# ` (`#` and a space) are comments and are removed before passing (lines of `#ID` are not comments,
/// because what follows `#` is not a space).
pub(super) fn auto_input(name: &str) -> String {
    let text = std::fs::read_to_string(playthrough_file(name, "auto"))
        .unwrap_or_else(|_| panic!("{name}.auto がない"));
    text.lines()
        .filter(|l| !l.starts_with("# "))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Runs automatic play (Japanese display, fixed seed).
pub(super) fn run_auto(dir: &Path, input: &str, texts: Option<&str>) -> Output {
    let main = main_yomi();
    let mut args = run_args(&main, "", texts);
    args.push("--auto");
    yomi(dir, &args, input)
}

/// Removes the ` #ID` at the end of option lines (to compare with the output of interactive play).
pub(super) fn without_choice_ids(out: &str) -> String {
    out.lines()
        .map(|l| {
            let is_choice = l
                .split_once(": ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            match l.rsplit_once(" #") {
                Some((head, id))
                    if is_choice
                        && !id.is_empty()
                        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                {
                    format!("{head}\n")
                }
                _ => format!("{l}\n"),
            }
        })
        .collect()
}

#[test]
fn autoplay_answers_by_id_reaches_ending_with_expected_output() {
    for (name, ending) in [("end_good", "【封印の夜】"), ("end_normal", "【帰り道】")] {
        let dir = scratch(&format!("auto-{name}"));
        let o = run_auto(&dir, &auto_input(name), None);
        assert_eq!(o.code, 0, "{name}: {}", o.stderr);
        assert_eq!(o.stderr, "", "{name}");
        assert!(o.stdout.contains(ending), "{name}: {}", o.stdout);
        // Stable IDs appear on option lines
        assert!(
            o.stdout.contains("「弓の練習場に行く」 #archery\n"),
            "{name}"
        );
        check_golden(&playthrough_file(name, "auto.out"), &o.stdout);
    }
}

#[test]
fn autoplay_output_matches_interactive_output_except_choice_ids() {
    // `end_good.auto` makes the same choices as `end_good.answers`, written with IDs
    let dir = scratch("auto-same");
    let o = run_auto(&dir, &auto_input("end_good"), None);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let interactive = std::fs::read_to_string(playthrough_file("end_good", "out")).unwrap();
    assert_eq!(without_choice_ids(&o.stdout), interactive);
}

#[test]
fn autoplay_with_english_table_answers_same_ids_and_reaches_ending() {
    let dir = scratch("auto-en");
    let texts = texts_en();
    let o = run_auto(&dir, &auto_input("end_good"), Some(&texts));
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("[The Night of the Seal]"), "{}", o.stdout);
    // The display is translated, and the IDs are the same
    assert!(
        o.stdout.contains("\"Go to the archery range\" #archery\n"),
        "{}",
        o.stdout
    );
    check_golden(&playthrough_file("end_good", "auto.en.out"), &o.stdout);
    let interactive = std::fs::read_to_string(playthrough_file("end_good", "en.out")).unwrap();
    assert_eq!(without_choice_ids(&o.stdout), interactive);
}

#[test]
fn autoplay_answering_unlisted_id_stops_with_exit_code_1() {
    let dir = scratch("auto-bad-id");
    let input = auto_input("end_good").replace("#vit5vc", "#nope00");
    let o = run_auto(&dir, &input, None);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("答え #nope00"), "{}", o.stderr);
    assert!(!o.stdout.contains("（おわり）"));
    // It also stops when there are not enough answers
    let short: String = auto_input("end_good")
        .lines()
        .take(8)
        .map(|l| format!("{l}\n"))
        .collect();
    let o = run_auto(&dir, &short, None);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("入力が終わりました"), "{}", o.stderr);
}
