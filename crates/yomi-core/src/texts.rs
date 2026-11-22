//! Text tables that replace bodies (localization; `spec/host/05-texts.md`).
//!
//! A text table maps, per language, keys (`module.label#id`; for identity display names, `identity:module.name`) to
//! translations. A translation is a body in the same form as the original, and can use body escapes (section 1.10 of
//! `spec/language/01-lexical.md`) and `${n}`, which refers to the `n`-th `${expr}` of the original. Translations are
//! parsed when put into the table (so that the positions of `${n}` can be looked up right away).
//!
//! Errors are held only as codes and arguments (no message text).

use std::collections::BTreeMap;

use crate::diag::Severity;
use crate::program::{Program, StopKind, TextArg, TextPart};
use crate::value::{Identity, Value};

/// The kind of text table error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextErrorKind {
    /// An escape that cannot be used (in a translation). Argument `found` (`\` and the next character).
    InvalidEscape {
        /// The spelling found.
        found: String,
    },
    /// A `${…}` that is not in the form `${n}` (`n` is an integer of 1 or more), or an unclosed `${` (in a translation).
    /// Argument `found`.
    InvalidPlaceholder {
        /// The spelling found.
        found: String,
    },
    /// A line of the table file has no TAB separating the key and the translation.
    MissingTab,
    /// The key of a line of the table file is empty.
    EmptyKey,
    /// The same key appears twice in the table file.
    DuplicateKey,
    /// An unknown directive in the table file (an `@…` other than `@lang`). Argument `name`.
    UnknownDirective {
        /// The name of the directive (without `@`).
        name: String,
    },
}

impl TextErrorKind {
    /// The stable English name.
    pub fn code(&self) -> &'static str {
        match self {
            TextErrorKind::InvalidEscape { .. } => "invalid_escape",
            TextErrorKind::InvalidPlaceholder { .. } => "invalid_placeholder",
            TextErrorKind::MissingTab => "missing_tab",
            TextErrorKind::EmptyKey => "empty_key",
            TextErrorKind::DuplicateKey => "duplicate_key",
            TextErrorKind::UnknownDirective { .. } => "unknown_directive",
        }
    }
}

/// An error in a translation that could not be put into the table, or in a line of the table file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    /// Kind.
    pub kind: TextErrorKind,
    /// The key of the translation with the error (when known).
    pub key: Option<String>,
    /// The line of the table file (1-based; only when reading a file).
    pub line: Option<u32>,
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind.code())?;
        if let Some(k) = &self.key {
            write!(f, " (key {k})")?;
        }
        if let Some(n) = self.line {
            write!(f, " (line {n})")?;
        }
        Ok(())
    }
}

impl std::error::Error for TextError {}

/// The kind of result of checking a text table against the scripts ([`TextTable::check`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextIssueKind {
    /// A missing translation. A key that is in the scripts but not in the table (warning).
    Missing,
    /// An extra translation. A key that is in the table but not in the scripts (warning).
    Unused,
    /// A number error. The `n` of `${n}` in a translation exceeds the number of `${expr}` in the original (error).
    BadNumber {
        /// The `n` of `${n}` in the translation (the largest among those that exceed).
        number: u32,
        /// The number of `${expr}` in the original.
        count: u32,
    },
}

impl TextIssueKind {
    /// The stable English name.
    pub fn code(&self) -> &'static str {
        match self {
            TextIssueKind::Missing => "missing",
            TextIssueKind::Unused => "unused",
            TextIssueKind::BadNumber { .. } => "bad_number",
        }
    }

    /// Severity (missing and extra translations are warnings, number errors are errors).
    pub fn severity(&self) -> Severity {
        match self {
            TextIssueKind::BadNumber { .. } => Severity::Error,
            _ => Severity::Warning,
        }
    }
}

/// One result of checking a text table against the scripts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIssue {
    /// Severity.
    pub severity: Severity,
    /// The key (`module.label#id`).
    pub key: String,
    /// Kind.
    pub kind: TextIssueKind,
}

impl TextIssue {
    /// Makes an issue for an item. The severity is decided by the kind.
    fn new(key: &str, kind: TextIssueKind) -> Self {
        Self {
            severity: kind.severity(),
            key: key.to_string(),
            kind,
        }
    }
}

