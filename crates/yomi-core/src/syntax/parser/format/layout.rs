//! The formatter: statements whose expressions are written over several lines (8.5a).
//!
//! The groups of brackets that span several lines are replaced by markers (`__yg0__`) while the statement is read and
//! written in the standard form on one line; then each marker is expanded into the lines of its group.

use super::super::Seg;
use super::super::continuation::{Cmt, Joined};
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paren,
    Bracket,
    Map,
    Struct,
}

#[derive(Clone, Copy)]
struct Group {
    open: usize,
    close: usize,
    kind: Kind,
}

impl Group {
    fn marker_text(&self, index: usize) -> String {
        match self.kind {
            Kind::Paren => format!("(__yg{index}__)"),
            Kind::Bracket => format!("[__yg{index}__]"),
            Kind::Map | Kind::Struct => format!("{{ __yg{index}__: 0 }}"),
        }
    }

    fn opener(&self) -> &'static str {
        match self.kind {
            Kind::Paren => "(",
            Kind::Bracket => "[",
            Kind::Map | Kind::Struct => "{",
        }
    }

    fn closer(&self) -> &'static str {
        match self.kind {
            Kind::Paren => ")",
            Kind::Bracket => "]",
            Kind::Map | Kind::Struct => "}",
        }
    }
}

