//! Values and types.

mod list;
mod map;
mod str;

pub use list::ListData;
pub use map::{MapData, MapEntries};
pub use str::StrValue;

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A type of the script language.
///
/// `int`, `float`, `bool`, `str`, `list<T>`, `map<K, V>`, `Option<T>`, and `struct`s and `enum`s declared by users can be
/// made by script expressions. `Identity` is a value that comes from quote line declarations
/// (`identity 案内人 = "案内人"`) and can also be written in type annotations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ty {
    /// No return value.
    Unit,
    /// A 64-bit integer.
    Int,
    /// A 64-bit floating point number. `NaN` and `±∞` never appear as values.
    Float,
    /// A boolean.
    Bool,
    /// A string.
    Str,
    /// The built-in `Identity` (`{ id: str, name: str }`).
    Identity,
    /// `list<T>`.
    List(Box<Ty>),
    /// `map<K, V>` (keys and values; iteration is in insertion order).
    Map(Box<Ty>, Box<Ty>),
    /// `Option<T>`.
    Option(Box<Ty>),
    /// A `struct` declared by users. The name is `module.TypeName` (the declaration is a [`TypeDecl`]).
    Struct(String),
    /// An `enum` declared by users. The name is `module.TypeName` (the declaration is a [`TypeDecl`]).
    Enum(String),
    /// A function type `fn(parameter types, ...) -> return type`. Cannot be put into saves.
    Fn(Vec<Ty>, Box<Ty>),
}

/// The maximum nesting depth of types (the depth following `list`, `Option`, `struct` fields and `enum` arguments).
/// Recursive types are not allowed, so the nesting depth of values does not exceed this either (a limit so that the
/// recursion of drop, cloning and serialization does not overflow even a 1 MiB stack).
pub const MAX_TYPE_DEPTH: u32 = 32;

/// The maximum nesting depth when reading saved values (so that broken saves do not exhaust the stack).
pub(crate) const MAX_VALUE_DEPTH: u32 = 64;

/// The declaration of a type declared by users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    /// `module.TypeName`.
    pub name: String,
    /// Contents.
    pub kind: TypeKind,
}

/// The contents of a type declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    /// A `struct`. Fields are in declaration order.
    Struct {
        /// Fields (name and type).
        fields: Vec<(String, Ty)>,
    },
    /// An `enum`. Variants are in declaration order.
    Enum {
        /// Variants.
        variants: Vec<VariantDecl>,
    },
}

/// The declaration of an `enum` variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDecl {
    /// The name of the variant.
    pub name: String,
    /// The argument types (empty if none).
    pub args: Vec<Ty>,
}

impl TypeDecl {
    /// A string describing the shape of the declaration (put into saves and compared with the script's declaration on load).
    pub fn signature(&self) -> String {
        match &self.kind {
            TypeKind::Struct { fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {}", t.name()))
                    .collect();
                if parts.is_empty() {
                    "struct {}".to_string()
                } else {
                    format!("struct {{ {} }}", parts.join(", "))
                }
            }
            TypeKind::Enum { variants } => {
                let parts: Vec<String> = variants
                    .iter()
                    .map(|v| {
                        if v.args.is_empty() {
                            v.name.clone()
                        } else {
                            let a: Vec<String> = v.args.iter().map(Ty::name).collect();
                            format!("{}({})", v.name, a.join(", "))
                        }
                    })
                    .collect();
                format!("enum {{ {} }}", parts.join(", "))
            }
        }
    }

    /// The names of user-defined types that this declaration uses directly.
    pub(crate) fn used_types(&self) -> Vec<&str> {
        let mut out = Vec::new();
        match &self.kind {
            TypeKind::Struct { fields } => {
                fields.iter().for_each(|(_, t)| t.collect_user(&mut out))
            }
            TypeKind::Enum { variants } => variants
                .iter()
                .flat_map(|v| &v.args)
                .for_each(|t| t.collect_user(&mut out)),
        }
        out
    }
}

/// Finds a type declaration by name.
pub fn find_type<'a>(types: &'a [TypeDecl], name: &str) -> Option<&'a TypeDecl> {
    types.iter().find(|t| t.name == name)
}

/// The part of a type name without the module (`main.Chara` → `Chara`).
pub(crate) fn short_type_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

