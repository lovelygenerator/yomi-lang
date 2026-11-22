//! Conversion between values (defaults and constants) and JSON in declaration files.

use super::*;
use crate::value::{EnumValue, StructValue};
use std::sync::Arc;

/// Turns a value into JSON following its type (`int` and `float` are numbers, `bool` and `str` as is, `Identity` is
/// `{id, name}`, `list` is an array, `map` is an array of `[key, value]` pairs, `Option` is `null` or `{"some": value}`,
/// `struct` is an object of field names, `enum` is `{"variant": name, "args": [...]}`).
pub(super) fn value_json(v: &Value) -> Json {
    match v {
        Value::Unit | Value::Fn(_) => Json::Null,
        Value::Int(n) => Json::from(*n),
        Value::Float(x) => Json::from(*x),
        Value::Bool(b) => Json::Bool(*b),
        Value::Str(t) => s(t),
        Value::Identity(i) => obj(vec![("id".into(), s(&i.id)), ("name".into(), s(&i.name))]),
        Value::List(items) => Json::Array(items.iter().map(value_json).collect()),
        Value::Map(entries) => Json::Array(
            entries
                .iter()
                .map(|(k, v)| Json::Array(vec![value_json(k), value_json(v)]))
                .collect(),
        ),
        Value::Option(None) => Json::Null,
        Value::Option(Some(inner)) => obj(vec![("some".into(), value_json(inner))]),
        Value::Struct(st) => Json::Object(
            st.fields
                .iter()
                .map(|(n, v)| (n.clone(), value_json(v)))
                .collect(),
        ),
        Value::Enum(e) => obj(vec![
            ("variant".into(), s(&e.variant)),
            (
                "args".into(),
                Json::Array(e.args.iter().map(value_json).collect()),
            ),
        ]),
    }
}

/// Reads JSON as a value of type `ty`. `types` are the declarations of registered types.
pub(super) fn json_value(j: &Json, ty: &Ty, types: &[TypeDecl], path: &str) -> JResult<Value> {
    let wrong = || err(DeclErrorKind::WrongType, path);
    let invalid = || err(DeclErrorKind::InvalidValue, path);
    Ok(match ty {
        // Declaration files cannot have values of function types
        Ty::Fn(..) => return Err(wrong()),
        Ty::Unit => match j {
            Json::Null => Value::Unit,
            _ => return Err(wrong()),
        },
        Ty::Int => match j {
            // Decimal and exponent forms (`1.0`, `1e3`) and values outside the range of `i64` are not read as integers
            Json::Number(n) => Value::Int(n.as_i64().ok_or_else(invalid)?),
            _ => return Err(wrong()),
        },
        Ty::Float => match j {
            Json::Number(n) => {
                let x = n.as_f64().ok_or_else(invalid)?;
                Value::Float(crate::value::finite_float(x).ok_or_else(invalid)?)
            }
            _ => return Err(wrong()),
        },
        Ty::Bool => match j {
            Json::Bool(b) => Value::Bool(*b),
            _ => return Err(wrong()),
        },
        Ty::Str => match j {
            Json::String(t) => Value::Str(t.clone().into()),
            _ => return Err(wrong()),
        },
        Ty::Identity => match j {
            Json::Object(_) => {
                let id = match j.get("id") {
                    Some(Json::String(t)) => t.clone(),
                    Some(_) => return Err(err(DeclErrorKind::WrongType, &join(path, "id"))),
                    None => return Err(err(DeclErrorKind::MissingField, &join(path, "id"))),
                };
                let name = match j.get("name") {
                    Some(Json::String(t)) => t.clone(),
                    Some(_) => return Err(err(DeclErrorKind::WrongType, &join(path, "name"))),
                    None => return Err(err(DeclErrorKind::MissingField, &join(path, "name"))),
                };
                Value::Identity(Arc::new(Identity { id, name }))
            }
            _ => return Err(wrong()),
        },
        Ty::List(t) => match j {
            Json::Array(items) => Value::List(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| json_value(v, t, types, &join(path, &format!("[{i}]"))))
                    .collect::<JResult<Vec<_>>>()?
                    .into(),
            ),
            _ => return Err(wrong()),
        },
        Ty::Map(kt, vt) => match j {
            Json::Array(items) => {
                let mut entries = crate::value::MapData::new();
                for (i, pair) in items.iter().enumerate() {
                    let p = join(path, &format!("[{i}]"));
                    let Json::Array(kv) = pair else {
                        return Err(err(DeclErrorKind::WrongType, &p));
                    };
                    let [k, v] = kv.as_slice() else {
                        return Err(err(DeclErrorKind::InvalidValue, &p));
                    };
                    let k = json_value(k, kt, types, &join(&p, "[0]"))?;
                    let v = json_value(v, vt, types, &join(&p, "[1]"))?;
                    if !entries.push_new(k, v) {
                        return Err(err(DeclErrorKind::InvalidValue, &p));
                    }
                }
                Value::Map(entries)
            }
            _ => return Err(wrong()),
        },
        Ty::Option(t) => match j {
            Json::Null => Value::Option(None),
            Json::Object(_) => match j.get("some") {
                Some(inner) => Value::Option(Some(Box::new(json_value(
                    inner,
                    t,
                    types,
                    &join(path, "some"),
                )?))),
                None => return Err(err(DeclErrorKind::MissingField, &join(path, "some"))),
            },
            _ => return Err(wrong()),
        },
        Ty::Struct(n) => {
            let Json::Object(_) = j else {
                return Err(wrong());
            };
            let Some(TypeKind::Struct { fields }) =
                types.iter().find(|d| d.name == *n).map(|d| &d.kind)
            else {
                return Err(invalid());
            };
            let mut out = Vec::with_capacity(fields.len());
            for (fname, ft) in fields {
                let p = join(path, fname);
                let Some(fj) = j.get(fname) else {
                    return Err(err(DeclErrorKind::MissingField, &p));
                };
                out.push((fname.clone(), json_value(fj, ft, types, &p)?));
            }
            Value::Struct(Arc::new(StructValue {
                ty: n.clone().into(),
                fields: out,
            }))
        }
        Ty::Enum(n) => {
            let Json::Object(_) = j else {
                return Err(wrong());
            };
            let Some(TypeKind::Enum { variants }) =
                types.iter().find(|d| d.name == *n).map(|d| &d.kind)
            else {
                return Err(invalid());
            };
            let Some(Json::String(vname)) = j.get("variant") else {
                return Err(err(DeclErrorKind::MissingField, &join(path, "variant")));
            };
            let Some(v) = variants.iter().find(|v| v.name == *vname) else {
                return Err(err(DeclErrorKind::InvalidValue, &join(path, "variant")));
            };
            let empty = Vec::new();
            let args_json = match j.get("args") {
                Some(Json::Array(a)) => a,
                Some(_) => return Err(err(DeclErrorKind::WrongType, &join(path, "args"))),
                None => &empty,
            };
            if args_json.len() != v.args.len() {
                return Err(err(DeclErrorKind::InvalidValue, &join(path, "args")));
            }
            let mut args = Vec::with_capacity(args_json.len());
            for (i, (a, at)) in args_json.iter().zip(&v.args).enumerate() {
                let p = join(&join(path, "args"), &format!("[{i}]"));
                args.push(json_value(a, at, types, &p)?);
            }
            Value::Enum(Arc::new(EnumValue {
                ty: n.clone().into(),
                variant: vname.clone().into(),
                args,
            }))
        }
    })
}
