//! The component that runs a script, and the logic that drives it (independent of the schedule).

use std::collections::{HashMap, HashSet};

use bevy_asset::{Assets, Handle};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use yomi_core::{
    CommandId, Input, IntoValue, LoadPolicy, Program, Registry, TextTable, Value, Vm, VmConfig,
    Yield,
};
use yomi_std::audio::Audio;
use yomi_std::event::Outcome;
use yomi_std::image::Image;
use yomi_std::screen::Screen;

use crate::YomiTexts;
use crate::messages::*;

/// What a runner waits for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wait {
    /// No `Vm` yet; one is made when the program is ready.
    Starting,
    /// The runner moves on in this frame.
    Running,
    /// A stop command was sent to the game; waits for `YomiResume`.
    Command,
    /// `flow.wait`, in seconds of virtual time.
    Timer(f64),
    /// `event.wait`: the names, and the seconds left (`None` waits for ever).
    Event {
        names: Vec<String>,
        remaining: Option<f64>,
    },
    Ended,
    Failed,
}

/// One program: the compiled one, and a counter bumped when a new one is compiled.
#[derive(Default)]
pub(crate) struct Slot {
    pub program: Option<&'static Program>,
    pub generation: u64,
}

/// The things shared by every runner: the registry and the program.
pub(crate) struct Shared {
    pub registry: &'static Registry,
    /// The programs by name (the default program is `""`). Only those with scripts are here.
    pub programs: HashMap<String, Slot>,
    /// The commands that resume at once with `()`.
    pub immediate: HashSet<CommandId>,
    pub flow_wait: Option<CommandId>,
    pub event_wait: Option<CommandId>,
    pub policy: LoadPolicy,
    pub max_steps_per_frame: usize,
}

/// Runs one script. Spawn it as a component; it starts when the program is ready.
///
/// ```ignore
/// commands.spawn(YomiRunner::new().start("main.start").seed(7));
/// ```
#[derive(Component)]
pub struct YomiRunner {
    start: String,
    seed: u64,
    pub(crate) vm: Option<Vm<'static>>,
    pub(crate) wait: Wait,
    pub(crate) generation: u64,
    pub(crate) pending_load: Option<Vec<u8>>,
    pub(crate) restart: bool,
    /// The program this runner uses (`""` is the default one).
    pub(crate) program_name: String,
    /// The table asked for; it is turned into `texts_request` once the asset is loaded.
    texts_handle: Option<Handle<YomiTexts>>,
    texts_waiting: bool,
    /// A table (or `None` for the originals) to set in the next step.
    pub(crate) texts_request: Option<Option<TextTable>>,
    /// The table set in the VM. A VM made by a reload or a load gets it again.
    pub(crate) texts_table: Option<TextTable>,
    /// The arguments of the last `YomiCommand` the game has not answered.
    pub(crate) shown: Option<Vec<Value>>,
}

impl Default for YomiRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl YomiRunner {
    /// A runner that starts at the label `start`.
    pub fn new() -> Self {
        Self {
            start: "start".to_string(),
            seed: 0,
            vm: None,
            wait: Wait::Starting,
            generation: 0,
            pending_load: None,
            restart: false,
            program_name: String::new(),
            texts_handle: None,
            texts_waiting: false,
            texts_request: None,
            texts_table: None,
            shown: None,
        }
    }

    /// Starts with a table of replacement bodies. It is set when the file is loaded; the result comes as
    /// [`YomiTextsApplied`].
    pub fn texts(mut self, texts: Handle<YomiTexts>) -> Self {
        self.set_texts_handle(Some(texts));
        self
    }

    pub(crate) fn set_texts_handle(&mut self, handle: Option<Handle<YomiTexts>>) {
        self.texts_handle = handle;
        self.texts_waiting = true;
    }

    pub(crate) fn texts_handle(&self) -> Option<&Handle<YomiTexts>> {
        self.texts_handle.as_ref()
    }

    pub(crate) fn mark_texts_waiting(&mut self) {
        self.texts_waiting = true;
    }

    /// Makes the request from the table file, once it is loaded.
    pub(crate) fn resolve_texts(&mut self, assets: &Assets<YomiTexts>) {
        if !self.texts_waiting {
            return;
        }
        match &self.texts_handle {
            None => self.texts_request = Some(None),
            Some(h) => match assets.get(h) {
                Some(t) => self.texts_request = Some(Some(t.table.clone())),
                None => return,
            },
        }
        self.texts_waiting = false;
    }

    /// Sets the start label (`module.label`, or just the label when there is one module).
    pub fn start(mut self, label: &str) -> Self {
        self.start = label.to_string();
        self
    }