impl Ty {
    /// The name of a type in scripts (`int`, `list<str>`, `Option<Identity>`, and so on).
    pub fn name(&self) -> String {
        match self {
            Ty::Unit => "()".to_string(),
            Ty::Int => "int".to_string(),
            Ty::Float => "float".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Str => "str".to_string(),
            Ty::Identity => "Identity".to_string(),
            Ty::List(t) => format!("list<{}>", t.name()),
            Ty::Map(k, v) => format!("map<{}, {}>", k.name(), v.name()),
            Ty::Option(t) => format!("Option<{}>", t.name()),
            Ty::Struct(n) | Ty::Enum(n) => n.clone(),
            Ty::Fn(params, ret) => {
                let ps: Vec<String> = params.iter().map(Ty::name).collect();
                format!("fn({}) -> {}", ps.join(", "), ret.name())
            }
        }
    }

    /// Whether it contains a function type (also looks inside `list`, `map` and `Option`). Function values cannot be put into
    /// saves, so variables of a type for which this is true cannot be held across stop points. `struct`s and `enum`s cannot
    /// have function types.
    pub fn contains_fn(&self) -> bool {
        match self {
            Ty::Fn(..) => true,
            Ty::List(t) | Ty::Option(t) => t.contains_fn(),
            Ty::Map(k, v) => k.contains_fn() || v.contains_fn(),
            _ => false,
        }
    }

    /// The default value of a type. `types` are the declarations of `struct`s and `enum`s (without them, user-defined types
    /// give `()`).
    ///
    /// A `struct` has default values for all fields. An `enum` is its first variant (with default arguments).
    pub fn default_in(&self, types: &[TypeDecl]) -> Value {
        match self {
            Ty::Unit => Value::Unit,
            Ty::Int => Value::Int(0),
            Ty::Float => Value::Float(0.0),
            Ty::Bool => Value::Bool(false),
            Ty::Str => Value::Str(String::new().into()),
            Ty::Identity => Value::Identity(Arc::default()),
            Ty::List(_) => Value::List(ListData::new()),
            Ty::Map(..) => Value::Map(MapData::new()),
            Ty::Option(_) => Value::Option(None),
            // Variables of function types always have an initial value (the default is never used)
            Ty::Fn(..) => Value::Unit,
            Ty::Struct(n) | Ty::Enum(n) => match find_type(types, n).map(|d| &d.kind) {
                Some(TypeKind::Struct { fields }) => Value::Struct(Arc::new(StructValue {
                    ty: n.clone().into(),
                    fields: fields
                        .iter()
                        .map(|(f, t)| (f.clone(), t.default_in(types)))
                        .collect::<Vec<_>>(),
                })),
                Some(TypeKind::Enum { variants }) => match variants.first() {
                    Some(v) => Value::Enum(Arc::new(EnumValue {
                        ty: n.clone().into(),
                        variant: v.name.clone().into(),
                        args: v
                            .args
                            .iter()
                            .map(|t| t.default_in(types))
                            .collect::<Vec<_>>(),
                    })),
                    None => Value::Unit,
                },
                None => Value::Unit,
            },
        }
    }

    /// The nesting depth (`list<int>` is 2, `int` is 1). For `struct`s and `enum`s, follows the declarations (`types`).
    /// On meeting a recursive type (rejected by declaration checks), stops and returns the limit ([`MAX_TYPE_DEPTH`] + 1).
    pub fn depth_in(&self, types: &[TypeDecl]) -> u32 {
        self.depth_rec(types, 0)
    }

    /// Counts the nesting depth of a type. `from` is the current depth. Stops when the limit is exceeded.
    fn depth_rec(&self, types: &[TypeDecl], from: u32) -> u32 {
        if from > MAX_TYPE_DEPTH {
            return from;
        }
        match self {
            Ty::List(t) | Ty::Option(t) => 1 + t.depth_rec(types, from + 1),
            Ty::Map(k, v) => {
                1 + k
                    .depth_rec(types, from + 1)
                    .max(v.depth_rec(types, from + 1))
            }
            Ty::Fn(params, ret) => {
                1 + params
                    .iter()
                    .chain(std::iter::once(&**ret))
                    .map(|t| t.depth_rec(types, from + 1))
                    .max()
                    .unwrap_or(0)
            }
            Ty::Struct(n) | Ty::Enum(n) => {
                let inner = match find_type(types, n).map(|d| &d.kind) {
                    Some(TypeKind::Struct { fields }) => fields
                        .iter()
                        .map(|(_, t)| t.depth_rec(types, from + 1))
                        .max()
                        .unwrap_or(0),
                    Some(TypeKind::Enum { variants }) => variants
                        .iter()
                        .flat_map(|v| &v.args)
                        .map(|t| t.depth_rec(types, from + 1))
                        .max()
                        .unwrap_or(0),
                    None => 0,
                };
                1 + inner
            }
            _ => 1,
        }
    }

