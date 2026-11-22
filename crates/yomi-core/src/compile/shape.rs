//! Deciding the shape of statements (whether they can complete, whether they contain `break`), and names of operators.

use super::*;

/// Whether a sequence of statements may reach the end (false if it always ends with `return`, `jump` or `end`).
///
/// A `while true` loop without `break` is treated as never exiting. Other loops may exit when the condition becomes
/// false.
pub(super) fn completes(body: &[Stmt]) -> bool {
    body.iter().all(stmt_completes)
}

/// Whether a statement can complete and proceed to the next statement. `return`, `jump`, `end`, `break` and `continue`, an
/// `if` whose arms all do so, and an infinite `while` without a way out, do not complete.
pub(super) fn stmt_completes(st: &Stmt) -> bool {
    match st {
        Stmt::Return { .. }
        | Stmt::Jump { .. }
        | Stmt::End(_)
        | Stmt::Break(_)
        | Stmt::Continue(_) => false,
        Stmt::If {
            arms, else_body, ..
        } => {
            // Without `else`, it can proceed without entering any arm
            else_body.is_empty()
                || arms.iter().any(|(_, body)| completes(body))
                || completes(else_body)
        }
        Stmt::While { cond, body, .. } => {
            !(matches!(cond, Expr::Bool(true, _)) && !contains_break(body))
        }
        // If every body ends with `jump` / `return` / `end`, it does not proceed past the `choice`
        Stmt::Choice { options, .. } => options.iter().any(|o| completes(&o.body)),
        // If exhaustive, it goes through one of the arms
        Stmt::Match { arms, .. } => arms.is_empty() || arms.iter().any(|a| completes(&a.body)),
        _ => true,
    }
}

/// Whether there is a `break` that exits that loop (`break`s of nested loops are not counted).
pub(super) fn contains_break(body: &[Stmt]) -> bool {
    body.iter().any(|st| match st {
        Stmt::Break(_) => true,
        Stmt::If {
            arms, else_body, ..
        } => arms.iter().any(|(_, b)| contains_break(b)) || contains_break(else_body),
        Stmt::Choice { options, .. } => options.iter().any(|o| contains_break(&o.body)),
        Stmt::Match { arms, .. } => arms.iter().any(|a| contains_break(&a.body)),
        _ => false,
    })
}

/// The notation of assignment operators (used for diagnostic arguments).
pub(super) fn assign_op_name(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Set => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::Mul => "*=",
        AssignOp::Div => "/=",
        AssignOp::Rem => "%=",
    }
}

/// The notation of binary operators (used for diagnostic arguments).
pub(super) fn bin_op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}