/// A parsed part of a translation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// Characters (after unescaping).
    Lit(String),
    /// The value of the `n`-th (1-based) `${expr}` of the original.
    Arg(u32),
}

/// One item of the translation table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: String,
    /// The translation as written.
    raw: String,
    parts: Vec<Part>,
    /// The largest `n` of `${n}` (0 if none).
    max_arg: u32,
}

/// A per-language table that replaces bodies.
///
/// Keys are `module.label#id` (the same form as passed stop points). Values are bodies in the same form as the original.
/// Pass it to the VM with [`Vm::set_texts`](crate::Vm::set_texts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTable {
    language: String,
    /// In insertion order.
    entries: Vec<Entry>,
    /// Key → index into `entries`.
    index: BTreeMap<String, usize>,
}

impl TextTable {
    /// Makes an empty table. `language` is the name of the language (such as `en`; the core gives it no meaning).
    pub fn new(language: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            entries: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    /// The name of the language.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Puts a translation. If the same key exists, replaces it.
    ///
    /// A translation is a body in the same form as the original. Body escapes (`\「` `\」` `\$` `\\` `\=` `\#`) can be used.
    /// `${n}` (`n` is an integer of 1 or more) becomes the value of the `n`-th `${expr}` of the original. Any other `\`
    /// combinations and `${…}` are errors (the table is unchanged). Whether `n` exceeds the count in the original is not
    /// checked here, because the scripts are unknown ([`Vm::set_texts`](crate::Vm::set_texts) and [`TextTable::check`] check
    /// it).
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), TextError> {
        let key = key.into();
        let raw = value.into();
        let parts = parse_value(&raw).map_err(|kind| TextError {
            kind,
            key: Some(key.clone()),
            line: None,
        })?;
        let max_arg = parts
            .iter()
            .map(|p| match p {
                Part::Arg(n) => *n,
                Part::Lit(_) => 0,
            })
            .max()
            .unwrap_or(0);
        let entry = Entry {
            key: key.clone(),
            raw,
            parts,
            max_arg,
        };
        match self.index.get(&key) {
            Some(&i) => self.entries[i] = entry,
            None => {
                self.index.insert(key, self.entries.len());
                self.entries.push(entry);
            }
        }
        Ok(())
    }

