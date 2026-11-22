//! Syntax: the syntax tree, expression parsing and line parsing.

pub(crate) mod ast;
pub(crate) mod canon;
pub(crate) mod expr;
pub(crate) mod keywords;
pub(crate) mod parser;

#[cfg(test)]
mod tests;
