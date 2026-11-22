//! VM: runs bytecode and stops at stop points.

#![expect(
    clippy::unnecessary_lazy_evaluations,
    reason = "`VmErrorKind` は解放の処理を持つので、`ok_or` で先に作ると、成功するたびに作って捨てる（整数の演算ごとに解放の関数を呼ぶ）。失敗したときだけ作るよう、`ok_or_else` を使う"
)]

mod build;
mod config;
mod debug;
mod error;
mod exec;
mod inspect;
mod interrupt;
mod mutate;
pub(crate) mod persistent;
mod regexec;
pub(crate) mod repl;
mod rng;
pub(crate) mod save;
mod slices;
mod snapshot;
mod texts;
mod types;

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::diag::DiagArg;
use crate::package::{Env, Package, SliceBox};
use crate::program::{
    Action, ArithOp, LabelCode, PathStep, PlaceRef, ProgRef, Program, RegOp, StopInfo, StopKind,
};
use crate::registry::{CommandId, Kind, Registry};
use crate::texts::TextTable;
use crate::value::{ListData, MapData, Ty, TypeKind, Value, finite_float};

pub use config::*;
pub use debug::{
    DebugHook, DebugYield, Debugger, FrameInfo, HookAction, LineEvent, PauseHandle, StepMode,
    UnitKind,
};
pub use error::*;
pub use inspect::CueRef;
pub use interrupt::InterruptHandle;
pub use persistent::PersistentReport;
pub(crate) use save::StateKey;
pub use save::{
    LoadError, LoadErrorKind, LoadReport, PendingStop, ReloadError, ResolvedBy, pending_stop,
};
pub use snapshot::{Snapshot, SnapshotLog};

pub use rng::Rng;
pub use types::*;

/// The original (before replacement) arguments of the current stop point, and the evaluated values of `${expr}` for each
/// body ([`Vm::refresh_current`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StopSrc {
    pub args: Vec<Value>,
    pub values: Vec<Vec<String>>,
}

/// The VM's state: `Ready` (can run), `Stopped` (waiting for input at a stop point), `Ended` (finished), `Poisoned` (after
/// a runtime error; cannot continue).
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ready,
    /// Waiting for input at a stop point (`ordinal` is the sequential number of the stop point).
    Stopped {
        ordinal: u32,
    },
    /// Paused by the debug hook in the middle of a `run_debug`.
    Paused,
    Ended,
    Poisoned,
}

/// Why `exec` stopped: at a stop point or the end (`Yield`), or on reaching a given position (`Reached`).
enum Inner {
    Yield(Yield),
    Reached,
    /// One instruction was run in single-step mode (`Vm::dbg_single`).
    Stepped,
}

/// A frame of the call stack (the return point of the side that did `call`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    /// The caller's label (a sequential number across the whole program).
    pub label: usize,
    /// The return point (the instruction after `Call`).
    pub ret_pc: usize,
    /// The caller's local variables.
    pub locals: Vec<Value>,
}

/// A frame of a `fn` call (the caller's return point and state). `fn`s do not stop, so these are not in saves or
/// snapshots (they are pushed and popped within one `run`).
#[derive(Debug, Clone, PartialEq)]
struct FnFrame {
    /// If the caller is a `fn`, its index (`None` for a label).
    caller: Option<usize>,
    /// The return point (the instruction after `CallFn`).
    ret_pc: usize,
    /// The caller's local variables.
    locals: Vec<Value>,
}

/// The list of a choice (built from `ChoiceBegin` to `ChoiceStop`).
#[derive(Debug, Clone, Default)]
pub(crate) struct ChoiceBuild {
    /// The texts of the options included in the list.
    texts: Vec<Value>,
    /// The stable IDs in the same order (`Option<str>`).
    ids: Vec<Value>,
    /// The numbers in the order written, in the same order.
    orig: Vec<u32>,
}

