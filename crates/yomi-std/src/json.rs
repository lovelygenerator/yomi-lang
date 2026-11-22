//! The `json` package: reads and rewrites JSON text by path (query commands only; `spec/packages/json.md`).
//!
//! JSON stays a `str` in scripts. Every function parses the text it is given, so a failure of any kind (invalid JSON,
//! a path that finds nothing, a value of another type) is `None` or `false`, never a runtime error.

use serde::Deserialize;
use serde_json::{Map, Value};
use yomi_core::{Ctx, Package, PackageReg};

/// The `json` package. It has no state.
pub struct Json;

/// The deepest nesting of arrays and objects that a document may have.
const MAX_DEPTH: usize = 128;

#[derive(Debug, PartialEq)]
enum Seg {
    Key(String),
    Index(usize),
}

/// Parses a document. The nesting limit is checked here, ahead of parsing, so that `serde_json`'s own limit
/// (one level lower than [`MAX_DEPTH`]) is switched off.
fn parse_doc(s: &str) -> Option<Value> {
    if nesting_exceeds_limit(s) {
        return None;
    }
    let mut de = serde_json::Deserializer::from_str(s);
    de.disable_recursion_limit();
    let v = Value::deserialize(&mut de).ok()?;
    de.end().ok()?;
    Some(v)
}

fn nesting_exceeds_limit(s: &str) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for b in s.bytes() {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Parses a path. `None` when the path is malformed.
fn parse_path(path: &str) -> Option<Vec<Seg>> {
    let b = path.as_bytes();
    let mut segs = Vec::new();
    let mut i = 0;
    let mut first = true;
    while i < b.len() {
        match b[i] {
            b'[' => {
                i += 1;
                if b.get(i) == Some(&b'"') {
                    let start = i;
                    i += 1;
                    loop {
                        match b.get(i)? {
                            b'\\' => i += 2,
                            b'"' => break,
                            _ => i += 1,
                        }
                    }
                    let key: String = serde_json::from_str(&path[start..=i]).ok()?;
                    i += 1;
                    segs.push(Seg::Key(key));
                } else {
                    let start = i;
                    while b.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    segs.push(Seg::Index(path[start..i].parse().ok()?));
                }
                if b.get(i) != Some(&b']') {
                    return None;
                }
                i += 1;
            }
            b'.' if !first => {
                i += 1;
                i = push_key(path, i, &mut segs)?;
            }
            _ if first => i = push_key(path, i, &mut segs)?,
            _ => return None,
        }
        first = false;
    }
    Some(segs)
}

/// Reads a bare key starting at `i` and returns the position after it.
fn push_key(path: &str, i: usize, segs: &mut Vec<Seg>) -> Option<usize> {
    let rest = &path[i..];
    let len = rest.find(['.', '[']).unwrap_or(rest.len());
    let key = &rest[..len];
    if key.is_empty() || key.contains([']', '"']) {
        return None;
    }
    segs.push(Seg::Key(key.to_string()));
    Some(i + len)
}

fn find<'a>(v: &'a Value, segs: &[Seg]) -> Option<&'a Value> {
    segs.iter().try_fold(v, |v, seg| match seg {
        Seg::Key(k) => v.as_object()?.get(k),
        Seg::Index(n) => v.as_array()?.get(*n),
    })
}

