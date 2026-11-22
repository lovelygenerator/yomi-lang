//! Position conversion. Script columns are in characters (`char`); LSP columns are in UTF-16 code units.

/// From a UTF-16 column (0-based) in a line to a character column (1-based). If beyond the line, the end of the line.
pub fn char_col(line: &str, utf16: u32) -> u32 {
    let mut units = 0u32;
    for (i, c) in line.chars().enumerate() {
        if units >= utf16 {
            return i as u32 + 1;
        }
        units += c.len_utf16() as u32;
    }
    line.chars().count() as u32 + 1
}

/// From a character column (1-based) in a line to a UTF-16 column (0-based).
pub fn utf16_col(line: &str, col: u32) -> u32 {
    line.chars()
        .take(col.saturating_sub(1) as usize)
        .map(|c| c.len_utf16() as u32)
        .sum()
}

/// The length in characters of the name (alphanumerics and `_`) at a character column (1-based). 1 if it is not a name.
pub fn name_len_at(line: &str, col: u32) -> u32 {
    let n = line
        .chars()
        .skip(col.saturating_sub(1) as usize)
        .take_while(|c| yomi_core::is_name_char(*c))
        .count() as u32;
    n.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_character_columns_convert_both_ways() {
        // "😀" is 2 units in UTF-16 and 1 character
        let line = "a😀いb";
        assert_eq!(utf16_col(line, 1), 0);
        assert_eq!(utf16_col(line, 3), 3);
        assert_eq!(utf16_col(line, 5), 5);
        assert_eq!(char_col(line, 0), 1);
        assert_eq!(char_col(line, 3), 3);
        assert_eq!(char_col(line, 4), 4);
        assert_eq!(char_col(line, 99), 5);
    }

    #[test]
    fn name_length_is_1_when_not_a_name() {
        assert_eq!(name_len_at("jump 次へ!", 6), 2);
        assert_eq!(name_len_at("jump 次へ!", 8), 1);
        assert_eq!(name_len_at("", 1), 1);
    }
}
