//! The `str` value (`StrValue`). Holds a shared string with one thin pointer.

use super::*;

/// A `str` value. The contents are shared (cloning only copies the `Arc`; strings are never modified).
///
/// Held as one thin pointer (8 bytes) to keep `Value` small. As a string, it derefs to `&str` via [`std::ops::Deref`].
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrValue(Arc<String>);

impl StrValue {
    /// A reference to the string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for StrValue {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for StrValue {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for StrValue {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for StrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl std::fmt::Display for StrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl From<String> for StrValue {
    fn from(s: String) -> Self {
        Self(Arc::new(s))
    }
}

impl From<&str> for StrValue {
    fn from(s: &str) -> Self {
        Self(Arc::new(s.to_owned()))
    }
}

impl From<&String> for StrValue {
    fn from(s: &String) -> Self {
        Self(Arc::new(s.clone()))
    }
}

impl From<Arc<str>> for StrValue {
    fn from(s: Arc<str>) -> Self {
        Self(Arc::new(s.to_string()))
    }
}

impl From<Box<str>> for StrValue {
    fn from(s: Box<str>) -> Self {
        Self(Arc::new(s.into_string()))
    }
}

impl From<std::borrow::Cow<'_, str>> for StrValue {
    fn from(s: std::borrow::Cow<'_, str>) -> Self {
        Self(Arc::new(s.into_owned()))
    }
}

impl PartialEq<str> for StrValue {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for StrValue {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl Serialize for StrValue {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self)
    }
}

impl<'de> Deserialize<'de> for StrValue {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(String::deserialize(de)?.into())
    }
}
