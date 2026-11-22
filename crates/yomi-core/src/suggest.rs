//! Candidates with close spellings (section 6.4 of `spec/language/06-diagnostics.md`). The edit distance is hand-written
//! (no added dependency).
//!
//! Names are normalized to NFC and compared as sequences of Unicode scalar values (section 6.4 of
//! `spec/language/06-diagnostics.md`). Candidates are also returned in NFC.

/// The maximum length (in characters) of a name to look for candidates. Names longer than this give no candidates,
/// because comparing them is costly.
const MAX_NAME_CHARS: usize = 64;

/// The maximum number of close candidates.
const MAX_SUGGESTIONS: usize = 3;

/// Levenshtein distance (insertions, deletions and substitutions each count as 1).
pub(crate) fn distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Chooses names close to `name` from `candidates` (names of the same kind). Up to 3, in order of distance (for equal
/// distances, in name order, by code point).
///
/// The distance considered close is 1 if the shorter of the two names is 4 characters or less, and 2 if it is 5 or more.
pub(crate) fn suggest<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let name = crate::ident::nfc(name);
    let target: Vec<char> = name.chars().take(MAX_NAME_CHARS + 1).collect();
    if target.len() > MAX_NAME_CHARS {
        return Vec::new();
    }
    let mut found: Vec<(usize, String)> = Vec::new();
    for cand in candidates {
        let cand = crate::ident::nfc(cand);
        if found.iter().any(|(_, c)| *c == *cand) {
            continue;
        }
        let chars: Vec<char> = cand.chars().take(MAX_NAME_CHARS + 1).collect();
        if chars.len() > MAX_NAME_CHARS {
            continue;
        }
        let limit = if target.len().min(chars.len()) <= 4 {
            1
        } else {
            2
        };
        if target.len().abs_diff(chars.len()) > limit {
            continue;
        }
        let d = distance(&target, &chars);
        if d >= 1 && d <= limit {
            found.push((d, cand.into_owned()));
        }
    }
    found.sort_unstable();
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn edit_distance_counts_insert_delete_and_replace_as_1() {
        assert_eq!(distance(&chars("kitten"), &chars("sitting")), 3);
        assert_eq!(distance(&chars(""), &chars("abc")), 3);
        assert_eq!(distance(&chars("abc"), &chars("abc")), 0);
        assert_eq!(distance(&chars("あいう"), &chars("あう")), 1);
    }

    #[test]
    fn short_names_close_at_distance_1_long_at_distance_2() {
        // The shorter one is 4 characters or less: distance 1
        assert_eq!(suggest("gld", ["gold"]), vec!["gold"]);
        assert!(suggest("gl", ["gold"]).is_empty());
        // The shorter one is 5 characters or more: distance 2
        assert_eq!(suggest("healht", ["health"]), vec!["health"]);
        assert_eq!(suggest("helth", ["health"]), vec!["health"]);
        assert_eq!(suggest("heatlh", ["health"]), vec!["health"]);
        assert!(suggest("hlt", ["health"]).is_empty());
    }

    #[test]
    fn suggestions_are_up_to_3_in_order_of_distance() {
        let c = ["abcdef", "abcdex", "abcdzz", "abcyzz", "abcdefg"];
        assert_eq!(suggest("abcdef!", c), vec!["abcdef", "abcdefg", "abcdex"]);
        // Equal distances are ordered by name
        assert_eq!(suggest("abcde", ["abcdy", "abcdx"]), vec!["abcdx", "abcdy"]);
    }

    #[test]
    fn no_suggestions_for_same_name_or_duplicates() {
        assert!(suggest("same", ["same"]).is_empty());
        assert_eq!(suggest("sane", ["same", "same"]), vec!["same"]);
    }

    #[test]
    fn japanese_names_are_compared_by_character_count() {
        assert_eq!(suggest("信頼", ["信頼度"]), vec!["信頼度"]);
        assert_eq!(suggest("信頼度", ["信頼"]), vec!["信頼"]);
        assert!(suggest("信", ["信頼度"]).is_empty());
    }

    #[test]
    fn very_long_names_get_no_suggestions() {
        let long = "a".repeat(100);
        assert!(suggest(&long, [long.as_str()]).is_empty());
        assert!(suggest("a", [long.as_str()]).is_empty());
    }
}
