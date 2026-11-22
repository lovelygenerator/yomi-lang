//! Expansion of declared type names in type strings.

use std::collections::HashMap;

use crate::value::MAX_TYPE_DEPTH;

/// Expands the names of `struct`s and `enum`s in a type string into their declarations, so that two types can be compared
/// by what they contain.
pub(crate) struct Expander<'a> {
    decls: HashMap<&'a str, &'a str>,
}

impl<'a> Expander<'a> {
    /// `types` are the declarations (`name`, shape), as in `TypeDecl::signature`.
    pub(crate) fn new(types: &'a [(String, String)]) -> Self {
        Self {
            decls: types
                .iter()
                .map(|(n, s)| (n.as_str(), s.as_str()))
                .collect(),
        }
    }

    /// The type string with every declared type written as `name shape`.
    pub(crate) fn expand(&self, ty: &str) -> String {
        let mut out = String::with_capacity(ty.len());
        self.expand_into(ty, 0, &mut out);
        out
    }

    fn expand_into(&self, text: &str, depth: u32, out: &mut String) {
        let mut chars = text.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            if !(c.is_alphabetic() || c == '_') {
                out.push(c);
                continue;
            }
            let mut end = start + c.len_utf8();
            while let Some(&(i, n)) = chars.peek() {
                if n.is_alphanumeric() || n == '_' || n == '.' {
                    end = i + n.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let token = &text[start..end];
            match self.decls.get(token) {
                Some(shape) if depth < MAX_TYPE_DEPTH => {
                    out.push_str(token);
                    out.push(' ');
                    self.expand_into(shape, depth + 1, out);
                }
                _ => out.push_str(token),
            }
        }
    }
}
