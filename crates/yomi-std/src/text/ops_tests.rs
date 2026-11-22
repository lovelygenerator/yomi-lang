use super::*;

const FLAG: &str = "\u{1F1EF}\u{1F1F5}";
const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
const E_ACUTE: &str = "e\u{0301}";

fn msg(r: Result<impl std::fmt::Debug, CommandError>) -> String {
    format!("{:?}", r.unwrap_err())
}

#[test]
fn ends_with_empty_suffix_is_always_true() {
    assert!(ends_with("こんにちは", "ちは"));
    assert!(!ends_with("こんにちは", "こん"));
    assert!(ends_with("abc", ""));
    assert!(ends_with("", ""));
    assert!(!ends_with("", "a"));
}

#[test]
fn replace_replaces_all_non_overlapping_matches() {
    assert_eq!(replace("aaaa", "aa", "b").unwrap(), "bb");
    assert_eq!(replace("aaa", "aa", "b").unwrap(), "ba");
    assert_eq!(replace("あいうあ", "あ", "").unwrap(), "いう");
    assert_eq!(replace("abc", "x", "y").unwrap(), "abc");
    assert_eq!(replace("", "x", "y").unwrap(), "");
}

#[test]
fn replace_with_empty_from_is_runtime_error() {
    assert!(
        msg(replace("abc", "", "x")).contains("text.empty_arg")
            && msg(replace("abc", "", "x")).contains("\"from\"")
    );
}

#[test]
fn replace_over_limit_errors_before_building() {
    let s = "a".repeat(1 << 20);
    // Replacing 1 MiB of a with 64 bytes gives 64 MiB (exceeding the 16 MiB limit)
    assert!(msg(replace(&s, "a", &"b".repeat(64))).contains("text.result_too_large"));
    assert_eq!(replace(&s, "a", "bb").unwrap().len(), 2 << 20);
}

#[test]
fn split_splits_and_consecutive_separators_make_empty_elements() {
    assert_eq!(split("a,b,c", ",").unwrap(), ["a", "b", "c"]);
    assert_eq!(split("a,,b", ",").unwrap(), ["a", "", "b"]);
    assert_eq!(split(",a,", ",").unwrap(), ["", "a", ""]);
    assert_eq!(split("", ",").unwrap(), [""]);
    assert_eq!(split("あ→い→う", "→").unwrap(), ["あ", "い", "う"]);
    assert_eq!(split("abc", "x").unwrap(), ["abc"]);
}

#[test]
fn split_with_empty_separator_is_runtime_error() {
    assert!(
        msg(split("abc", "")).contains("text.empty_arg")
            && msg(split("abc", "")).contains("\"sep\"")
    );
}

#[test]
fn join_joins_with_separator() {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(join(&v(&["a", "b", "c"]), "-").unwrap(), "a-b-c");
    assert_eq!(join(&v(&["a"]), "-").unwrap(), "a");
    assert_eq!(join(&v(&[]), "-").unwrap(), "");
    assert_eq!(join(&v(&["", ""]), "-").unwrap(), "-");
    assert_eq!(join(&v(&["あ", "い"]), "").unwrap(), "あい");
}

#[test]
fn join_over_limit_is_runtime_error() {
    let parts = vec![String::new(); 1 << 12];
    let sep = "x".repeat(1 << 20);
    assert!(msg(join(&parts, &sep)).contains("text.result_too_large"));
}

#[test]
fn slice_cuts_at_grapheme_positions() {
    assert_eq!(slice("あいうえお", 1, 3).unwrap(), "いう");
    assert_eq!(slice("abc", 0, 3).unwrap(), "abc");
    assert_eq!(slice("abc", 3, 3).unwrap(), "");
    assert_eq!(slice("", 0, 0).unwrap(), "");
    let s = format!("a{E_ACUTE}{FLAG}{FAMILY}z");
    assert_eq!(slice(&s, 1, 2).unwrap(), E_ACUTE);
    assert_eq!(slice(&s, 2, 3).unwrap(), FLAG);
    assert_eq!(slice(&s, 3, 4).unwrap(), FAMILY);
    assert_eq!(slice(&s, 4, 5).unwrap(), "z");
}

#[test]
fn slice_out_of_range_is_runtime_error() {
    assert!(slice("abc", -1, 2).is_err());
    assert!(slice("abc", 0, 4).is_err());
    assert!(slice("abc", 2, 1).is_err());
    assert!(slice("abc", 4, 4).is_err());
    assert!(slice("abc", i64::MAX, i64::MAX).is_err());
    assert!(slice("", 0, 1).is_err());
}

