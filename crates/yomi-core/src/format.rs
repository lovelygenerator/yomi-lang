//! Formatting of scripts (the core of `yomi fmt`; chapter 8 of `spec/language/08-format.md`).

use crate::syntax::parser::format::migrations::format_migrations;
use crate::syntax::parser::{IdentMap, format::format_module, scan_identities};

pub use crate::syntax::parser::format::FormatError;

/// Formats the script `module` in the standard form. `modules` (name and source) are all the modules that are collected
/// together; the names of their identities decide how lines are classified. The line ending (`\n` or `\r\n`) and a leading
/// BOM of the source are kept. The result is checked by reading it again; if it does not mean the same as the source,
/// [`FormatError::Unverified`] is returned.
pub fn format_source(modules: &[(&str, &str)], module: &str) -> Result<String, FormatError> {
    let Some((_, source)) = modules.iter().find(|(name, _)| *name == module) else {
        return Err(FormatError::Unverified);
    };
    let bom = source.starts_with('\u{feff}');
    let body = source.strip_prefix('\u{feff}').unwrap_or(source);
    let crlf = body.contains("\r\n");
    let body = if crlf {
        body.replace("\r\n", "\n")
    } else {
        body.to_string()
    };
    let mut idents = IdentMap::new();
    for (name, src) in modules {
        idents
            .entry((*name).to_string())
            .or_insert_with(|| scan_identities(src));
    }
    let mut text = if module == "migrations" {
        format_migrations(&body, module)?
    } else {
        format_module(&body, module, &idents)?
    };
    if crlf {
        text = text.replace('\n', "\r\n");
    }
    if bom {
        text.insert(0, '\u{feff}');
    }
    Ok(text)
}
