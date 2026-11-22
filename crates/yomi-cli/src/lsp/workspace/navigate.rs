//! Queries on the workspace: definition, references, rename, highlight, list of symbols.

use super::*;
use lsp_types::WorkspaceEdit;

impl Workspace {
    /// The declaration of the symbol at a position.
    pub fn definition(&mut self, path: &Path, pos: Position) -> Option<Location> {
        let (module, line, col) = self.locate(path, pos)?;
        let snap = self.snapshot();
        let id = snap.analysis.symbol_at(&module, line, col)?;
        let s = &snap.analysis.symbols[id];
        snap.location(&s.module, s.span)
    }

    /// Where the symbol at a position is used (with `include_declaration`, the declaration too).
    pub fn references(
        &mut self,
        path: &Path,
        pos: Position,
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some((module, line, col)) = self.locate(path, pos) else {
            return Vec::new();
        };
        let snap = self.snapshot();
        let Some(id) = snap.analysis.symbol_at(&module, line, col) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if include_declaration {
            let s = &snap.analysis.symbols[id];
            out.extend(snap.location(&s.module, s.span));
        }
        out.extend(
            snap.analysis
                .references_to(id)
                .filter_map(|r| snap.location(&r.module, r.span)),
        );
        out
    }

    /// The range of the name at a position, and the symbol it refers to.
    pub(super) fn name_at(
        &mut self,
        path: &Path,
        pos: Position,
    ) -> Option<(usize, yomi_core::Span, String)> {
        let (module, line, col) = self.locate(path, pos)?;
        let snap = self.snapshot();
        let id = snap.analysis.symbol_at(&module, line, col)?;
        let span = snap
            .analysis
            .references
            .iter()
            .find(|r| r.module == module && r.span.contains(line, col))
            .map(|r| r.span)
            .unwrap_or(snap.analysis.symbols[id].span);
        Some((id, span, module))
    }

    /// The range that can be renamed (the token of the name).
    pub fn prepare_rename(&mut self, path: &Path, pos: Position) -> Option<Range> {
        let (_, span, module) = self.name_at(path, pos)?;
        let snap = self.snapshot();
        Some(snap.location(&module, span)?.range)
    }

    /// Edits that rename (per file). If the new name cannot be used or the position is not a name, the reason.
    #[expect(
        clippy::mutable_key_type,
        reason = "WorkspaceEdit の編集の表は、lsp-types が `HashMap<Uri, _>` と決めている。`Uri` の中身は変えない"
    )]
    pub fn rename(
        &mut self,
        path: &Path,
        pos: Position,
        new_name: &str,
    ) -> Result<WorkspaceEdit, String> {
        let valid = new_name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            && new_name.chars().all(is_name_char)
            && !is_reserved(new_name);
        if !valid {
            return Err(format!("`{new_name}` は名前に使えません"));
        }
        let (id, _, _) = self
            .name_at(path, pos)
            .ok_or_else(|| "名前の上ではありません".to_string())?;
        let snap = self.snapshot();
        let symbol = &snap.analysis.symbols[id];
        let mut spans = vec![(symbol.module.clone(), symbol.span)];
        spans.extend(
            snap.analysis
                .references_to(id)
                .map(|r| (r.module.clone(), r.span)),
        );
        let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> = Default::default();
        for (module, span) in spans {
            if let Some(loc) = snap.location(&module, span) {
                changes
                    .entry(loc.uri)
                    .or_default()
                    .push(TextEdit::new(loc.range, new_name.to_string()));
            }
        }
        Ok(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        })
    }

    /// Declarations and uses of the same name within the same file.
    pub fn highlights(&mut self, path: &Path, pos: Position) -> Vec<DocumentHighlight> {
        let Some((id, _, module)) = self.name_at(path, pos) else {
            return Vec::new();
        };
        let snap = self.snapshot();
        let symbol = &snap.analysis.symbols[id];
        let mut spans = Vec::new();
        if symbol.module == module {
            spans.push(symbol.span);
        }
        spans.extend(
            snap.analysis
                .references_to(id)
                .filter(|r| r.module == module)
                .map(|r| r.span),
        );
        spans
            .into_iter()
            .filter_map(|span| snap.location(&module, span))
            .map(|l| DocumentHighlight {
                range: l.range,
                kind: None,
            })
            .collect()
    }

    /// The tree of declarations of a file (variants and fields of types are children of the type).
    pub fn document_symbols(&mut self, path: &Path) -> Vec<DocumentSymbol> {
        let snap = self.snapshot();
        let Some(file) = snap.file_of(path) else {
            return Vec::new();
        };
        let range_of = |s: &Symbol| snap.location(&s.module, s.span).map(|l| l.range);
        let mut out = Vec::new();
        for s in snap
            .analysis
            .symbols
            .iter()
            .filter(|s| s.module == file.module)
        {
            if !is_top_level(s.kind) {
                continue;
            }
            let Some(range) = range_of(s) else {
                continue;
            };
            let children: Vec<DocumentSymbol> = snap
                .analysis
                .symbols
                .iter()
                .filter(|c| {
                    c.module == s.module
                        && matches!(c.kind, SymbolKind::Variant | SymbolKind::Field)
                        && c.container.as_deref() == Some(s.name.as_str())
                })
                .filter_map(|c| range_of(c).map(|r| document_symbol(c, r, None)))
                .collect();
            out.push(document_symbol(s, range, Some(children)));
        }
        out
    }

    /// Searches declarations across the whole workspace by part of the name (case-insensitive).
    pub fn workspace_symbols(&mut self, query: &str) -> Vec<SymbolInformation> {
        let query = query.to_lowercase();
        let snap = self.snapshot();
        snap.analysis
            .symbols
            .iter()
            .filter(|s| is_top_level(s.kind) && s.name.to_lowercase().contains(&query))
            .filter_map(|s| {
                let location = snap.location(&s.module, s.span)?;
                #[expect(deprecated, reason = "lsp-types の構造体は、非推奨の `deprecated` フィールドも書かないと作れない（`tags` を使う）")]
                Some(SymbolInformation {
                    name: s.name.clone(),
                    kind: lsp_kind(s.kind),
                    tags: None,
                    deprecated: None,
                    location,
                    container_name: Some(s.module.clone()),
                })
            })
            .collect()
    }
}

pub(super) fn is_top_level(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Label
            | SymbolKind::Fn
            | SymbolKind::Var
            | SymbolKind::Identity
            | SymbolKind::Struct
            | SymbolKind::Enum
    )
}

pub(super) fn lsp_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Label => LspSymbolKind::NAMESPACE,
        SymbolKind::Fn => LspSymbolKind::FUNCTION,
        SymbolKind::Var | SymbolKind::Local | SymbolKind::Param => LspSymbolKind::VARIABLE,
        SymbolKind::Identity => LspSymbolKind::CONSTANT,
        SymbolKind::Struct => LspSymbolKind::STRUCT,
        SymbolKind::Enum => LspSymbolKind::ENUM,
        SymbolKind::Variant => LspSymbolKind::ENUM_MEMBER,
        SymbolKind::Field => LspSymbolKind::FIELD,
    }
}

pub(super) fn document_symbol(
    s: &Symbol,
    range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    #[expect(
        deprecated,
        reason = "lsp-types の構造体は、非推奨の `deprecated` フィールドも書かないと作れない（`tags` を使う）"
    )]
    DocumentSymbol {
        name: s.name.clone(),
        detail: Some(s.detail.clone()),
        kind: lsp_kind(s.kind),
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children,
    }
}