#[test]
fn find_returns_grapheme_position_of_first_match() {
    assert_eq!(find("こんにちは", "にち"), Some(2));
    assert_eq!(find("abcabc", "c"), Some(2));
    assert_eq!(find("abc", "x"), None);
    assert_eq!(find("", "x"), None);
    assert_eq!(find("abc", ""), Some(0));
    assert_eq!(find("", ""), Some(0));
    let s = format!("a{E_ACUTE}b{FLAG}c");
    assert_eq!(find(&s, "b"), Some(2));
    assert_eq!(find(&s, "c"), Some(4));
}

#[test]
fn find_returns_cluster_position_when_match_starts_mid_cluster() {
    let s = format!("a{E_ACUTE}b");
    // A match starting at 'e' is inside 'é' with a combining character (position 1)
    assert_eq!(find(&s, "e"), Some(1));
    assert_eq!(find(&s, "\u{0301}b"), Some(1));
    // A match starting at the second regional indicator of a flag
    assert_eq!(find(FLAG, "\u{1F1F5}"), Some(0));
}

#[test]
fn repeat_repeats_n_times() {
    assert_eq!(repeat("ab", 3).unwrap(), "ababab");
    assert_eq!(repeat("ab", 0).unwrap(), "");
    assert_eq!(repeat("", 5).unwrap(), "");
    assert_eq!(repeat("あ", 1).unwrap(), "あ");
}

#[test]
fn repeat_with_negative_n_is_runtime_error() {
    assert!(
        msg(repeat("a", -1)).contains("text.negative") && msg(repeat("a", -1)).contains("\"n\"")
    );
}

#[test]
fn repeat_with_huge_n_errors_before_building_and_empty_string_stays_empty() {
    assert!(msg(repeat("a", i64::MAX)).contains("text.result_too_large"));
    assert!(msg(repeat("あい", 1 << 40)).contains("text.result_too_large"));
    assert!(repeat("a", MAX_RESULT_BYTES as i64).is_ok());
    assert!(repeat("a", MAX_RESULT_BYTES as i64 + 1).is_err());
    assert_eq!(repeat("", i64::MAX).unwrap(), "");
    assert_eq!(repeat("a", 0).unwrap(), "");
}

#[test]
fn pad_fills_to_grapheme_width() {
    assert_eq!(pad("t", "7", 3, "0", true).unwrap(), "007");
    assert_eq!(pad("t", "7", 3, "0", false).unwrap(), "700");
    assert_eq!(pad("t", "あ", 3, "　", true).unwrap(), "　　あ");
    let s = format!("{E_ACUTE}{FLAG}");
    assert_eq!(pad("t", &s, 4, "*", true).unwrap(), format!("**{s}"));
    assert_eq!(
        pad("t", "a", 3, FAMILY, false).unwrap(),
        format!("a{FAMILY}{FAMILY}")
    );
}

#[test]
fn pad_does_nothing_when_width_is_enough() {
    assert_eq!(pad("t", "abc", 3, "0", true).unwrap(), "abc");
    assert_eq!(pad("t", "abc", 2, "0", true).unwrap(), "abc");
    assert_eq!(pad("t", "abc", 0, "0", false).unwrap(), "abc");
    assert_eq!(pad("t", "", 0, "0", true).unwrap(), "");
}

#[test]
fn pad_fill_not_one_grapheme_is_runtime_error() {
    assert!(msg(pad("t", "a", 3, "", true)).contains("text.fill_not_grapheme"));
    assert!(pad("t", "a", 3, "ab", true).is_err());
    assert!(pad("t", "a", 3, "ab", false).is_err());
    // Even if the width is enough, fill is checked first
    assert!(pad("t", "abc", 1, "", true).is_err());
}

#[test]
fn pad_negative_and_huge_widths_are_runtime_errors() {
    assert!(
        msg(pad("t", "a", -1, "0", true)).contains("text.negative")
            && msg(pad("t", "a", -1, "0", true)).contains("\"width\"")
    );
    assert!(msg(pad("t", "a", i64::MAX, "0", true)).contains("text.result_too_large"));
    assert!(pad("t", "a", i64::MAX, "あ", false).is_err());
    assert!(pad("t", "a", MAX_RESULT_BYTES as i64 + 1, "0", true).is_err());
    assert_eq!(
        pad("t", "", MAX_RESULT_BYTES as i64, "0", true)
            .unwrap()
            .len(),
        MAX_RESULT_BYTES
    );
}

