//! Queries on the workspace: completion.

use super::*;

impl Workspace {
    /// Completion candidates.
    pub fn completion(&mut self, path: &Path, pos: Position) -> Vec<CompletionItem> {
        let Some((module, line, col)) = self.locate(path, pos) else {
            return Vec::new();
        };
        let prefix: String = {
            let file = self.snapshot().file_of(path).expect("探した");
            file.line(line).chars().take(col as usize - 1).collect()
        };
        match context(&prefix) {
            Context::Command => self.commands(None),
            Context::Label => self.labels(&module),
            Context::Import => self.import_targets(&module),
            Context::Member(q) => self.members(&module, line, &q),
            Context::Plain => self.plain(&module, line),
        }
    }

    pub(super) fn commands(&self, package: Option<&str>) -> Vec<CompletionItem> {
        self.registry
            .commands()
            .into_iter()
            .filter_map(|c| {
                let label = match package {
                    Some(p) => c.name.strip_prefix(&format!("{p}."))?.to_string(),
                    None => c.name.clone(),
                };
                Some(CompletionItem {
                    label,
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(command_signature(&c)),
                    documentation: c.doc.clone().map(Documentation::String),
                    ..CompletionItem::default()
                })
            })
            .collect()
    }

    pub(super) fn labels(&mut self, module: &str) -> Vec<CompletionItem> {
        let snap = self.snapshot();
        snap.analysis
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Label)
            .map(|s| {
                let label = if s.module == module {
                    s.name.clone()
                } else {
                    format!("{}.{}", last_segment(&s.module), s.name)
                };
                symbol_item(label, s)
            })
            .collect()
    }

    pub(super) fn import_targets(&mut self, module: &str) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = self
            .registry
            .packages()
            .map(|p| item(format!("std::{}", p.name), CompletionItemKind::MODULE, None))
            .collect();
        let snap = self.snapshot();
        items.extend(
            snap.files
                .iter()
                .filter(|f| f.module != module)
                .map(|f| item(f.module.clone(), CompletionItemKind::MODULE, None)),
        );
        items
    }

    pub(super) fn members(&mut self, module: &str, line: u32, path: &str) -> Vec<CompletionItem> {
        let qualifier = path.rsplit('.').next().unwrap_or(path);
        let mut items = self.typed_members(module, line, path);
        items.extend(self.commands(Some(qualifier)));
        let prefix = format!("{qualifier}.");
        items.extend(self.registry.vars().iter().filter_map(|v| {
            let name = v.name.strip_prefix(&prefix)?;
            Some(item(
                name.to_string(),
                CompletionItemKind::VARIABLE,
                Some(v.ty.name()),
            ))
        }));
        let snap = self.snapshot();
        for s in &snap.analysis.symbols {
            let qualified = match s.kind {
                SymbolKind::Variant | SymbolKind::Field => {
                    s.container.as_deref() == Some(qualifier)
                }
                SymbolKind::Var | SymbolKind::Fn | SymbolKind::Identity => {
                    s.module != module && last_segment(&s.module) == qualifier
                }
                _ => false,
            };
            if qualified {
                items.push(symbol_item(s.name.clone(), s));
            }
        }
        items
    }

    /// Fields and methods after `value.` (when the value's type is known from declarations).
    pub(super) fn typed_members(
        &mut self,
        module: &str,
        line: u32,
        path: &str,
    ) -> Vec<CompletionItem> {
        let snap = self.snapshot();
        let mut parts = path.split('.');
        let first = parts.next().unwrap_or("");
        let current = current_label(snap, module, line);
        let declared = snap.analysis.symbols.iter().find_map(|s| {
            let scoped = matches!(s.kind, SymbolKind::Param | SymbolKind::Local)
                && s.module == module
                && s.span.line <= line
                && s.container == current;
            ((scoped || (s.kind == SymbolKind::Var && s.module == module)) && s.name == first)
                .then(|| type_of_detail(&s.detail))
                .flatten()
        });
        let Some(mut ty) = declared else {
            return Vec::new();
        };
        for field in parts {
            let next = snap.analysis.symbols.iter().find_map(|s| {
                (s.kind == SymbolKind::Field
                    && s.module == module
                    && s.container.as_deref() == Some(base_type(&ty))
                    && s.name == field)
                    .then(|| type_of_detail(&s.detail))
                    .flatten()
            });
            match next {
                Some(t) => ty = t,
                None => return Vec::new(),
            }
        }
        let base = base_type(&ty);
        let methods: &[(&str, &str)] = match base {
            "list" => &[
                ("len", "() -> int"),
                ("is_empty", "() -> bool"),
                ("contains", "(x) -> bool"),
                ("map", "(f) -> list"),
                ("filter", "(f) -> list"),
                ("any", "(f) -> bool"),
                ("all", "(f) -> bool"),
                ("find", "(f) -> Option"),
                ("count", "(f) -> int"),
                ("fold", "(init, f)"),
                ("sort_by", "(f) -> list"),
                ("min_by", "(f) -> Option"),
                ("max_by", "(f) -> Option"),
                ("sort", "() -> list"),
                ("reverse", "() -> list"),
                ("sum", "() -> int | float"),
                ("push", "(x)"),
                ("insert", "(i, x)"),
                ("remove", "(i)"),
                ("clear", "()"),
                ("shuffle", "()"),
            ],
            "map" => &[
                ("len", "() -> int"),
                ("is_empty", "() -> bool"),
                ("has", "(k) -> bool"),
                ("get", "(k) -> Option"),
                ("keys", "() -> list"),
                ("values", "() -> list"),
                ("remove", "(k)"),
                ("clear", "()"),
            ],
            _ => &[],
        };
        let mut items: Vec<CompletionItem> = methods
            .iter()
            .map(|(name, sig)| {
                item(
                    (*name).to_string(),
                    CompletionItemKind::METHOD,
                    Some(format!("{name}{sig}")),
                )
            })
            .collect();
        items.extend(
            snap.analysis
                .symbols
                .iter()
                .filter(|s| {
                    s.kind == SymbolKind::Field
                        && s.module == module
                        && s.container.as_deref() == Some(base)
                })
                .map(|s| symbol_item(s.name.clone(), s)),
        );
        items
    }

    pub(super) fn plain(&mut self, module: &str, line: u32) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = KEYWORDS
            .iter()
            .map(|k| {
                item(
                    k.word.to_string(),
                    CompletionItemKind::KEYWORD,
                    Some(crate::keyword::kind_name(k.kind).to_string()),
                )
            })
            .collect();
        items.extend(CONTEXTUAL_KEYWORDS.iter().map(|k| {
            item(
                k.word.to_string(),
                CompletionItemKind::KEYWORD,
                Some("contextual".into()),
            )
        }));
        let packages: Vec<String> = self.registry.packages().map(|p| p.name).collect();
        items.extend(
            packages
                .into_iter()
                .map(|p| item(p, CompletionItemKind::MODULE, None)),
        );
        let snap = self.snapshot();
        let current = current_label(snap, module, line);
        for s in &snap.analysis.symbols {
            if s.module != module {
                continue;
            }
            let visible = match s.kind {
                SymbolKind::Var
                | SymbolKind::Fn
                | SymbolKind::Identity
                | SymbolKind::Struct
                | SymbolKind::Enum => true,
                SymbolKind::Param | SymbolKind::Local => {
                    s.span.line <= line && s.container == current
                }
                _ => false,
            };
            if visible {
                items.push(symbol_item(s.name.clone(), s));
            }
        }
        items
    }
}