/// The VM that runs scripts.
#[derive(Debug)]
pub struct Vm<'a> {
    program: ProgRef<'a>,
    registry: &'a Registry,
    cfg: VmConfig,
    /// The token that `run` watches. The same as `cfg.interrupt` if given; forks share it.
    interrupt: InterruptHandle,
    cmd_ids: Vec<CommandId>,
    /// The index in the registry of each name in the program's `natives`.
    native_ids: Vec<usize>,
    /// The index in the registry of each name in the program's `host_vars`.
    host_var_ids: Vec<usize>,
    globals: Vec<Value>,
    /// Host-declared variables (in registry order).
    host_vars: Vec<Value>,
    /// The state slices of packages (in the registry's package order). Included in saves and undone by rollback.
    states: Vec<SliceBox>,
    /// A table of cues in the order they were added (`(package number, position of the cue in that package)`). Emptied by
    /// `clear_cues`.
    cue_order: Vec<(u32, usize)>,
    /// The persistent slices of packages (in the registry's package order). Go into persistent data; not undone by rollback.
    persist: Vec<SliceBox>,
    /// The label being run (a sequential number across the whole program).
    label: usize,
    pc: usize,
    locals: Vec<Value>,
    /// The call stack (the bottom is the first caller).
    frames: Vec<Frame>,
    /// The stack of `fn` calls (only inside `run`; empty at stop points). The running `fn` is `cur_fn`.
    fn_frames: Vec<FnFrame>,
    /// Empty arrays of local variables from returned `fn`s (reused by the next call to save allocating and freeing). Not
    /// part of the state.
    spare_locals: Vec<Vec<Value>>,
    /// A working array to line up the indices and keys of `Mutate` (to avoid allocating per instruction). Not part of the
    /// state.
    scratch_idxs: Vec<Value>,
    /// If what is running is a `fn`, its index. Then `label` and `frames` stay those of the caller (a label).
    cur_fn: Option<usize>,
    /// Makes `exec` run one instruction and return (the debug run).
    dbg_single: bool,
    /// The code and line of the instruction run last in a debug run. Not part of the state.
    dbg_last: Option<debug::DbgKey>,
    /// The total size of values held in variables ([`VmConfig::max_total_bytes`]). Correct only when `total_valid` is true.
    total: usize,
    /// Whether `total` matches the current state of the variables (set to false after reading, restoring or writing from
    /// outside, and recounted at the start of `run`).
    total_valid: bool,
    stack: Vec<Value>,
    state: State,
    current: Option<Yield>,
    /// Labels entered (`visited`). Included in saves and undone by rollback.
    visited: BTreeSet<u32>,
    /// `once` options chosen (label, stable ID). Included in saves and undone by rollback.
    once_done: BTreeSet<(u32, String)>,
    /// Passed stop points (`module.label#id`; `seen`). Not included in saves.
    seen: BTreeSet<String>,
    choice: ChoiceBuild,
    /// The random generator. Included in saves and undone by rollback.
    rng: Rng,
    /// The state of the random generator when computing the arguments of the current stop point began (`ArgsBegin`). This is
    /// what goes into saves (rebuilding the arguments on load brings it back to the current state).
    args_rng: Rng,
    /// The values of `${expr}` for each body of the stop point being computed (`TextBegin`, `Capture`).
    cap: Vec<Vec<String>>,
    /// The text table that replaces bodies ([`Vm::set_texts`]). Not included in saves.
    texts: Option<TextTable>,
    /// The original arguments and `${expr}` values of the current stop point. Held only while stopped.
    cur_src: Option<StopSrc>,
    /// The history of stop points (oldest first; accumulates up to twice the limit and then drops in bulk; the newest
    /// `history_limit` entries are shown). Records are held in `Arc`s and shared with snapshots (records are not copied for
    /// each snapshot).
    history: Vec<Arc<StopRecord>>,
    /// A number that identifies this VM (REPL's `attach` checks whether it is the same VM).
    uid: u64,
    /// How many times stop points were advanced or rewound (`resume`, `restore`). REPL's `apply` checks whether it is still
    /// at the stop point from `attach`.
    generation: u64,
    /// Held only by REPL copies: the indices of (the REPL module, the current module). Names in `seen` and `visited` written
    /// without a module are looked up in the current module when called from REPL module code.
    repl_cur: Option<(u32, u32)>,
}