    /// Collects the names of user-defined types used by a type.
    pub(crate) fn collect_user<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Ty::List(t) | Ty::Option(t) => t.collect_user(out),
            Ty::Map(k, v) => {
                k.collect_user(out);
                v.collect_user(out);
            }
            Ty::Fn(params, ret) => {
                params.iter().for_each(|t| t.collect_user(out));
                ret.collect_user(out);
            }
            Ty::Struct(n) | Ty::Enum(n) => out.push(n),
            _ => {}
        }
    }

    /// Whether the type can be a key of `map<K, V>` (section 3.1 of `spec/language/03-statements.md`). `int`, `str`, `bool`,
    /// `Identity` (`{ id: str, name: str }`), and `struct`s and `enum`s whose fields (arguments) all have key types. `float`,
    /// `list`, `map` and `Option` cannot be keys.
    pub fn is_key_in(&self, types: &[TypeDecl]) -> bool {
        self.key_rec(types, 0)
    }

    /// Checks whether the type can be a key of a `map`.
    fn key_rec(&self, types: &[TypeDecl], from: u32) -> bool {
        if from > MAX_TYPE_DEPTH {
            return false;
        }
        match self {
            Ty::Int | Ty::Str | Ty::Bool | Ty::Identity => true,
            Ty::Struct(n) | Ty::Enum(n) => match find_type(types, n).map(|d| &d.kind) {
                Some(TypeKind::Struct { fields }) => {
                    fields.iter().all(|(_, t)| t.key_rec(types, from + 1))
                }
                Some(TypeKind::Enum { variants }) => variants
                    .iter()
                    .flat_map(|v| &v.args)
                    .all(|t| t.key_rec(types, from + 1)),
                None => false,
            },
            _ => false,
        }
    }
}

/// The built-in type `Identity` (the identity of quote lines; `案内人` and `"案内人"` in `identity 案内人 = "案内人"`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identity {
    /// The declared name (`案内人`).
    pub id: String,
    /// The display name given in the declaration (`"案内人"`).
    pub name: String,
}

/// The contents of a `struct` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructValue {
    /// The type name (`module.TypeName`).
    pub ty: Arc<str>,
    /// Fields (in declaration order; name and value).
    pub fields: Vec<(String, Value)>,
}

/// The contents of an `enum` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumValue {
    /// The type name (`module.TypeName`).
    pub ty: Arc<str>,
    /// The name of the variant.
    pub variant: Arc<str>,
    /// The arguments of the variant.
    pub args: Vec<Value>,
}

/// A value.
///
/// `==` compares contents. `map`s are equal if they have the same values for the same keys, regardless of insertion order
/// (other types are equal only if order and names match too).
#[non_exhaustive]
#[derive(Clone, Serialize, Deserialize)]
pub enum Value {
    /// No return value.
    Unit,
    /// An integer.
    Int(i64),
    /// A floating point number. Finite, and never `-0.0` (normalized to `0.0`).
    Float(f64),
    /// A boolean.
    Bool(bool),
    /// A string. The contents are shared (cloning only copies the `Arc`; strings are never modified).
    Str(StrValue),
    /// An `Identity` (shared; the contents live behind an `Arc` to keep `Value` small).
    Identity(Arc<Identity>),
    /// `list<T>`. The elements are a [`ListData`] (cloning shares them; an update copies the contents if shared).
    List(ListData),
    /// `map<K, V>`. Holds pairs of keys and values in insertion order (keys do not repeat).
    Map(MapData),
    /// `Option<T>` (`None` is `None`).
    Option(Option<Box<Value>>),
    /// A `struct` value (shared; the contents live behind an `Arc` to keep `Value` small). To modify it, use
    /// `Arc::make_mut`, which copies first if shared.
    Struct(Arc<StructValue>),
    /// An `enum` value (shared; the contents live behind an `Arc` to keep `Value` small). To modify it, use
    /// `Arc::make_mut`, which copies first if shared.
    Enum(Arc<EnumValue>),
    /// A function value (a reference to a `fn`, or a closure made by a lambda). It does not go into saves, so it cannot be
    /// serialized.
    #[serde(skip)]
    Fn(Arc<FnValue>),
}

