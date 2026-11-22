//! Rules for identifiers (section 1.5 of spec/language/01-lexical.md).
//!
//! The start is `XID_Start` or `_`, and the rest is `XID_Continue` (UAX #31). Identifiers read from the source are
//! normalized to Unicode Normalization Form C (identifiers that look the same and differ only in the sequence of code
//! points become the same name). NFKC conversion is not done.

use std::borrow::Cow;
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

/// Whether the character can be the first character of an identifier (`XID_Start` or `_`).
pub(crate) fn is_ident_start(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_start(c)
}

/// Whether the character can be the second or later character of an identifier (`XID_Continue`, including `_`).
pub(crate) fn is_ident_continue(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_continue(c)
}

/// Whether the whole string has the form of an identifier token (does not check reserved words or NFC).
pub(crate) fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_continue)
}

/// Whether it is NFC.
pub(crate) fn is_nfc(s: &str) -> bool {
    match is_nfc_quick(s.chars()) {
        IsNormalized::Yes => true,
        IsNormalized::No => false,
        IsNormalized::Maybe => s.chars().nfc().eq(s.chars()),
    }
}

/// Normalizes to NFC. Does not copy if already NFC.
pub(crate) fn nfc(s: &str) -> Cow<'_, str> {
    if is_nfc(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfc().collect())
    }
}

/// Makes the identifier a string normalized to NFC.
pub(crate) fn normalize_name(s: &str) -> String {
    nfc(s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GA_D: &str = "\u{304B}\u{3099}";
    const GA_C: &str = "\u{304C}";

    #[test]
    fn identifier_start_and_continue_follow_uax31() {
        for c in ['a', 'Z', '_', 'あ', 'ア', '漢', 'é', 'α', 'ー', '々'] {
            assert!(is_ident_start(c), "{c}");
        }
        for c in [
            '1', '٣', '🐱', '-', ' ', '\u{3000}', '＋', '①', '$', '@', '#', '「', '\\',
        ] {
            assert!(!is_ident_start(c), "{c}");
        }
        // Digits and combining characters can be used in the rest
        for c in ['1', '٣', '_', '\u{3099}', '\u{301}'] {
            assert!(is_ident_continue(c), "{c}");
        }
        assert!(is_identifier("案内人2"));
        assert!(!is_identifier("2案内人"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("a b"));
    }

    #[test]
    fn nfc_unifies_composed_and_decomposed_and_does_not_copy_nfc() {
        assert_eq!(nfc(GA_D), GA_C);
        assert!(matches!(nfc(GA_C), Cow::Borrowed(_)));
        assert!(matches!(nfc("abc案内人"), Cow::Borrowed(_)));
        // No NFKC conversion (full-width and half-width are different)
        assert_eq!(nfc("ａ"), "ａ");
        assert!(is_nfc(GA_C) && !is_nfc(GA_D));
    }
}
