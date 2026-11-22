//! String operations of `text` (with limit checks).

use super::*;

/// The maximum size of a resulting string (bytes). The same value as the default of the core's
/// `VmConfig::max_value_bytes`. Settings cannot be read from `Ctx`, so `text` keeps its own. Huge strings are refused with
/// this value before they are built.
pub(crate) const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;

/// A runtime error if the size of the result (bytes) exceeds the limit. `None` (overflow in the calculation) is also
/// treated as exceeding.
pub(super) fn check_size(command: &str, bytes: Option<usize>) -> Result<(), CommandError> {
    match bytes {
        Some(n) if n <= MAX_RESULT_BYTES => Ok(()),
        _ => Err(CommandError::with_code(
            "text.result_too_large",
            [
                ("command", command.into()),
                ("limit", MAX_RESULT_BYTES.into()),
            ],
        )),
    }
}

/// Converts a non-negative `i64` argument to `usize`. A runtime error if negative.
pub(super) fn non_negative(
    command: &str,
    arg: &'static str,
    n: i64,
) -> Result<usize, CommandError> {
    if n < 0 {
        return Err(CommandError::with_code(
            "text.negative",
            [("command", command.into()), ("arg", arg.into())],
        ));
    }
    // Huge values may not fit in usize (then they are treated as exceeding the limit)
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

/// `text.ends_with`: whether `s` ends with `suffix`.
pub(super) fn ends_with(s: &str, suffix: &str) -> bool {
    s.ends_with(suffix)
}

/// `text.replace`: replaces all occurrences of `from` in `s` with `to`. A runtime error if `from` is empty or the result
/// exceeds the limit.
pub(super) fn replace(s: &str, from: &str, to: &str) -> Result<String, CommandError> {
    if from.is_empty() {
        return Err(CommandError::with_code(
            "text.empty_arg",
            [("command", "text.replace".into()), ("arg", "from".into())],
        ));
    }
    let count = s.matches(from).count();
    let size = s
        .len()
        .checked_sub(count * from.len())
        .and_then(|rest| rest.checked_add(count.checked_mul(to.len())?));
    check_size("text.replace", size)?;
    Ok(s.replace(from, to))
}

/// `text.split`: the list of `s` split by `sep`. A runtime error if `sep` is empty.
pub(super) fn split(s: &str, sep: &str) -> Result<Vec<String>, CommandError> {
    if sep.is_empty() {
        return Err(CommandError::with_code(
            "text.empty_arg",
            [("command", "text.split".into()), ("arg", "sep".into())],
        ));
    }
    Ok(s.split(sep).map(str::to_string).collect())
}

/// `text.join`: joins `parts` with `sep`. A runtime error if the result exceeds the limit.
pub(super) fn join(parts: &[String], sep: &str) -> Result<String, CommandError> {
    let body: usize = parts.iter().map(String::len).sum();
    let seps = sep.len().checked_mul(parts.len().saturating_sub(1));
    check_size("text.join", seps.and_then(|x| x.checked_add(body)))?;
    Ok(parts.join(sep))
}

/// `text.slice`: the part from grapheme cluster position `start` to `end` (`end` not included). A runtime error if out of
/// range or `start > end`.
pub(super) fn slice(s: &str, start: i64, end: i64) -> Result<String, CommandError> {
    let offsets = grapheme_offsets(s);
    let len = offsets.len() - 1;
    let ok = |n: i64| usize::try_from(n).ok().filter(|&n| n <= len);
    match (ok(start), ok(end)) {
        (Some(a), Some(b)) if a <= b => Ok(s[offsets[a]..offsets[b]].to_string()),
        _ => Err(CommandError::with_code(
            "text.slice_range",
            [
                ("start", start.into()),
                ("end", end.into()),
                ("len", len.into()),
            ],
        )),
    }
}

/// `text.find`: the position where `part` first appears (counted in grapheme clusters). `None` if not found.
pub(super) fn find(s: &str, part: &str) -> Option<i64> {
    let at = s.find(part)?;
    // The position of the cluster containing the start of the match (if the start is in the middle of a cluster, that
    // cluster)
    let offsets = grapheme_offsets(s);
    Some((offsets.partition_point(|&o| o <= at) - 1) as i64)
}

/// `text.repeat`: repeats `s` `n` times. A runtime error if `n` is negative or the result exceeds the limit.
pub(super) fn repeat(s: &str, n: i64) -> Result<String, CommandError> {
    let n = non_negative("text.repeat", "n", n)?;
    if s.is_empty() || n == 0 {
        return Ok(String::new());
    }
    check_size("text.repeat", s.len().checked_mul(n))?;
    Ok(s.repeat(n))
}

/// `text.pad_left` / `pad_right`: adds `fill` (one cluster) on the left if `left`, otherwise on the right, until the width
/// is `width` (in grapheme clusters).
/// If the width is already enough, returns it as is. A runtime error if `width` is negative, `fill` is not one cluster, or
/// the result exceeds the limit.
pub(super) fn pad(
    command: &str,
    s: &str,
    width: i64,
    fill: &str,
    left: bool,
) -> Result<String, CommandError> {
    let width = non_negative(command, "width", width)?;
    let mut fill_clusters = Clusters { rest: fill };
    match (fill_clusters.next(), fill_clusters.next()) {
        (Some(_), None) => {}
        _ => {
            return Err(CommandError::with_code(
                "text.fill_not_grapheme",
                [("command", command.into())],
            ));
        }
    }
    let len = grapheme_count(s);
    if width <= len {
        return Ok(s.to_string());
    }
    let need = width - len;
    check_size(
        command,
        need.checked_mul(fill.len())
            .and_then(|x| x.checked_add(s.len())),
    )?;
    let padding = fill.repeat(need);
    Ok(if left {
        padding + s
    } else {
        let mut out = s.to_string();
        out.push_str(&padding);
        out
    })
}
