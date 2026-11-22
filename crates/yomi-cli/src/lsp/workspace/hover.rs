//! Queries on the workspace: hover.

use super::*;

impl Workspace {
    /// The description of the name at a position.
    pub fn hover(&mut self, path: &Path, pos: Position) -> Option<Hover> {
        let (module, line, col) = self.locate(path, pos)?;
        let doc = {
            let snap = self.snapshot();
            snap.analysis
                .symbol_at(&module, line, col)
                .map(|id| code_block(&snap.analysis.symbols[id].detail))
        };
        let text = doc.or_else(|| {
            let file = self.snapshot.as_ref()?.file_of(path)?;
            let word = dotted_word_at(file.line(line), col)?;
            self.describe_word(&word)
        })?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            }),
            range: None,
        })
    }

    /// Descriptions of registry commands, variables and reserved words.
    pub(super) fn describe_word(&self, word: &str) -> Option<String> {
        let word = word.strip_prefix('@').unwrap_or(word);
        if let Some(c) = self
            .registry
            .commands()
            .into_iter()
            .find(|c| c.name == word || c.aliases.iter().any(|a| a == word))
        {
            let mut text = code_block(&command_signature(&c));
            if let Some(doc) = &c.doc {
                text.push_str("\n\n");
                text.push_str(doc);
            }
            return Some(text);
        }
        if let Some(v) = self.registry.vars().iter().find(|v| v.name == word) {
            let mut text = code_block(&format!("var {}: {}", v.name, v.ty.name()));
            if let Some(doc) = &v.doc {
                text.push_str("\n\n");
                text.push_str(doc);
            }
            return Some(text);
        }
        if let Some(k) = KEYWORDS.iter().find(|k| k.word == word) {
            return Some(code_block(&format!(
                "{} ({})",
                k.word,
                crate::keyword::kind_name(k.kind)
            )));
        }
        None
    }
}

/// The name joined with `.` (including one with `@`) at `col` (1-based) in the line.
pub(super) fn dotted_word_at(line: &str, col: u32) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let at = (col as usize).saturating_sub(1).min(chars.len());
    let part = |c: char| is_name_char(c) || c == '.' || c == '@';
    let start = (0..at)
        .rev()
        .take_while(|&i| part(chars[i]))
        .last()
        .unwrap_or(at);
    let end = (at..chars.len())
        .take_while(|&i| part(chars[i]))
        .last()
        .map_or(at, |i| i + 1);
    (start < end).then(|| chars[start..end].iter().collect())
}

pub(super) fn code_block(text: &str) -> String {
    format!("```yomi\n{text}\n```")
}