/// The contents of a function value.
#[derive(Debug, Clone, PartialEq)]
pub struct FnValue {
    /// The function to call (a sequential number within the program; meaningful only within the same program).
    pub(crate) f: u32,
    /// The values captured when it was made (by value).
    pub(crate) captures: Box<[Value]>,
}

/// `struct`s and `enum`s are shown with their fields, without showing the inner `Arc` (`Struct { ty, fields }`).
impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Unit => f.write_str("Unit"),
            Value::Int(n) => f.debug_tuple("Int").field(n).finish(),
            Value::Float(x) => f.debug_tuple("Float").field(x).finish(),
            Value::Bool(b) => f.debug_tuple("Bool").field(b).finish(),
            Value::Str(s) => f.debug_tuple("Str").field(s).finish(),
            Value::Identity(i) => f.debug_tuple("Identity").field(i).finish(),
            Value::List(items) => f.debug_tuple("List").field(items).finish(),
            Value::Map(entries) => f.debug_tuple("Map").field(entries).finish(),
            Value::Option(v) => f.debug_tuple("Option").field(v).finish(),
            Value::Struct(st) => f
                .debug_struct("Struct")
                .field("ty", &st.ty)
                .field("fields", &st.fields)
                .finish(),
            Value::Enum(e) => f
                .debug_struct("Enum")
                .field("ty", &e.ty)
                .field("variant", &e.variant)
                .field("args", &e.args)
                .finish(),
            Value::Fn(fv) => f.debug_tuple("Fn").field(&fv.f).finish(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Identity(a), Value::Identity(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Option(a), Value::Option(b)) => a == b,
            (Value::Struct(a), Value::Struct(b)) => a == b,
            (Value::Enum(a), Value::Enum(b)) => a == b,
            // Function values cannot be compared with `==` (rejected by type checking). Only checks whether they are the same value
            (Value::Fn(a), Value::Fn(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Turns the result of a floating point operation into a script value. `None` for `NaN` or `±∞`. `-0.0` becomes `0.0`.
pub(crate) fn finite_float(x: f64) -> Option<f64> {
    if !x.is_finite() {
        None
    } else if x == 0.0 {
        Some(0.0)
    } else {
        Some(x)
    }
}

/// The string form of a floating point number (the shortest round trip of Rust's `{}`; integer values in the form `1.0`).
pub(crate) fn float_text(x: f64) -> String {
    let mut s = x.to_string();
    if !s.contains('.') {
        s.push_str(".0");
    }
    s
}

impl Value {
    /// Makes floating point numbers finite with `-0.0` normalized to `0.0` (also inside nesting). `None` if it contains `NaN`
    /// or `±∞`.
    pub(crate) fn normalized(self) -> Option<Value> {
        Some(match self {
            Value::Float(x) => Value::Float(finite_float(x)?),
            Value::List(items) => Value::List(
                items
                    .into_vec()
                    .into_iter()
                    .map(Value::normalized)
                    .collect::<Option<Vec<_>>>()?
                    .into(),
            ),
            Value::Map(entries) => Value::Map(MapData::from_pairs(
                entries
                    .into_entries()
                    .into_iter()
                    .map(|(k, v)| Some((k.normalized()?, v.normalized()?)))
                    .collect::<Option<Vec<_>>>()?,
            )),
            Value::Option(Some(v)) => Value::Option(Some(Box::new((*v).normalized()?))),
            Value::Struct(st) => {
                let StructValue { ty, fields } = Arc::unwrap_or_clone(st);
                Value::Struct(Arc::new(StructValue {
                    ty,
                    fields: fields
                        .into_iter()
                        .map(|(n, v)| Some((n, v.normalized()?)))
                        .collect::<Option<Vec<_>>>()?,
                }))
            }
            Value::Enum(e) => {
                let EnumValue { ty, variant, args } = Arc::unwrap_or_clone(e);
                Value::Enum(Arc::new(EnumValue {
                    ty,
                    variant,
                    args: args
                        .into_iter()
                        .map(Value::normalized)
                        .collect::<Option<Vec<_>>>()?,
                }))
            }
            other => other,
        })
    }

    /// The type of the value (as far as can be known). The element type of an empty `list` and the content type of `None`
    /// cannot be decided, so they are returned as `()`. Use [`Value::matches`] to check types.
    pub fn ty(&self) -> Ty {
        match self {
            Value::Unit => Ty::Unit,
            Value::Int(_) => Ty::Int,
            Value::Float(_) => Ty::Float,
            Value::Bool(_) => Ty::Bool,
            Value::Str(_) => Ty::Str,
            Value::Identity(_) => Ty::Identity,
            Value::List(items) => Ty::List(Box::new(items.first().map_or(Ty::Unit, Value::ty))),
            Value::Map(entries) => Ty::Map(
                Box::new(entries.entries().next().map_or(Ty::Unit, |(k, _)| k.ty())),
                Box::new(entries.entries().next().map_or(Ty::Unit, |(_, v)| v.ty())),
            ),
            Value::Option(inner) => {
                Ty::Option(Box::new(inner.as_deref().map_or(Ty::Unit, Value::ty)))
            }
            Value::Struct(st) => Ty::Struct(st.ty.to_string()),
            Value::Enum(e) => Ty::Enum(e.ty.to_string()),
            // Function values do not carry parameter and return types
            Value::Fn(_) => Ty::Fn(Vec::new(), Box::new(Ty::Unit)),
        }
    }

    /// Whether the value matches type `ty` (`list` and `Option` are checked down to their contents). For `struct`s and
    /// `enum`s, only the type name is checked (to check fields too, use [`Value::matches_in`]).
    pub fn matches(&self, ty: &Ty) -> bool {
        self.matches_in(ty, &[])
    }

    /// Whether the value matches type `ty`. For `struct`s and `enum`s declared in `types`, names and types of fields and
    /// arguments are checked too (without a declaration, only the type name).
    pub fn matches_in(&self, ty: &Ty, types: &[TypeDecl]) -> bool {
        match (self, ty) {
            (Value::Unit, Ty::Unit)
            | (Value::Int(_), Ty::Int)
            | (Value::Float(_), Ty::Float)
            | (Value::Bool(_), Ty::Bool)
            | (Value::Str(_), Ty::Str)
            | (Value::Identity(_), Ty::Identity) => true,
            (Value::List(items), Ty::List(t)) => items.iter().all(|v| v.matches_in(t, types)),
            (Value::Map(entries), Ty::Map(kt, vt)) => entries
                .iter()
                .all(|(k, v)| k.matches_in(kt, types) && v.matches_in(vt, types)),
            (Value::Option(None), Ty::Option(_)) | (Value::Fn(_), Ty::Fn(..)) => true,
            (Value::Option(Some(v)), Ty::Option(t)) => v.matches_in(t, types),
            (Value::Struct(st), Ty::Struct(tn)) => {
                let StructValue { ty: vn, fields } = &**st;
                **vn == **tn
                    && match find_type(types, tn).map(|d| &d.kind) {
                        Some(TypeKind::Struct { fields: decl }) => {
                            decl.len() == fields.len()
                                && decl.iter().zip(fields.iter()).all(|((dn, dt), (fnm, fv))| {
                                    dn == fnm && fv.matches_in(dt, types)
                                })
                        }
                        Some(_) => false,
                        None => true,
                    }
            }
            (Value::Enum(e), Ty::Enum(tn)) => {
                let EnumValue {
                    ty: vn,
                    variant,
                    args,
                } = &**e;
                **vn == **tn
                    && match find_type(types, tn).map(|d| &d.kind) {
                        Some(TypeKind::Enum { variants }) => variants
                            .iter()
                            .find(|v| *v.name == **variant)
                            .is_some_and(|v| {
                                v.args.len() == args.len()
                                    && v.args
                                        .iter()
                                        .zip(args.iter())
                                        .all(|(t, a)| a.matches_in(t, types))
                            }),
                        Some(_) => false,
                        None => true,
                    }
            }
            _ => false,
        }
    }

    /// The nesting depth of the value (scalars are 1).
    pub fn depth(&self) -> u32 {
        match self {
            Value::List(items) => 1 + items.iter().map(Value::depth).max().unwrap_or(0),
            Value::Map(entries) => {
                1 + entries
                    .iter()
                    .map(|(k, v)| k.depth().max(v.depth()))
                    .max()
                    .unwrap_or(0)
            }
            Value::Option(Some(v)) => 1 + v.depth(),
            Value::Struct(st) => 1 + st.fields.iter().map(|(_, v)| v.depth()).max().unwrap_or(0),
            Value::Enum(e) => 1 + e.args.iter().map(Value::depth).max().unwrap_or(0),
            _ => 1,
        }
    }

    /// An approximate size of the value (bytes). Used to check `VmConfig::max_value_bytes`. Scalars are 8, strings are length
    /// + 24, and `list`, `struct`, `enum` and `Option` are 24 + the sum of their contents.
    #[inline]
    pub fn approx_bytes(&self) -> usize {
        match self {
            Value::Unit | Value::Int(_) | Value::Float(_) | Value::Bool(_) => 8,
            _ => self.approx_bytes_composite(),
        }
    }

    /// The non-number part of `approx_bytes` (not inlined into callers, to keep the number case fast).
    #[inline(never)]
    fn approx_bytes_composite(&self) -> usize {
        match self {
            Value::Unit | Value::Int(_) | Value::Float(_) | Value::Bool(_) => 8,
            Value::Str(s) => 24 + s.len(),
            Value::Identity(i) => 48 + i.id.len() + i.name.len(),
            // The size of the contents is held by `ListData` and `MapData` (not recounted on every read)
            Value::List(items) => 24 + items.content_bytes(),
            Value::Map(entries) => 24 + entries.content_bytes(),
            Value::Option(None) => 8,
            Value::Option(Some(v)) => 8 + v.approx_bytes(),
            Value::Struct(st) => {
                24 + st
                    .fields
                    .iter()
                    .map(|(_, v)| v.approx_bytes())
                    .sum::<usize>()
            }
            Value::Enum(e) => 24 + e.args.iter().map(Value::approx_bytes).sum::<usize>(),
            Value::Fn(fv) => 24 + fv.captures.iter().map(Value::approx_bytes).sum::<usize>(),
        }
    }

    /// The value as a string (used for `${expr}` interpolation).
    pub fn to_text(&self) -> String {
        self.render(false)
    }

    /// The display string for the REPL and `:vars` (section 7.2.4 of spec/host/07-repl.md). Same as the `${expr}` form
    /// ([`Value::to_text`]), except that `str` is enclosed in `"` including inside nesting (escaped with `\"`, `\\`, `\n`,
    /// `\t`, `\$`), and `Identity` is written `Identity { id: "…", name: "…" }`.
    pub fn to_repl_text(&self) -> String {
        self.render(true)
    }

    /// The body of `to_text` (`repl` false) and `to_repl_text` (true).
    fn render(&self, repl: bool) -> String {
        match self {
            Value::Unit => String::new(),
            Value::Int(n) => n.to_string(),
            Value::Float(x) => float_text(*x),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) if repl => quoted_text(s),
            Value::Str(s) => s.to_string(),
            Value::Identity(i) if repl => format!(
                "Identity {{ id: {}, name: {} }}",
                quoted_text(&i.id),
                quoted_text(&i.name)
            ),
            Value::Identity(i) => i.name.clone(),
            Value::List(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.render(repl)).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(entries) => {
                if entries.is_empty() {
                    return "map {}".to_string();
                }
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.render(repl), v.render(repl)))
                    .collect();
                format!("map {{ {} }}", parts.join(", "))
            }
            Value::Option(None) => "None".to_string(),
            Value::Option(Some(v)) => format!("Some({})", v.render(repl)),
            Value::Struct(st) => {
                let StructValue { ty, fields } = &**st;
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{n}: {}", v.render(repl)))
                    .collect();
                if parts.is_empty() {
                    format!("{} {{ }}", short_type_name(ty))
                } else {
                    format!("{} {{ {} }}", short_type_name(ty), parts.join(", "))
                }
            }
            Value::Enum(e) => {
                let EnumValue { ty, variant, args } = &**e;
                if args.is_empty() {
                    format!("{}.{variant}", short_type_name(ty))
                } else {
                    let parts: Vec<String> = args.iter().map(|v| v.render(repl)).collect();
                    format!("{}.{variant}({})", short_type_name(ty), parts.join(", "))
                }
            }
            Value::Fn(_) => "<fn>".to_string(),
        }
    }
}

