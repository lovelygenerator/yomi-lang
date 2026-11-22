//! The formatter (`yomi fmt`; chapter 8 of `spec/language/08-format.md`). It works line by line: each line is read with
//! the parser of the same module, and written in the standard form.

use super::*;
use crate::syntax::canon;

mod decl;
mod layout;
pub(crate) mod migrations;
mod stmt;

/// Why a source could not be formatted.
#[derive(Debug)]
pub enum FormatError {
    /// The source has syntax errors (the diagnostics).
    Syntax(Vec<Diagnostic>),
    /// The formatted source does not mean the same as the original, so nothing can be written.
    Unverified,
}

enum Out {
    Blank,
    /// A line with its nesting level.
    Line(usize, String),
    /// A line of text (never read as opening a block).
    Text(usize, String),
    /// A line that is never removed or merged (inside a multi-line quote line).
    Keep(usize, String),
    /// An option of `choice` whose `=>` is aligned with the other options of the same `choice`.
    Opt {
        depth: usize,
        group: usize,
        head: String,
        width: usize,
        body: String,
    },
}

enum Ctx {
    Block,
    Match,
    Choice(usize),
}

pub(super) struct Formatter<'a> {
    p: Parser<'a>,
    out: Vec<Out>,
    ctx: Vec<Ctx>,
    groups: usize,
    /// The multi-line statement being written: its layout and the groups to expand.
    layout: Option<(layout::Layout, Vec<usize>)>,
}

type FResult<T> = Result<T, FormatError>;

fn syntax<T>(r: PResult<T>) -> FResult<T> {
    r.map_err(|e| {
        FormatError::Syntax(vec![Diagnostic {
            module: String::new(),
            line: e.pos.line,
            col: e.pos.col,
            severity: Severity::Error,
            code: e.code,
            args: e.args,
        }])
    })
}

/// The comment at the end of a line (`// …`), if any.
fn comment_of(raw: &str) -> Option<String> {
    let rest = &raw[strip_comment(raw).len()..];
    (!rest.is_empty()).then(|| rest.trim_end().to_string())
}

fn with_comment(text: String, comment: Option<String>) -> String {
    match comment {
        Some(c) => format!("{text}  {c}"),
        None => text,
    }
}

/// The display width of a string (8.6).
pub(super) fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

impl<'a> Formatter<'a> {
    pub(super) fn new(source: &'a str, module: &'a str, idents: &'a IdentMap) -> Self {
        Formatter {
            p: Parser::new(source, module, idents),
            out: Vec::new(),
            ctx: Vec::new(),
            groups: 0,
            layout: None,
        }
    }

    fn depth(&self) -> usize {
        self.ctx.len()
    }

    fn line(&mut self, text: String) {
        let d = self.depth();
        let Some((mut lay, used)) = self.layout.take() else {
            self.out.push(Out::Line(d, text));
            return;
        };
        let mut lines = lay.expand(&text, &used).unwrap_or_else(|_| vec![text]);
        let mut rest = Vec::new();
        for (c, only) in lay.leftovers() {
            if only {
                rest.push(c);
            } else {
                let last = lines.len() - 1;
                lines[last] = format!("{}  {c}", lines[last]);
            }
        }
        for l in lines {
            self.out.push(Out::Line(d, l));
        }
        for c in rest {
            self.out.push(Out::Line(d, c));
        }
    }

    pub(super) fn run(&mut self) -> FResult<()> {
        while self.p.i < self.p.lines.len() {
            let raw = self.p.lines[self.p.i];
            let no = self.p.i as u32 + 1;
            self.p.i += 1;
            let off = indent_len(raw);
            let t = &raw[off..];
            if t.is_empty() {
                self.out.push(Out::Blank);
                continue;
            }
            if t.starts_with("//") {
                self.line(t.trim_end().to_string());
                continue;
            }
            match self.ctx.last() {
                None => self.top_line(raw, no, off)?,
                Some(Ctx::Block) => self.block_line(raw, no, off)?,
                Some(Ctx::Match) => self.arm_line(raw, no, off)?,
                Some(Ctx::Choice(_)) => self.option_line(raw, no, off)?,
            }
        }
        Ok(())
    }

