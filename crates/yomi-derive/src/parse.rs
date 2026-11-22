//! Parsing the input of `#[derive(YomiType)]` (pure functions independent of `proc_macro`).

/// A token (a copy of `proc_macro::TokenTree` in a testable form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// An identifier (including keywords).
    Ident(String),
    /// A single symbol character. The second field tells whether it joins the next symbol (such as the first `:` of `::`).
    Punct(char, bool),
    /// A literal (as written).
    Literal(String),
    /// A pair of brackets (`(`, `{`, `[`) and their contents.
    Group(char, Vec<Tok>),
}

/// A parsed type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The name of the Rust type.
    pub rust_name: String,
    /// The name in scripts (`namespace.TypeName`).
    pub full_name: String,
    /// Contents.
    pub kind: ItemKind,
}

/// Whether it is a `struct` or an `enum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// A `struct` with named fields (field names, and the types as written).
    Struct(Vec<(String, String)>),
    /// An `enum` (variant names, and the argument types as strings).
    Enum(Vec<(String, Vec<String>)>),
}

/// Turns tokens into a Rust source string (used to reproduce types).
pub fn render(toks: &[Tok]) -> String {
    let mut out = String::new();
    for t in toks {
        match t {
            Tok::Ident(s) | Tok::Literal(s) => {
                out.push_str(s);
                out.push(' ');
            }
            Tok::Punct(c, joint) => {
                out.push(*c);
                if !joint {
                    out.push(' ');
                }
            }
            Tok::Group(open, inner) => {
                let close = match open {
                    '(' => ')',
                    '{' => '}',
                    '[' => ']',
                    _ => ' ',
                };
                if *open != ' ' {
                    out.push(*open);
                }
                out.push_str(&render(inner));
                if close != ' ' {
                    out.push(close);
                }
                out.push(' ');
            }
        }
    }
    out
}

/// Returns the contents of a string literal's source notation without the quotes. `None` if it contains `\` or `"`.
fn unquote(lit: &str) -> Option<String> {
    let inner = lit.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains(['\\', '"'])).then(|| inner.to_string())
}

/// The contents of `#[yomi(...)]`.
#[derive(Default)]
struct YomiAttr {
    name: Option<String>,
    namespace: Option<String>,
}

/// Reads the contents of `#[yomi(...)]` (`name = "…"` and `namespace = "…"` separated by commas) into `attr`.
/// `Err` (a message shown to users) if the form is wrong, a value is empty or contains whitespace, or a key is unknown.
fn parse_yomi_args(inner: &[Tok], attr: &mut YomiAttr) -> Result<(), String> {
    let mut i = 0;
    while i < inner.len() {
        let (Some(Tok::Ident(key)), Some(Tok::Punct('=', _)), Some(Tok::Literal(lit))) =
            (inner.get(i), inner.get(i + 1), inner.get(i + 2))
        else {
            return Err(
                "#[yomi(...)] は `name = \"…\"` か `namespace = \"…\"` の形で書いてください".into(),
            );
        };
        let value = unquote(lit)
            .filter(|v| !v.is_empty() && !v.contains(char::is_whitespace))
            .ok_or("#[yomi(...)] の値は、空でない文字列リテラルにしてください")?;
        match key.as_str() {
            "name" => attr.name = Some(value),
            "namespace" => attr.namespace = Some(value),
            other => {
                return Err(format!(
                    "#[yomi({other} = …)] は使えません（使えるのは name と namespace だけです）"
                ));
            }
        }
        i += 3;
        match inner.get(i) {
            None => {}
            Some(Tok::Punct(',', _)) => i += 1,
            Some(_) => return Err("#[yomi(...)] の項目は、コンマで区切ってください".into()),
        }
    }
    Ok(())
}

/// Skips leading attributes. If there is a `yomi` attribute, collects it into `attr` (an error if `allow_yomi` is false).
fn skip_attrs(toks: &[Tok], mut i: usize, attr: Option<&mut YomiAttr>) -> Result<usize, String> {
    let mut attr = attr;
    while let Some(Tok::Punct('#', _)) = toks.get(i) {
        let Some(Tok::Group('[', inner)) = toks.get(i + 1) else {
            return Err("属性の形が読めません".into());
        };
        if let Some(Tok::Ident(name)) = inner.first()
            && name == "yomi"
        {
            let Some(a) = attr.as_deref_mut() else {
                return Err("#[yomi(...)] は、型にだけ付けられます（フィールド・バリアントには付けられません）".into());
            };
            let Some(Tok::Group('(', args)) = inner.get(1) else {
                return Err("#[yomi(...)] の形で書いてください".into());
            };
            parse_yomi_args(args, a)?;
        }
        i += 2;
    }
    Ok(i)
}

/// Skips a visibility specifier (`pub`, `pub(...)`) and returns the position of the next token. `i` as is if there is
/// none.
fn skip_vis(toks: &[Tok], mut i: usize) -> usize {
    if let Some(Tok::Ident(p)) = toks.get(i)
        && p == "pub"
    {
        i += 1;
        if let Some(Tok::Group('(', _)) = toks.get(i) {
            i += 1;
        }
    }
    i
}

