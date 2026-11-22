//! Debugging by source line: line events, a ready-made debugger and inspection of the call stack (`spec/host/09-debug.md`).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Inner, State, Variable, Vm, VmError, VmErrorKind, Yield};
use crate::program::Program;

/// Whether a line belongs to a label or to a `fn` (including lambdas and the functions the compiler makes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    /// A label.
    Label,
    /// A `fn`.
    Fn,
}

/// What the hook decides at a line event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    /// Run the line.
    Continue,
    /// Pause before running the line.
    Pause,
}

/// A line is about to run (`spec/host/09-debug.md`, 9.2).
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct LineEvent<'a> {
    /// The module that has the line.
    pub module: &'a str,
    /// The name of the label or `fn` that has the line (the functions the compiler makes have names starting with `<`).
    pub unit: &'a str,
    /// Whether `unit` is a label or a `fn`.
    pub kind: UnitKind,
    /// The line number, counted from 1.
    pub line: u32,
    /// The nesting of calls (9.3).
    pub depth: usize,
}

/// Decides at each line event whether to pause.
pub trait DebugHook {
    /// Called just before the first instruction of a line runs.
    fn on_line(&mut self, event: &LineEvent<'_>) -> HookAction;
}

/// Why [`Vm::run_debug`] returned.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum DebugYield {
    /// A stop point or the end, as with [`Vm::run`].
    Stop(Yield),
    /// The hook asked to pause. Call [`Vm::run_debug`] or [`Vm::run`] to go on.
    Paused,
}

/// One frame of the call stack ([`Vm::call_stack`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    /// The module of the frame's code.
    pub module: String,
    /// The name of the label or `fn`.
    pub name: String,
    /// Whether `name` is a label or a `fn`.
    pub kind: UnitKind,
    /// The line the frame is running (for outer frames, the line of the call).
    pub line: u32,
}

/// The code and line of an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DbgKey {
    unit: Unit,
    line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Label(usize),
    Fn(usize),
}

impl<'a> Vm<'a> {
    /// Whether the VM is paused by the debug hook.
    pub fn is_paused(&self) -> bool {
        self.state == State::Paused
    }

    fn dbg_unit(&self) -> Unit {
        match self.cur_fn {
            Some(f) => Unit::Fn(f),
            None => Unit::Label(self.label),
        }
    }

    fn unit_lines(program: &Program, unit: Unit) -> &[u32] {
        match unit {
            Unit::Label(l) => &program.labels[l].lines,
            Unit::Fn(f) => &program.fns[f].lines,
        }
    }

    /// Like [`Vm::run`], but calls `hook` at each line event, and returns [`DebugYield::Paused`] when it asks to pause.
    pub fn run_debug(&mut self, hook: &mut dyn DebugHook) -> Result<DebugYield, VmError> {
        let resuming = match self.state {
            State::Stopped { .. } => return Err(self.misuse(VmErrorKind::AlreadyStopped)),
            State::Ended => return Err(self.misuse(VmErrorKind::Ended)),
            State::Poisoned => return Err(self.misuse(VmErrorKind::Poisoned)),
            State::Paused => true,
            State::Ready => false,
        };
        if self.total_valid {
            debug_assert_eq!(self.total, self.recount_total());
        } else {
            self.total = self.recount_total();
            self.total_valid = true;
        }
        if !resuming {
            self.clear_cues();
        }
        self.state = State::Ready;
        let program_ref = self.program.clone();
        let program: &Program = &program_ref;
        let limit = self.cfg.max_steps_per_run;
        let mut executed: u64 = 0;
        self.dbg_single = true;
        let result = self.debug_loop(program, hook, limit, &mut executed);
        self.dbg_single = false;
        result
    }

