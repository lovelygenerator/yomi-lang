//! The formatter for the `migrations` module (8.7).

use super::super::schema::{
    MigrateStmt, MigrationsAst, VarRef, bracket_balance, parse_migrations, split_lines,
};
use super::*;

fn var_text(v: &VarRef) -> String {
    let mut s = String::new();
    if let Some(n) = v.version {
        s.push_str(&format!("v{n}::"));
    }
    if !v.module.is_empty() {
        s.push_str(&v.module);
        s.push('.');
    }
    s.push_str(&v.name);
    s
}

fn stmt_text(st: &MigrateStmt) -> String {
    match st {
        MigrateStmt::Rename { old, new } => {
            format!("rename {} -> {}", var_text(old), var_text(new))
        }
        MigrateStmt::Drop { old } => format!("drop {}", var_text(old)),
        MigrateStmt::Convert {
            old,
            old_ty,
            new,
            new_ty,
            ..
        } => {
            let new_side = match new {
                Some(v) => format!("{}: {}", var_text(v), canon::ty(new_ty)),
                None => canon::ty(new_ty),
            };
            format!(
                "convert {}: {} -> {new_side} =",
                var_text(old),
                canon::ty(old_ty)
            )
        }
    }
}

fn comment_lines(raw: &str) -> Option<String> {
    comment_of(raw)
}

/// Formats a `migrations` module. The result is checked by reading it again.
pub(crate) fn format_migrations(source: &str, module: &str) -> Result<String, FormatError> {
    let (before, diags) = parse_migrations(source, module);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(FormatError::Syntax(diags));
    }
    let mut stmts: std::collections::HashMap<u32, &MigrateStmt> = std::collections::HashMap::new();
    for b in &before.blocks {
        for (pos, s) in &b.stmts {
            stmts.insert(pos.line, s);
        }
    }
    let lines = split_lines(source);
    // (depth, text); `None` is a blank line
    let mut out: Vec<Option<(usize, String)>> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let no = i as u32 + 1;
        i += 1;
        let off = indent_len(raw);
        let t = &raw[off..];
        if t.is_empty() {
            out.push(None);
            continue;
        }
        if t.starts_with("//") {
            out.push(Some((depth, t.trim_end().to_string())));
            continue;
        }
        let word = if t.starts_with('}') {
            "}"
        } else {
            first_word(t)
        };
        let comment = comment_lines(raw);
        match word {
            "min_save_version" => {
                let n = t[word.len()..].trim_start();
                let digits: String = n.chars().take_while(char::is_ascii_digit).collect();
                out.push(Some((
                    0,
                    with_comment(format!("min_save_version {digits}"), comment),
                )));
            }
            "migrate" => {
                let n = t[word.len()..].trim_start();
                let digits: String = n.chars().take_while(char::is_ascii_digit).collect();
                out.push(Some((
                    0,
                    with_comment(format!("migrate {digits} {{"), comment),
                )));
                depth = 1;
            }
            "}" => {
                depth = 0;
                out.push(Some((0, with_comment("}".to_string(), comment))));
            }
            _ => {
                let st = stmts.get(&no).ok_or(FormatError::Unverified)?;
                let mut head = stmt_text(st);
                if let MigrateStmt::Convert { lambda, .. } = st {
                    // The lines the lambda continues over
                    let mut code = strip_comment(raw).trim_end().to_string();
                    let mut depth_b = bracket_balance(&code);
                    let mut extra = Vec::new();
                    let ended_with_eq = code.ends_with('=');
                    let mut first_ended = ended_with_eq;
                    let mut continued = false;
                    while (code.ends_with('=') || depth_b > 0) && i < lines.len() {
                        let next = lines[i];
                        i += 1;
                        continued = true;
                        let c = strip_comment(next);
                        let stripped = c.trim_start_matches([' ', '\t']);
                        code.push(' ');
                        code.push_str(stripped.trim_end_matches([' ', '\t']));
                        depth_b += bracket_balance(stripped);
                        if let Some(cm) = comment_of(next) {
                            extra.push(cm);
                        }
                        let _ = &mut first_ended;
                    }
                    for c in extra {
                        out.push(Some((depth, c)));
                    }
                    let body = canon::expr(lambda);
                    if continued && ended_with_eq {
                        out.push(Some((depth, with_comment(head, comment))));
                        out.push(Some((depth + 1, body)));
                    } else {
                        out.push(Some((
                            depth,
                            with_comment(format!("{head} {body}"), comment),
                        )));
                    }
                } else {
                    head = with_comment(head, comment);
                    out.push(Some((depth, head)));
                }
            }
        }
    }
    let mut text = String::new();
    let mut blank = false;
    let mut prev_opens = true;
    for o in out {
        match o {
            None => blank = true,
            Some((d, t)) => {
                if blank && !prev_opens && !t.starts_with('}') {
                    text.push('\n');
                }
                blank = false;
                prev_opens = code_ends_with_open(&t);
                text.push_str(&"    ".repeat(d));
                text.push_str(&t);
                text.push('\n');
            }
        }
    }
    let (after, diags) = parse_migrations(&text, module);
    let same = |a: &MigrationsAst, b: &MigrationsAst| {
        scrub_positions(&format!("{a:?}")) == scrub_positions(&format!("{b:?}"))
    };
    if diags.iter().any(|d| d.severity == Severity::Error) || !same(&before, &after) {
        return Err(FormatError::Unverified);
    }
    Ok(text)
}