/// The name of the label or function containing the line (the closest declaration before the line).
pub(super) fn current_label(snap: &Snapshot, module: &str, line: u32) -> Option<String> {
    snap.analysis
        .symbols
        .iter()
        .filter(|s| {
            s.module == module
                && matches!(s.kind, SymbolKind::Label | SymbolKind::Fn)
                && s.span.line <= line
        })
        .max_by_key(|s| s.span.line)
        .map(|s| s.name.clone())
}

/// The type part of a description (`name: type`).
pub(super) fn type_of_detail(detail: &str) -> Option<String> {
    let (_, ty) = detail.split_once(": ")?;
    (!ty.is_empty()).then(|| ty.to_string())
}

/// A type name without type arguments (`list<int>` gives `list`).
pub(super) fn base_type(ty: &str) -> &str {
    ty.split('<').next().unwrap_or(ty).trim()
}

/// The context of the input.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Context {
    /// After `@` (a command name).
    Command,
    /// After `jump` / `call` (a label name).
    Label,
    /// After `import` (a path name).
    Import,
    /// After `name.` (a member name).
    Member(String),
    /// Anything else.
    Plain,
}

/// Decides the context from the characters of the line before the cursor.
pub(super) fn context(prefix: &str) -> Context {
    let word_start = prefix
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name_char(*c) || *c == '.' || *c == '@' || *c == ':')
        .last()
        .map(|(i, _)| i)
        .unwrap_or(prefix.len());
    let word = &prefix[word_start..];
    let before = prefix[..word_start].trim_start();
    if word.starts_with('@') {
        return Context::Command;
    }
    if before.starts_with("import") && (before == "import" || before.starts_with("import ")) {
        return Context::Import;
    }
    let last_word = before.split_whitespace().last().unwrap_or("");
    if last_word == "jump" || last_word == "call" {
        return Context::Label;
    }
    if let Some((q, _)) = word.rsplit_once('.')
        && !q.is_empty()
    {
        return Context::Member(q.to_string());
    }
    Context::Plain
}

pub(super) fn last_segment(module: &str) -> &str {
    module.rsplit("::").next().unwrap_or(module)
}

pub(super) fn item(
    label: String,
    kind: CompletionItemKind,
    detail: Option<String>,
) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        detail,
        ..CompletionItem::default()
    }
}

pub(super) fn symbol_item(label: String, s: &Symbol) -> CompletionItem {
    let kind = match s.kind {
        SymbolKind::Label => CompletionItemKind::MODULE,
        SymbolKind::Fn => CompletionItemKind::FUNCTION,
        SymbolKind::Var | SymbolKind::Local | SymbolKind::Param => CompletionItemKind::VARIABLE,
        SymbolKind::Identity => CompletionItemKind::CONSTANT,
        SymbolKind::Struct => CompletionItemKind::STRUCT,
        SymbolKind::Enum => CompletionItemKind::ENUM,
        SymbolKind::Variant => CompletionItemKind::ENUM_MEMBER,
        SymbolKind::Field => CompletionItemKind::FIELD,
    };
    item(label, kind, Some(s.detail.clone()))
}
