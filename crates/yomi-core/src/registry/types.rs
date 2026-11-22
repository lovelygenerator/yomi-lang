//! The correspondence between script types and Rust types (`YomiType`, `IntoValue`, `FromValue`), and argument lists
//! (`YomiArgs`, `FromArgs`).

use super::*;

/// The script type corresponding to a Rust type.
///
/// Types corresponding to `struct`s and `enum`s are made with `#[derive(YomiType)]` (or a hand-written implementation of
/// [`YomiDecl`]).
pub trait YomiType {
    /// The type in scripts.
    fn ty() -> Ty;
}

/// A Rust type that can be put into a registry as a `struct` or `enum` usable from scripts (implemented by
/// `#[derive(YomiType)]`).
///
/// Register it with [`Registry::ty`] or [`PackageReg::ty`]. The type name is `namespace.TypeName`, and scripts `import`
/// the namespace and write `namespace.TypeName` (section 2.5 of `spec/host/02-registry.md`).
pub trait YomiDecl: YomiType {
    /// The type declaration (the name is `namespace.TypeName`; the types of fields and variants refer by name to types
    /// registered first).
    fn decl() -> TypeDecl;
}

impl YomiType for i64 {
    fn ty() -> Ty {
        Ty::Int
    }
}
impl YomiType for f64 {
    fn ty() -> Ty {
        Ty::Float
    }
}
impl YomiType for bool {
    fn ty() -> Ty {
        Ty::Bool
    }
}
impl YomiType for String {
    fn ty() -> Ty {
        Ty::Str
    }
}
impl YomiType for () {
    fn ty() -> Ty {
        Ty::Unit
    }
}
impl YomiType for Identity {
    fn ty() -> Ty {
        Ty::Identity
    }
}
impl<T: YomiType> YomiType for Vec<T> {
    fn ty() -> Ty {
        Ty::List(Box::new(T::ty()))
    }
}
impl<T: YomiType> YomiType for Option<T> {
    fn ty() -> Ty {
        Ty::Option(Box::new(T::ty()))
    }
}

/// The Rust type corresponding to `map<K, V>` in scripts. Holds pairs of keys and values in **insertion order** (not a
/// `HashMap`, because the iteration order of `map` in scripts is defined as insertion order). When converted to a script
/// value with `IntoValue`, if a key repeats, the later value updates it (the position stays the first one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YomiMap<K, V>(pub Vec<(K, V)>);

impl<K: YomiType, V: YomiType> YomiType for YomiMap<K, V> {
    fn ty() -> Ty {
        Ty::Map(Box::new(K::ty()), Box::new(V::ty()))
    }
}

/// Conversion from Rust values to script values (command return values, initial values of host-declared variables).
pub trait IntoValue: YomiType {
    /// Makes a script value.
    fn into_value(self) -> Value;
}

/// Conversion from script values to Rust values (command arguments).
pub trait FromValue: YomiType + Sized {
    /// `None` if the type does not match.
    fn from_value(v: &Value) -> Option<Self>;
}

impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::Int(self)
    }
}
impl FromValue for i64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }
}
impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Float(self)
    }
}
impl FromValue for f64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Float(x) => Some(*x),
            _ => None,
        }
    }
}
impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}
impl FromValue for bool {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}
impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::Str(self.into())
    }
}
impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        }
    }
}
impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Unit
    }
}
impl FromValue for () {
    fn from_value(v: &Value) -> Option<Self> {
        matches!(v, Value::Unit).then_some(())
    }
}
impl IntoValue for Identity {
    fn into_value(self) -> Value {
        Value::Identity(self.into())
    }
}
impl FromValue for Identity {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Identity(i) => Some((**i).clone()),
            _ => None,
        }
    }
}
impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        Value::List(self.into_iter().map(IntoValue::into_value).collect())
    }
}
impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::List(items) => items.iter().map(T::from_value).collect(),
            _ => None,
        }
    }
}
impl<K: IntoValue, V: IntoValue> IntoValue for YomiMap<K, V> {
    fn into_value(self) -> Value {
        let pairs: Vec<(Value, Value)> = self
            .0
            .into_iter()
            .map(|(k, v)| (k.into_value(), v.into_value()))
            .collect();
        Value::Map(pairs.into())
    }
}
impl<K: FromValue, V: FromValue> FromValue for YomiMap<K, V> {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Map(entries) => entries
                .iter()
                .map(|(k, v)| Some((K::from_value(k)?, V::from_value(v)?)))
                .collect::<Option<Vec<_>>>()
                .map(YomiMap),
            _ => None,
        }
    }
}
impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        Value::Option(self.map(|v| Box::new(v.into_value())))
    }
}
impl<T: FromValue> FromValue for Option<T> {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Option(None) => Some(None),
            Value::Option(Some(inner)) => T::from_value(inner).map(Some),
            _ => None,
        }
    }
}

/// A list of parameter types (a tuple of 0 to 6).
pub trait YomiArgs {
    /// The list of types in scripts.
    fn types() -> Vec<Ty>;
}

/// Conversion from a list of evaluated arguments to a tuple.
pub trait FromArgs: YomiArgs + Sized {
    /// `None` if the count or types do not match.
    fn from_values(args: &[Value]) -> Option<Self>;
}

/// A macro that implements `YomiArgs` for argument tuples (0 to 6 elements).
macro_rules! impl_args {
    ($n:expr; $($A:ident $i:tt),*) => {
        impl<$($A: YomiType),*> YomiArgs for ($($A,)*) {
            fn types() -> Vec<Ty> {
                vec![$($A::ty()),*]
            }
        }
        impl<$($A: FromValue),*> FromArgs for ($($A,)*) {
            fn from_values(args: &[Value]) -> Option<Self> {
                if args.len() != $n {
                    return None;
                }
                Some(($($A::from_value(&args[$i])?,)*))
            }
        }
    };
}

impl_args!(0;);
impl_args!(1; A 0);
impl_args!(2; A 0, B 1);
impl_args!(3; A 0, B 1, C 2);
impl_args!(4; A 0, B 1, C 2, D 3);
impl_args!(5; A 0, B 1, C 2, D 3, E 4);
impl_args!(6; A 0, B 1, C 2, D 3, E 4, F 5);
