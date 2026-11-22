//! VM: rewriting places of variables, and accounting for the sizes of values.

use super::*;

impl<'a> Vm<'a> {
    /// The total size of a sequence of local variables.
    pub(super) fn locals_bytes(locals: &[Value]) -> usize {
        locals.iter().map(Value::approx_bytes).sum()
    }

    /// Counts from scratch the total size of values held in variables (including `fn` frames). Of each frame's array, only the
    /// local variables (the declared count) are counted (temporaries of the register backend are not).
    pub(super) fn recount_total(&self) -> usize {
        let p = &self.program;
        let label_n = |l: usize| p.labels.get(l).map(|c| c.locals.len());
        let code_n = |f: Option<usize>| match f {
            Some(f) => p.fns.get(f).map(|c| c.locals.len()),
            None => label_n(self.label),
        };
        Self::locals_bytes(&self.globals)
            + Self::locals_bytes(&self.host_vars)
            + Self::locals_bytes(vars(&self.locals, code_n(self.cur_fn)))
            + self
                .frames
                .iter()
                .map(|f| Self::locals_bytes(vars(&f.locals, label_n(f.label))))
                .sum::<usize>()
            + self
                .fn_frames
                .iter()
                .map(|f| Self::locals_bytes(vars(&f.locals, code_n(f.caller))))
                .sum::<usize>()
    }

    /// Records that the total size of variables decreased by `shrink` and increased by `grow`. `MemoryLimitExceeded` if the
    /// limit is exceeded.
    pub(super) fn account(&mut self, shrink: usize, grow: usize) -> Result<(), VmErrorKind> {
        self.total = (self.total + grow).saturating_sub(shrink);
        match self.cfg.max_total_bytes {
            Some(max) if self.total > max => Err(VmErrorKind::MemoryLimitExceeded),
            _ => Ok(()),
        }
    }

    /// Returns the value of a place (a variable) in a mutable form.
    pub(super) fn place_mut(&mut self, place: PlaceRef) -> &mut Value {
        match place {
            PlaceRef::Local(i) => &mut self.locals[i as usize],
            PlaceRef::Global(i) => &mut self.globals[i as usize],
            PlaceRef::Host(i) => {
                let id = self.host_var_ids[i as usize];
                &mut self.host_vars[id]
            }
        }
    }

