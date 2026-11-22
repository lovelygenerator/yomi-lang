//! The table of reserved words (section 1.6 of `spec/language/01-lexical.md`). The parser, the registry, the REPL and
//! tools look only at this table.

/// The category of a reserved word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeywordKind {
    /// Words that start declarations (`label`, `fn`, `save`, and so on). Cannot be written inside a label.
    Declaration,
    /// Words that start statements (`jump`, `if`, `return`, and so on).
    Statement,
    /// Words that appear in the middle of syntax (`var`, `in`). Never at the start of a statement.
    Modifier,
    /// Type names.
    Type,
    /// Words that make values (`true`, `false`, `Some`, `None`).
    Value,
}

/// One reserved word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keyword {
    /// The word.
    pub word: &'static str,
    /// The category.
    pub kind: KeywordKind,
    /// Whether it is the first word of syntax that opens a block with `{` at the end of the line.
    pub opens_block: bool,
}

/// Positions where a contextual word has meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualPosition {
    /// The start of an option in `choice { }`.
    ChoiceOption,
    /// Before the alias of an `import`.
    ImportAlias,
    /// The start of the path of an `import` (the namespace of standard packages).
    ImportNamespace,
    /// After the `.` in the statement `expr.each { name =>`.
    EachStatement,
}

/// A contextual word. Not a reserved word; it has meaning only in certain positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextualKeyword {
    /// The word.
    pub word: &'static str,
    /// The position where it has meaning.
    pub position: ContextualPosition,
}

/// The list of contextual words. In the same order as section 1.6 of the spec.
pub const CONTEXTUAL_KEYWORDS: &[ContextualKeyword] = &[
    ContextualKeyword {
        word: "once",
        position: ContextualPosition::ChoiceOption,
    },
    ContextualKeyword {
        word: "as",
        position: ContextualPosition::ImportAlias,
    },
    ContextualKeyword {
        word: "std",
        position: ContextualPosition::ImportNamespace,
    },
    ContextualKeyword {
        word: "each",
        position: ContextualPosition::EachStatement,
    },
];

const fn kw(word: &'static str, kind: KeywordKind, opens_block: bool) -> Keyword {
    Keyword {
        word,
        kind,
        opens_block,
    }
}

use KeywordKind::{Declaration, Modifier, Statement, Type, Value};

/// The list of reserved words. In the same order as section 1.6 of the spec.
pub const KEYWORDS: &[Keyword] = &[
    kw("identity", Declaration, false),
    kw("label", Declaration, true),
    kw("jump", Statement, false),
    kw("call", Statement, false),
    kw("choice", Statement, true),
    kw("import", Declaration, false),
    kw("end", Statement, false),
    kw("save", Declaration, false),
    kw("persistent", Declaration, false),
    kw("var", Modifier, false),
    kw("let", Statement, false),
    kw("fn", Declaration, true),
    kw("if", Statement, true),
    kw("else", Statement, true),
    kw("while", Statement, true),
    kw("for", Statement, true),
    kw("in", Modifier, false),
    kw("match", Statement, true),
    kw("break", Statement, false),
    kw("continue", Statement, false),
    kw("true", Value, false),
    kw("false", Value, false),
    kw("enum", Declaration, true),
    kw("struct", Declaration, true),
    kw("return", Statement, false),
    kw("Some", Value, false),
    kw("None", Value, false),
    kw("int", Type, false),
    kw("float", Type, false),
    kw("bool", Type, false),
    kw("str", Type, false),
    kw("list", Type, false),
    kw("map", Type, false),
    kw("Option", Type, false),
    kw("Identity", Type, false),
];

/// The entry for the word (`None` if not a reserved word).
pub fn keyword(word: &str) -> Option<&'static Keyword> {
    KEYWORDS.iter().find(|k| k.word == word)
}

/// Whether it is a reserved word.
pub fn is_reserved(word: &str) -> bool {
    keyword(word).is_some()
}

/// Whether it is the first word of syntax that opens a block with `{` at the end of the line.
pub(crate) fn starts_block(word: &str) -> bool {
    keyword(word).is_some_and(|k| k.opens_block)
}

/// Whether it is the first word of a declaration that cannot be written inside a label.
pub(crate) fn is_declaration(word: &str) -> bool {
    keyword(word).is_some_and(|k| k.kind == Declaration)
}

/// Whether it is the first word of a statement that opens a block (`if`, `else`, `while`, `for`, `match`, `choice`).
pub(crate) fn is_block_statement(word: &str) -> bool {
    keyword(word).is_some_and(|k| k.kind == Statement && k.opens_block)
}
