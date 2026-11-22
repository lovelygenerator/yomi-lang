//! Internal VM operations for the REPL (section 7.8 of `spec/host/07-repl.md`).
//!
//! The REPL runs an extended program with added declarations on a copy of the `Vm` ([`Vm::fork`]). This has copying,
//! swapping the program, starting input, and `apply` (writing values of the copy back to the stopped VM). It does not
//! change the behavior of the script's `Vm`.

use super::save::SavedValue;
use super::{
    ChoiceBuild, Inner, NEXT_UID, State, Variable, Vm, VmError, VmErrorKind, default_locals,
    resolve_ids,
};
use crate::program::{ProgRef, Program};
use crate::registry::Kind;
use crate::value::Value;

/// Where `apply` writes back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplyTarget {
    /// A module variable (an index into `Program::globals`).
    Global(usize),
    /// A host-declared variable (an index into the registry's `vars`).
    Host(usize),
    /// A local variable of the current label (a slot).
    Local(usize),
}

/// A failure of `apply`. The `usize` is an index into the list of changes.
#[derive(Debug)]
pub(crate) enum ApplyFail {
    Type(usize),
    ValueTooLarge(usize),
    MemoryLimit(usize),
    NotSavable(usize),
    StopArgs(VmError),
}

impl<'a> Vm<'a> {
    /// Makes a VM that copies the state entirely (including package state, persistent data, passed stop points, the random
    /// generator and history). The original VM is unchanged. The copy gets a new number.
    pub(crate) fn fork(&self) -> Vm<'a> {
        Vm {
            program: self.program.clone(),
            registry: self.registry,
            cfg: self.cfg.clone(),
            interrupt: self.interrupt.clone(),
            cmd_ids: self.cmd_ids.clone(),
            native_ids: self.native_ids.clone(),
            host_var_ids: self.host_var_ids.clone(),
            globals: self.globals.clone(),
            host_vars: self.host_vars.clone(),
            states: self.states.clone(),
            cue_order: self.cue_order.clone(),
            persist: self.persist.clone(),
            label: self.label,
            pc: self.pc,
            locals: self.locals.clone(),
            frames: self.frames.clone(),
            fn_frames: self.fn_frames.clone(),
            dbg_single: false,
            dbg_last: None,
            spare_locals: Vec::new(),
            scratch_idxs: Vec::new(),
            cur_fn: self.cur_fn,
            total: self.total,
            total_valid: self.total_valid,
            stack: self.stack.clone(),
            state: self.state.clone(),
            current: self.current.clone(),
            visited: self.visited.clone(),
            once_done: self.once_done.clone(),
            seen: self.seen.clone(),
            choice: self.choice.clone(),
            rng: self.rng.clone(),
            args_rng: self.args_rng.clone(),
            cap: self.cap.clone(),
            texts: self.texts.clone(),
            cur_src: self.cur_src.clone(),
            history: self.history.clone(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            generation: 0,
            repl_cur: self.repl_cur,
        }
    }

    /// The number of this VM and how many times it has advanced past stop points (REPL's `apply` checks whether it is still at
    /// the same stop point).
    pub(crate) fn identity(&self) -> (u64, u64) {
        (self.uid, self.generation)
    }