/// Turns a string into string literal form (enclosed in `"`, escaped with `\"`, `\\`, `\n`, `\t`, `\$`).
fn quoted_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '$' => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Value {
    /// A type-tagged string of a value that can be a key. Two keys are equal exactly when these strings are equal (used to
    /// check for duplicate keys in saved `map`s).
    pub(crate) fn key_repr(&self) -> String {
        let mut out = String::new();
        self.key_repr_into(&mut out);
        out
    }

    /// Appends the `key_repr` string to `out`.
    fn key_repr_into(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            Value::Int(n) => {
                let _ = write!(out, "i{n};");
            }
            Value::Bool(b) => out.push_str(if *b { "b1;" } else { "b0;" }),
            Value::Str(t) => {
                let _ = write!(out, "s{}:{t};", t.len());
            }
            Value::Identity(i) => {
                let _ = write!(out, "I{}:{}{}:{};", i.id.len(), i.id, i.name.len(), i.name);
            }
            Value::Struct(st) => {
                let StructValue { ty, fields } = &**st;
                let _ = write!(out, "S{}:{ty}(", ty.len());
                for (_, v) in fields.iter() {
                    v.key_repr_into(out);
                }
                out.push_str(");");
            }
            Value::Enum(e) => {
                let EnumValue { ty, variant, args } = &**e;
                let _ = write!(out, "E{}:{ty}{}:{variant}(", ty.len(), variant.len());
                for a in args.iter() {
                    a.key_repr_into(out);
                }
                out.push_str(");");
            }
            // Types that cannot be keys (rejected by type checking)
            other => {
                let t = other.to_text();
                let _ = write!(out, "?{}:{t};", t.len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_checked_against_expected_type_including_nesting() {
        let list = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())].into());
        assert!(list.matches(&Ty::List(Box::new(Ty::Str))));
        assert!(!list.matches(&Ty::List(Box::new(Ty::Int))));
        assert!(!list.matches(&Ty::Str));
        // An empty list matches any element type
        assert!(Value::List(vec![].into()).matches(&Ty::List(Box::new(Ty::Int))));
        let some = Value::Option(Some(Box::new(Value::Identity(Arc::new(
            Identity::default(),
        )))));
        assert!(some.matches(&Ty::Option(Box::new(Ty::Identity))));
        assert!(Value::Option(None).matches(&Ty::Option(Box::new(Ty::Int))));
        assert!(!some.matches(&Ty::Option(Box::new(Ty::Str))));
    }

    #[test]
    fn display_form_quotes_and_escapes_nested_strings() {
        let list = Value::List(vec![Value::Str("お面".into()), Value::Str("a\"b".into())].into());
        assert_eq!(list.to_text(), "[お面, a\"b]");
        assert_eq!(list.to_repl_text(), "[\"お面\", \"a\\\"b\"]");
        let s = Value::Str("x\\y\n\t$1".into());
        assert_eq!(s.to_repl_text(), "\"x\\\\y\\n\\t\\$1\"");
        let some = Value::Option(Some(Box::new(Value::Str("a".into()))));
        assert_eq!(some.to_repl_text(), "Some(\"a\")");
        assert_eq!(Value::Int(-3).to_repl_text(), "-3");
        assert_eq!(Value::Float(3.0).to_repl_text(), "3.0");
        let id = Value::Identity(Arc::new(Identity {
            id: "案内人".into(),
            name: "リオン".into(),
        }));
        assert_eq!(id.to_text(), "リオン");
        assert_eq!(
            id.to_repl_text(),
            "Identity { id: \"案内人\", name: \"リオン\" }"
        );
    }

    #[test]
    fn type_names_use_script_notation() {
        assert_eq!(Ty::Identity.name(), "Identity");
        assert_eq!(Ty::List(Box::new(Ty::Str)).name(), "list<str>");
        assert_eq!(Ty::Option(Box::new(Ty::Int)).name(), "Option<int>");
    }

    #[test]
    fn values_round_trip_through_postcard_including_identity() {
        let id = Identity {
            id: "案内人".into(),
            name: "案内人".into(),
        };
        let bytes = postcard::to_allocvec(&id).unwrap();
        let back: Identity = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, id);
    }
}
