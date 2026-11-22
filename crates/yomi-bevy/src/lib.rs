//! Runs yomi scripts in a Bevy app.
//!
//! A script runs in a [`YomiRunner`] component. The script talks to the game with messages: a stop command comes as
//! [`YomiCommand`], and the game answers with [`YomiResume`]. Scripts are Bevy assets (`.yomi` files), and a change of a file
//! replaces the program of the running scripts.
//!
//! ```ignore
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(
//!         YomiPlugin::new(|reg: &mut Registry| {
//!             reg.stop::<(String,), ()>("ui.say")?;
//!             reg.bind(Sugar::Plain, "ui.say")?;
//!             Ok(())
//!         })
//!         .script("scripts/main.yomi")
//!         .immediate(["ui.say"]),
//!     );
//! ```

mod asset;
mod messages;
mod runner;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_asset::{AssetApp, AssetEvent, AssetServer, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_time::{Time, Virtual};
use yomi_core::{LoadPolicy, Registry, RegistryError, Ty, compile_modules};

pub use asset::{YomiScript, YomiScriptLoader, YomiTexts, YomiTextsLoader};
pub use messages::{
    YomiAudioCue, YomiCommand, YomiDiagnostics, YomiEnded, YomiEvent, YomiFailed, YomiImageCue,
    YomiLoaded, YomiReloaded, YomiResume, YomiScreenCue, YomiSetTexts, YomiTextsApplied,
};
pub use runner::YomiRunner;
use runner::{Inbox, Out, Shared, Slot};

/// The system set that runs the scripts. Order the game's systems after it to receive the messages in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct YomiSet;

type Setup = Box<dyn FnOnce(&mut Registry) -> Result<(), RegistryError> + Send + Sync>;

/// The plugin. Needs `AssetPlugin` (in `DefaultPlugins`) and `TimePlugin`.
pub struct YomiPlugin {
    setup: Mutex<Option<Setup>>,
    /// The programs by name; the default one (`""`) is made by `.script`.
    programs: Vec<(String, Vec<String>)>,
    immediate: Vec<String>,
    with_std: bool,
    policy: LoadPolicy,
    max_steps_per_frame: usize,
}

impl YomiPlugin {
    /// `setup` registers the commands, types and bindings of the game. It runs after the standard packages are registered.
    ///
    /// If it fails, building the plugin panics.
    pub fn new(
        setup: impl FnOnce(&mut Registry) -> Result<(), RegistryError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            setup: Mutex::new(Some(Box::new(setup))),
            programs: Vec::new(),
            immediate: Vec::new(),
            with_std: true,
            policy: LoadPolicy::default(),
            max_steps_per_frame: 1000,
        }
    }

    /// Adds a script file (an asset path) to the default program. The module name is the file name without the
    /// extension.
    pub fn script(mut self, path: &str) -> Self {
        self.program_entry("").push(path.to_string());
        self
    }

    /// Adds a named program: a set of script files compiled together, apart from the other programs. A runner uses it
    /// by [`YomiRunner::program`]. The registry and [`immediate`](Self::immediate) are shared by every program.
    ///
    /// Building the plugin panics if `name` is empty or already used.
    pub fn program<'a>(mut self, name: &str, paths: impl IntoIterator<Item = &'a str>) -> Self {
        assert!(!name.is_empty(), "the name of a program is empty");
        assert!(
            !self.programs.iter().any(|(n, _)| n == name),
            "the program `{name}` is defined twice"
        );
        self.programs.push((
            name.to_string(),
            paths.into_iter().map(str::to_string).collect(),
        ));
        self
    }

    fn program_entry(&mut self, name: &str) -> &mut Vec<String> {
        let at = match self.programs.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                self.programs.push((name.to_string(), Vec::new()));
                self.programs.len() - 1
            }
        };
        &mut self.programs[at].1
    }

    /// Names the stop commands that resume at once with `()` instead of waiting for [`YomiResume`].
    ///
    /// A command that returns a value cannot be named; building the plugin panics.
    pub fn immediate<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.immediate.extend(names.into_iter().map(str::to_string));
        self
    }

    /// Leaves the standard packages (`yomi-std`) out of the registry.
    pub fn without_std(mut self) -> Self {
        self.with_std = false;
        self
    }

    /// Sets how loads treat a change of the variables (default: strict).
    pub fn load_policy(mut self, policy: LoadPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Sets how many times a runner may go on without stopping in one frame (default 1000).
    pub fn max_steps_per_frame(mut self, steps: usize) -> Self {
        self.max_steps_per_frame = steps.max(1);
        self
    }
}