    fn debug_loop(
        &mut self,
        program: &Program,
        hook: &mut dyn DebugHook,
        limit: Option<u64>,
        executed: &mut u64,
    ) -> Result<DebugYield, VmError> {
        loop {
            let unit = self.dbg_unit();
            let line = Self::unit_lines(program, unit)
                .get(self.pc)
                .copied()
                .unwrap_or(0);
            if line != 0 {
                let key = DbgKey { unit, line };
                if self.dbg_last != Some(key) {
                    let (module, name, kind) = match unit {
                        Unit::Label(l) => {
                            let c = &program.labels[l];
                            (c.module, c.name.as_str(), UnitKind::Label)
                        }
                        Unit::Fn(f) => {
                            let c = &program.fns[f];
                            (c.module, c.name.as_str(), UnitKind::Fn)
                        }
                    };
                    let event = LineEvent {
                        module: &program.modules[module as usize],
                        unit: name,
                        kind,
                        line,
                        depth: self.frames.len() + self.fn_frames.len(),
                    };
                    // Remember the line before pausing, so that going on does not report it again
                    self.dbg_last = Some(key);
                    if hook.on_line(&event) == HookAction::Pause {
                        self.state = State::Paused;
                        return Ok(DebugYield::Paused);
                    }
                }
            }
            *executed += 1;
            if limit.is_some_and(|m| *executed > m) {
                return Err(self.err(VmErrorKind::StepLimitExceeded));
            }
            match self.exec(None)? {
                Inner::Yield(y) => return Ok(DebugYield::Stop(y)),
                Inner::Stepped => {}
                Inner::Reached => unreachable!("終点を指定していない"),
            }
        }
    }

    /// The call stack, from the innermost frame (index 0, the code running now) to the outermost. Available while paused or
    /// at a stop point.
    pub fn call_stack(&self) -> Result<Vec<FrameInfo>, VmError> {
        Ok(self
            .frame_list()?
            .into_iter()
            .map(|f| {
                let (module, name, kind) = self.unit_names(f.unit);
                FrameInfo {
                    module,
                    name,
                    kind,
                    line: f.line,
                }
            })
            .collect())
    }

    /// The local variables (parameters included) of frame `frame` of [`Vm::call_stack`]. Available while paused or at a stop
    /// point. An index out of range gives an empty list.
    pub fn frame_variables(&self, frame: usize) -> Result<Vec<Variable>, VmError> {
        let frames = self.frame_list()?;
        let Some(f) = frames.get(frame) else {
            return Ok(Vec::new());
        };
        let infos = match f.unit {
            Unit::Label(l) => &self.program.labels[l].locals,
            Unit::Fn(i) => &self.program.fns[i].locals,
        };
        Ok(infos
            .iter()
            .zip(f.locals)
            .filter(|(l, _)| !l.name.starts_with('$') && !l.ty.contains_fn())
            .map(|(l, v)| Variable {
                module: None,
                name: l.name.clone(),
                value: v.clone(),
                local: true,
                persistent: false,
            })
            .collect())
    }

    fn unit_names(&self, unit: Unit) -> (String, String, UnitKind) {
        match unit {
            Unit::Label(l) => {
                let c = &self.program.labels[l];
                (
                    self.program.modules[c.module as usize].clone(),
                    c.name.clone(),
                    UnitKind::Label,
                )
            }
            Unit::Fn(f) => {
                let c = &self.program.fns[f];
                (
                    self.program.modules[c.module as usize].clone(),
                    c.name.clone(),
                    UnitKind::Fn,
                )
            }
        }
    }

    fn frame_list(&self) -> Result<Vec<RawFrame<'_>>, VmError> {
        let current_line = match self.state {
            State::Paused => Some(self.pc),
            State::Stopped { .. } => self.pc.checked_sub(1),
            _ => None,
        };
        let Some(at) = current_line else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let line_of = |unit: Unit, i: usize| {
            Self::unit_lines(&self.program, unit)
                .get(i)
                .copied()
                .unwrap_or(0)
        };
        let unit = self.dbg_unit();
        let mut out = vec![RawFrame {
            unit,
            line: line_of(unit, at),
            locals: &self.locals,
        }];
        for fr in self.fn_frames.iter().rev() {
            let unit = match fr.caller {
                Some(f) => Unit::Fn(f),
                None => Unit::Label(self.label),
            };
            out.push(RawFrame {
                unit,
                line: line_of(unit, fr.ret_pc.saturating_sub(1)),
                locals: &fr.locals,
            });
        }
        for f in self.frames.iter().rev() {
            let unit = Unit::Label(f.label);
            out.push(RawFrame {
                unit,
                line: line_of(unit, f.ret_pc.saturating_sub(1)),
                locals: &f.locals,
            });
        }
        Ok(out)
    }
}