/// The number to assign to VMs.
static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Follows one step of the path. `Index` and `Key` take one index or key from `idxs` (shared `list`s and `map`s are copied
/// on the spot).
fn step_into<'a>(
    cur: &'a mut Value,
    step: &PathStep,
    idxs: &mut std::slice::Iter<'_, Value>,
) -> Result<&'a mut Value, VmErrorKind> {
    Ok(match (step, cur) {
        (PathStep::Field(i), Value::Struct(st)) => &mut Arc::make_mut(st).fields[*i as usize].1,
        (PathStep::Index, Value::List(items)) => {
            let Some(Value::Int(k)) = idxs.next() else {
                unreachable!("添字は整数のはず")
            };
            let len = items.len();
            match usize::try_from(*k).ok().and_then(|k| items.item_mut(k)) {
                Some(item) => item,
                None => {
                    return Err(VmErrorKind::IndexOutOfRange {
                        index: *k,
                        len: len as i64,
                    });
                }
            }
        }
        (PathStep::Key, Value::Map(entries)) => {
            let key = idxs.next().expect("キーの数は、歩みの数と同じ");
            match entries.slot(key) {
                Some(i) => entries.value_mut(i),
                None => return Err(VmErrorKind::KeyNotFound { key: key.to_text() }),
            }
        }
        (_, other) => unreachable!("たどれる値のはず: {other:?}"),
    })
}

/// Makes a command's return value follow the rules of script values (`float` is finite, `-0.0` becomes `0.0`; the size
/// limit of strings).
fn normalize_result(v: Value, budget: &mut usize) -> Result<Value, VmErrorKind> {
    Ok(match v {
        Value::Float(x) => {
            Value::Float(finite_float(x).ok_or_else(|| VmErrorKind::NonFiniteFloat)?)
        }
        Value::Str(s) => {
            *budget = budget
                .checked_sub(s.len())
                .ok_or_else(|| VmErrorKind::ValueTooLarge)?;
            Value::Str(s)
        }
        Value::List(items) => Value::List(
            items
                .into_iter()
                .map(|v| normalize_result(v, budget))
                .collect::<Result<_, _>>()?,
        ),
        Value::Map(entries) => Value::Map(MapData::from_pairs(
            entries
                .into_entries()
                .into_iter()
                .map(|(k, v)| Ok((normalize_result(k, budget)?, normalize_result(v, budget)?)))
                .collect::<Result<_, VmErrorKind>>()?,
        )),
        Value::Option(Some(inner)) => {
            Value::Option(Some(Box::new(normalize_result(*inner, budget)?)))
        }
        other => other,
    })
}

/// Makes the local variables of a label with the default values of their types, followed by temporaries (`()`).
fn default_locals(program: &Program, code: &LabelCode) -> Vec<Value> {
    let mut locals = Vec::with_capacity(frame_len(code));
    locals.extend(code.locals.iter().map(|l| l.ty.default_in(&program.types)));
    locals.resize(frame_len(code), Value::Unit);
    locals
}

/// The length of a label's frame (local variables and temporaries).
fn frame_len(code: &LabelCode) -> usize {
    code.reg.regs as usize
}

/// The notification of a stop, rebuilt from the stopping instruction.
enum Rebuilt {
    Yield(Yield),
    /// The list of a choice is empty (the script changed and no options can be chosen).
    NoChoice,
    /// Not a stopping instruction (the data is broken).
    NotStop,
}

/// Integer arithmetic.
#[inline(always)]
fn int_arith(op: ArithOp, a: i64, b: i64) -> Result<i64, VmErrorKind> {
    let r = match op {
        ArithOp::Add => a.checked_add(b),
        ArithOp::Sub => a.checked_sub(b),
        ArithOp::Mul => a.checked_mul(b),
        ArithOp::Div | ArithOp::Rem => {
            if b == 0 {
                return Err(VmErrorKind::DivisionByZero);
            }
            if op == ArithOp::Div {
                a.checked_div(b)
            } else {
                a.checked_rem(b)
            }
        }
    };
    r.ok_or_else(|| VmErrorKind::IntegerOverflow)
}