/// The registry and the program, shared by every runner.
#[derive(Resource)]
struct YomiShared(Shared);

/// The script files of the programs and their sources.
#[derive(Resource, Default)]
struct YomiProject {
    programs: Vec<ProjectProgram>,
}

#[derive(Default)]
struct ProjectProgram {
    name: String,
    paths: Vec<String>,
    handles: Vec<(String, Handle<YomiScript>)>,
    modules: Vec<(String, String)>,
    dirty: bool,
}

impl Plugin for YomiPlugin {
    fn build(&self, app: &mut App) {
        let mut registry = Registry::new();
        if self.with_std {
            yomi_std::register_all(&mut registry).expect("the standard packages register");
        }
        let setup = self.setup.lock().expect("the setup is not poisoned").take();
        if let Some(setup) = setup {
            setup(&mut registry).expect("the setup of yomi commands failed");
        }
        let mut immediate = HashSet::new();
        for name in &self.immediate {
            let id = registry
                .command_id(name)
                .unwrap_or_else(|| panic!("`{name}` is not a stop command"));
            if registry.command(id).ret != Ty::Unit {
                panic!("`{name}` returns a value, so it cannot be immediate");
            }
            immediate.insert(id);
        }
        let flow_wait = registry.command_id("flow.wait");
        let event_wait = registry.command_id("event.wait");
        // The VM borrows the registry, and lives in a component
        let registry: &'static Registry = Box::leak(Box::new(registry));
        app.insert_resource(YomiShared(Shared {
            registry,
            programs: self
                .programs
                .iter()
                .filter(|(_, paths)| !paths.is_empty())
                .map(|(name, _)| (name.clone(), Slot::default()))
                .collect::<HashMap<_, _>>(),
            immediate,
            flow_wait,
            event_wait,
            policy: self.policy,
            max_steps_per_frame: self.max_steps_per_frame,
        }))
        .insert_resource(YomiProject {
            programs: self
                .programs
                .iter()
                .filter(|(_, paths)| !paths.is_empty())
                .map(|(name, paths)| ProjectProgram {
                    name: name.clone(),
                    paths: paths.clone(),
                    ..Default::default()
                })
                .collect(),
        })
        .init_asset::<YomiScript>()
        .register_asset_loader(YomiScriptLoader)
        .init_asset::<YomiTexts>()
        .register_asset_loader(YomiTextsLoader)
        .add_message::<YomiSetTexts>()
        .add_message::<YomiTextsApplied>()
        .add_message::<YomiCommand>()
        .add_message::<YomiResume>()
        .add_message::<YomiEvent>()
        .add_message::<YomiEnded>()
        .add_message::<YomiFailed>()
        .add_message::<YomiDiagnostics>()
        .add_message::<YomiReloaded>()
        .add_message::<YomiLoaded>()
        .add_message::<YomiAudioCue>()
        .add_message::<YomiImageCue>()
        .add_message::<YomiScreenCue>()
        .add_systems(Startup, load_scripts)
        .add_systems(
            Update,
            (collect_scripts, compile_project, update_texts, run_runners)
                .chain()
                .in_set(YomiSet),
        );
    }
}

/// The module name of a script path: the file name without the extension.
fn module_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .to_string()
}

fn load_scripts(mut project: ResMut<YomiProject>, server: Res<AssetServer>) {
    for program in &mut project.programs {
        program.handles = program
            .paths
            .iter()
            .map(|p| (module_name(p), server.load::<YomiScript>(p.clone())))
            .collect();
    }
}

/// Gathers the sources of each program when all its files are loaded, and marks it dirty when they changed.
fn collect_scripts(
    mut project: ResMut<YomiProject>,
    assets: Res<Assets<YomiScript>>,
    mut events: MessageReader<AssetEvent<YomiScript>>,
) {
    if events.read().count() == 0 {
        return;
    }
    for program in &mut project.programs {
        if program.handles.is_empty() {
            continue;
        }
        let modules: Option<Vec<(String, String)>> = program
            .handles
            .iter()
            .map(|(name, h)| assets.get(h).map(|s| (name.clone(), s.source.clone())))
            .collect();
        if let Some(modules) = modules
            && modules != program.modules
        {
            program.modules = modules;
            program.dirty = true;
        }
    }
}

