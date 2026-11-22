//! Rollback: snapshots (in-memory copies) per stop point, and a ring that stores them.
//!
//! A snapshot holds the same state as a save (variables, local variables, call stack, labels entered, `once` already
//! chosen, random generator, history, position, packages' state slices). **It does not hold or restore `persistent var`s,
//! host-declared persistent variables, persistent slices, or passed stop points (`seen`)**. It is not turned into bytes
//! (what is written to disk is a save).

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use super::rng::Rng;
use super::{ChoiceBuild, Frame, State, StopRecord, StopSrc, Vm, VmError, VmErrorKind, Yield};
use crate::package::SliceBox;
use crate::program::Program;
use crate::registry::Kind;
use crate::value::Value;

/// A snapshot of the state at a stop point. Can only be restored into a VM of the same program.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The fingerprint of the program (a check against restoring into another program).
    fingerprint: u64,
    label: usize,
    pc: usize,
    ordinal: u32,
    locals: Vec<Value>,
    frames: Vec<Frame>,
    stack: Vec<Value>,
    /// The stop notification (its command and ID are used; the arguments are those after replacement).
    current: Yield,
    /// The original arguments and `${expr}` values (rebuilt with the current text table when restoring).
    src: StopSrc,
    visited: BTreeSet<u32>,
    once_done: BTreeSet<(u32, String)>,
    choice: ChoiceBuild,
    /// Module variables (those of `persistent var`s are not used when restoring).
    globals: Vec<Value>,
    /// Host-declared variables (those of `Kind::Persistent` are not used when restoring).
    host_vars: Vec<Value>,
    /// Packages' state slices (persistent slices are not held).
    states: Vec<SliceBox>,
    rng: Rng,
    args_rng: Rng,
    history: Vec<Arc<StopRecord>>,
}

/// A rough fingerprint made from the shape of the program.
fn fingerprint(program: &Program) -> u64 {
    let ops: usize = program.labels.iter().map(|l| l.reg.ops.len()).sum();
    let fn_ops: usize = program.fns.iter().map(|f| f.reg.ops.len()).sum();
    ((program.labels.len() as u64) << 40)
        ^ ((program.globals.len() as u64) << 20)
        ^ ((program.fns.len() as u64) << 52)
        ^ (ops + fn_ops) as u64
}

impl<'a> Vm<'a> {
    /// Takes a snapshot at a stop point (`NotStopped` if not stopped).
    ///
    /// The host takes one at each stop point, stores it in a [`SnapshotLog`], and goes back with [`Vm::restore`].
    pub fn snapshot(&self) -> Result<Snapshot, VmError> {
        let (State::Stopped { ordinal }, Some(current)) = (&self.state, &self.current) else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        Ok(Snapshot {
            fingerprint: fingerprint(&self.program),
            label: self.label,
            pc: self.pc,
            ordinal: *ordinal,
            locals: self.locals.clone(),
            frames: self.frames.clone(),
            stack: self.stack.clone(),
            current: current.clone(),
            src: self.cur_src.clone().unwrap_or(StopSrc {
                args: match current {
                    Yield::Stop { args, .. } => args.clone(),
                    Yield::End => Vec::new(),
                },
                values: Vec::new(),
            }),
            visited: self.visited.clone(),
            once_done: self.once_done.clone(),
            choice: self.choice.clone(),
            globals: self.globals.clone(),
            host_vars: self.host_vars.clone(),
            states: self.states.clone(),
            rng: self.rng.clone(),
            args_rng: self.args_rng.clone(),
            history: self.history().to_vec(),
        })
    }

    /// Goes back to the state of the stop point where the snapshot was taken. The VM is left stopped at that stop point (it can
    /// go back even after a runtime error).
    ///
    /// What is restored is the save state (`save var`s, local variables, call stack, labels entered, `once` already chosen,
    /// random generator, history, position). **`persistent var`s and passed stop points are not restored**.
    /// A snapshot of another program gives `SnapshotMismatch` (the VM is unchanged).
    pub fn restore(&mut self, snap: &Snapshot) -> Result<(), VmError> {
        let ok = snap.fingerprint == fingerprint(&self.program)
            && snap.globals.len() == self.globals.len()
            && snap.host_vars.len() == self.host_vars.len()
            && snap.states.len() == self.states.len()
            && snap.label < self.program.labels.len();
        if !ok {
            return Err(self.misuse(VmErrorKind::SnapshotMismatch));
        }
        for ((slot, saved), g) in self
            .globals
            .iter_mut()
            .zip(&snap.globals)
            .zip(&self.program.globals)
        {
            if !g.persistent {
                *slot = saved.clone();
            }
        }
        for ((slot, saved), d) in self
            .host_vars
            .iter_mut()
            .zip(&snap.host_vars)
            .zip(self.registry.vars())
        {
            if d.kind == Kind::Save {
                *slot = saved.clone();
            }
        }
        self.states = snap.states.clone();
        self.clear_cues();
        self.label = snap.label;
        self.pc = snap.pc;
        self.locals = snap.locals.clone();
        self.frames = snap.frames.clone();
        self.fn_frames.clear();
        self.cur_fn = None;
        self.dbg_last = None;
        self.total_valid = false;
        self.stack = snap.stack.clone();
        self.visited = snap.visited.clone();
        self.once_done = snap.once_done.clone();
        self.choice = snap.choice.clone();
        self.rng.clone_from(&snap.rng);
        self.args_rng.clone_from(&snap.args_rng);
        self.history = snap.history.clone();
        self.cap.clear();
        self.state = State::Stopped {
            ordinal: snap.ordinal,
        };
        // The language is not part of the save or snapshot state. The body of the restored stop point is rebuilt with the
        // current text table
        self.current = Some(match &snap.current {
            Yield::Stop { command, id, .. } => Yield::Stop {
                command: *command,
                args: self.localize(
                    snap.ordinal,
                    id.as_deref(),
                    &snap.src.args,
                    &snap.src.values,
                ),
                id: id.clone(),
            },
            other => other.clone(),
        });
        self.cur_src = Some(snap.src.clone());
        self.generation += 1;
        Ok(())
    }
}

/// A ring of snapshots (with a maximum count; when full, the oldest are dropped).
///
/// The host `push`es a [`Vm::snapshot`] at each stop point and takes one out with `pop_back` to go back.
#[derive(Debug, Clone)]
pub struct SnapshotLog {
    items: VecDeque<Snapshot>,
    capacity: usize,
}

impl SnapshotLog {
    /// Makes an empty ring. `capacity` is the maximum number of entries (0 holds nothing).
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            capacity,
        }
    }

    /// The maximum number of entries.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of entries held now.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds to the end (the newest). If over the limit, drops the front (the oldest).
    pub fn push(&mut self, snapshot: Snapshot) {
        if self.capacity == 0 {
            return;
        }
        while self.items.len() >= self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(snapshot);
    }

    /// Takes out the end (the newest).
    pub fn pop_back(&mut self) -> Option<Snapshot> {
        self.items.pop_back()
    }

    /// Looks at the end (the newest).
    pub fn back(&self) -> Option<&Snapshot> {
        self.items.back()
    }

    /// Drops everything.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
