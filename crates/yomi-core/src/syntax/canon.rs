//! The standard form of expressions (section 3.9 of `spec/language/03-statements.md`): one string decided by the syntax
//! tree alone. It is the input of the position hash, so a change of its rules also changes the format version.

use super::ast::{BinOp, Expr, StrPart, TyName, UnOp};

/// Binding strength. A larger value binds tighter.
const LAMBDA: u8 = 0;
const UNARY: u8 = 7;
const POSTFIX: u8 = 8;
const ATOM: u8 = 9;

fn binary_level(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
    }
}

/// `==` `!=` and the order comparisons do not chain.
fn non_associative(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    )
}

fn binary_text(op: BinOp) -> &'static str {
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

fn level(e: &Expr) -> u8 {
    match e {
        Expr::Binary(op, ..) => binary_level(*op),
        Expr::Unary(..) => UNARY,
        Expr::Lambda(_) => LAMBDA,
        Expr::Index(..)
        | Expr::Field(..)
        | Expr::Method(..)
        | Expr::Call(..)
        | Expr::QueryCall(..) => POSTFIX,
        _ => ATOM,
    }
}

/// The standard form of an expression.
pub(crate) fn expr(e: &Expr) -> String {
    let mut out = String::new();
    write_expr(e, &mut out);
    out
}

/// The standard forms of arguments, joined with `, `.
pub(crate) fn args(args: &[Expr]) -> String {
    let mut out = String::new();
    write_list(args, &mut out);
    out
}

/// The standard form of the body of a text line or option: the characters as written, with the expressions inside `${ }`
/// in the standard form.
pub(crate) fn body(parts: &[StrPart]) -> String {
    let mut out = String::new();
    write_parts(parts, &mut out);
    out
}

fn write_list(items: &[Expr], out: &mut String) {
    for (i, a) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_expr(a, out);
    }
}

fn write_parts(parts: &[StrPart], out: &mut String) {
    for p in parts {
        match p {
            StrPart::Text(_, written) => out.push_str(written),
            StrPart::Interp(e) => {
                out.push_str("${");
                write_expr(e, out);
                out.push('}');
            }
        }
    }
}

/// Writes `e`, enclosed in `( )` when `needs` says so.
fn write_child(e: &Expr, needs: bool, out: &mut String) {
    if needs {
        out.push('(');
        write_expr(e, out);
        out.push(')');
    } else {
        write_expr(e, out);
    }
}

/// The standard form of a type name.
pub(crate) fn ty(t: &TyName) -> String {
    let mut out = String::new();
    write_ty(t, &mut out);
    out
}

fn write_ty(t: &TyName, out: &mut String) {
    if t.name == "fn" {
        out.push_str("fn(");
        let (ret, params) = t.args.split_last().expect("関数の型は戻り値の型を持つ");
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_ty(p, out);
        }
        out.push_str(") -> ");
        write_ty(ret, out);
        return;
    }
    out.push_str(&t.name);
    if !t.args.is_empty() {
        out.push('<');
        for (i, a) in t.args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_ty(a, out);
        }
        out.push('>');
    }
}

fn write_expr(e: &Expr, out: &mut String) {
    match e {
        Expr::Int(n, _) => out.push_str(&n.to_string()),
        Expr::Float(x, _) => out.push_str(&crate::value::float_text(*x)),
        Expr::Bool(b, _) => out.push_str(if *b { "true" } else { "false" }),
        Expr::Str(parts, _) => {
            out.push('"');
            write_parts(parts, out);
            out.push('"');
        }
        Expr::Name(n, _) => out.push_str(n),
        Expr::Qualified(m, n, _) => {
            out.push_str(m);
            out.push('.');
            out.push_str(n);
        }
        Expr::List(items, _) => {
            out.push('[');
            write_list(items, out);
            out.push(']');
        }
        Expr::Map(entries, _) => {
            if entries.is_empty() {
                out.push_str("map {}");
                return;
            }
            out.push_str("map { ");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(k, out);
                out.push_str(": ");
                write_expr(v, out);
            }
            out.push_str(" }");
        }
        Expr::Some(x, _) => {
            out.push_str("Some(");
            write_expr(x, out);
            out.push(')');
        }
        Expr::None(_) => out.push_str("None"),
        Expr::Index(target, index, _) => {
            write_child(target, level(target) < POSTFIX, out);
            out.push('[');
            write_expr(index, out);
            out.push(']');
        }
        Expr::Field(target, name, _) => {
            write_child(target, level(target) < POSTFIX, out);
            out.push('.');
            out.push_str(name);
        }
        Expr::Method(target, name, a, _) => {
            write_child(target, level(target) < POSTFIX, out);
            out.push('.');
            out.push_str(name);
            out.push('(');
            write_list(a, out);
            out.push(')');
        }
        Expr::StructLit(ty, fields, _) => {
            out.push_str(&ty.written());
            if fields.is_empty() {
                out.push_str(" {}");
                return;
            }
            out.push_str(" { ");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&f.name);
                out.push_str(": ");
                write_expr(&f.value, out);
            }
            out.push_str(" }");
        }
        Expr::Unary(op, x, _) => {
            out.push(match op {
                UnOp::Neg => '-',
                UnOp::Not => '!',
            });
            write_child(x, level(x) < UNARY, out);
        }
        Expr::Binary(op, l, r, _) => {
            let me = binary_level(*op);
            let left_needs = level(l) < me || (non_associative(*op) && level(l) == me);
            write_child(l, left_needs, out);
            out.push(' ');
            out.push_str(binary_text(*op));
            out.push(' ');
            write_child(r, level(r) <= me, out);
        }
        Expr::Call(name, a, _) => {
            out.push_str(name);
            out.push('(');
            write_list(a, out);
            out.push(')');
        }
        Expr::QueryCall(package, name, a, _) => {
            out.push_str(package);
            out.push('.');
            out.push_str(name);
            out.push('(');
            write_list(a, out);
            out.push(')');
        }
        Expr::LabelCall(c) => {
            out.push_str("call ");
            out.push_str(&c.target.written());
            out.push('(');
            write_list(&c.args, out);
            out.push(')');
        }
        Expr::Command(c) | Expr::MisplacedCommand(c) => {
            out.push('@');
            out.push_str(&c.name);
            out.push('(');
            write_list(&c.args, out);
            out.push(')');
        }
        Expr::Named(name, x, _) => {
            out.push_str(name);
            out.push_str(": ");
            write_expr(x, out);
        }
        Expr::Default(..) => {}
        Expr::Lambda(l) => {
            out.push('|');
            for (i, p) in l.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&p.name);
                if let Some(t) = &p.ty {
                    out.push_str(": ");
                    write_ty(t, out);
                }
            }
            out.push_str("| ");
            write_expr(&l.body, out);
        }
    }
}