    /// The reference to the program.
    pub(crate) fn program_ref(&self) -> &ProgRef<'a> {
        &self.program
    }

    /// The registry.
    pub(crate) fn registry(&self) -> &'a crate::registry::Registry {
        self.registry
    }

    /// The sequential number of the label being run (stopped at).
    pub(crate) fn label_index(&self) -> usize {
        self.label
    }

    /// The values of module variables (in the order of `Program::globals`).
    pub(crate) fn raw_globals(&self) -> &[Value] {
        &self.globals
    }

    /// The values of host-declared variables (in registry order).
    pub(crate) fn raw_host_vars(&self) -> &[Value] {
        &self.host_vars
    }

    /// The values of the local variables of the current label (in slot order).
    pub(crate) fn raw_locals(&self) -> &[Value] {
        &self.locals
    }

    /// Whether it cannot continue because of a runtime error.
    pub(crate) fn is_poisoned(&self) -> bool {
        self.state == State::Poisoned
    }

    /// Swaps the program for an extended one. Module variables that were added get their declared initial values (existing
    /// variables keep their values). `repl_cur` are the indices of the REPL module and the current module (so that `seen` and
    /// `visited` look up names written without a module in the current module).
    pub(crate) fn set_program(
        &mut self,
        program: ProgRef<'a>,
        repl_cur: Option<(u32, u32)>,
    ) -> Result<(), VmError> {
        let (cmd_ids, native_ids, host_var_ids) = resolve_ids(&program, self.registry)?;
        self.cmd_ids = cmd_ids;
        self.native_ids = native_ids;
        self.host_var_ids = host_var_ids;
        let n = program.globals.len();
        self.globals.truncate(n);
        while self.globals.len() < n {
            let g = &program.globals[self.globals.len()];
            self.globals.push(g.init.clone());
        }
        self.program = program;
        self.repl_cur = repl_cur;
        self.total_valid = false;
        Ok(())
    }

    /// Puts a value into a module variable (initializing REPL variables; the size is not checked).
    pub(crate) fn set_global(&mut self, index: usize, value: Value) {
        self.globals[index] = value;
        self.total_valid = false;
    }

    /// Prepares to run from the start of the input label (clears the call stack, the stack and the stop state).
    pub(crate) fn begin_input(&mut self, label: usize) {
        let program = self.program.clone();
        self.label = label;
        self.pc = 0;
        self.locals = default_locals(&program, &program.labels[label]);
        self.frames.clear();
        self.fn_frames.clear();
        self.cur_fn = None;
        self.stack.clear();
        self.state = State::Ready;
        self.current = None;
        self.cur_src = None;
        self.cap.clear();
        self.choice = ChoiceBuild::default();
        self.total_valid = false;
    }

    /// The value on top of the stack (the value of expression input).
    pub(crate) fn stack_top(&self) -> Option<Value> {
        self.stack.last().cloned()
    }

    /// Empties the stack.
    pub(crate) fn clear_stack(&mut self) {
        self.stack.clear();
    }

    /// The `Variable`s targeted by `apply` (with the applied values).
    pub(crate) fn variable_of(&self, target: ApplyTarget) -> Variable {
        match target {
            ApplyTarget::Global(i) => {
                let g = &self.program.globals[i];
                Variable {
                    module: Some(self.program.modules[g.module as usize].clone()),
                    name: g.name.clone(),
                    value: self.globals[i].clone(),
                    local: false,
                    persistent: g.persistent,
                }
            }
            ApplyTarget::Host(i) => {
                let d = &self.registry.vars()[i];
                let (ns, name) = d.name.split_once('.').unwrap_or(("", d.name.as_str()));
                Variable {
                    module: Some(ns.to_string()),
                    name: name.to_string(),
                    value: self.host_vars[i].clone(),
                    local: false,
                    persistent: d.kind == Kind::Persistent,
                }
            }
            ApplyTarget::Local(slot) => Variable {
                module: None,
                name: self.program.labels[self.label].locals[slot].name.clone(),
                value: self.locals[slot].clone(),
                local: true,
                persistent: false,
            },
        }
    }

    /// Writes values of variables back to the stopped VM (all or nothing). Checks types, limits and saveable shapes, and after
    /// writing, rebuilds the arguments of the stop point (re-evaluating `${expr}`, `if` and so on). On failure, the VM is
    /// unchanged.
    pub(crate) fn apply_values(
        &mut self,
        changes: &[(ApplyTarget, Value)],
    ) -> Result<(), ApplyFail> {
        let State::Stopped { ordinal } = self.state else {
            return Err(ApplyFail::StopArgs(self.misuse(VmErrorKind::NotStopped)));
        };
        let program = self.program.clone();
        // 1. Types
        for (i, (target, v)) in changes.iter().enumerate() {
            let ty = match target {
                ApplyTarget::Global(g) => &program.globals[*g].ty,
                ApplyTarget::Host(h) => &self.registry.vars()[*h].ty,
                ApplyTarget::Local(l) => &program.labels[self.label].locals[*l].ty,
            };
            if !v.matches_in(ty, program.types()) {
                return Err(ApplyFail::Type(i));
            }
        }
        // 2. Limits (the size of one value, the total of values held in variables)
        for (i, (_, v)) in changes.iter().enumerate() {
            if v.approx_bytes() > self.cfg.max_value_bytes {
                return Err(ApplyFail::ValueTooLarge(i));
            }
        }
        if let Some(max) = self.cfg.max_total_bytes {
            let mut total = self.recount_total();
            for (i, (target, v)) in changes.iter().enumerate() {
                let old = match target {
                    ApplyTarget::Global(g) => self.globals[*g].approx_bytes(),
                    ApplyTarget::Host(h) => self.host_vars[*h].approx_bytes(),
                    ApplyTarget::Local(l) => self.locals[*l].approx_bytes(),
                };
                total = (total + v.approx_bytes()).saturating_sub(old);
                if total > max {
                    return Err(ApplyFail::MemoryLimit(i));
                }
            }
        }
        // 3. Saveable shape (nesting depth, finite `float`s)
        for (i, (_, v)) in changes.iter().enumerate() {
            if SavedValue::from_value(v).decode().is_none() {
                return Err(ApplyFail::NotSavable(i));
            }
        }
        // 4. Write, and rebuild the arguments of the stop point. On failure, roll back
        let backup = (
            self.globals.clone(),
            self.host_vars.clone(),
            self.locals.clone(),
            self.states.clone(),
            self.rng.clone(),
            self.pc,
            self.stack.clone(),
            self.current.clone(),
            self.cur_src.clone(),
            self.cap.clone(),
            self.choice.clone(),
            self.seen.clone(),
        );
        for (target, v) in changes {
            match target {
                ApplyTarget::Global(g) => self.globals[*g] = v.clone(),
                ApplyTarget::Host(h) => self.host_vars[*h] = v.clone(),
                ApplyTarget::Local(l) => self.locals[*l] = v.clone(),
            }
        }
        self.total_valid = false;
        match self.rebuild_yield_checked(ordinal) {
            Ok(y) => {
                self.install_current(y, ordinal);
                self.sync_current_record();
                Ok(())
            }
            Err(e) => {
                (
                    self.globals,
                    self.host_vars,
                    self.locals,
                    self.states,
                    self.rng,
                    self.pc,
                    self.stack,
                    self.current,
                    self.cur_src,
                    self.cap,
                    self.choice,
                    self.seen,
                ) = backup;
                self.total_valid = false;
                Err(ApplyFail::StopArgs(e))
            }
        }
    }

    /// Rebuilds the arguments of the current stop point, the same as `rebuild_yield`. On failure, returns a runtime error.
    fn rebuild_yield_checked(&mut self, ordinal: u32) -> Result<super::Yield, VmError> {
        let program = self.program.clone();
        let code = &program.labels[self.label];
        let stop = &code.stops[ordinal as usize];
        match self.rerun_stop_args(stop)? {
            Inner::Reached => {}
            Inner::Yield(_) | Inner::Stepped => return Err(self.misuse(VmErrorKind::Poisoned)),
        }
        let rebuilt = match self.rebuilt_yield(code, stop) {
            super::Rebuilt::Yield(y) => y,
            super::Rebuilt::NoChoice => {
                return Err(self.misuse(VmErrorKind::NoChoiceAvailable));
            }
            super::Rebuilt::NotStop => return Err(self.misuse(VmErrorKind::Poisoned)),
        };
        self.stack.clear();
        self.pc = stop.pc as usize + 1;
        Ok(rebuilt)
    }
}

/// The current module when the REPL starts (an index into `Program::modules`). The start label's module if there is one;
/// otherwise the first module (section 7.1 of `spec/host/07-repl.md`).
pub(crate) fn start_module(program: &Program, start: &str) -> Option<usize> {
    match super::find_label(program, start) {
        Some(i) => Some(program.labels[i].module as usize),
        None if program.modules.is_empty() => None,
        None => Some(0),
    }
}
