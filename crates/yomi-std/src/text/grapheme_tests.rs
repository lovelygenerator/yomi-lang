use super::grapheme_count as n;

#[test]
fn empty_string_has_0_chars() {
    assert_eq!(n(""), 0);
}

#[test]
fn kana_kanji_and_alphanumerics_count_one_each() {
    assert_eq!(n("あいう漢字abc"), 8);
}

#[test]
fn combining_marks_and_variation_selectors_join_previous_char() {
    assert_eq!(n("e\u{0301}"), 1);
    assert_eq!(n("か\u{3099}"), 1);
    assert_eq!(n("葛\u{E0100}"), 1);
    assert_eq!(n("\u{2764}\u{FE0F}"), 1);
}

#[test]
fn crlf_is_one_character_and_lone_cr_and_lf_count_separately() {
    assert_eq!(n("\r\n"), 1);
    assert_eq!(n("\n\n"), 2);
    assert_eq!(n("a\rb"), 3);
}

#[test]
fn zwj_joined_emoji_are_one_character() {
    // Family: man ZWJ woman ZWJ woman ZWJ man
    assert_eq!(
        n("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"),
        1
    );
    // Skin tone modifier
    assert_eq!(n("\u{1F44D}\u{1F3FD}"), 1);
}

#[test]
fn flag_is_two_regional_indicators_and_third_starts_next_flag() {
    assert_eq!(n("\u{1F1EF}\u{1F1F5}"), 1);
    assert_eq!(n("\u{1F1EF}\u{1F1F5}\u{1F1FA}\u{1F1F8}"), 2);
    assert_eq!(n("\u{1F1EF}"), 1);
    assert_eq!(n("\u{1F1EF}\u{1F1F5}\u{1F1FA}"), 2);
}

#[test]
fn zwj_before_non_emoji_includes_only_zwj_in_previous() {
    assert_eq!(n("a\u{200D}b"), 2);
}