    /// Updates a place (a variable, and the way to follow fields, indices and keys) in place (the `Mutate` instruction).
    /// `idxs` are the indices and keys of the path (in step order), `insert_at` is the index of `insert`, and `operand` is the
    /// value (none for `Clear` and `Shuffle`). Returns the change in the total size of variables `(decrease, increase)` (the
    /// caller records it).
    ///
    /// `list`s and `map`s that are not shared are updated in place (`push` and the like are amortized O(1)). If shared (when a
    /// `for`'s iteration target, a snapshot or the like has the same contents), only those `list`s and `map`s are copied while
    /// following the path (copy on write; other owners are not affected). After the update, the total sizes held by nested
    /// `list`s and `map`s are corrected by adding or subtracting only the change, along the `list`s and `map`s of the path
    /// followed.
    pub(super) fn mutate_with(
        &mut self,
        place: PlaceRef,
        (path, action): (&[PathStep], Action),
        idxs: &[Value],
        insert_at: Option<i64>,
        operand: Option<Value>,
    ) -> Result<(usize, usize), VmErrorKind> {
        let max = self.cfg.max_value_bytes;
        let mut operand = operand;
        if let ([step], Some(key)) = (path, idxs.first())
            && let Some(delta) = self.mutate_one(place, *step, action, key, &mut operand)
        {
            return delta;
        }
        // `shuffle` uses the random generator while the place is borrowed, so use a copy and put it back afterwards
        let mut rng = self.rng.clone();
        let (delta, adjust_nodes) = {
            let mut it = idxs.iter();
            let mut cur: &mut Value = self.place_mut(place);
            for (n, step) in path.iter().enumerate() {
                // Only an assignment at the last step adds a missing key (check the size before adding)
                if n + 1 == path.len() && action == Action::Set && *step == PathStep::Key {
                    let Value::Map(entries) = &mut *cur else {
                        unreachable!("map のはず")
                    };
                    let key = it.next().expect("キーの数は、歩みの数と同じ");
                    match entries.slot(key) {
                        Some(i) => cur = entries.value_mut(i),
                        None => {
                            let v = operand.expect("値がある");
                            let add = key.approx_bytes() + v.approx_bytes();
                            if 24 + entries.content_bytes() + add > max || v.approx_bytes() > max {
                                return Err(VmErrorKind::ValueTooLarge);
                            }
                            entries.push_new(key.clone(), v);
                            // The total size of the `map` itself was corrected when adding. Report it outward
                            return self.finish_mutate(place, path, idxs, path.len() - 1, (0, add));
                        }
                    }
                } else {
                    cur = step_into(cur, step, &mut it)?;
                }
            }
            let delta = match action {
                Action::Set => {
                    let v = operand.expect("値がある");
                    let grow = v.approx_bytes();
                    if grow > max {
                        return Err(VmErrorKind::ValueTooLarge);
                    }
                    let shrink = cur.approx_bytes();
                    *cur = v;
                    (shrink, grow)
                }
                Action::Arith(op) => {
                    let rhs = operand.expect("値がある");
                    // Every number has the same size (8)
                    *cur = arith(op, cur.clone(), rhs)?;
                    (0, 0)
                }
                Action::Push => {
                    let Value::List(items) = cur else {
                        unreachable!("list のはず")
                    };
                    let item = operand.expect("値がある");
                    let grow = item.approx_bytes();
                    if 24 + items.content_bytes() + grow > max {
                        return Err(VmErrorKind::ValueTooLarge);
                    }
                    items.push(item);
                    (0, grow)
                }
                Action::Insert => {
                    let Value::List(items) = cur else {
                        unreachable!("list のはず")
                    };
                    let k = insert_at.expect("添字がある");
                    let item = operand.expect("値がある");
                    let len = items.len();
                    // Inserting at the end (`len`) is also allowed
                    let Some(at) = usize::try_from(k).ok().filter(|&k| k <= len) else {
                        return Err(VmErrorKind::IndexOutOfRange {
                            index: k,
                            len: len as i64,
                        });
                    };
                    let grow = item.approx_bytes();
                    if 24 + items.content_bytes() + grow > max {
                        return Err(VmErrorKind::ValueTooLarge);
                    }
                    items.insert(at, item);
                    (0, grow)
                }
                Action::Remove => match cur {
                    Value::List(items) => {
                        let Value::Int(k) = operand.expect("値がある") else {
                            unreachable!("整数のはず")
                        };
                        let len = items.len();
                        match usize::try_from(k).ok().filter(|&k| k < len) {
                            Some(k) => (items.remove(k).approx_bytes(), 0),
                            None => {
                                return Err(VmErrorKind::IndexOutOfRange {
                                    index: k,
                                    len: len as i64,
                                });
                            }
                        }
                    }
                    Value::Map(entries) => {
                        // A missing key does nothing
                        match entries.slot(&operand.expect("値がある")) {
                            Some(i) => {
                                let (k, v) = entries.remove_at(i);
                                (k.approx_bytes() + v.approx_bytes(), 0)
                            }
                            None => (0, 0),
                        }
                    }
                    other => unreachable!("list か map のはず: {other:?}"),
                },
                Action::Clear => {
                    let shrink = cur.approx_bytes();
                    match cur {
                        Value::List(items) => items.clear(),
                        Value::Map(entries) => entries.clear(),
                        other => unreachable!("list か map のはず: {other:?}"),
                    }
                    // An empty `list` or `map` is 24
                    (shrink, 24)
                }
                Action::Shuffle => {
                    let Value::List(items) = cur else {
                        unreachable!("list のはず")
                    };
                    // Fisher–Yates (from the back, swap with a position in 0..=i). The size does not change
                    for i in (1..items.len()).rev() {
                        let j = rng.range_inclusive(0, i as i64) as usize;
                        items.swap(i, j);
                    }
                    (0, 0)
                }
            };
            (delta, path.len())
        };
        if action == Action::Shuffle {
            self.rng = rng;
        }
        self.finish_mutate(place, path, idxs, adjust_nodes, delta)
    }