/// The end of the string literal that starts at `start` (the byte after the closing `"`), skipping `${ }`.
fn string_end(text: &str, start: usize) -> usize {
    let b = text.as_bytes();
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            b'$' if b.get(i + 1) == Some(&b'{') => {
                i += 2;
                let mut depth = 1;
                while i < b.len() && depth > 0 {
                    match b[i] {
                        b'"' => i = string_end(text, i),
                        b'{' => {
                            depth += 1;
                            i += 1;
                        }
                        b'}' => {
                            depth -= 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            _ => i += 1,
        }
    }
    b.len()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A multi-line statement: its text, where the lines were, and its comments.
pub(super) struct Layout {
    text: String,
    segs: Vec<Seg>,
    groups: Vec<Group>,
    comments: Vec<(Cmt, bool)>,
}

impl Layout {
    /// `None` if no group of brackets spans lines.
    pub(super) fn new(j: &Joined, header: bool) -> Option<Self> {
        let mut lay = Layout {
            text: j.text.clone(),
            segs: j.segs.clone(),
            groups: Vec::new(),
            comments: j
                .comments
                .iter()
                .map(|c| {
                    (
                        Cmt {
                            line: c.line,
                            text: c.text.clone(),
                            only: c.only,
                        },
                        false,
                    )
                })
                .collect(),
        };
        lay.scan(header);
        (!lay.groups.is_empty()).then_some(lay)
    }

    fn line_of(&self, byte: usize) -> u32 {
        let idx = self.segs.partition_point(|s| s.start <= byte).max(1) - 1;
        self.segs[idx].line
    }

    fn scan(&mut self, header: bool) {
        let text = self.text.clone();
        let text = text.as_str();
        let b = text.as_bytes();
        let mut stack: Vec<(u8, usize, Option<Kind>)> = Vec::new();
        let mut found = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            match c {
                b'"' => {
                    i = string_end(text, i);
                    continue;
                }
                b'(' => {
                    let call = text[..i]
                        .chars()
                        .next_back()
                        .is_some_and(|p| is_word_char(p) || p == ')' || p == ']');
                    stack.push((c, i, call.then_some(Kind::Paren)));
                }
                b'[' => stack.push((c, i, Some(Kind::Bracket))),
                b'{' => {
                    let before = text[..i].trim_end();
                    let each = before.ends_with(".each");
                    if !(stack.is_empty() && (header || each)) {
                        let word_end = before.chars().next_back().is_some_and(is_word_char);
                        let word: String = before
                            .chars()
                            .rev()
                            .take_while(|c| is_word_char(*c) || *c == '.')
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        let kind = if word == "map" {
                            Some(Kind::Map)
                        } else if word_end {
                            Some(Kind::Struct)
                        } else {
                            None
                        };
                        stack.push((c, i, kind));
                    }
                }
                b')' | b']' | b'}' => {
                    let want = match c {
                        b')' => b'(',
                        b']' => b'[',
                        _ => b'{',
                    };
                    if let Some((_, open, Some(kind))) = stack.pop_if(|t| t.0 == want)
                        && self.line_of(open) != self.line_of(i)
                    {
                        found.push(Group {
                            open,
                            close: i,
                            kind,
                        });
                    }
                }
                _ => {}
            }
            i += 1;
        }
        found.sort_by_key(|g| g.open);
        self.groups = found;
    }

    /// The indices of the outermost groups inside `a..b`.
    fn outermost(&self, a: usize, b: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut end = a;
        for (k, g) in self.groups.iter().enumerate() {
            if g.open >= end && g.open >= a && g.close < b {
                out.push(k);
                end = g.close + 1;
            }
        }
        out
    }

    /// `text[a..b]` with the groups replaced by markers.
    fn substitute(&self, a: usize, b: usize, used: &[usize]) -> String {
        let mut s = String::new();
        let mut at = a;
        for &k in used {
            let g = &self.groups[k];
            s.push_str(&self.text[at..g.open]);
            s.push_str(&g.marker_text(k));
            at = g.close + 1;
        }
        s.push_str(&self.text[at..b]);
        s
    }

    /// The text of the whole statement with the outermost groups replaced.
    pub(super) fn marked_text(&self) -> (String, Vec<usize>) {
        let used = self.outermost(0, self.text.len() + 1);
        (self.substitute(0, self.text.len(), &used), used)
    }

    fn take_trailing(&mut self, line: u32) -> Option<String> {
        self.comments
            .iter_mut()
            .find(|(c, used)| !*used && !c.only && c.line == line)
            .map(|(c, used)| {
                *used = true;
                c.text.clone()
            })
    }

    /// The comment-only lines before `line` that have not been written yet.
    fn take_before(&mut self, line: u32) -> Vec<String> {
        let mut out = Vec::new();
        for (c, used) in &mut self.comments {
            if !*used && c.only && c.line < line {
                *used = true;
                out.push(c.text.clone());
            }
        }
        out
    }

    /// Everything that is left (appended after the statement).
    pub(super) fn leftovers(&mut self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for (c, used) in &mut self.comments {
            if !*used {
                *used = true;
                out.push((c.text.clone(), c.only));
            }
        }
        out
    }

    /// Expands the markers in `canon` (the standard form of one line) into lines. Later lines are written relative to the
    /// first (4 spaces per level).
    pub(super) fn expand(&mut self, canon: &str, used: &[usize]) -> FResult<Vec<String>> {
        let mut lines = Vec::new();
        let mut cur = canon.to_string();
        loop {
            let next = used
                .iter()
                .filter_map(|&k| {
                    let m = self.groups[k].marker_text(k);
                    cur.find(&m).map(|p| (p, k, m))
                })
                .min_by_key(|(p, ..)| *p);
            let Some((pos, k, marker)) = next else {
                break;
            };
            let g = self.groups[k];
            let mut head = format!("{}{}", &cur[..pos], g.opener());
            if let Some(c) = self.take_trailing(self.line_of(g.open)) {
                head = format!("{head}  {c}");
            }
            lines.push(head);
            lines.extend(self.group_lines(k)?);
            cur = format!("{}{}", g.closer(), &cur[pos + marker.len()..]);
        }
        lines.push(cur);
        Ok(lines)
    }

    /// The elements of the group `k`, one level deeper, each ended with `,`.
    fn group_lines(&mut self, k: usize) -> FResult<Vec<String>> {
        let g = self.groups[k];
        let mut elems = Vec::new();
        for (a, b) in self.split_elements(g.open + 1, g.close) {
            let used = self.outermost(a, b);
            let et = self.substitute(a, b, &used);
            let (key, value) = element_text(g.kind, &et)?;
            let before = self.take_before(self.line_of(a));
            let value_lines = self.expand(&value, &used)?;
            let end_comment = self.take_trailing(self.line_of(b - 1));
            elems.push((before, key, value_lines, end_comment));
        }
        let width = elems
            .iter()
            .filter_map(|e| e.1.as_ref())
            .map(|k| display_width(k))
            .max()
            .unwrap_or(0);
        let mut out = Vec::new();
        for (before, key, mut value_lines, end_comment) in elems {
            for c in before {
                out.push(format!("    {c}"));
            }
            if let Some(key) = &key {
                let pad = " ".repeat(width - display_width(key) + 1);
                value_lines[0] = format!("{key}{pad}{}", value_lines[0]);
            }
            let last = value_lines.len() - 1;
            value_lines[last].push(',');
            if let Some(c) = end_comment {
                value_lines[last] = format!("{}  {c}", value_lines[last]);
            }
            for l in value_lines {
                out.push(format!("    {l}"));
            }
        }
        for c in self.take_before(self.line_of(g.close)) {
            out.push(format!("    {c}"));
        }
        Ok(out)
    }

    /// The byte ranges of the elements between `a` and `b` (split at commas outside brackets; empty ones dropped).
    fn split_elements(&self, a: usize, b: usize) -> Vec<(usize, usize)> {
        let text = self.text.as_str();
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut start = a;
        let mut i = a;
        let push = |s: usize, e: usize, out: &mut Vec<(usize, usize)>| {
            let part = &text[s..e];
            let t = part.trim_start();
            let lead = part.len() - t.len();
            let t = t.trim_end();
            if !t.is_empty() {
                out.push((s + lead, s + lead + t.len()));
            }
        };
        while i < b {
            match bytes[i] {
                b'"' => {
                    i = string_end(text, i);
                    continue;
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    push(start, i, &mut out);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        push(start, b, &mut out);
        out
    }
}

/// The standard form of one element: the key part (`key:` for `map` and `struct` literals) and the value.
fn element_text(kind: Kind, et: &str) -> FResult<(Option<String>, String)> {
    let unverified = |_| FormatError::Unverified;
    match kind {
        Kind::Paren => {
            let src = format!("{et})");
            let line = Line::new(&src, 1);
            let mut ep = ExprParser::new(&line, 0);
            let args = ep.parse_call_args().map_err(unverified)?;
            Ok((None, canon::args(&args)))
        }
        Kind::Bracket => {
            let line = Line::new(et, 1);
            let mut ep = ExprParser::new(&line, 0);
            let e = ep.parse_expr().map_err(unverified)?;
            Ok((None, canon::expr(&e)))
        }
        Kind::Map | Kind::Struct => {
            let colon = top_level_colon(et).ok_or(FormatError::Unverified)?;
            let (k, v) = (et[..colon].trim(), et[colon + 1..].trim());
            let key = if kind == Kind::Map {
                let line = Line::new(k, 1);
                let mut ep = ExprParser::new(&line, 0);
                canon::expr(&ep.parse_expr().map_err(unverified)?)
            } else {
                k.to_string()
            };
            let line = Line::new(v, 1);
            let mut ep = ExprParser::new(&line, 0);
            let value = canon::expr(&ep.parse_expr().map_err(unverified)?);
            Ok((Some(format!("{key}:")), value))
        }
    }
}

/// The first `:` outside brackets and strings.
fn top_level_colon(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i = string_end(s, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}
