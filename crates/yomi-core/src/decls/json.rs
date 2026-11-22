//! Reading and writing the JSON of declaration files (`serde_json`). Checking limits before reading, and extracting items.

use serde::Deserialize;

use super::*;

pub(super) use serde_json::Value as Json;

/// The result of reading a declaration file.
pub(super) type JResult<T> = Result<T, DeclError>;

/// Reads a JSON string (the whole as one value).
///
/// The size ([`MAX_DECLS_BYTES`]) and nesting depth ([`MAX_JSON_DEPTH`]) are checked before reading. Since the depth is
/// counted first, `serde_json`'s recursion limit is disabled when reading (the depth that uses the stack is bounded by the
/// limit).
pub(super) fn parse_json(src: &str) -> JResult<Json> {
    if src.len() > MAX_DECLS_BYTES {
        return Err(err(DeclErrorKind::TooLarge, ""));
    }
    check_depth(src)?;
    let mut de = serde_json::Deserializer::from_str(src);
    de.disable_recursion_limit();
    let value = Json::deserialize(&mut de).and_then(|v| de.end().map(|()| v));
    value.map_err(|e| syntax(src, &e))
}

/// Whether the nesting depth of arrays and objects is within the limit (brackets inside strings are not counted).
fn check_depth(src: &str) -> JResult<()> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for b in src.bytes() {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(err(DeclErrorKind::TooDeep, ""));
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Turns a `serde_json` syntax error into a line and column (in characters; `serde_json`'s column is in bytes).
fn syntax(src: &str, e: &serde_json::Error) -> DeclError {
    let line = e.line().max(1);
    let text = src.split('\n').nth(line - 1).unwrap_or("");
    let mut end = e.column().saturating_sub(1).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let col = text[..end].chars().count() + 1;
    err(
        DeclErrorKind::Syntax {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            col: u32::try_from(col).unwrap_or(u32::MAX),
        },
        "",
    )
}

/// Writes it formatted (2-space indent; object keys in name order). Adds a newline at the end.
pub(super) fn write_json(j: &Json) -> String {
    let mut out = serde_json::to_string_pretty(j).expect("JSON の値は、いつも書き出せる");
    out.push('\n');
    out
}

/// Makes an object.
pub(super) fn obj(items: Vec<(String, Json)>) -> Json {
    Json::Object(items.into_iter().collect())
}

/// Makes a JSON string value.
pub(super) fn s(v: &str) -> Json {
    Json::String(v.to_string())
}

/// Extracts a required string item (`MissingField` if missing, `WrongType` if the type differs).
pub(super) fn want_str<'j>(j: &'j Json, key: &str, path: &str) -> JResult<&'j str> {
    let p = join(path, key);
    match j.get(key) {
        Some(Json::String(t)) => Ok(t),
        Some(_) => Err(err(DeclErrorKind::WrongType, &p)),
        None => Err(err(DeclErrorKind::MissingField, &p)),
    }
}

/// Extracts an optional string item (`null` and absence are `None`).
pub(super) fn opt_str<'j>(j: &'j Json, key: &str, path: &str) -> JResult<Option<&'j str>> {
    match j.get(key) {
        Some(Json::String(t)) => Ok(Some(t)),
        Some(Json::Null) | None => Ok(None),
        Some(_) => Err(err(DeclErrorKind::WrongType, &join(path, key))),
    }
}

/// Extracts an optional array item (`null` and absence are empty).
pub(super) fn opt_arr<'j>(j: &'j Json, key: &str, path: &str) -> JResult<&'j [Json]> {
    match j.get(key) {
        Some(Json::Array(a)) => Ok(a),
        Some(Json::Null) | None => Ok(&[]),
        Some(_) => Err(err(DeclErrorKind::WrongType, &join(path, key))),
    }
}
