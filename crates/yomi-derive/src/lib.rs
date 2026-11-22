//! `#[derive(YomiType)]`.
//!
//! Written with only `proc_macro`, without `syn` or `quote`. Parsing the input (`parse`) and generating code (`generate`)
//! are pure functions independent of `proc_macro` types, and can be unit-tested. Forms that cannot be parsed become a
//! `compile_error!` in Japanese.
//!
//! ```ignore
//! #[derive(YomiType)]
//! #[yomi(namespace = "game")]
//! struct Chara { name: String, hp: i64 }
//!
//! #[derive(YomiType)]
//! #[yomi(namespace = "game", name = "気分")]
//! enum Mood { Happy, Angry(i64) }
//! ```
//!
//! It generates the implementations of `YomiType`, `YomiDecl`, `IntoValue` and `FromValue` (with absolute paths
//! `::yomi_core::…`).

#![forbid(unsafe_code)]

mod gen_code;
mod parse;

#[cfg(test)]
mod tests;

use gen_code::generate;
use parse::{Tok, parse};
use proc_macro::{Delimiter, Spacing, TokenStream, TokenTree};

/// Makes it a `struct` / `enum` usable from scripts. Requires `#[yomi(namespace = "namespace")]`
/// (`name = "TypeName"` changes the type name in scripts).
#[proc_macro_derive(YomiType, attributes(yomi))]
pub fn derive_yomi_type(input: TokenStream) -> TokenStream {
    let toks: Vec<Tok> = input.into_iter().map(convert).collect();
    let code = match parse(&toks).map(|item| generate(&item)) {
        Ok(code) => code,
        Err(message) => format!("::core::compile_error!({message:?});"),
    };
    code.parse().unwrap_or_else(|_| {
        "::core::compile_error!(\"yomi-derive: 生成したコードを読めません\");"
            .parse()
            .unwrap()
    })
}

/// Converts `proc_macro` tokens into the parser's `Tok` (groups become `Tok::Group` with their contents converted
/// recursively).
fn convert(t: TokenTree) -> Tok {
    match t {
        TokenTree::Ident(i) => Tok::Ident(i.to_string()),
        TokenTree::Punct(p) => Tok::Punct(p.as_char(), p.spacing() == Spacing::Joint),
        TokenTree::Literal(l) => Tok::Literal(l.to_string()),
        TokenTree::Group(g) => {
            let delim = match g.delimiter() {
                Delimiter::Parenthesis => '(',
                Delimiter::Brace => '{',
                Delimiter::Bracket => '[',
                Delimiter::None => ' ',
            };
            Tok::Group(delim, g.stream().into_iter().map(convert).collect())
        }
    }
}
