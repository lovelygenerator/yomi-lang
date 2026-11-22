//! Statements whose expressions continue over several physical lines (an open bracket at the end of a line).

use super::*;

/// The most continuation lines after the first line.
const MAX_CONTINUATION_LINES: usize = 256;

/// The physical lines of one statement, joined into one line.
pub(super) struct Joined {
    pub text: String,
    pub segs: Vec<Seg>,
    /// The comments of the physical lines (dropped from `text`), in order.
    pub comments: Vec<Cmt>,
}

/// A comment inside a statement over several lines.
pub(super) struct Cmt {
    pub line: u32,
    pub text: String,
    /// The line has nothing but the comment.
    pub only: bool,
}

/// Whether the first line of a block statement: its `{` at the outermost level opens the block, not a literal.
pub(super) fn is_block_header(t: &str) -> bool {
    let word = first_word(t);
    word == "else" || starts_block(word)
}

/// Whether a line is classified as a statement line (②③ of 2.1), so that its brackets are counted.
fn is_statement_line(t: &str) -> bool {
    if t.starts_with('@') {
        return true;
    }
    let word = first_word(t);
    if word.is_empty() {
        return false;
    }
    if is_reserved(word) {
        return true;
    }
    let after = &t[word.len()..];
    if assign_op_at(after).is_some() {
        return true;
    }
    after.starts_with(['.', '['])
        && (STATEMENT_METHODS
            .iter()
            .any(|m| after.contains(&format!(".{m}(")))
            || has_assignment(after))
}

/// Whether there is an `=` that is not part of `==`, `=>`, `<=`, `>=` or `!=`.
fn has_assignment(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| {
        b[i] == b'='
            && !matches!(b.get(i + 1), Some(b'=' | b'>'))
            && !(i > 0 && matches!(b[i - 1], b'=' | b'!' | b'<' | b'>'))
    })
}

/// Counts the brackets of `text[from..to]` into `open` (the unclosed ones, with byte positions). Strings (including the
/// `${ }` in them) and comments are skipped. With `header`, a `{` while nothing is open (and the `{` of `.each {`) opens a
/// block and is not counted.
fn count_brackets(text: &str, from: usize, to: usize, header: bool, open: &mut Vec<(char, usize)>) {
    let mut in_str = false;
    let mut esc = false;
    // The number of open brackets outside each `${` that is open inside a string
    let mut interps: Vec<usize> = Vec::new();
    let b = text.as_bytes();
    let mut i = from;
    while i < to {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            } else if c == b'$' && b.get(i + 1) == Some(&b'{') {
                interps.push(open.len());
                open.push(('{', i + 1));
                in_str = false;
                i += 1;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'/' if b.get(i + 1) == Some(&b'/') => return,
                b'(' | b'[' => open.push((c as char, i)),
                b'{' => {
                    let each = text[..i].trim_end().ends_with(".each");
                    if !(open.is_empty() && (header || each)) {
                        open.push(('{', i));
                    }
                }
                b')' | b']' | b'}' => {
                    let want = match c {
                        b')' => '(',
                        b']' => '[',
                        _ => '{',
                    };
                    if open.last().is_some_and(|(o, _)| *o == want) {
                        open.pop();
                        if c == b'}' && interps.last() == Some(&open.len()) {
                            interps.pop();
                            in_str = true;
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
}

impl<'a> Parser<'a> {
    /// If a statement line (`raw`, line number `no`) ends with an open bracket, joins the following lines until the brackets
    /// close (2.8) and moves past them. `None` if the line does not continue. `start` is the byte position where counting
    /// begins.
    pub(super) fn join_statement(
        &mut self,
        raw: &'a str,
        no: u32,
        off: usize,
        start: usize,
    ) -> PResult<Option<Joined>> {
        let t = &raw[off..];
        if !self.join_lines || !is_statement_line(t) {
            return Ok(None);
        }
        let mut open = Vec::new();
        count_brackets(raw, start, raw.len(), is_block_header(t), &mut open);
        self.join_lines(raw, no, open, is_block_header(t))
    }

    /// Like [`Parser::join_statement`] for an option line of a `choice` (`「text」 #id => statement`): the brackets after the
    /// closing `」` are counted.
    pub(super) fn join_option(
        &mut self,
        raw: &'a str,
        no: u32,
        off: usize,
    ) -> PResult<Option<Joined>> {
        let t = &raw[off..];
        if !self.join_lines || !t.starts_with('「') {
            return Ok(None);
        }
        let mut depth = 0u32;
        let mut escaped = false;
        for (i, c) in t.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '「' => depth += 1,
                '」' => {
                    depth -= 1;
                    if depth == 0 {
                        return self.join_option_rest(raw, no, off + i + c.len_utf8());
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// The part of an option line after `」`: the condition, and the statement after `=>` if it is a statement line.
    fn join_option_rest(&mut self, raw: &'a str, no: u32, start: usize) -> PResult<Option<Joined>> {
        let mut open = Vec::new();
        let arrow = raw[start..].find("=>").map(|p| start + p);
        count_brackets(raw, start, arrow.unwrap_or(raw.len()), true, &mut open);
        if let Some(p) = arrow
            && open.is_empty()
            && is_statement_line(raw[p + 2..].trim_start())
        {
            count_brackets(raw, p + 2, raw.len(), true, &mut open);
        }
        self.join_lines(raw, no, open, true)
    }

    fn join_lines(
        &mut self,
        raw: &'a str,
        no: u32,
        mut open: Vec<(char, usize)>,
        header: bool,
    ) -> PResult<Option<Joined>> {
        if open.is_empty() {
            return Ok(None);
        }
        let first_open = open[0];
        let mut text = strip_comment(raw).trim_end().to_string();
        let mut comments = Vec::new();
        let rest = &raw[strip_comment(raw).len()..];
        if !rest.is_empty() {
            comments.push(Cmt {
                line: no,
                text: rest.trim_end().to_string(),
                only: false,
            });
        }
        let mut segs = vec![Seg {
            start: 0,
            line: no,
            col_base: 0,
        }];
        let first = Line::new(raw, no);
        let unclosed = |at: usize| {
            err(
                DiagCode::UnclosedBracket,
                vec![("open", DiagArg::Str(first_open.0.to_string()))],
                first.pos(at),
            )
        };
        let mut used = 0;
        while !open.is_empty() {
            if self.i >= self.lines.len() || used >= MAX_CONTINUATION_LINES {
                return unclosed(first_open.1);
            }
            let r = self.lines[self.i];
            let line_no = self.i as u32 + 1;
            self.i += 1;
            used += 1;
            let stripped = r.trim_start_matches([' ', '\t']);
            if stripped.is_empty() {
                continue;
            }
            if stripped.starts_with("//") {
                comments.push(Cmt {
                    line: line_no,
                    text: stripped.trim_end().to_string(),
                    only: true,
                });
                continue;
            }
            text.push(' ');
            segs.push(Seg {
                start: text.len(),
                line: line_no,
                col_base: (r.len() - stripped.len()) as u32,
            });
            let body = strip_comment(stripped).trim_end();
            let rest = &stripped[strip_comment(stripped).len()..];
            if !rest.is_empty() {
                comments.push(Cmt {
                    line: line_no,
                    text: rest.trim_end().to_string(),
                    only: false,
                });
            }
            let at = text.len();
            text.push_str(body);
            count_brackets(&text, at, text.len(), header, &mut open);
        }
        Ok(Some(Joined {
            text,
            segs,
            comments,
        }))
    }
}