struct RawFrame<'v> {
    unit: Unit,
    line: u32,
    locals: &'v [crate::value::Value],
}

/// How the [`Debugger`] goes on after a pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Run until a breakpoint or a pause request.
    Continue,
    /// Pause at the next line.
    In,
    /// Pause at the next line that is not deeper than where it paused.
    Over,
    /// Pause at the next line that is shallower than where it paused.
    Out,
}

/// Asks a [`Debugger`] to pause at the next line, from any thread.
#[derive(Debug, Clone)]
pub struct PauseHandle(Arc<AtomicBool>);

impl PauseHandle {
    /// Asks for a pause. The request is cleared when the debugger pauses.
    pub fn request(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

type TraceFn = Box<dyn FnMut(&LineEvent<'_>) + Send>;

/// A ready-made hook: breakpoints, stepping, pause requests and a trace function (`spec/host/09-debug.md`, 9.6).
pub struct Debugger {
    breakpoints: HashMap<String, BTreeSet<u32>>,
    mode: StepMode,
    /// The depth where it paused last; `usize::MAX` before the first pause (so that `Over` and `Out` act as `In`).
    base_depth: usize,
    pause: Arc<AtomicBool>,
    trace: Option<TraceFn>,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Debugger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Debugger")
            .field("breakpoints", &self.breakpoints)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl Debugger {
    /// A debugger with no breakpoints that runs without pausing.
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            mode: StepMode::Continue,
            base_depth: usize::MAX,
            pause: Arc::new(AtomicBool::new(false)),
            trace: None,
        }
    }

    /// Replaces the breakpoints of `module` (an empty list removes them).
    pub fn set_breakpoints(&mut self, module: &str, lines: &[u32]) {
        if lines.is_empty() {
            self.breakpoints.remove(module);
        } else {
            self.breakpoints
                .insert(module.to_string(), lines.iter().copied().collect());
        }
    }

    /// How to go on from the next [`Vm::run_debug`].
    pub fn set_mode(&mut self, mode: StepMode) {
        self.mode = mode;
    }

    /// A handle to ask this debugger to pause from another thread.
    pub fn pause_handle(&self) -> PauseHandle {
        PauseHandle(Arc::clone(&self.pause))
    }

    /// Sets the function called at every line event (also when not pausing). `None` removes it.
    pub fn set_trace(&mut self, f: Option<TraceFn>) {
        self.trace = f;
    }

    /// The lines of `module` that have instructions to run (labels and `fn`s), in ascending order.
    pub fn breakable_lines(program: &Program, module: &str) -> Vec<u32> {
        let Some(m) = program.modules.iter().position(|n| n == module) else {
            return Vec::new();
        };
        let m = m as u32;
        let mut lines: BTreeSet<u32> = BTreeSet::new();
        for c in &program.labels {
            if c.module == m {
                lines.extend(c.lines.iter().copied().filter(|&l| l != 0));
            }
        }
        for c in &program.fns {
            if c.module == m {
                lines.extend(c.lines.iter().copied().filter(|&l| l != 0));
            }
        }
        lines.into_iter().collect()
    }
}

impl DebugHook for Debugger {
    fn on_line(&mut self, event: &LineEvent<'_>) -> HookAction {
        if let Some(trace) = &mut self.trace {
            trace(event);
        }
        let hit = self.pause.swap(false, Ordering::Relaxed)
            || self
                .breakpoints
                .get(event.module)
                .is_some_and(|lines| lines.contains(&event.line))
            || match self.mode {
                StepMode::Continue => false,
                StepMode::In => true,
                StepMode::Over => event.depth <= self.base_depth,
                StepMode::Out => event.depth < self.base_depth,
            };
        if hit {
            self.base_depth = event.depth;
            self.mode = StepMode::Continue;
            HookAction::Pause
        } else {
            HookAction::Continue
        }
    }
}
