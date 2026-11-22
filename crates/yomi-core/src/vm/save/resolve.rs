//! Resolving positions on load (stable ID, content hash, ordinal, start of the label), and restoring local variables and
//! the call stack.

use super::*;

/// A name in the form `module.label.variable`.
pub(super) fn local_path(program: &Program, code: &LabelCode, name: &str) -> String {
    format!(
        "{}.{}.{name}",
        program.modules[code.module as usize], code.name
    )
}

/// Saved local variables with the same name whose type changed.
pub(super) fn local_type_changes(
    program: &Program,
    code: &LabelCode,
    saved: &[(u32, String, SavedValue)],
    compat: &Compat<'_>,
) -> Vec<String> {
    saved
        .iter()
        .filter(|(_, name, v)| {
            // Hidden local variables of `for` (starting with `$`) are not script names, so they are not reported
            !name.starts_with('$')
                && code.locals.iter().any(|l| {
                    l.name == *name
                        && !l.ty.contains_fn()
                        && !compat.ok(&v.clone().into_value(), &l.ty)
                })
        })
        .map(|(_, name, _)| local_path(program, code, name))
        .collect()
}

/// A name in the form `module.variable`.
pub(super) fn global_name(program: &Program, module: u32, name: &str) -> String {
    format!("{}.{name}", program.modules[module as usize])
}

/// Turns local variables into the form to save (slot number, name, value). Variables of function types are not used at
/// stop points (the compiler rejects that), so their values are not stored.
pub(super) fn save_locals(code: &LabelCode, locals: &[Value]) -> Vec<(u32, String, SavedValue)> {
    code.locals
        .iter()
        .zip(locals)
        .enumerate()
        .map(|(i, (l, v))| {
            let v = if l.ty.contains_fn() { &Value::Unit } else { v };
            (i as u32, l.name.clone(), SavedValue::from_value(v))
        })
        .collect()
}

/// Restores saved local variables to fit the label's declarations. `None` if even one name, type or count does not match.
pub(super) fn restore_locals(
    program: &Program,
    code: &LabelCode,
    saved: &[(u32, String, SavedValue)],
    compat: &Compat<'_>,
) -> Option<Vec<Value>> {
    let mut locals = default_locals(program, code);
    let mut ok = saved.len() == code.locals.len();
    for (slot, name, v) in saved {
        let v = v.clone().into_value();
        match code.locals.get(*slot as usize) {
            Some(l) if l.name == *name && l.ty.contains_fn() => {}
            Some(l) if l.name == *name && compat.ok(&v, &l.ty) => locals[*slot as usize] = v,
            _ => ok = false,
        }
    }
    ok.then_some(locals)
}

/// Finds the number of a label in the program from the module name and the label name. `None` if not found.
pub(in crate::vm) fn find_label(program: &Program, module: &str, label: &str) -> Option<usize> {
    let m = program.modules.iter().position(|x| x == module)?;
    program
        .labels
        .iter()
        .position(|l| l.module as usize == m && l.name == label)
}

/// The rank of the strength of a clue for resolving a position (smaller is stronger: ID, hash, ordinal, start of the
/// label).
pub(super) fn rank(by: ResolvedBy) -> u8 {
    match by {
        ResolvedBy::Id => 0,
        ResolvedBy::Hash => 1,
        ResolvedBy::Ordinal => 2,
        ResolvedBy::LabelStart => 3,
    }
}

/// The less reliable of two ways of finding.
pub(super) fn weakest(a: ResolvedBy, b: ResolvedBy) -> ResolvedBy {
    if rank(b) > rank(a) { b } else { a }
}

/// Restores a saved call stack to fit the call lines of the scripts.
///
/// `None` if, for any frame, the label does not exist, the call line cannot be found, or the local variables do not
/// match, or if the way frames are stacked no longer fits the types (when the scripts changed and the value the caller
/// expects differs from the value the callee returns). On success, the frames and the least reliable way of finding.
/// `top` is the label to run now (the top of the stack).
pub(super) fn restore_frames(
    program: &Program,
    saved: &[FrameBody],
    top: usize,
    compat: &Compat<'_>,
) -> Option<(Vec<Frame>, ResolvedBy)> {
    let mut frames = Vec::with_capacity(saved.len());
    // The label called by the call line of each frame
    let mut targets = Vec::with_capacity(saved.len());
    let mut by = ResolvedBy::Id;
    for fb in saved {
        let p = &fb.position;
        let label = find_label(program, &p.module, &p.label)?;
        let code = &program.labels[label];
        let locals = restore_locals(program, code, &fb.locals, compat)?;
        let (site, how) = resolve_call(code, p)?;
        by = weakest(by, how);
        frames.push(Frame {
            label,
            ret_pc: code.calls[site].pc as usize + 1,
            locals,
        });
        targets.push(code.calls[site].target as usize);
    }
    for (i, &target) in targets.iter().enumerate() {
        let above = frames.get(i + 1).map_or(top, |f| f.label);
        let above_code = &program.labels[above];
        let consistent = if above_code.callable() {
            // A label that can only be entered with `call` is exactly the label that its call line called
            above == target
        } else {
            // After moving with `jump`. The caller does not receive a value
            program.labels[target].ret.is_none()
        };
        if !consistent {
            return None;
        }
    }
    Some((frames, by))
}

/// Finds a saved call line among the label's `call`s (hash → ordinal).
pub(super) fn resolve_call(code: &LabelCode, pos: &Position) -> Option<(usize, ResolvedBy)> {
    let nearest = |it: Vec<usize>| {
        it.into_iter()
            .min_by_key(|&i| (i as i64 - i64::from(pos.ordinal)).unsigned_abs())
    };
    let by_hash: Vec<usize> = code
        .calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.hash == pos.hash)
        .map(|(i, _)| i)
        .collect();
    if let Some(i) = nearest(by_hash) {
        return Some((i, ResolvedBy::Hash));
    }
    nearest((0..code.calls.len()).collect()).map(|i| (i, ResolvedBy::Ordinal))
}

/// Finds a saved position among the label's stop points (ID → hash → ordinal).
pub(super) fn resolve(
    code: &crate::program::LabelCode,
    pos: &Position,
) -> (Option<u32>, ResolvedBy) {
    if let Some(id) = &pos.id
        && let Some(i) = code.stops.iter().position(|s| s.id.as_ref() == Some(id))
    {
        return (Some(i as u32), ResolvedBy::Id);
    }
    let nearest = |it: Vec<usize>| {
        it.into_iter()
            .min_by_key(|&i| (i as i64 - i64::from(pos.ordinal)).unsigned_abs())
    };
    let by_hash: Vec<usize> = code
        .stops
        .iter()
        .enumerate()
        .filter(|(_, s)| s.hash == pos.hash)
        .map(|(i, _)| i)
        .collect();
    if let Some(i) = nearest(by_hash) {
        return (Some(i as u32), ResolvedBy::Hash);
    }
    let all: Vec<usize> = (0..code.stops.len()).collect();
    if let Some(i) = nearest(all) {
        return (Some(i as u32), ResolvedBy::Ordinal);
    }
    (None, ResolvedBy::LabelStart)
}