    /// The translation of a key (as written).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.index.get(key).map(|&i| self.entries[i].raw.as_str())
    }

    /// The number of translations.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `(key, translation)` in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|e| (e.key.as_str(), e.raw.as_str()))
    }

    /// Checks against the scripts. Results are in script order (missing translations, number errors), followed by table order
    /// (extra translations).
    ///
    /// - Missing translation (warning): a key of a body with a stable ID in the scripts that is not in the table.
    /// - Extra translation (warning): a key in the table that is not in the scripts.
    /// - Number error (error): the `n` of `${n}` exceeds the number of `${expr}` in the original.
    pub fn check(&self, program: &Program) -> Vec<TextIssue> {
        let mut out = Vec::new();
        let script = program.text_entries();
        for (key, parts) in &script {
            match self.index.get(key) {
                None => out.push(TextIssue::new(key, TextIssueKind::Missing)),
                Some(&i) => {
                    let count = interp_count(parts);
                    let e = &self.entries[i];
                    if e.max_arg > count {
                        out.push(TextIssue::new(
                            key,
                            TextIssueKind::BadNumber {
                                number: e.max_arg,
                                count,
                            },
                        ));
                    }
                }
            }
        }
        let known: std::collections::HashSet<&str> =
            script.iter().map(|(k, _)| k.as_str()).collect();
        for e in &self.entries {
            if !known.contains(e.key.as_str()) {
                out.push(TextIssue::new(&e.key, TextIssueKind::Unused));
            }
        }
        out
    }

    /// Makes a table (a template) whose translations are the originals, for all bodies with stable IDs in the scripts.
    /// `${expr}` becomes a numbered `${n}` (in the order written). Translators rewrite it into translations.
    pub fn template(program: &Program, language: impl Into<String>) -> Self {
        let mut table = Self::new(language);
        for (key, parts) in program.text_entries() {
            let mut value = String::new();
            let mut n = 0;
            for p in parts {
                match p {
                    TextPart::Lit(t) => escape_literal(t, &mut value),
                    TextPart::Interp => {
                        n += 1;
                        value.push_str(&format!("${{{n}}}"));
                    }
                }
            }
            table
                .insert(key, value)
                .expect("原文から作った訳文は解析できる");
        }
        table
    }

    /// Reads a table from the text of a file (a line-oriented format for tools; section 5.7 of `spec/host/05-texts.md`).
    ///
    /// - Empty lines are skipped. Lines starting with `#` are comments.
    /// - A line `@lang language` sets the language name (empty if absent).
    /// - Other lines are `key` TAB `translation`. In translations, `\n` is a newline, `\t` a TAB and `\r` a carriage return.
    ///   Other `\` sequences are translation escapes (such as `\\`).
    /// - A key appearing twice, or a translation that is an error in [`TextTable::insert`], gives an error with the line.
    pub fn from_file(source: &str) -> Result<Self, TextError> {
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        let mut table = Self::new("");
        for (i, line) in source.lines().enumerate() {
            let no = (i + 1) as u32;
            let fail = |kind, key: Option<String>| TextError {
                kind,
                key,
                line: Some(no),
            };
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('@') {
                let (name, arg) = match rest.split_once([' ', '\t']) {
                    Some((n, a)) => (n, a.trim()),
                    None => (rest, ""),
                };
                if name != "lang" {
                    return Err(fail(
                        TextErrorKind::UnknownDirective {
                            name: name.to_string(),
                        },
                        None,
                    ));
                }
                table.language = arg.to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('\t') else {
                return Err(fail(TextErrorKind::MissingTab, None));
            };
            if key.is_empty() {
                return Err(fail(TextErrorKind::EmptyKey, None));
            }
            if table.index.contains_key(key) {
                return Err(fail(TextErrorKind::DuplicateKey, Some(key.to_string())));
            }
            table
                .insert(key, unescape_file(value))
                .map_err(|e| TextError {
                    line: Some(no),
                    ..e
                })?;
        }
        Ok(table)
    }

    /// Turns the table into the text of a file (it can be read back with [`TextTable::from_file`]).
    pub fn to_file(&self) -> String {
        let mut out = String::new();
        if !self.language.is_empty() {
            out.push_str("@lang ");
            out.push_str(&self.language);
            out.push('\n');
        }
        for e in &self.entries {
            out.push_str(&e.key);
            out.push('\t');
            for c in e.raw.chars() {
                match c {
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
            out.push('\n');
        }
        out
    }

    /// Returns the translation of the key, filled with `values` (the values of `${expr}` of the original, from the first).
    /// `None` if the key is missing.
    pub(crate) fn render(&self, key: &str, values: &[String]) -> Option<String> {
        let e = &self.entries[*self.index.get(key)?];
        let mut out = String::new();
        for p in &e.parts {
            match p {
                Part::Lit(t) => out.push_str(t),
                Part::Arg(n) => {
                    if let Some(v) = values.get(*n as usize - 1) {
                        out.push_str(v);
                    }
                }
            }
        }
        Some(out)
    }

    /// Checks only for number errors (errors) against the scripts ([`Vm::set_texts`](crate::Vm::set_texts)).
    pub(crate) fn number_errors(&self, program: &Program) -> Vec<TextIssue> {
        self.check(program)
            .into_iter()
            .filter(|i| matches!(i.kind, TextIssueKind::BadNumber { .. }))
            .collect()
    }
}

/// The number of `${expr}` in the original.
fn interp_count(parts: &[TextPart]) -> u32 {
    parts
        .iter()
        .filter(|p| matches!(p, TextPart::Interp))
        .count() as u32
}

/// Turns characters of the original into translation notation (escapes `\` and a `$` before `{`).
fn escape_literal(text: &str, out: &mut String) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
}

/// Turns `\n`, `\t` and `\r` in translations in the file into a newline, TAB and carriage return. Other `\` and the next
/// character are kept as is (`insert` reads them as translation escapes).
fn unescape_file(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses a translation.
fn parse_value(value: &str) -> Result<Vec<Part>, TextErrorKind> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        match c {
            '\\' => {
                let after = &rest[1..];
                match after.chars().next() {
                    Some(n @ ('「' | '」' | '$' | '\\' | '=' | '#')) => {
                        text.push(n);
                        rest = &after[n.len_utf8()..];
                    }
                    Some(n) => {
                        return Err(TextErrorKind::InvalidEscape {
                            found: format!("\\{n}"),
                        });
                    }
                    None => {
                        return Err(TextErrorKind::InvalidEscape {
                            found: "\\".to_string(),
                        });
                    }
                }
            }
            '$' if rest[1..].starts_with('{') => {
                let inner_all = &rest[2..];
                let Some(close) = inner_all.find('}') else {
                    return Err(TextErrorKind::InvalidPlaceholder {
                        found: rest.to_string(),
                    });
                };
                let inner = &inner_all[..close];
                let n = (!inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| inner.parse::<u32>().ok())
                    .flatten()
                    .filter(|&n| n >= 1);
                let Some(n) = n else {
                    return Err(TextErrorKind::InvalidPlaceholder {
                        found: format!("${{{inner}}}"),
                    });
                };
                if !text.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut text)));
                }
                parts.push(Part::Arg(n));
                rest = &inner_all[close + 1..];
            }
            c => {
                text.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if !text.is_empty() {
        parts.push(Part::Lit(text));
    }
    Ok(parts)
}