    /// A line in a block: a statement, or the `}` that closes it.
    fn block_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        let t = &raw[off..];
        if t.starts_with('}') {
            return self.close_line(raw, no, off);
        }
        self.statement_line(raw, no, off)
    }

    /// A `}` line (also `} else {` and `} else if … {`, and `},` after an arm).
    fn close_line(&mut self, raw: &'a str, no: u32, off: usize) -> FResult<()> {
        let comment = comment_of(raw);
        let tail = strip_comment(&raw[off + 1..]).trim();
        let closed = self.ctx.pop();
        if let Some(Ctx::Choice(g)) = closed {
            self.finish_choice(g);
        }
        if tail.is_empty() || tail == "," {
            let d = self.depth();
            self.out
                .push(Out::Line(d, with_comment("}".to_string(), comment)));
            return Ok(());
        }
        let line = Line::new(raw, no);
        let mut ep = ExprParser::new(&line, off + 1);
        ep.skip_ws().ok();
        let kw = off + 1 + (raw[off + 1..].len() - raw[off + 1..].trim_start().len());
        let after_else = kw + "else".len();
        let rest = raw[after_else..].trim_start();
        let text = if rest.starts_with("if") {
            let at = raw.len() - rest.len() + 2;
            let cond = syntax(self.header_cond(&line, at))?;
            format!("}} else if {} {{", canon::expr(&cond))
        } else {
            "} else {".to_string()
        };
        let d = self.depth();
        self.out.push(Out::Line(d, with_comment(text, comment)));
        self.ctx.push(Ctx::Block);
        Ok(())
    }

    /// Reads `expr {` after the keyword.
    fn header_cond(&mut self, line: &Line<'_>, after_kw: usize) -> PResult<Expr> {
        let mut ep = ExprParser::new_header(line, after_kw);
        let cond = ep.parse_expr()?;
        ep.expect_char('{')?;
        Ok(cond)
    }

    /// Resolves the alignment of the options of one `choice`.
    fn finish_choice(&mut self, group: usize) {
        let max = self
            .out
            .iter()
            .filter_map(|o| match o {
                Out::Opt {
                    group: g, width, ..
                } if *g == group => Some(*width),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        for o in &mut self.out {
            if let Out::Opt {
                depth,
                group: g,
                head,
                width,
                body,
            } = o
                && *g == group
            {
                let pad = if *width == usize::MAX {
                    0
                } else {
                    max - *width
                };
                let text = format!("{head}{} => {body}", " ".repeat(pad));
                *o = Out::Line(*depth, text);
            }
        }
    }

    /// The final text: indentation, and the rules for empty lines (8.3).
    pub(super) fn finish(self) -> String {
        let mut s = String::new();
        let mut blank_pending = false;
        let mut prev_opens = true;
        for o in self.out {
            match o {
                Out::Blank => blank_pending = true,
                Out::Line(d, t) => {
                    if blank_pending && !prev_opens && !t.starts_with('}') {
                        s.push('\n');
                    }
                    blank_pending = false;
                    prev_opens = code_ends_with_open(&t);
                    s.push_str(&"    ".repeat(d));
                    s.push_str(&t);
                    s.push('\n');
                }
                Out::Text(d, t) => {
                    if blank_pending && !prev_opens {
                        s.push('\n');
                    }
                    blank_pending = false;
                    prev_opens = false;
                    s.push_str(&"    ".repeat(d));
                    s.push_str(&t);
                    s.push('\n');
                }
                Out::Keep(d, t) => {
                    blank_pending = false;
                    prev_opens = false;
                    if !t.is_empty() {
                        s.push_str(&"    ".repeat(d));
                        s.push_str(&t);
                    }
                    s.push('\n');
                }
                Out::Opt { .. } => unreachable!("選択肢のそろえは終わっている"),
            }
        }
        s
    }
}

/// Whether a written line opens a block with `{` (a comment at the end is ignored).
fn code_ends_with_open(t: &str) -> bool {
    strip_comment(t).trim_end().ends_with('{')
}

/// Removes the positions from the debug text of a module, so that two modules can be compared by what they say.
pub(super) fn scrub_positions(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len());
    let mut rest = debug;
    while let Some(i) = rest.find("Pos {") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find('}') {
            Some(j) => rest = &tail[j + 1..],
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Formats a module. The result is checked by reading it again: it must say the same as the source (8.1).
pub(crate) fn format_module(
    source: &str,
    module: &str,
    idents: &IdentMap,
) -> Result<String, FormatError> {
    let (before, diags) = parse_with(source, module, idents);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(FormatError::Syntax(diags));
    }
    let mut f = Formatter::new(source, module, idents);
    f.run()?;
    let text = f.finish();
    let (after, diags) = parse_with(&text, module, idents);
    if diags.iter().any(|d| d.severity == Severity::Error)
        || scrub_positions(&format!("{before:?}")) != scrub_positions(&format!("{after:?}"))
    {
        return Err(FormatError::Unverified);
    }
    Ok(text)
}
