//! The workspace (a collection of script modules) and queries on it. Independent of communication.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lingokit::Localizer;
use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, DocumentHighlight,
    DocumentSymbol, Documentation, Hover, HoverContents, Location, MarkupContent, MarkupKind,
    NumberOrString, Position, Range, SymbolInformation, SymbolKind as LspSymbolKind, TextEdit, Uri,
};
use yomi_core::{
    Analysis, CONTEXTUAL_KEYWORDS, CompileOptions, KEYWORDS, Registry, Severity, Symbol,
    SymbolKind, analyze, compile_modules_with_options, infer_local_types, is_name_char,
};

use super::text::{char_col, name_len_at, utf16_col};
use super::uri::path_to_uri;
use crate::diag::diagnostic_message;
use crate::modules::{module_name, scripts_under};
use yomi_core::is_reserved;

/// One script file.
struct File {
    module: String,
    path: PathBuf,
    source: String,
}

impl File {
    /// Whether the file is a module (not `migrations.yomi` or a frozen schema).
    fn is_module(&self) -> bool {
        self.module != crate::modules::MIGRATIONS && !self.module.starts_with("schema::")
    }

    /// The compile options with the files of save migration among `files`.
    fn compile_options(files: &[File]) -> CompileOptions {
        let mut options = CompileOptions::default();
        for f in files {
            if f.module == crate::modules::MIGRATIONS {
                options = options.migrations(&f.source);
            } else if let Some(rest) = f.module.strip_prefix("schema::") {
                options = options.frozen_schema_file(
                    &format!("schema/{}.yomi", rest.replace("::", "/")),
                    &f.source,
                );
            }
        }
        options
    }

    fn line(&self, line: u32) -> &str {
        self.source
            .split('\n')
            .nth(line.saturating_sub(1) as usize)
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .unwrap_or("")
    }
}

/// A collection of modules at some point, and their analysis.
struct Snapshot {
    files: Vec<File>,
    analysis: Analysis,
}

impl Snapshot {
    fn file_of(&self, path: &Path) -> Option<&File> {
        self.files.iter().find(|f| f.path == path)
    }

    fn file_of_module(&self, module: &str) -> Option<&File> {
        self.files.iter().find(|f| f.module == module)
    }

    fn location(&self, module: &str, span: yomi_core::Span) -> Option<Location> {
        let file = self.file_of_module(module)?;
        let text = file.line(span.line);
        let start = Position::new(span.line - 1, utf16_col(text, span.col));
        let end = Position::new(span.line - 1, utf16_col(text, span.col + span.len));
        Some(Location::new(
            path_to_uri(&file.path)?,
            Range::new(start, end),
        ))
    }
}

/// The workspace.
pub struct Workspace {
    root: Option<PathBuf>,
    registry: Registry,
    loc: Localizer,
    docs: BTreeMap<PathBuf, String>,
    snapshot: Option<Snapshot>,
}

mod completion;
mod hover;
mod navigate;

impl Workspace {
    /// A new workspace. `root` is the base for module names (if absent, the directory of the first file opened).
    pub fn new(root: Option<PathBuf>, registry: Registry, loc: Localizer) -> Self {
        Workspace {
            root,
            registry,
            loc,
            docs: BTreeMap::new(),
            snapshot: None,
        }
    }

    /// Records the contents of a file opened or changed in the editor.
    pub fn set_document(&mut self, path: PathBuf, text: String) {
        self.docs.insert(path, text);
        self.snapshot = None;
    }

    /// A file closed in the editor (from then on, the contents on disk are used).
    pub fn close_document(&mut self, path: &Path) {
        self.docs.remove(path);
        self.snapshot = None;
    }

    /// Notifies that a file on disk changed.
    pub fn invalidate(&mut self) {
        self.snapshot = None;
    }

    fn snapshot(&mut self) -> &Snapshot {
        if self.snapshot.is_none() {
            self.snapshot = Some(self.build());
        }
        self.snapshot.as_ref().expect("作った")
    }