/// Information about a stop point whose arguments are replaced ([`localize_args`]).
pub(crate) struct StopCtx<'a> {
    pub program: &'a Program,
    pub kind: StopKind,
    /// `module.label`.
    pub label: &'a str,
    /// `module.label#id` of a plain line, quote line or command (`None` if there is no ID).
    pub key: Option<&'a str>,
    /// The translatable arguments of the command.
    pub text_arg: Option<TextArg>,
}

/// Replaces, among the arguments of a stop point, the bodies (plain lines, quote lines, option texts and translatable
/// command arguments) and the display name of a quote line's identity with the translations of the text table. Without a
/// table, without the key, or when the arguments do not have the expected shape, they stay as the original. `values` are
/// the values of `${expr}` for each body.
pub(crate) fn localize_args(
    table: Option<&TextTable>,
    ctx: &StopCtx<'_>,
    args: &[Value],
    values: &[Vec<String>],
) -> Vec<Value> {
    let mut out = args.to_vec();
    let Some(table) = table else {
        return out;
    };
    let label = ctx.label;
    match ctx.kind {
        StopKind::Plain | StopKind::Quote => {
            let at = if ctx.kind == StopKind::Plain { 0 } else { 1 };
            if let (Some(key), Some(Value::Str(_))) = (ctx.key, args.get(at))
                && let Some(t) = table.render(key, values.first().map_or(&[], Vec::as_slice))
            {
                out[at] = Value::Str(t.into());
            }
            if ctx.kind == StopKind::Quote
                && let Some(Value::Option(Some(who))) = args.first()
                && let Value::Identity(who) = &**who
                && let module = label.split_once('.').map_or("", |(m, _)| m)
                && let Some(key) = ctx.program.identity_key_of(module, &who.id, &who.name)
                && let Some(t) = table.render(&key, &[])
            {
                let new = Identity {
                    id: who.id.clone(),
                    name: t,
                };
                out[0] = Value::Option(Some(Box::new(Value::Identity(new.into()))));
            }
        }
        StopKind::Choice => {
            if let [Value::List(texts), Value::List(ids)] = args {
                let mut new = texts.clone().into_vec();
                for (i, id) in ids.iter().enumerate() {
                    let (Some(slot), Value::Option(Some(id))) = (new.get_mut(i), id) else {
                        continue;
                    };
                    let Value::Str(id) = &**id else { continue };
                    let key = format!("{label}#{id}");
                    if let Some(t) = table.render(&key, values.get(i).map_or(&[], Vec::as_slice)) {
                        *slot = Value::Str(t.into());
                    }
                }
                out[0] = Value::List(new.into());
            }
        }
        StopKind::Command => {
            if let (Some(ta), Some(key)) = (ctx.text_arg, ctx.key)
                && let Some(Value::Str(_)) = args.get(ta.arg as usize)
                && let Some(t) = table.render(
                    key,
                    values.get(ta.values as usize).map_or(&[], Vec::as_slice),
                )
            {
                out[ta.arg as usize] = Value::Str(t.into());
            }
        }
    }
    out
}