/// Properties checked with random strings (the seed is fixed; the number of cases is `YOMI_INVARIANT_CASES`, default 300).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n
    }
}

const POOL: &[&str] = &[
    "a",
    "B",
    "z",
    "0",
    " ",
    ",",
    "→",
    "あ",
    "漢",
    "ー",
    "é",
    "e\u{0301}",
    "か\u{3099}",
    "\u{1F1EF}\u{1F1F5}",
    "\u{1F1FA}",
    "\u{1F468}\u{200D}\u{1F469}",
    "\u{1F44D}\u{1F3FD}",
    "\r\n",
    "\n",
    "\u{200D}",
    "\u{0301}",
    "ＡＢ",
    "ß",
    "İ",
];

fn random_string(rng: &mut Rng, max: usize) -> String {
    (0..rng.below(max + 1))
        .map(|_| POOL[rng.below(POOL.len())])
        .collect()
}

fn cases() -> usize {
    std::env::var("YOMI_INVARIANT_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
}

#[test]
fn random_strings_give_same_boundaries_from_three_public_functions() {
    let mut rng = Rng(seed().wrapping_add(13) | 1);
    for _ in 0..cases() {
        let s = random_string(&mut rng, 12);
        let parts: Vec<&str> = graphemes(&s).collect();
        let offsets = grapheme_offsets(&s);
        // The number of pieces, the positions of boundaries and the concatenated results agree across the three functions
        assert_eq!(parts.len(), grapheme_count(&s), "{s:?}");
        assert_eq!(offsets.len(), parts.len() + 1, "{s:?}");
        assert_eq!(parts.concat(), s, "{s:?}");
        for (i, p) in parts.iter().enumerate() {
            assert!(!p.is_empty(), "{s:?}");
            assert_eq!(&s[offsets[i]..offsets[i + 1]], *p, "{s:?} {i}");
            // Position i of text.slice and offsets[i] point to the same place
            assert_eq!(slice(&s, i as i64, i as i64 + 1).unwrap(), *p, "{s:?} {i}");
        }
    }
}

/// Samples of counting. If the results change, increase both [`GRAPHEME_RULES_VERSION`] and `Text::RULES_VERSION`, then
/// fix this table and the pair of versions below.
#[test]
fn counting_samples_are_fixed_per_version() {
    // The pair of the counting version and the rule version of all of text at that counting version. Increasing only the
    // counting version fails here
    assert_eq!(
        (GRAPHEME_RULES_VERSION, <Text as Package>::RULES_VERSION),
        (1, 1),
        "版を上げたら、下の見本を見直す（数え方を直したら、両方を上げる）"
    );
    let samples: &[(&str, usize)] = &[
        ("", 0),
        ("あいう漢字abc", 8),
        ("\r\n\n\r", 3),
        ("e\u{0301}か\u{3099}葛\u{E0100}", 3),
        ("\u{2764}\u{FE0F}👍🏽", 2),
        ("👨\u{200D}👩\u{200D}👧", 1),
        ("🇯🇵🇺🇸🇫", 3),
        ("\u{1100}\u{1161}", 2),
        ("\u{0915}\u{093F}", 2),
    ];
    for (s, n) in samples {
        assert_eq!(grapheme_count(s), *n, "{s:?}");
    }
}

/// Samples of rules other than counting. If the results change, increase `Text::RULES_VERSION`, then fix this table.
#[test]
fn non_counting_rule_samples_are_fixed_per_version() {
    assert_eq!(
        <Text as Package>::RULES_VERSION,
        1,
        "版を上げたら、下の見本を見直す"
    );
    // trim uses Unicode White_Space (removes the full-width space, keeps the zero-width space)
    assert_eq!("\u{3000} a\u{00A0}\t".trim(), "a");
    assert_eq!("\u{200B}a".trim(), "\u{200B}a");
    // upper and lower affect only ASCII letters
    assert_eq!("aéｚß".to_ascii_uppercase(), "Aéｚß");
    assert_eq!("ABÉＺ".to_ascii_lowercase(), "abÉＺ");
    // find gives the position of the cluster containing the match (a match that starts in the middle of a cluster gives that
    // cluster)
    assert_eq!(find("ae\u{0301}b", "e"), Some(1));
    assert_eq!(find("あいう", "う"), Some(2));
    assert_eq!(find("あいう", "x"), None);
}

#[test]
fn only_position_0_in_empty_string() {
    assert_eq!(grapheme_offsets(""), [0]);
    assert_eq!(graphemes("").count(), 0);
}

fn seed() -> u64 {
    std::env::var("YOMI_INVARIANT_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x7E87_0001)
}

#[test]
fn random_strings_round_trip_join_split_and_slice_concatenation() {
    let mut rng = Rng(seed() | 1);
    for _ in 0..cases() {
        let s = random_string(&mut rng, 12);
        let sep = [",", "→", "ab", "\n", "あい"][rng.below(5)];
        assert_eq!(join(&split(&s, sep).unwrap(), sep).unwrap(), s, "{s:?}");

        let len = grapheme_count(&s);
        // Wherever slice cuts, joining gives back the original, and the counts add up
        let i = rng.below(len + 1) as i64;
        let j = i + rng.below(len + 1 - i as usize) as i64;
        let a = slice(&s, 0, i).unwrap();
        let b = slice(&s, i, j).unwrap();
        let c = slice(&s, j, len as i64).unwrap();
        assert_eq!(format!("{a}{b}{c}"), s, "{s:?} {i} {j}");
        // Clusters are split from the front in order, so the number of pieces cut out equals the length of the range
        assert_eq!(grapheme_count(&b), (j - i) as usize, "{s:?} {i} {j}");
        assert_eq!(slice(&s, 0, len as i64).unwrap(), s);
    }
}

#[test]
fn random_strings_replace_is_idempotent_and_find_points_to_prefix() {
    let mut rng = Rng(seed().wrapping_add(7) | 1);
    for _ in 0..cases() {
        let s = random_string(&mut rng, 12);
        let from = random_string(&mut rng, 2);
        if from.is_empty() {
            continue;
        }
        // to does not contain from (if it did, each replacement could create a new match)
        let to = ["", "x", "yy", "→"][rng.below(4)];
        let once = replace(&s, &from, to).unwrap();
        let twice = replace(&once, &from, to).unwrap();
        // If from appears neither in to nor where replacements join, a second replacement does nothing
        if !to.contains(&from) && !once.contains(&from) {
            assert_eq!(once, twice);
        }
        assert_eq!(once, s.replace(&from, to));

        // find: cutting out and joining up to just before the position reaches the position of the match (the first code point
        // is inside the match)
        if let Some(i) = find(&s, &from) {
            assert!(i >= 0 && i as usize <= grapheme_count(&s));
            let head = slice(&s, 0, i).unwrap();
            assert!(s.starts_with(&head));
            assert!(s[head.len()..].contains(&from));
        } else {
            assert!(!s.contains(&from));
        }
    }
}

#[test]
fn random_input_never_panics_any_function_and_pad_fits_width() {
    let mut rng = Rng(seed().wrapping_add(13) | 1);
    for _ in 0..cases() {
        let s = random_string(&mut rng, 10);
        let t = random_string(&mut rng, 3);
        let n = rng.below(40) as i64 - 8;
        let m = rng.below(40) as i64 - 8;
        let _ = (
            ends_with(&s, &t),
            find(&s, &t),
            split(&s, &t),
            replace(&s, &t, &s),
        );
        let _ = (
            slice(&s, n, m),
            repeat(&s, n),
            join(&[s.clone(), t.clone()], &t),
        );
        let _ = (pad("t", &s, n, &t, true), pad("t", &s, m, &t, false));
        let _ = (s.to_ascii_uppercase(), s.to_ascii_lowercase());
        // The width grows until the number of clusters is width (when fill is one cluster)
        let fill = ["0", "あ", "e\u{0301}", "\u{1F1EF}\u{1F1F5}"][rng.below(4)];
        let width = rng.below(30);
        let len = grapheme_count(&s);
        for left in [true, false] {
            let out = pad("t", &s, width as i64, fill, left).unwrap();
            // Combining characters at the end or start can attach to fill and change the count, so count only when they do not
            // attach
            if !s.is_empty() && !is_extend(s.chars().next().unwrap()) && s != "\u{200D}" {
                let want = width.max(len);
                let got = grapheme_count(&out);
                assert!(
                    got <= want + 1,
                    "{s:?} {fill:?} {width} {left}: {got} > {want}"
                );
            }
            assert!(out.len() >= s.len());
        }
    }
}

#[test]
fn upper_and_lower_change_only_ascii_letters() {
    assert_eq!("aBc xyz".to_ascii_uppercase(), "ABC XYZ");
    assert_eq!("ß é ａ İ".to_ascii_uppercase(), "ß é ａ İ");
    assert_eq!("ABC ＡＢＣ É".to_ascii_lowercase(), "abc ＡＢＣ É");
}