    fn build(&mut self) -> Snapshot {
        if self.root.is_none() {
            self.root = self
                .docs
                .keys()
                .next()
                .and_then(|p| p.parent().map(Path::to_path_buf));
        }
        let mut paths: Vec<PathBuf> = match &self.root {
            Some(root) => scripts_under(root).unwrap_or_default(),
            None => Vec::new(),
        };
        for p in self.docs.keys() {
            if !paths.contains(p) {
                paths.push(p.clone());
            }
        }
        paths.sort();
        let root = self.root.clone().unwrap_or_default();
        let files: Vec<File> = paths
            .into_iter()
            .filter_map(|path| {
                let source = match self.docs.get(&path) {
                    Some(s) => s.clone(),
                    None => std::fs::read_to_string(&path).ok()?,
                };
                Some(File {
                    module: module_name(&path, &root),
                    path,
                    source,
                })
            })
            .collect();
        let sources: Vec<(&str, &str)> = files
            .iter()
            .filter(|f| f.is_module())
            .map(|f| (f.module.as_str(), f.source.as_str()))
            .collect();
        let mut analysis = analyze(&sources);
        let types = infer_local_types(&sources, &self.registry);
        for s in analysis
            .symbols
            .iter_mut()
            .filter(|s| matches!(s.kind, SymbolKind::Local | SymbolKind::Param))
            .filter(|s| !s.detail.contains(": "))
        {
            let found = types
                .iter()
                .find(|t| t.module == s.module && t.line == s.span.line && t.col == s.span.col);
            if let Some(t) = found {
                // Types of the same module are written without the module name
                let ty = t.ty.replace(&format!("{}.", s.module), "");
                s.detail = format!("{}: {ty}", s.name);
            }
        }
        Snapshot { files, analysis }
    }

    /// Diagnostics of all files (files without diagnostics get an empty list, to clear previous diagnostics).
    pub fn diagnostics(&mut self) -> Vec<(PathBuf, Vec<Diagnostic>)> {
        self.snapshot();
        let snap = self.snapshot.as_ref().expect("作った");
        let sources: Vec<(&str, &str)> = snap
            .files
            .iter()
            .filter(|f| f.is_module())
            .map(|f| (f.module.as_str(), f.source.as_str()))
            .collect();
        let diags = match compile_modules_with_options(
            &sources,
            &self.registry,
            &File::compile_options(&snap.files),
        ) {
            Ok((_, warnings)) => warnings,
            Err(diags) => diags,
        };
        snap.files
            .iter()
            .map(|file| {
                let items = diags
                    .iter()
                    .filter(|d| d.module == file.module)
                    .map(|d| {
                        let text = file.line(d.line);
                        let start = Position::new(d.line.saturating_sub(1), utf16_col(text, d.col));
                        let end = Position::new(
                            start.line,
                            utf16_col(text, d.col + name_len_at(text, d.col)),
                        );
                        Diagnostic {
                            range: Range::new(start, end),
                            severity: Some(match d.severity {
                                Severity::Error => DiagnosticSeverity::ERROR,
                                _ => DiagnosticSeverity::WARNING,
                            }),
                            code: Some(NumberOrString::String(d.code.as_str().into())),
                            source: Some("yomi".into()),
                            message: diagnostic_message(&self.loc, d),
                            ..Diagnostic::default()
                        }
                    })
                    .collect();
                (file.path.clone(), items)
            })
            .collect()
    }

    /// The edits that format a file (one edit that replaces the whole text). `None` when the file has syntax errors; an
    /// empty list when it is already formatted.
    pub fn formatting(&mut self, path: &Path) -> Option<Vec<TextEdit>> {
        self.snapshot();
        let snap = self.snapshot.as_ref().expect("作った");
        let file = snap.file_of(path)?;
        let all: Vec<(&str, &str)> = snap
            .files
            .iter()
            .map(|f| (f.module.as_str(), f.source.as_str()))
            .collect();
        let text = yomi_core::format_source(&all, &file.module).ok()?;
        if text == file.source {
            return Some(Vec::new());
        }
        let last_line = file.source.split('\n').count() - 1;
        let last_len = file
            .source
            .rsplit('\n')
            .next()
            .map_or(0, |l| l.encode_utf16().count());
        let range = Range::new(
            Position::new(0, 0),
            Position::new(last_line as u32, last_len as u32),
        );
        Some(vec![TextEdit::new(range, text)])
    }

    /// The character position from a file and an LSP position.
    fn locate(&mut self, path: &Path, pos: Position) -> Option<(String, u32, u32)> {
        let file = self.snapshot().file_of(path)?;
        let line = pos.line + 1;
        let col = char_col(file.line(line), pos.character);
        Some((file.module.clone(), line, col))
    }
}

fn command_signature(c: &yomi_core::CommandDecl) -> String {
    let args: Vec<String> = c
        .args
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let name = c
                .params
                .get(i)
                .and_then(|n| n.clone())
                .unwrap_or_else(|| format!("arg{}", i + 1));
            format!("{name}: {}", ty.name())
        })
        .collect();
    let ret = match c.ret.name().as_str() {
        "()" | "" => String::new(),
        r => format!(" -> {r}"),
    };
    format!("@{}({}){ret}", c.name, args.join(", "))
}

#[cfg(test)]
mod tests;