fn compile_project(
    mut shared: ResMut<YomiShared>,
    mut project: ResMut<YomiProject>,
    mut diagnostics: MessageWriter<YomiDiagnostics>,
) {
    for program in &mut project.programs {
        if !program.dirty {
            continue;
        }
        program.dirty = false;
        let modules: Vec<(&str, &str)> = program
            .modules
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        match compile_modules(&modules, shared.0.registry) {
            Ok(compiled) => {
                if let Some(slot) = shared.0.programs.get_mut(&program.name) {
                    // The VMs borrow the program, and a replaced one stays until the app ends
                    slot.program = Some(Box::leak(Box::new(compiled)));
                    slot.generation += 1;
                }
            }
            Err(d) => {
                diagnostics.write(YomiDiagnostics {
                    program: program.name.clone(),
                    diagnostics: d,
                });
            }
        }
    }
}

/// Turns `YomiSetTexts` and changes of the table files into requests the runners handle in their next step.
fn update_texts(
    assets: Res<Assets<YomiTexts>>,
    mut runners: Query<(Entity, &mut YomiRunner)>,
    mut set: MessageReader<YomiSetTexts>,
    mut events: MessageReader<AssetEvent<YomiTexts>>,
) {
    let changed: Vec<_> = events
        .read()
        .filter_map(|e| match e {
            AssetEvent::Added { id }
            | AssetEvent::Modified { id }
            | AssetEvent::LoadedWithDependencies { id } => Some(*id),
            _ => None,
        })
        .collect();
    let set: Vec<YomiSetTexts> = set.read().cloned().collect();
    for (entity, mut runner) in &mut runners {
        for m in set.iter().filter(|m| m.runner == entity) {
            runner.set_texts_handle(m.texts.clone());
        }
        if runner
            .texts_handle()
            .is_some_and(|h| changed.contains(&h.id()))
        {
            runner.mark_texts_waiting();
        }
        runner.resolve_texts(&assets);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_runners(
    shared: Res<YomiShared>,
    time: Res<Time<Virtual>>,
    mut runners: Query<(Entity, &mut YomiRunner)>,
    mut resumes: MessageReader<YomiResume>,
    mut events: MessageReader<YomiEvent>,
    mut commands: MessageWriter<YomiCommand>,
    mut ended: MessageWriter<YomiEnded>,
    mut failed: MessageWriter<YomiFailed>,
    mut reloaded: MessageWriter<YomiReloaded>,
    mut loaded: MessageWriter<YomiLoaded>,
    mut applied: MessageWriter<YomiTextsApplied>,
    mut audio: MessageWriter<YomiAudioCue>,
    mut image: MessageWriter<YomiImageCue>,
    mut screen: MessageWriter<YomiScreenCue>,
) {
    let resumes: Vec<YomiResume> = resumes.read().cloned().collect();
    let events: Vec<YomiEvent> = events.read().cloned().collect();
    let dt = time.delta_secs_f64();
    let mut out = Vec::new();
    for (entity, mut runner) in &mut runners {
        let inbox = Inbox {
            resumes: resumes
                .iter()
                .filter(|r| r.runner == entity)
                .map(|r| r.input.clone())
                .collect(),
            events: events
                .iter()
                .filter(|e| e.runner.is_none_or(|r| r == entity))
                .map(|e| (e.name.clone(), e.data.clone()))
                .collect(),
        };
        runner::step(&shared.0, &mut out, entity, &mut runner, inbox, dt);
    }
    for o in out {
        match o {
            Out::Command(m) => {
                commands.write(m);
            }
            Out::Ended(m) => {
                ended.write(m);
            }
            Out::Failed(m) => {
                failed.write(m);
            }
            Out::Reloaded(m) => {
                reloaded.write(m);
            }
            Out::Loaded(m) => {
                loaded.write(m);
            }
            Out::TextsApplied(m) => {
                applied.write(m);
            }
            Out::Audio(m) => {
                audio.write(m);
            }
            Out::Image(m) => {
                image.write(m);
            }
            Out::Screen(m) => {
                screen.write(m);
            }
        }
    }
}

#[cfg(test)]
mod tests;
