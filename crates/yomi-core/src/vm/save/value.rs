//! The shape of values in saves (`SavedValue`; nesting is flattened into a sequence of nodes and read back within the depth
//! limit).

use super::*;
use crate::value::{EnumValue, StructValue};

/// One node of a value put into saves. A value is held as a sequence of nodes (in postfix order: children first, then the
/// parent). It is not a recursive type so that reading a broken save does not exhaust the stack with deep nesting (reading
/// builds it with its own stack and checks the depth).
#[derive(Serialize, Deserialize, Clone)]
pub(in crate::vm) enum SavedNode {
    Unit,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// `identity` (stable ID and name).
    Identity {
        id: String,
        name: String,
    },
    /// A `list` whose elements are the previous `n` values.
    List(u32),
    /// A `map` (in insertion order) of the previous `2 * n` values, lined up as key, value.
    Map(u32),
    None,
    /// A `Some` wrapping the previous value.
    Some,
    /// A `struct` whose fields (in declaration order; names in `names`) are the previous `names.len()` values.
    Struct {
        ty: String,
        names: Vec<String>,
    },
    /// An `enum` value whose arguments are the previous `n` values.
    Enum {
        ty: String,
        variant: String,
        n: u32,
    },
}

/// A value put into saves.
#[derive(Serialize, Deserialize, Clone, Default)]
pub(in crate::vm) struct SavedValue(pub(in crate::vm) Vec<SavedNode>);

/// Appends a value to a sequence of nodes for saving (with nesting expanded, from the start).
pub(super) fn encode(v: &Value, out: &mut Vec<SavedNode>) {
    match v {
        // Function values cannot be held across stop points (the compiler rejects that), so they never come here
        Value::Unit | Value::Fn(_) => out.push(SavedNode::Unit),
        Value::Int(n) => out.push(SavedNode::Int(*n)),
        Value::Float(x) => out.push(SavedNode::Float(*x)),
        Value::Bool(b) => out.push(SavedNode::Bool(*b)),
        Value::Str(t) => out.push(SavedNode::Str(t.to_string())),
        Value::Identity(i) => out.push(SavedNode::Identity {
            id: i.id.clone(),
            name: i.name.clone(),
        }),
        Value::List(items) => {
            items.iter().for_each(|i| encode(i, out));
            out.push(SavedNode::List(items.len() as u32));
        }
        Value::Map(entries) => {
            for (k, v) in entries {
                encode(k, out);
                encode(v, out);
            }
            out.push(SavedNode::Map(entries.len() as u32));
        }
        Value::Option(None) => out.push(SavedNode::None),
        Value::Option(Some(inner)) => {
            encode(inner, out);
            out.push(SavedNode::Some);
        }
        Value::Struct(st) => {
            let StructValue { ty, fields } = &**st;
            fields.iter().for_each(|(_, v)| encode(v, out));
            out.push(SavedNode::Struct {
                ty: ty.to_string(),
                names: fields.iter().map(|(n, _)| n.clone()).collect(),
            });
        }
        Value::Enum(e) => {
            let EnumValue { ty, variant, args } = &**e;
            args.iter().for_each(|a| encode(a, out));
            out.push(SavedNode::Enum {
                ty: ty.to_string(),
                variant: variant.to_string(),
                n: args.len() as u32,
            });
        }
    }
}

impl SavedValue {
    /// Turns a value into the form for saving.
    pub(in crate::vm) fn from_value(v: &Value) -> Self {
        let mut nodes = Vec::new();
        encode(v, &mut nodes);
        Self(nodes)
    }

    /// Turns it back into a value. `NaN`, `±∞`, nesting that is too deep, and sequences with the wrong number of nodes are
    /// broken (`None`). `-0.0` is normalized to `0.0`. Does not use recursion.
    pub(in crate::vm) fn decode(&self) -> Option<Value> {
        // (value, nesting depth)
        let mut stack: Vec<(Value, u32)> = Vec::new();
        for node in &self.0 {
            let (v, depth) = match node {
                SavedNode::Unit => (Value::Unit, 1),
                SavedNode::Int(n) => (Value::Int(*n), 1),
                SavedNode::Float(x) => (Value::Float(finite_float(*x)?), 1),
                SavedNode::Bool(b) => (Value::Bool(*b), 1),
                SavedNode::Str(t) => (Value::Str(t.clone().into()), 1),
                SavedNode::Identity { id, name } => (
                    Value::Identity(Arc::new(Identity {
                        id: id.clone(),
                        name: name.clone(),
                    })),
                    1,
                ),
                SavedNode::None => (Value::Option(None), 1),
                SavedNode::Some => {
                    let (inner, d) = stack.pop()?;
                    (Value::Option(Some(Box::new(inner))), d + 1)
                }
                SavedNode::List(n) => {
                    let (items, d) = take_children(&mut stack, *n as usize)?;
                    (Value::List(items.into()), d + 1)
                }
                SavedNode::Map(n) => {
                    let (flat, d) = take_children(&mut stack, (*n as usize).checked_mul(2)?)?;
                    // The index is not saved. It is rebuilt here
                    let mut entries = crate::value::MapData::new();
                    let mut it = flat.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        // Duplicate keys mean it is broken
                        if !entries.push_new(k, v) {
                            return None;
                        }
                    }
                    (Value::Map(entries), d + 1)
                }
                SavedNode::Struct { ty, names } => {
                    let (vals, d) = take_children(&mut stack, names.len())?;
                    (
                        Value::Struct(Arc::new(StructValue {
                            ty: ty.clone().into(),
                            fields: names.iter().cloned().zip(vals).collect::<Vec<_>>(),
                        })),
                        d + 1,
                    )
                }
                SavedNode::Enum { ty, variant, n } => {
                    let (args, d) = take_children(&mut stack, *n as usize)?;
                    (
                        Value::Enum(Arc::new(EnumValue {
                            ty: ty.clone().into(),
                            variant: variant.clone().into(),
                            args,
                        })),
                        d + 1,
                    )
                }
            };
            if depth > MAX_VALUE_DEPTH {
                return None;
            }
            stack.push((v, depth));
        }
        match stack.len() {
            1 => stack.pop().map(|(v, _)| v),
            _ => None,
        }
    }

    /// Turns it back into a value (`()` if broken; check with `decode` before reading).
    pub(in crate::vm) fn into_value(self) -> Value {
        self.decode().unwrap_or(Value::Unit)
    }
}

/// Takes the last `n` entries of the stack (the sequence of values, and the depth of the deepest child).
pub(super) fn take_children(stack: &mut Vec<(Value, u32)>, n: usize) -> Option<(Vec<Value>, u32)> {
    let at = stack.len().checked_sub(n)?;
    let children = stack.split_off(at);
    let depth = children.iter().map(|(_, d)| *d).max().unwrap_or(0);
    Some((children.into_iter().map(|(v, _)| v).collect(), depth))
}