fn at(s: &str, path: &str) -> Option<Value> {
    let doc = parse_doc(s)?;
    find(&doc, &parse_path(path)?).cloned()
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn dump(v: &Value) -> Option<String> {
    (depth(v) <= MAX_DEPTH).then(|| v.to_string())
}

fn set(s: &str, path: &str, value: &str) -> Option<String> {
    let mut doc = parse_doc(s)?;
    let new = parse_doc(value)?;
    let segs = parse_path(path)?;
    let Some((last, parents)) = segs.split_last() else {
        return dump(&new);
    };
    let parent = parents.iter().try_fold(&mut doc, |v, seg| match seg {
        Seg::Key(k) => v.as_object_mut()?.get_mut(k),
        Seg::Index(n) => v.as_array_mut()?.get_mut(*n),
    })?;
    match last {
        Seg::Key(k) => {
            parent.as_object_mut()?.insert(k.clone(), new);
        }
        Seg::Index(n) => {
            let a = parent.as_array_mut()?;
            match (*n).cmp(&a.len()) {
                std::cmp::Ordering::Less => a[*n] = new,
                std::cmp::Ordering::Equal => a.push(new),
                std::cmp::Ordering::Greater => return None,
            }
        }
    }
    dump(&doc)
}

fn remove(s: &str, path: &str) -> Option<String> {
    let mut doc = parse_doc(s)?;
    let segs = parse_path(path)?;
    let (last, parents) = segs.split_last()?;
    let parent = parents.iter().try_fold(&mut doc, |v, seg| match seg {
        Seg::Key(k) => v.as_object_mut()?.get_mut(k),
        Seg::Index(n) => v.as_array_mut()?.get_mut(*n),
    })?;
    match last {
        Seg::Key(k) => {
            parent.as_object_mut()?.remove(k)?;
        }
        Seg::Index(n) => {
            let a = parent.as_array_mut()?;
            if *n >= a.len() {
                return None;
            }
            a.remove(*n);
        }
    }
    dump(&doc)
}

fn keys(m: &Map<String, Value>) -> Vec<String> {
    m.keys().cloned().collect()
}

impl Package for Json {
    const NAME: &'static str = "json";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.query("json.valid", |_: &Ctx<Json>, s: String| {
            parse_doc(&s).is_some()
        })
        .doc("文字列が、ちょうど 1 つの正しい JSON の値か")
        .param_names(&["s"]);
        reg.query("json.has", |_: &Ctx<Json>, s: String, path: String| {
            at(&s, &path).is_some()
        })
        .doc("パスに値があるか（null も値）")
        .param_names(&["s", "path"]);
        reg.query("json.kind", |_: &Ctx<Json>, s: String, path: String| {
            at(&s, &path).map(|v| kind(&v).to_string())
        })
        .doc("値の種類（null・bool・number・string・array・object）")
        .param_names(&["s", "path"]);
        reg.query(
            "json.get_str",
            |_: &Ctx<Json>, s: String, path: String| match at(&s, &path)? {
                Value::String(x) => Some(x),
                _ => None,
            },
        )
        .doc("文字列の値を取り出す")
        .param_names(&["s", "path"]);
        reg.query("json.get_int", |_: &Ctx<Json>, s: String, path: String| {
            at(&s, &path)?.as_i64()
        })
        .doc("整数の値を取り出す（小数点・指数のある書き方と範囲の外は None）")
        .param_names(&["s", "path"]);
        reg.query(
            "json.get_float",
            |_: &Ctx<Json>, s: String, path: String| {
                at(&s, &path)?.as_f64().filter(|x| x.is_finite())
            },
        )
        .doc("数の値を取り出す（整数も含む）")
        .param_names(&["s", "path"]);
        reg.query("json.get_bool", |_: &Ctx<Json>, s: String, path: String| {
            at(&s, &path)?.as_bool()
        })
        .doc("真偽の値を取り出す")
        .param_names(&["s", "path"]);
        reg.query("json.get_raw", |_: &Ctx<Json>, s: String, path: String| {
            dump(&at(&s, &path)?)
        })
        .doc("値を、コンパクトな JSON の文字列として取り出す")
        .param_names(&["s", "path"]);
        reg.query(
            "json.len",
            |_: &Ctx<Json>, s: String, path: String| match at(&s, &path)? {
                Value::Array(a) => i64::try_from(a.len()).ok(),
                Value::Object(o) => i64::try_from(o.len()).ok(),
                _ => None,
            },
        )
        .doc("配列の要素数、またはオブジェクトのキーの数")
        .param_names(&["s", "path"]);
        reg.query(
            "json.keys",
            |_: &Ctx<Json>, s: String, path: String| match at(&s, &path)? {
                Value::Object(o) => Some(keys(&o)),
                _ => None,
            },
        )
        .doc("オブジェクトのキー（コードポイント順）")
        .param_names(&["s", "path"]);
        reg.query("json.quote", |_: &Ctx<Json>, s: String| {
            Value::String(s).to_string()
        })
        .doc("文字列を JSON の文字列のリテラルにする")
        .param_names(&["s"]);
        reg.query(
            "json.set",
            |_: &Ctx<Json>, s: String, path: String, value: String| set(&s, &path, &value),
        )
        .doc("パスの値を、JSON の文字列 value で置き換えた文書を返す")
        .param_names(&["s", "path", "value"]);
        reg.query("json.remove", |_: &Ctx<Json>, s: String, path: String| {
            remove(&s, &path)
        })
        .doc("パスの値を消した文書を返す")
        .param_names(&["s", "path"]);
    }
}

#[cfg(test)]
mod tests;
