//! Parsing type strings (such as `list<int>`) in declaration files.

use super::*;

/// The maximum nesting depth (`list<list<…>>`) when reading a type name string.
pub(super) const MAX_TY_TEXT_DEPTH: usize = (MAX_TYPE_DEPTH as usize) + 2;

/// Turns a type name string (`int`, `list<str>`, `map<str, int>`, `Option<Identity>`, a registered type name) into a
/// type. `kinds` maps the names of types to register → whether it is a `struct` (false for `enum`).
pub(super) fn parse_ty(text: &str, kinds: &HashMap<String, bool>, path: &str) -> JResult<Ty> {
    let bad = || err(DeclErrorKind::InvalidType(text.to_string()), path);
    let tokens = lex_ty(text).ok_or_else(bad)?;
    let mut pos = 0;
    let ty = ty_rec(&tokens, &mut pos, kinds, 0, path, text)?;
    if pos != tokens.len() {
        return Err(bad());
    }
    Ok(ty)
}

/// The notation of a type split into tokens.
#[derive(Debug, PartialEq)]
pub(super) enum Tok<'a> {
    Word(&'a str),
    Lt,
    Gt,
    Comma,
}

/// Splits the notation of a type (such as `list<int>`) into tokens. `None` if there are characters that cannot be used.
pub(super) fn lex_ty(text: &str) -> Option<Vec<Tok<'_>>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let special = matches!(c, '<' | '>' | ',') || c.is_whitespace();
        if special {
            if let Some(st) = start.take() {
                out.push(Tok::Word(&text[st..i]));
            }
            match c {
                '<' => out.push(Tok::Lt),
                '>' => out.push(Tok::Gt),
                ',' => out.push(Tok::Comma),
                _ => {}
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push(Tok::Word(&text[st..]));
    }
    Some(out)
}

/// Reads a type recursively from the tokens. `depth` bounds the nesting depth.
pub(super) fn ty_rec(
    tokens: &[Tok<'_>],
    pos: &mut usize,
    kinds: &HashMap<String, bool>,
    depth: usize,
    path: &str,
    text: &str,
) -> JResult<Ty> {
    let bad = || err(DeclErrorKind::InvalidType(text.to_string()), path);
    if depth > MAX_TY_TEXT_DEPTH {
        return Err(err(DeclErrorKind::TooDeep, path));
    }
    let Some(Tok::Word(w)) = tokens.get(*pos) else {
        return Err(bad());
    };
    *pos += 1;
    let args = |pos: &mut usize, n: usize| -> JResult<Vec<Ty>> {
        if tokens.get(*pos) != Some(&Tok::Lt) {
            return Err(bad());
        }
        *pos += 1;
        let mut out = Vec::new();
        for i in 0..n {
            if i > 0 {
                if tokens.get(*pos) != Some(&Tok::Comma) {
                    return Err(bad());
                }
                *pos += 1;
            }
            out.push(ty_rec(tokens, pos, kinds, depth + 1, path, text)?);
        }
        if tokens.get(*pos) != Some(&Tok::Gt) {
            return Err(bad());
        }
        *pos += 1;
        Ok(out)
    };
    Ok(match *w {
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "str" => Ty::Str,
        "Identity" => Ty::Identity,
        "list" => Ty::List(Box::new(args(pos, 1)?.remove(0))),
        "Option" => Ty::Option(Box::new(args(pos, 1)?.remove(0))),
        "map" => {
            let mut a = args(pos, 2)?;
            let v = a.remove(1);
            let k = a.remove(0);
            Ty::Map(Box::new(k), Box::new(v))
        }
        name => match kinds.get(name) {
            Some(true) => Ty::Struct(name.to_string()),
            Some(false) => Ty::Enum(name.to_string()),
            None => return Err(bad()),
        },
    })
}
