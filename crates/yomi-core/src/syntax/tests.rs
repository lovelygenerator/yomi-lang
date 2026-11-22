use super::ast::{Expr, Stmt, StrPart};
use super::parser::parse;
use crate::diag::DiagCode;

fn codes(src: &str) -> Vec<DiagCode> {
    parse(src, "m").1.iter().map(|d| d.code).collect()
}

mod canon;
mod command_returns;
mod continuation;
mod format;
mod imports;
mod parse;
mod quotes;