    /// Chooses the program (see [`YomiPlugin::program`](crate::YomiPlugin::program)). The default is the one made by
    /// `.script`. It cannot change later; make another runner for another program.
    pub fn program(mut self, name: &str) -> Self {
        self.program_name = name.to_string();
        self
    }

    /// Sets the seed of the random numbers.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// The VM, for reading the state of the packages (read only).
    pub fn vm(&self) -> Option<&Vm<'static>> {
        self.vm.as_ref()
    }

    /// The VM, for the core API such as `snapshot` and `restore`. Do not call `run` or `resume` through it.
    pub fn vm_mut(&mut self) -> Option<&mut Vm<'static>> {
        self.vm.as_mut()
    }

    /// Whether the script has reached `end`.
    pub fn is_ended(&self) -> bool {
        self.wait == Wait::Ended
    }

    /// Makes the save. Only while the script is stopped (waiting for a command, `flow.wait` or `event.wait`).
    pub fn save(&self) -> Result<Vec<u8>, String> {
        match (&self.vm, &self.wait) {
            (Some(vm), Wait::Command | Wait::Timer(_) | Wait::Event { .. }) => {
                vm.save().map_err(|e| e.to_string())
            }
            _ => Err("the script is not stopped".to_string()),
        }
    }

    /// Makes the data that persists across saves (`persistent var`, passed stop points).
    pub fn save_persistent(&self) -> Result<Vec<u8>, String> {
        match &self.vm {
            Some(vm) => Ok(vm.save_persistent()),
            None => Err("the script has not started".to_string()),
        }
    }

    /// Loads a save. It happens in the next update; the result comes as [`YomiLoaded`].
    pub fn load(&mut self, bytes: Vec<u8>) {
        self.pending_load = Some(bytes);
    }

    /// Starts the script over from the start label (the persistent data is kept). It happens in the next update.
    pub fn restart(&mut self) {
        self.restart = true;
    }
}

/// What the logic wants to tell the game.
#[derive(Debug)]
pub(crate) enum Out {
    Command(YomiCommand),
    Ended(YomiEnded),
    Failed(YomiFailed),
    Reloaded(YomiReloaded),
    Loaded(YomiLoaded),
    TextsApplied(YomiTextsApplied),
    Audio(YomiAudioCue),
    Image(YomiImageCue),
    Screen(YomiScreenCue),
}

/// What an arrived resume or event is, for one runner in one frame.
#[derive(Default)]
pub(crate) struct Inbox {
    pub resumes: Vec<Input>,
    /// The events for this runner (`runner` was `None` or this one), in order: (name, data).
    pub events: Vec<(String, String)>,
}

fn fail(out: &mut Vec<Out>, runner: Entity, runner_state: &mut YomiRunner, error: String) {
    runner_state.wait = Wait::Failed;
    out.push(Out::Failed(YomiFailed { runner, error }));
}

/// Sends the cues the last `run` added, in the order written.
fn emit_cues(out: &mut Vec<Out>, runner: Entity, vm: &Vm<'static>) {
    for cue in vm.cue_order() {
        match cue.package {
            "audio" => {
                if let Some(c) = vm
                    .package_state::<Audio>()
                    .and_then(|s| s.cues.get(cue.index))
                {
                    out.push(Out::Audio(YomiAudioCue {
                        runner,
                        cue: c.clone(),
                    }));
                }
            }
            "image" => {
                if let Some(c) = vm
                    .package_state::<Image>()
                    .and_then(|s| s.cues.get(cue.index))
                {
                    out.push(Out::Image(YomiImageCue {
                        runner,
                        cue: c.clone(),
                    }));
                }
            }
            "screen" => {
                if let Some(c) = vm
                    .package_state::<Screen>()
                    .and_then(|s| s.cues.get(cue.index))
                {
                    out.push(Out::Screen(YomiScreenCue {
                        runner,
                        cue: c.clone(),
                    }));
                }
            }
            _ => {}
        }
    }
}

