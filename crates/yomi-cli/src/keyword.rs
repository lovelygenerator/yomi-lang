//! Names of reserved word categories (MCP and LSP use the same notation).

use yomi_core::KeywordKind;

pub(crate) fn kind_name(k: KeywordKind) -> &'static str {
    match k {
        KeywordKind::Declaration => "declaration",
        KeywordKind::Statement => "statement",
        KeywordKind::Modifier => "modifier",
        KeywordKind::Type => "type",
        KeywordKind::Value => "value",
    }
}
