//! A one-line string, and the mapping of positions when multiple physical lines are joined.

use super::*;

/// The part of a joined line that corresponds to one physical line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seg {
    /// The start (byte) of this part in the joined string.
    pub start: usize,
    /// The line number of the physical line.
    pub line: u32,
    /// The column within the physical line of the first character of this part (0-based, in Unicode scalar values).
    pub col_base: u32,
}

/// A one-line string and its line number. When a quote line or option text spans multiple physical lines, it is the
/// string of the joined physical lines (with newlines in the source and the leading half-width spaces and tabs of
/// continuation lines removed), and `pos` maps back to the line and column of the physical line.
pub(crate) struct Line<'a> {
    pub text: &'a str,
    pub no: u32,
    /// A table from byte positions to character positions (0-based). Not kept for ASCII-only lines.
    /// Used to find columns in constant time without recounting from the start of the line.
    cols: Option<Cow<'a, [u32]>>,
    /// The parts of a joined line per physical line (empty if not joined).
    segs: Cow<'a, [Seg]>,
    /// Does not continue to later physical lines (a line cut out partway; quote lines are not extended to the next line).
    pub single: bool,
}

/// A table from character positions (bytes) to positions in characters. Not needed for ASCII only, so `None`.
fn char_cols(text: &str) -> Option<Vec<u32>> {
    if text.is_ascii() {
        None
    } else {
        let mut v = Vec::with_capacity(text.len() + 1);
        let mut n = 0u32;
        for c in text.chars() {
            for _ in 0..c.len_utf8() {
                v.push(n);
            }
            n = n.saturating_add(1);
        }
        v.push(n);
        Some(v)
    }
}

impl<'a> Line<'a> {
    /// Makes a token reader for one line (line number `no`).
    pub fn new(text: &'a str, no: u32) -> Self {
        Self {
            text,
            no,
            cols: char_cols(text).map(Cow::Owned),
            segs: Cow::Borrowed(&[]),
            single: false,
        }
    }

    /// A line of joined physical lines. `no` is the line number of the first physical line.
    pub fn joined(text: &'a str, no: u32, segs: Vec<Seg>) -> Self {
        Self {
            text,
            no,
            cols: char_cols(text).map(Cow::Owned),
            segs: Cow::Owned(segs),
            single: false,
        }
    }

    /// A line cut out up to the middle of the line (`end` bytes). The column and line number tables are shared (not rebuilt).
    pub fn truncated(&self, end: usize) -> Line<'_> {
        Line {
            text: &self.text[..end],
            no: self.no,
            cols: self.cols.as_deref().map(Cow::Borrowed),
            segs: Cow::Borrowed(&self.segs),
            single: true,
        }
    }

    /// The parts per physical line (a line that is not joined is one part covering the whole line).
    pub fn segments(&self) -> Vec<Seg> {
        if self.segs.is_empty() {
            vec![Seg {
                start: 0,
                line: self.no,
                col_base: 0,
            }]
        } else {
            self.segs.to_vec()
        }
    }

    /// Turns a byte position into a position in characters (column).
    fn char_index(&self, byte: usize) -> u32 {
        match &self.cols {
            None => byte as u32,
            Some(v) => v[byte],
        }
    }

    /// Turns a byte position in the line into a line and column (1-based, in Unicode scalar values).
    pub fn pos(&self, byte: usize) -> Pos {
        if self.segs.is_empty() {
            return Pos {
                line: self.no,
                col: self.char_index(byte).saturating_add(1),
            };
        }
        let idx = self.segs.partition_point(|s| s.start <= byte).max(1) - 1;
        let seg = self.segs[idx];
        let col = self
            .char_index(byte)
            .saturating_sub(self.char_index(seg.start))
            .saturating_add(seg.col_base);
        Pos {
            line: seg.line,
            col: col.saturating_add(1),
        }
    }
}
