//! Planning of automatic stable IDs (the core of `yomi assign-ids`; section 5.8 of `spec/host/05-texts.md`).
//!
//! Finds, with pure functions, which lines can get an ID and where to append it. Reading and writing files is up to the
//! caller. The random generator used to make IDs is a seedable hand-written one ([`IdRng`]), passed as an argument.

use std::collections::{HashMap, HashSet};

use crate::Program;
use crate::compile::{CompileOptions, compile_modules_with_options, compile_modules_with_sites};
use crate::diag::{DiagCode, Diagnostic, Severity};
use crate::ident::normalize_name;
use crate::program::{IdSite, StopKind};
use crate::registry::Registry;

/// The length of an automatic ID (without `#`).
pub const AUTO_ID_LEN: usize = 6;

/// The characters used in IDs (lowercase letters and digits).
const ID_CHARS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// One line that can have an ID but has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingId {
    /// The name of the module.
    pub module: String,
    /// The name of the label it belongs to.
    pub label: String,
    /// The kind of line (`Plain`, `Quote`, `Choice` (the text of an option), `Command` (a line of a stop command)).
    pub kind: StopKind,
    /// The line (1-based) of the position to append `#id`.
    pub line: u32,
    /// The column (1-based, in Unicode scalar values) of the position to append `#id`.
    pub col: u32,
}

impl Program {
    /// Lines that can have a stable ID but have none (in script order).
    pub fn missing_ids(&self) -> Vec<MissingId> {
        self.id_sites
            .iter()
            .filter(|s| s.id.is_none())
            .map(|s| MissingId {
                module: self.modules[s.module as usize].clone(),
                label: s.label.clone(),
                kind: s.kind,
                line: s.line,
                col: s.col,
            })
            .collect()
    }

    /// The stable IDs already written in the given label (in script order).
    pub fn written_ids(&self, module: &str, label: &str) -> Vec<&str> {
        self.id_sites
            .iter()
            .filter(|s| self.modules[s.module as usize] == module && s.label == label)
            .filter_map(|s| s.id.as_deref())
            .collect()
    }
}

/// The random generator for IDs (xorshift64*; can be fixed by a seed; not cryptographic).
#[derive(Debug, Clone)]
pub struct IdRng {
    state: u64,
}

impl IdRng {
    /// Creates it from a seed (0 is replaced with another value).
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Makes 6 characters of `[a-z0-9]`.
    pub fn next_id(&mut self) -> String {
        let mut out = String::with_capacity(AUTO_ID_LEN);
        let mut v = self.next_u64();
        for i in 0..AUTO_ID_LEN {
            if i == 5 {
                v = self.next_u64();
            }
            out.push(ID_CHARS[(v % 36) as usize] as char);
            v /= 36;
        }
        out
    }

    /// Makes an ID not in `used` and adds it to `used`.
    pub fn unique_id(&mut self, used: &mut HashSet<String>) -> String {
        loop {
            let id = self.next_id();
            if used.insert(id.clone()) {
                return id;
            }
        }
    }
}

/// One addition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    /// The name of the module.
    pub module: String,
    /// The name of the label it belongs to.
    pub label: String,
    /// The kind of line.
    pub kind: StopKind,
    /// The line of the position (1-based).
    pub line: u32,
    /// The column of the position (1-based).
    pub col: u32,
    /// The byte offset of the position (from the start of the original source, including the BOM).
    pub byte: usize,
    /// The ID to add (without `#`).
    pub id: String,
    /// The text to append (including surrounding spaces).
    pub text: String,
}

/// Converts a (line, column) of the source to a byte offset that includes the BOM.
fn byte_of(source: &str, line: u32, col: u32) -> Option<usize> {
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let body = &source[bom..];
    let mut start = 0usize;
    for _ in 1..line {
        start += body[start..].find('\n')? + 1;
    }
    let rest = &body[start..];
    let mut off = 0usize;
    let mut chars = rest.char_indices();
    for _ in 1..col {
        let (i, c) = chars.next()?;
        off = i + c.len_utf8();
    }
    Some(bom + start + off)
}

/// Collects and compiles the scripts, and finds the IDs to give to lines without one and where to append them.
///
/// If there are compile errors, returns the diagnostics (and adds nothing). However, if every error is **caused by a
/// missing ID** (`once_without_id` for a `once` option without an ID, or `missing_text_id` when `require_text_ids` is
/// enabled), those lines get IDs too. The scripts with the IDs added are compiled again, and the result is returned only
/// after confirming there are no errors (if that cannot be confirmed, the diagnostics of that compilation are returned and
/// nothing is added). IDs are unique within a label (they do not collide with IDs already written). The result is in
/// module order, then position order.
pub fn plan(
    sources: &[(&str, &str)],
    registry: &Registry,
    options: &CompileOptions,
    rng: &mut IdRng,
) -> Result<Vec<Insertion>, Vec<Diagnostic>> {
    let (result, failed_sites) = compile_modules_with_sites(sources, registry, options);
    let diags = match result {
        Ok((program, _)) => return Ok(plan_program(&program, sources, rng)),
        Err(diags) => diags,
    };
    if !fixable_by_ids(&diags) {
        return Err(diags);
    }
    let modules: Vec<String> = sources.iter().map(|(n, _)| normalize_name(n)).collect();
    let plan = plan_sites(&modules, &failed_sites, sources, rng);
    // Confirm there are no other errors in the scripts with the IDs added (write nothing until confirmed)
    let applied: Vec<(String, String)> = sources
        .iter()
        .map(|(m, src)| {
            let name = normalize_name(m);
            let mine: Vec<Insertion> = plan.iter().filter(|i| i.module == name).cloned().collect();
            ((*m).to_string(), apply(src, &mine))
        })
        .collect();
    let refs: Vec<(&str, &str)> = applied
        .iter()
        .map(|(m, s)| (m.as_str(), s.as_str()))
        .collect();
    match compile_modules_with_options(&refs, registry, options) {
        Ok(_) => Ok(plan),
        Err(again) => Err(again),
    }
}