/// Splits by commas (does not split at commas inside `<` `>`).
fn split_commas(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Punct('<', _) => depth += 1,
            Tok::Punct('>', _) => {
                // Do not count the `>` of `->`
                let arrow = i > 0 && matches!(toks[i - 1], Tok::Punct('-', true));
                if !arrow {
                    depth -= 1;
                }
            }
            Tok::Punct(',', _) if depth <= 0 => {
                parts.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < toks.len() {
        parts.push(&toks[start..]);
    }
    parts
}

/// Returns the name without the raw identifier prefix `r#`.
fn plain(name: &str) -> String {
    name.strip_prefix("r#").unwrap_or(name).to_string()
}

/// Turns a list of named fields (`name: type`) into a sequence of `(name, type source)`. `Err` if the form is wrong.
fn parse_fields(body: &[Tok]) -> Result<Vec<(String, String)>, String> {
    let mut fields = Vec::new();
    for part in split_commas(body) {
        let i = skip_attrs(part, 0, None)?;
        let i = skip_vis(part, i);
        let (Some(Tok::Ident(name)), Some(Tok::Punct(':', _))) = (part.get(i), part.get(i + 1))
        else {
            return Err("フィールドは `名前: 型` の形で書いてください".into());
        };
        let ty = &part[i + 2..];
        if ty.is_empty() {
            return Err(format!("フィールド {name} の型がありません"));
        }
        fields.push((plain(name), render(ty)));
    }
    Ok(fields)
}

/// Turns a list of enum variants into `(name, sequence of argument type sources)`. `Err` if a name cannot be read.
fn parse_variants(body: &[Tok]) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut variants = Vec::new();
    for part in split_commas(body) {
        let i = skip_attrs(part, 0, None)?;
        let Some(Tok::Ident(name)) = part.get(i) else {
            return Err("バリアントの名前が読めません".into());
        };
        let args = match part.get(i + 1) {
            None => Vec::new(),
            Some(Tok::Group('(', inner)) => {
                if part.len() > i + 2 {
                    return Err(format!(
                        "バリアント {name} の後ろに、読めない記述があります"
                    ));
                }
                let mut tys = Vec::new();
                for t in split_commas(inner) {
                    if t.is_empty() {
                        return Err(format!("バリアント {name} の引数の型が空です"));
                    }
                    tys.push(render(t));
                }
                tys
            }
            Some(Tok::Group('{', _)) => {
                return Err(format!(
                    "バリアント {name} は波括弧の形（名前つきフィールド）です。タプルの形 `{name}(型, …)` で書いてください"
                ));
            }
            Some(Tok::Punct('=', _)) => {
                return Err(format!("バリアント {name} の `= 値`（判別値）は使えません"));
            }
            Some(_) => {
                return Err(format!(
                    "バリアント {name} の後ろに、読めない記述があります"
                ));
            }
        };
        variants.push((plain(name), args));
    }
    Ok(variants)
}

/// Parses the input of `derive` (a `struct` or `enum` definition). Forms that cannot be parsed give a message in
/// Japanese.
pub fn parse(toks: &[Tok]) -> Result<Item, String> {
    let mut attr = YomiAttr::default();
    let i = skip_attrs(toks, 0, Some(&mut attr))?;
    let i = skip_vis(toks, i);
    let keyword = match toks.get(i) {
        Some(Tok::Ident(k)) if k == "struct" || k == "enum" => k.clone(),
        Some(Tok::Ident(k)) if k == "union" => {
            return Err("union には #[derive(YomiType)] を付けられません".into());
        }
        _ => return Err("#[derive(YomiType)] は struct と enum にだけ付けられます".into()),
    };
    let Some(Tok::Ident(rust_name)) = toks.get(i + 1) else {
        return Err("型の名前が読めません".into());
    };
    let rust_name = plain(rust_name);
    let kind = match toks.get(i + 2) {
        Some(Tok::Punct('<', _)) => {
            return Err(format!(
                "{rust_name}: ジェネリクス・ライフタイムのある型には #[derive(YomiType)] を付けられません"
            ));
        }
        Some(Tok::Ident(w)) if w == "where" => {
            return Err(format!(
                "{rust_name}: where 節のある型には #[derive(YomiType)] を付けられません"
            ));
        }
        Some(Tok::Group('{', body)) => {
            if toks.len() > i + 3 {
                return Err(format!("{rust_name}: 定義の後ろに、読めない記述があります"));
            }
            if keyword == "struct" {
                ItemKind::Struct(parse_fields(body)?)
            } else {
                let variants = parse_variants(body)?;
                if variants.is_empty() {
                    return Err(format!(
                        "{rust_name}: バリアントが 1 つもない enum は使えません"
                    ));
                }
                ItemKind::Enum(variants)
            }
        }
        Some(Tok::Group('(', _)) if keyword == "struct" => {
            return Err(format!(
                "{rust_name}: 名前のないフィールド（タプル struct）は使えません。名前つきフィールドの struct にしてください"
            ));
        }
        Some(Tok::Punct(';', _)) if keyword == "struct" => {
            return Err(format!(
                "{rust_name}: フィールドのない struct（`struct {rust_name};`）は使えません。`struct {rust_name} {{}}` と書いてください"
            ));
        }
        _ => return Err(format!("{rust_name}: 定義の形が読めません")),
    };
    // The name in scripts
    let local = attr.name.clone().unwrap_or_else(|| rust_name.clone());
    let full_name = match (&attr.namespace, local.contains('.')) {
        (Some(_), true) => {
            return Err(format!(
                "{rust_name}: name に名前空間（`.`）を含めるときは、namespace を併記できません"
            ));
        }
        (Some(ns), false) => {
            if ns.contains('.') {
                return Err(format!("{rust_name}: namespace に `.` は使えません"));
            }
            format!("{ns}.{local}")
        }
        (None, true) => local,
        (None, false) => {
            return Err(format!(
                "{rust_name}: 台本から使うには、名前空間が要ります。#[yomi(namespace = \"名前空間\")] を付けてください"
            ));
        }
    };
    Ok(Item {
        rust_name,
        full_name,
        kind,
    })
}