/// Sets the wait from a stop the VM is at. `emit` sends `YomiCommand` for a command that waits for the game.
/// Returns whether the runner can go on at once (a command that resumes with `()` is not resumed here).
fn enter_stop(
    shared: &Shared,
    out: &mut Vec<Out>,
    runner: Entity,
    state: &mut YomiRunner,
    stop: &Yield,
    emit: bool,
) {
    let Yield::Stop { command, args, id } = stop else {
        state.wait = Wait::Ended;
        return;
    };
    if Some(*command) == shared.flow_wait {
        let seconds = match args.first() {
            Some(Value::Float(s)) => *s,
            _ => 0.0,
        };
        state.wait = if seconds <= 0.0 {
            Wait::Running
        } else {
            Wait::Timer(seconds)
        };
        if state.wait == Wait::Running {
            resume_with(state, out, runner, Value::Unit);
        }
    } else if Some(*command) == shared.event_wait {
        let names = match args.first() {
            Some(Value::List(list)) => list.iter().map(|v| v.to_text()).collect(),
            _ => Vec::new(),
        };
        let remaining = match args.get(1) {
            Some(Value::Option(Some(v))) => match **v {
                Value::Float(s) => Some(s.max(0.0)),
                _ => None,
            },
            _ => None,
        };
        state.wait = Wait::Event { names, remaining };
    } else if shared.immediate.contains(command) {
        if emit {
            out.push(Out::Command(command_message(
                shared, runner, *command, args, id,
            )));
        }
        resume_with(state, out, runner, Value::Unit);
    } else {
        if emit {
            out.push(Out::Command(command_message(
                shared, runner, *command, args, id,
            )));
        }
        state.shown = Some(args.clone());
        state.wait = Wait::Command;
    }
}

fn command_message(
    shared: &Shared,
    runner: Entity,
    command: CommandId,
    args: &[Value],
    id: &Option<String>,
) -> YomiCommand {
    YomiCommand {
        runner,
        command: shared.registry.command(command).name.clone(),
        args: args.to_vec(),
        id: id.clone(),
    }
}

/// Resumes the VM and marks the runner as running. A failed resume stops the runner.
fn resume_with(state: &mut YomiRunner, out: &mut Vec<Out>, runner: Entity, value: Value) {
    let Some(vm) = state.vm.as_mut() else {
        return;
    };
    match vm.resume(Input::Return(value)) {
        Ok(()) => state.wait = Wait::Running,
        Err(e) => fail(out, runner, state, e.to_string()),
    }
}

/// Sets the table of the runner in a VM made by a reload or a load. A failure leaves the VM with the originals.
fn reapply_table(
    out: &mut Vec<Out>,
    runner: Entity,
    vm: &mut Vm<'static>,
    table: &mut Option<TextTable>,
) {
    if let Some(t) = table.clone()
        && let Err(issues) = vm.set_texts(Some(t))
    {
        *table = None;
        out.push(Out::TextsApplied(YomiTextsApplied {
            runner,
            result: Err(describe(&issues)),
        }));
    }
}