    /// The fast path for assignment and compound assignment to a one-step path (`m[k]`, `xs[i]`) (does not follow the path
    /// again). Handles only the common cases; for others (including those that become errors, such as compound assignment to
    /// a missing key or an out-of-range index), returns `None` without taking `operand`, leaving it to the general path.
    fn mutate_one(
        &mut self,
        place: PlaceRef,
        step: PathStep,
        action: Action,
        key: &Value,
        operand: &mut Option<Value>,
    ) -> Option<Result<(usize, usize), VmErrorKind>> {
        let max = self.cfg.max_value_bytes;
        match (step, action, self.place_mut(place)) {
            (PathStep::Key, Action::Set, Value::Map(entries)) => {
                let v = operand.take()?;
                Some(match entries.slot_or_hash(key) {
                    Ok(i) => set_in(entries.value_mut(i), v, max).inspect(|&(shrink, grow)| {
                        if shrink != grow {
                            entries.adjust_bytes(shrink, grow);
                        }
                    }),
                    Err(hash) => {
                        let add = key.approx_bytes() + v.approx_bytes();
                        if 24 + entries.content_bytes() + add > max || v.approx_bytes() > max {
                            return Some(Err(VmErrorKind::ValueTooLarge));
                        }
                        entries.push_hashed(hash, key.clone(), v);
                        Ok((0, add))
                    }
                })
            }
            (PathStep::Key, Action::Arith(op), Value::Map(entries)) => {
                let i = entries.slot(key)?;
                let rhs = operand.take()?;
                Some(arith_in(entries.value_mut(i), op, rhs))
            }
            (PathStep::Index, Action::Set | Action::Arith(_), Value::List(items)) => {
                let Value::Int(k) = key else { return None };
                let item = usize::try_from(*k).ok().and_then(|k| items.item_mut(k))?;
                let v = operand.take()?;
                Some(match action {
                    Action::Arith(op) => arith_in(item, op, v),
                    _ => set_in(item, v, max).inspect(|&(shrink, grow)| {
                        if shrink != grow {
                            items.adjust_bytes(shrink, grow);
                        }
                    }),
                })
            }
            _ => None,
        }
    }

    /// Finishes `mutate`. Of the first `adjust_nodes` nodes of the path (from the variable's value to where the steps
    /// led), corrects the total size held by the nodes that are `list`s or `map`s by `delta` (decrease, increase).
    /// Returns `delta` as the change in the total size of variables.
    pub(super) fn finish_mutate(
        &mut self,
        place: PlaceRef,
        path: &[PathStep],
        idxs: &[Value],
        adjust_nodes: usize,
        delta: (usize, usize),
    ) -> Result<(usize, usize), VmErrorKind> {
        if delta != (0, 0) && adjust_nodes > 0 {
            let mut it = idxs.iter();
            let mut cur: &mut Value = self.place_mut(place);
            for step in &path[..adjust_nodes] {
                match &mut *cur {
                    Value::List(items) => items.adjust_bytes(delta.0, delta.1),
                    Value::Map(entries) => entries.adjust_bytes(delta.0, delta.1),
                    _ => {}
                }
                cur = step_into(cur, step, &mut it).expect("同じ道筋は、もう一度たどれる");
            }
        }
        Ok(delta)
    }
}

/// Of a frame's array, the local variables (the first `n`; all if `None`).
fn vars(locals: &[Value], n: Option<usize>) -> &[Value] {
    &locals[..n.map_or(locals.len(), |n| n.min(locals.len()))]
}

/// The number of index and key steps in a path.
pub(super) fn index_count(path: &[PathStep]) -> usize {
    path.iter()
        .filter(|st| matches!(st, PathStep::Index | PathStep::Key))
        .count()
}

/// Assigns to a place and returns the change in size `(decrease, increase)`.
fn set_in(slot: &mut Value, v: Value, max: usize) -> Result<(usize, usize), VmErrorKind> {
    let grow = v.approx_bytes();
    if grow > max {
        return Err(VmErrorKind::ValueTooLarge);
    }
    let shrink = slot.approx_bytes();
    *slot = v;
    Ok((shrink, grow))
}

/// Does a compound assignment to a place (every number has the same size 8, so there is no change).
fn arith_in(slot: &mut Value, op: ArithOp, rhs: Value) -> Result<(usize, usize), VmErrorKind> {
    *slot = match (&*slot, rhs) {
        (Value::Int(a), Value::Int(b)) => Value::Int(int_arith(op, *a, b)?),
        (_, rhs) => arith(op, slot.clone(), rhs)?,
    };
    Ok((0, 0))
}