/// Whether there is at least one error and all of them would disappear by adding IDs (missing IDs).
fn fixable_by_ids(diags: &[Diagnostic]) -> bool {
    let mut errors = diags.iter().filter(|d| d.severity == Severity::Error);
    let Some(first) = errors.next() else {
        return false;
    };
    std::iter::once(first)
        .chain(errors)
        .all(|d| matches!(d.code, DiagCode::OnceWithoutId | DiagCode::MissingTextId))
}

/// Finds the positions to append from a compiled program (the second half of `plan`).
pub fn plan_program(
    program: &Program,
    sources: &[(&str, &str)],
    rng: &mut IdRng,
) -> Vec<Insertion> {
    plan_sites(&program.modules, &program.id_sites, sources, rng)
}

/// Finds the positions to append from the lines that can have IDs (`modules` are module names indexed by a line's `module`).
fn plan_sites(
    modules: &[String],
    id_sites: &[IdSite],
    sources: &[(&str, &str)],
    rng: &mut IdRng,
) -> Vec<Insertion> {
    // Look up module names after normalizing to NFC, the same as compilation
    let src: HashMap<String, &str> = sources
        .iter()
        .map(|(m, s)| (normalize_name(m), *s))
        .collect();
    let mut used: HashMap<(String, String), HashSet<String>> = HashMap::new();
    let mut out = Vec::new();
    for s in id_sites {
        let module = &modules[s.module as usize];
        let set = used.entry((module.clone(), s.label.clone())).or_default();
        if let Some(id) = &s.id {
            set.insert(id.clone());
        }
    }
    for s in id_sites {
        if s.id.is_some() {
            continue;
        }
        let module = &modules[s.module as usize];
        let Some(source) = src.get(module).copied() else {
            continue;
        };
        let Some(byte) = byte_of(source, s.line, s.col) else {
            continue;
        };
        let set = used
            .get_mut(&(module.clone(), s.label.clone()))
            .expect("登録済み");
        let id = rng.unique_id(set);
        let spaced = source[..byte].ends_with([' ', '\t']);
        let text = match (s.kind, spaced) {
            (StopKind::Choice, true) => format!("#{id} "),
            (StopKind::Choice, false) => format!(" #{id} "),
            _ => format!(" #{id}"),
        };
        out.push(Insertion {
            module: module.clone(),
            label: s.label.clone(),
            kind: s.kind,
            line: s.line,
            col: s.col,
            byte,
            id,
            text,
        });
    }
    out
}

/// Applies the additions to a source (the original text is unchanged except where text is appended). `insertions` must
/// all be for the same module (in any order).
pub fn apply(source: &str, insertions: &[Insertion]) -> String {
    let mut sorted: Vec<&Insertion> = insertions.iter().collect();
    sorted.sort_by_key(|i| i.byte);
    let mut out = String::with_capacity(source.len() + sorted.len() * 10);
    let mut last = 0;
    for i in sorted {
        out.push_str(&source[last..i.byte]);
        out.push_str(&i.text);
        last = i.byte;
    }
    out.push_str(&source[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Identity, Sugar, compile};

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.stop::<(String,), ()>("ui.say").unwrap();
        r.stop::<(Option<Identity>, String), ()>("ui.say_as")
            .unwrap();
        r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
            .unwrap();
        r.stop::<(f64,), ()>("flow.wait").unwrap();
        r.bind(Sugar::Plain, "ui.say").unwrap();
        r.bind(Sugar::Quote, "ui.say_as").unwrap();
        r.bind(Sugar::Choice, "ui.choose").unwrap();
        r
    }

    #[test]
    fn position_hash_does_not_change_when_ids_are_added() {
        let reg = registry();
        let src = "identity 案内人 = \"案内人\"\nlabel start {\n  あ  \n  案内人「い」\n  「う\n   え」\n  @flow.wait(1.0)\n  choice {\n    「は」 => jump start\n    「ひ」 once #h => jump start\n  }\n}\n";
        let before = compile(src, "main", &reg).unwrap();
        let ins = plan(
            &[("main", src)],
            &reg,
            &Default::default(),
            &mut IdRng::new(1),
        )
        .unwrap();
        let out = apply(src, &ins);
        let after = compile(&out, "main", &reg).unwrap();
        let key = |p: &Program| {
            p.labels
                .iter()
                .flat_map(|l| {
                    l.stops.iter().map(|s| {
                        (
                            s.hash,
                            s.kind,
                            s.text.clone(),
                            s.line,
                            s.choice.as_ref().map(|c| c.options.len()),
                        )
                    })
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&before), key(&after));
        assert!(!after.labels[0].stops.is_empty());
        assert!(after.missing_ids().is_empty());
        assert_eq!(after.written_ids("main", "start").len(), 6);
    }
}
