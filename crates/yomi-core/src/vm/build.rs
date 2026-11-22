//! VM: building and reading values.

use super::*;
use crate::value::{EnumValue, StructValue};

/// Makes a `struct` value. `given` are the values in the order written, and `order` is which declared field each is.
pub(super) fn make_struct(program: &Program, ty: u32, order: &[u32], given: Vec<Value>) -> Value {
    let decl = &program.types[ty as usize];
    let TypeKind::Struct { fields } = &decl.kind else {
        unreachable!("struct の宣言のはず")
    };
    let mut slots: Vec<Option<Value>> = vec![None; fields.len()];
    for (v, &di) in given.into_iter().zip(order) {
        slots[di as usize] = Some(v);
    }
    Value::Struct(Arc::new(StructValue {
        ty: decl.name.clone().into(),
        fields: fields
            .iter()
            .zip(slots)
            .map(|((n, _), v)| (n.clone(), v.unwrap_or(Value::Unit)))
            .collect::<Vec<_>>(),
    }))
}

/// Makes an `enum` value.
pub(super) fn make_enum(program: &Program, ty: u32, variant: u32, args: Vec<Value>) -> Value {
    let decl = &program.types[ty as usize];
    let TypeKind::Enum { variants } = &decl.kind else {
        unreachable!("enum の宣言のはず")
    };
    Value::Enum(Arc::new(EnumValue {
        ty: decl.name.clone().into(),
        variant: variants[variant as usize].name.clone().into(),
        args,
    }))
}

/// Makes a `map` from values lined up as key, value. If a key repeats, the later value updates it (the position stays the
/// first one).
pub(super) fn make_map(flat: Vec<Value>) -> Value {
    let mut pairs: Vec<(Value, Value)> = Vec::with_capacity(flat.len() / 2);
    let mut it = flat.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        pairs.push((k, v));
    }
    Value::Map(MapData::from_pairs(pairs))
}

/// Reads by index (`int` for `list`, the key for `map`). Out of range or a missing key is a runtime error.
pub(super) fn index(base: &Value, k: &Value) -> Result<Value, VmErrorKind> {
    match base {
        Value::List(items) => {
            let Value::Int(k) = k else {
                unreachable!("添字は整数のはず")
            };
            let len = items.len();
            usize::try_from(*k)
                .ok()
                .and_then(|i| items.get(i).cloned())
                .ok_or_else(|| VmErrorKind::IndexOutOfRange {
                    index: *k,
                    len: len as i64,
                })
        }
        Value::Map(entries) => entries
            .get(k)
            .cloned()
            .ok_or_else(|| VmErrorKind::KeyNotFound { key: k.to_text() }),
        other => unreachable!("list か map のはず: {other:?}"),
    }
}