fn describe(issues: &[yomi_core::TextIssue]) -> String {
    issues
        .iter()
        .map(|i| format!("{i:?}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Rebuilds the current command with the table of the VM, and sends it again when its arguments changed.
fn resend_if_changed(out: &mut Vec<Out>, shared: &Shared, runner: Entity, state: &mut YomiRunner) {
    if state.wait != Wait::Command {
        return;
    }
    let Some(vm) = state.vm.as_mut() else {
        return;
    };
    if let Ok(Yield::Stop { command, args, id }) = vm.refresh_current()
        && state.shown.as_ref() != Some(&args)
    {
        out.push(Out::Command(command_message(
            shared, runner, command, &args, &id,
        )));
        state.shown = Some(args);
    }
}

/// Sets a requested table. The previous table stays when it cannot be set.
fn apply_request(
    shared: &Shared,
    out: &mut Vec<Out>,
    runner: Entity,
    state: &mut YomiRunner,
    table: Option<TextTable>,
) {
    let Some(vm) = state.vm.as_mut() else {
        return;
    };
    let result = match vm.set_texts(table.clone()) {
        Ok(()) => {
            state.texts_table = table;
            Ok(())
        }
        Err(issues) => Err(describe(&issues)),
    };
    if result.is_ok() {
        resend_if_changed(out, shared, runner, state);
    }
    out.push(Out::TextsApplied(YomiTextsApplied { runner, result }));
}

fn config(state: &YomiRunner, shared: &Shared) -> VmConfig {
    VmConfig::default()
        .start(&state.start)
        .seed(state.seed)
        .load_policy(shared.policy)
}

/// Runs the script until it waits for something, or the cap of this frame is reached.
fn run_until_wait(shared: &Shared, out: &mut Vec<Out>, runner: Entity, state: &mut YomiRunner) {
    let mut steps = 0;
    while state.wait == Wait::Running {
        if steps >= shared.max_steps_per_frame {
            return;
        }
        steps += 1;
        let Some(vm) = state.vm.as_mut() else {
            return;
        };
        match vm.run() {
            Ok(y) => {
                if let Some(vm) = state.vm.as_ref() {
                    emit_cues(out, runner, vm);
                }
                match y {
                    Yield::End => {
                        state.wait = Wait::Ended;
                        out.push(Out::Ended(YomiEnded { runner }));
                    }
                    stop => enter_stop(shared, out, runner, state, &stop, true),
                }
            }
            Err(e) => fail(out, runner, state, e.to_string()),
        }
    }
}

/// Advances one runner by one frame: starts, reloads, loads, delivers what arrived, and runs.
pub(crate) fn step(
    shared: &Shared,
    out: &mut Vec<Out>,
    runner: Entity,
    state: &mut YomiRunner,
    inbox: Inbox,
    dt: f64,
) {
    let Some(slot) = shared.programs.get(&state.program_name) else {
        if !matches!(state.wait, Wait::Failed | Wait::Ended) {
            let error = if state.program_name.is_empty() {
                "the default program has no scripts".to_string()
            } else {
                format!("unknown program `{}`", state.program_name)
            };
            fail(out, runner, state, error);
        }
        return;
    };
    let Some(program) = slot.program else {
        return;
    };
    if state.restart {
        state.restart = false;
        state.vm = None;
        state.wait = Wait::Starting;
    }
    // A new program: replace the one of a runner that is stopped.
    if state.generation != slot.generation {
        state.generation = slot.generation;
        let stopped = matches!(
            state.wait,
            Wait::Command | Wait::Timer(_) | Wait::Event { .. }
        );
        if let (Some(vm), true) = (state.vm.as_ref(), stopped) {
            let result = match vm.reload(program) {
                Ok((mut new, _)) => {
                    reapply_table(out, runner, &mut new, &mut state.texts_table);
                    state.vm = Some(new);
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            };
            if result.is_ok() && state.wait != Wait::Command {
                // The command stays as it is; a timer or an event wait starts counting again
                let stop = state.vm.as_ref().and_then(|v| v.current()).cloned();
                if let Some(stop) = stop {
                    enter_stop(shared, out, runner, state, &stop, false);
                }
            }
            if result.is_ok() {
                resend_if_changed(out, shared, runner, state);
            }
            out.push(Out::Reloaded(YomiReloaded { runner, result }));
        }
    }
    if let Some(bytes) = state.pending_load.take() {
        let persistent = state.vm.as_ref().map(|v| v.save_persistent());
        let mut cfg = config(state, shared);
        if let Some(p) = persistent {
            cfg = cfg.persistent_data(p);
        }
        match Vm::load(program, shared.registry, cfg, &bytes) {
            Ok((mut vm, _)) => {
                reapply_table(out, runner, &mut vm, &mut state.texts_table);
                // The stop of a load is made without the table
                let stop = vm.refresh_current().ok().or_else(|| vm.current().cloned());
                state.vm = Some(vm);
                out.push(Out::Loaded(YomiLoaded {
                    runner,
                    result: Ok(()),
                }));
                if let Some(stop) = stop {
                    enter_stop(shared, out, runner, state, &stop, true);
                } else {
                    state.wait = Wait::Running;
                }
            }
            Err(e) => out.push(Out::Loaded(YomiLoaded {
                runner,
                result: Err(e.to_string()),
            })),
        }
    }
    if state.wait == Wait::Starting {
        match Vm::new(program, shared.registry, config(state, shared)) {
            Ok(vm) => {
                state.vm = Some(vm);
                state.wait = Wait::Running;
            }
            Err(e) => fail(out, runner, state, e.to_string()),
        }
    }
    if state.vm.is_some()
        && let Some(table) = state.texts_request.take()
    {
        apply_request(shared, out, runner, state, table);
    }
    match state.wait.clone() {
        Wait::Command => {
            if let Some(Input::Return(v)) = inbox.resumes.into_iter().next() {
                resume_with(state, out, runner, v);
            }
        }
        Wait::Timer(left) => {
            let left = left - dt;
            if left <= 0.0 {
                resume_with(state, out, runner, Value::Unit);
            } else {
                state.wait = Wait::Timer(left);
            }
        }
        Wait::Event { names, remaining } => {
            if let Some((name, data)) = inbox.events.into_iter().find(|(n, _)| names.contains(n)) {
                resume_with(
                    state,
                    out,
                    runner,
                    Outcome::Arrived(name, data).into_value(),
                );
            } else if let Some(left) = remaining {
                let left = left - dt;
                if left <= 0.0 {
                    resume_with(state, out, runner, Outcome::TimedOut.into_value());
                } else {
                    state.wait = Wait::Event {
                        names,
                        remaining: Some(left),
                    };
                }
            }
        }
        _ => {}
    }
    run_until_wait(shared, out, runner, state);
}