/// Binary arithmetic (`int` with `int`, `float` with `float`).
fn arith(op: ArithOp, a: Value, b: Value) -> Result<Value, VmErrorKind> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => int_arith(op, a, b).map(Value::Int),
        (Value::Float(a), Value::Float(b)) => {
            if op == ArithOp::Div && b == 0.0 {
                return Err(VmErrorKind::DivisionByZero);
            }
            let r = match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
                ArithOp::Div => a / b,
                ArithOp::Rem => unreachable!("float の % はコンパイルできない"),
            };
            finite_float(r)
                .map(Value::Float)
                .ok_or_else(|| VmErrorKind::NonFiniteFloat)
        }
        other => unreachable!("同じ型の数のはず: {other:?}"),
    }
}

/// Finds a label by `module.label` (if there is one module, just the label name also works).
fn find_label(program: &Program, start: &str) -> Option<usize> {
    // Names passed by the host are also normalized to NFC before looking up
    let start = &*crate::ident::nfc(start);
    match start.split_once('.') {
        Some((module, label)) => {
            let m = program.modules.iter().position(|x| x == module)?;
            program
                .labels
                .iter()
                .position(|l| l.module as usize == m && l.name == label)
        }
        None if program.modules.len() == 1 => program.labels.iter().position(|l| l.name == start),
        None => None,
    }
}

/// The `module.label` of a label.
fn label_path(program: &Program, label: usize) -> String {
    let code = &program.labels[label];
    format!("{}.{}", program.modules[code.module as usize], code.name)
}

/// The key of a position written only with a label name, in the module of the running code (a label or `fn`).
fn seen_key_in(program: &Program, module: u32, label: &str, id: &str) -> String {
    let module = &program.modules[module as usize];
    format!("{module}.{label}#{id}")
}

/// The key of a passed stop point (`module.label#id`).
fn seen_key(program: &Program, label: usize, id: &str) -> String {
    format!("{}#{id}", label_path(program, label))
}

/// The result of `resolve_ids` (lists of command, host variable and another set of indices).
type ResolvedIds = (Vec<CommandId>, Vec<usize>, Vec<usize>);

/// Turns the commands and variables used by the program into registry indices (`CommandMissing` if not in the registry).
fn resolve_ids(program: &Program, registry: &Registry) -> Result<ResolvedIds, VmError> {
    let missing = |name: &String| VmError {
        kind: VmErrorKind::CommandMissing(name.clone()),
        module: None,
        label: None,
        line: None,
    };
    let mut cmd_ids = Vec::new();
    for name in &program.commands {
        cmd_ids.push(registry.command_id(name).ok_or_else(|| missing(name))?);
    }
    let mut native_ids = Vec::new();
    for name in &program.natives {
        native_ids.push(registry.native_index(name).ok_or_else(|| missing(name))?);
    }
    let mut host_var_ids = Vec::new();
    for name in &program.host_vars {
        host_var_ids.push(registry.var_index(name).ok_or_else(|| missing(name))?);
    }
    Ok((cmd_ids, native_ids, host_var_ids))
}

impl<'a> Vm<'a> {
    /// Creates a VM. It starts from the start label. If `cfg` has persistent data ([`VmConfig::persistent`]), the
    /// `persistent var`s and passed stop points are replaced with it (`PersistentData` if it cannot be read).
    pub fn new(
        program: &'a Program,
        registry: &'a Registry,
        cfg: VmConfig,
    ) -> Result<Self, VmError> {
        Self::new_inner(ProgRef::Borrowed(program), registry, cfg, true)
    }

    /// The body of `new`. If `require_start` is false, the start label may be missing (the REPL; section 7.1 of
    /// `spec/host/07-repl.md`).
    pub(crate) fn new_inner(
        program: ProgRef<'a>,
        registry: &'a Registry,
        cfg: VmConfig,
        require_start: bool,
    ) -> Result<Self, VmError> {
        let persistent = match &cfg.persistent {
            Some(bytes) => Some(persistent::parse(bytes).map_err(|e| VmError {
                kind: VmErrorKind::PersistentData(e.kind),
                module: None,
                label: None,
                line: None,
            })?),
            None => None,
        };
        let mut vm = Self::blank(program, registry, cfg)?;
        if let Some(body) = persistent {
            vm.apply_persistent(body).map_err(|e| VmError {
                kind: VmErrorKind::PersistentData(e.kind),
                module: None,
                label: None,
                line: None,
            })?;
        }
        let start = vm.cfg.start.clone();
        let program = vm.program.clone();
        let Some(idx) = find_label(&program, &start) else {
            if require_start {
                return Err(VmError {
                    kind: VmErrorKind::StartLabelNotFound(start),
                    module: None,
                    label: None,
                    line: None,
                });
            }
            return Ok(vm);
        };
        vm.label = idx;
        vm.locals = default_locals(&program, &program.labels[idx]);
        vm.visited.insert(idx as u32);
        Ok(vm)
    }

