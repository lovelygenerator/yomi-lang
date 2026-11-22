//! Counting grapheme clusters (an approximation; see the parent module for documentation).

/// The version of the counting of grapheme clusters. Increase it by 1 each time the counting (the rules described in the
/// parent module) is fixed.
///
/// [`grapheme_count`], [`grapheme_offsets`], [`graphemes`], and the results of `text.len`, `text.slice`, `text.find` and
/// others in scripts do not change while this value stays the same. It is unrelated to the save format version.
/// Fixing the counting also changes the results of `text`, so increase `text`'s `Package::RULES_VERSION` together.
pub const GRAPHEME_RULES_VERSION: u32 = 1;

/// Whether the character attaches to the previous one within a grapheme cluster (combining characters, variation
/// selectors and so on).
pub(super) fn is_extend(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200C
            | 0x20D0..=0x20FF
            | 0x3099..=0x309A
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0x1F3FB..=0x1F3FF
            | 0xE0020..=0xE007F
            | 0xE0100..=0xE01EF
    )
}

/// Whether it is a regional indicator symbol (a part of a flag).
pub(super) fn is_regional_indicator(c: char) -> bool {
    matches!(u32::from(c), 0x1F1E6..=0x1F1FF)
}

/// Whether the character, coming after a ZWJ, joins the previous one (the range used for emoji).
pub(super) fn is_pictographic(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2190..=0x2BFF | 0x3030 | 0x303D | 0x1F000..=0x1FAFF
    )
}

/// Returns grapheme clusters as `&str` from the start, in order (an approximation; see the module description above).
pub(super) struct Clusters<'a> {
    pub(super) rest: &'a str,
}

impl<'a> Iterator for Clusters<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let mut chars = self.rest.char_indices().peekable();
        let (_, c) = chars.next()?;
        // Take in the attaching characters after the first character and decide the end of the cluster (byte position)
        let mut end = c.len_utf8();
        if c == '\r' {
            if let Some(&(i, '\n')) = chars.peek() {
                end = i + 1;
            }
        } else if c != '\n' && !c.is_control() {
            if is_regional_indicator(c)
                && let Some(&(i, n)) = chars.peek()
                && is_regional_indicator(n)
            {
                chars.next();
                end = i + n.len_utf8();
            }
            // Take in attaching characters and emoji joined with ZWJ
            while let Some(&(i, n)) = chars.peek() {
                if is_extend(n) {
                    chars.next();
                    end = i + n.len_utf8();
                } else if n == '\u{200D}' {
                    chars.next();
                    end = i + n.len_utf8();
                    if let Some(&(j, m)) = chars.peek()
                        && is_pictographic(m)
                    {
                        chars.next();
                        end = j + m.len_utf8();
                    }
                } else {
                    break;
                }
            }
        }
        let (head, tail) = self.rest.split_at(end);
        self.rest = tail;
        Some(head)
    }
}

/// Returns grapheme clusters as `&str` from the start, in order (an approximation; see the module description above).
///
/// The same boundaries as `text.len`, `text.slice` and the like. Concatenating the pieces gives back the original `s`.
///
/// ```
/// let v: Vec<&str> = yomi_std::text::graphemes("が👍🏽🇯🇵").collect();
/// assert_eq!(v, ["が", "👍🏽", "🇯🇵"]);
/// ```
pub fn graphemes(s: &str) -> impl Iterator<Item = &str> {
    Clusters { rest: s }
}

/// The number of grapheme clusters (an approximation; see the module description above). The same value as `text.len`.
///
/// ```
/// assert_eq!(yomi_std::text::grapheme_count("か\u{3099}な"), 2);
/// assert_eq!(yomi_std::text::grapheme_count(""), 0);
/// ```
pub fn grapheme_count(s: &str) -> usize {
    Clusters { rest: s }.count()
}

/// The byte positions of the starts of grapheme clusters, followed by `s.len()` (the length is "number of clusters + 1").
///
/// `offsets[i]` is the start of the `i`-th cluster, and `offsets[n]` (`n` is the number) is the end (`s.len()`).
/// Position `i` of `text.find` and `text.slice` corresponds to the byte position of `s[offsets[i]..]`.
/// For an empty string, `[0]`.
///
/// ```
/// let s = "a👍🏽b";
/// let offsets = yomi_std::text::grapheme_offsets(s);
/// assert_eq!(offsets, [0, 1, 9, 10]);
/// assert_eq!(&s[offsets[1]..offsets[2]], "👍🏽");
/// ```
pub fn grapheme_offsets(s: &str) -> Vec<usize> {
    let mut v = Vec::new();
    let mut pos = 0;
    for c in (Clusters { rest: s }) {
        v.push(pos);
        pos += c.len();
    }
    v.push(pos);
    v
}