    /// Creates a VM whose state is only initialized (the caller decides where to start). With a registry made only from a
    /// declaration file, the error `DeclOnlyRegistry`.
    pub(crate) fn blank(
        program: ProgRef<'a>,
        registry: &'a Registry,
        cfg: VmConfig,
    ) -> Result<Self, VmError> {
        if registry.is_decl_only() {
            return Err(VmError {
                kind: VmErrorKind::DeclOnlyRegistry,
                module: None,
                label: None,
                line: None,
            });
        }
        let (cmd_ids, native_ids, host_var_ids) = resolve_ids(&program, registry)?;
        let seen = cfg.seen_points.iter().cloned().collect();
        let cfg_seed = cfg.seed;
        let interrupt = cfg.interrupt.clone().unwrap_or_default();
        let globals = program.globals.iter().map(|g| g.init.clone()).collect();
        Ok(Self {
            program,
            registry,
            cfg,
            interrupt,
            cmd_ids,
            native_ids,
            host_var_ids,
            host_vars: registry.vars().iter().map(|v| v.init.clone()).collect(),
            cue_order: Vec::new(),
            states: registry
                .package_entries()
                .iter()
                .map(|e| (e.new_state)())
                .collect(),
            persist: registry
                .package_entries()
                .iter()
                .map(|e| (e.new_persistent)())
                .collect(),
            globals,
            label: 0,
            pc: 0,
            locals: Vec::new(),
            frames: Vec::new(),
            fn_frames: Vec::new(),
            spare_locals: Vec::new(),
            scratch_idxs: Vec::new(),
            cur_fn: None,
            dbg_single: false,
            dbg_last: None,
            total: 0,
            total_valid: false,
            stack: Vec::new(),
            state: State::Ready,
            current: None,
            visited: BTreeSet::new(),
            once_done: BTreeSet::new(),
            seen,
            choice: ChoiceBuild::default(),
            rng: Rng::from_seed(cfg_seed),
            args_rng: Rng::from_seed(cfg_seed),
            cap: Vec::new(),
            texts: None,
            cur_src: None,
            history: Vec::new(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            generation: 0,
            repl_cur: None,
        })
    }

    /// The module index, name (the `fn` name for a `fn`) and line table of the code being run.
    fn cur_code(&self) -> (u32, &str, &[u32]) {
        match self.cur_fn {
            Some(f) => {
                let c = &self.program.fns[f];
                (c.module, c.name.as_str(), c.lines.as_slice())
            }
            None => {
                let c = &self.program.labels[self.label];
                (c.module, c.name.as_str(), c.lines.as_slice())
            }
        }
    }

    /// Returns a runtime error with the current position (module, label, line).
    fn err(&mut self, kind: VmErrorKind) -> VmError {
        let (module, label, line) = {
            let (module, name, lines) = self.cur_code();
            let line = self.pc.checked_sub(1).and_then(|i| lines.get(i).copied());
            (
                self.program.modules[module as usize].clone(),
                name.to_string(),
                line,
            )
        };
        self.state = State::Poisoned;
        VmError {
            kind,
            module: Some(module),
            label: Some(label),
            line,
        }
    }

    /// Returns a runtime error for misuse, without a position.
    fn misuse(&self, kind: VmErrorKind) -> VmError {
        VmError {
            kind,
            module: None,
            label: None,
            line: None,
        }
    }

    /// Records the stop point just stopped at in the history. `y` is the notification before replacement (the original).
    fn record_current(&mut self, y: &Yield, ordinal: u32) {
        let Yield::Stop { command, args, id } = y else {
            return;
        };
        let label = label_path(&self.program, self.label);
        let key = id.as_ref().map(|id| format!("{label}#{id}"));
        let kind = self.program.labels[self.label].stops[ordinal as usize].kind;
        let record = StopRecord {
            label,
            kind,
            key,
            command: self.registry.command(*command).name.clone(),
            args: args.clone(),
            values: std::mem::take(&mut self.cap),
            ret: None,
        };
        self.record_stop(record);
    }

    /// Records a stop point in the history.
    fn record_stop(&mut self, record: StopRecord) {
        let limit = self.cfg.history_limit;
        if limit == 0 {
            return;
        }
        self.history.push(Arc::new(record));
        // Removing one entry at a time from the front is slow, so drop in bulk when twice the limit is reached
        if self.history.len() >= limit.saturating_mul(2) {
            let drop = self.history.len() - limit;
            self.history.drain(..drop);
        }
    }

    /// The token this VM watches for interrupts (`spec/host/01-execution.md`, 1.9a). Another thread can call
    /// [`InterruptHandle::interrupt`] on it to stop a running `run`. It is the token in [`VmConfig::interrupt`] if one was
    /// given, otherwise one the VM made.
    pub fn interrupt_handle(&self) -> InterruptHandle {
        self.interrupt.clone()
    }

    /// Runs until the next stop point (or the end).
    pub fn run(&mut self) -> Result<Yield, VmError> {
        match self.state {
            State::Stopped { .. } => return Err(self.misuse(VmErrorKind::AlreadyStopped)),
            State::Ended => return Err(self.misuse(VmErrorKind::Ended)),
            State::Poisoned => return Err(self.misuse(VmErrorKind::Poisoned)),
            State::Ready | State::Paused => {}
        }
        self.dbg_last = None;
        if self.total_valid {
            debug_assert_eq!(
                self.total,
                self.recount_total(),
                "変数の大きさの合計の記録がずれた（VM の不具合）"
            );
        } else {
            self.total = self.recount_total();
            self.total_valid = true;
        }
        self.clear_cues();
        match self.exec(None)? {
            Inner::Yield(y) => Ok(y),
            Inner::Reached | Inner::Stepped => unreachable!("終点を指定していない"),
        }
    }

    /// Resumes a stop point with a return value.
    pub fn resume(&mut self, input: Input) -> Result<(), VmError> {
        let State::Stopped { .. } = self.state else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let Input::Return(v) = input;
        let Some(Yield::Stop { command, args, .. }) = &self.current else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let expected = self.registry.command(*command).ret.clone();
        let v = match v {
            // `NaN` and `±∞`, which never appear as values, are not accepted. `-0.0` becomes `0.0`.
            Value::Float(x) => match finite_float(x) {
                Some(x) => Value::Float(x),
                None => return Err(self.misuse(VmErrorKind::NonFiniteFloat)),
            },
            other => other,
        };
        if !v.matches(&expected) {
            return Err(self.misuse(VmErrorKind::BadInput {
                found: v.ty(),
                expected,
            }));
        }
        // Stop commands whose return values are validated (such as `input.*`): values that break the constraints are runtime
        // errors
        if let Some(check) = self.registry.stop_ret_check(*command)
            && let Err(e) = check.run(args, &v)
        {
            return Err(self.err(e.into_vm_kind()));
        }
        // Choice: convert the number in the given list into the number in the order written and push it (used by `JumpTable`)
        if let State::Stopped { ordinal } = self.state
            && let Some(info) = &self.program.labels[self.label].stops[ordinal as usize].choice
        {
            let Value::Int(k) = v else {
                return Err(self.misuse(VmErrorKind::BadInput {
                    expected: Ty::Int,
                    found: v.ty(),
                }));
            };
            let count = self.choice.orig.len();
            let Some(orig) = usize::try_from(k)
                .ok()
                .and_then(|k| self.choice.orig.get(k).copied())
            else {
                return Err(self.misuse(VmErrorKind::ChoiceOutOfRange {
                    index: k,
                    count: count as i64,
                }));
            };
            if let Some(id) = info
                .options
                .get(orig as usize)
                .and_then(|o| o.id.as_deref())
            {
                self.seen.insert(seen_key(&self.program, self.label, id));
            }
            self.deliver(Value::Int(i64::from(orig)));
        }
        // A stop point that receives the return value in a variable (`let name = @command(...)`): pass the value. The next
        // instruction stores it in the variable.
        if let State::Stopped { ordinal } = self.state
            && self.program.labels[self.label].stops[ordinal as usize].binds
        {
            self.deliver(v.clone());
        }
        // Keep the return value in the history of the current stop point (the last entry)
        if v != Value::Unit
            && let Some(last) = self.history.last_mut()
        {
            // If shared with a snapshot, copy only this one entry before modifying it
            Arc::make_mut(last).ret = Some(v);
        }
        self.clear_cues();
        self.state = State::Ready;
        self.current = None;
        self.cur_src = None;
        self.generation += 1;
        Ok(())
    }

    /// Puts the resume value of a stop point into the `dst` of the stopping instruction.
    fn deliver(&mut self, v: Value) {
        let dst = match &self.program.labels[self.label].reg.ops[self.pc - 1] {
            RegOp::Stop { dst, .. } | RegOp::ChoiceStop { dst, .. } => *dst,
            other => unreachable!("停止の命令のはず: {other:?}"),
        };
        self.locals[dst as usize] = v;
    }

    /// Rebuilds the notification of a stop from the stopping instruction (at `stop.pc`, after running up to just before it).
    fn rebuilt_yield(&mut self, code: &LabelCode, stop: &StopInfo) -> Rebuilt {
        match &code.reg.ops[stop.pc as usize] {
            RegOp::Stop { start, n, cmd, .. } => {
                let at = *start as usize;
                Rebuilt::Yield(Yield::Stop {
                    command: self.cmd_ids[*cmd as usize],
                    args: self.locals[at..at + *n as usize].to_vec(),
                    id: stop.id.clone(),
                })
            }
            RegOp::ChoiceStop { cmd, .. } if !self.choice.orig.is_empty() => {
                Rebuilt::Yield(self.choice_yield(*cmd))
            }
            RegOp::ChoiceStop { .. } => Rebuilt::NoChoice,
            _ => Rebuilt::NotStop,
        }
    }

    /// Clears the cues of all packages (`Package::clear_cues`). Called at the start of `run` and `resume`, and right after
    /// loading or restoring.
    pub(crate) fn clear_cues(&mut self) {
        self.cue_order.clear();
        for (e, slice) in self.registry.package_entries().iter().zip(&mut self.states) {
            (e.clear_cues)(slice);
        }
    }

    /// Makes the notification of a stop from the current list of a choice.
    fn choice_yield(&self, cmd: u32) -> Yield {
        Yield::Stop {
            command: self.cmd_ids[cmd as usize],
            args: vec![
                Value::List(self.choice.texts.clone().into()),
                Value::List(self.choice.ids.clone().into()),
            ],
            id: None,
        }
    }

    /// The module in which names written without a module (the strings of `seen` and `visited`) are looked up for the code
    /// being run. For REPL module code, the current module.
    fn name_module(&self) -> u32 {
        let m = self.cur_code().0;
        match self.repl_cur {
            Some((repl, cur)) if m == repl => cur,
            _ => m,
        }
    }

    /// `seen("label#id")` / `seen("module.label#id")`.
    fn has_seen(&self, text: &str) -> bool {
        let Some((target, id)) = text.split_once('#') else {
            return false;
        };
        let key = if target.contains('.') {
            text.to_string()
        } else {
            seen_key_in(&self.program, self.name_module(), target, id)
        };
        self.seen.contains(&key)
    }

    /// `visited("label")` / `visited("module.label")`.
    fn has_visited(&self, text: &str) -> bool {
        let (module, label) = match text.split_once('.') {
            Some((m, l)) => (m.to_string(), l),
            None => (
                self.program.modules[self.name_module() as usize].clone(),
                text,
            ),
        };
        save::find_label(&self.program, &module, label)
            .is_some_and(|i| self.visited.contains(&(i as u32)))
    }
}

#[cfg(test)]
mod tests;
