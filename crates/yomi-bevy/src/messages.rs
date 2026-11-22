//! The messages exchanged between scripts and the game.

use bevy_asset::Handle;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;
use yomi_core::{Diagnostic, Input, Value};
use yomi_std::audio::AudioCue;
use yomi_std::image::ImageCue;
use yomi_std::screen::ScreenCue;

/// A stop command the script called (script → game). The script waits until the game writes a [`YomiResume`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct YomiCommand {
    /// The runner that called the command.
    pub runner: Entity,
    /// The command name, such as `ui.say`.
    pub command: String,
    /// The evaluated arguments.
    pub args: Vec<Value>,
    /// The stable ID at the end of the line.
    pub id: Option<String>,
}

/// Resumes a runner that waits for a command (game → script).
#[derive(Message, Debug, Clone, PartialEq)]
pub struct YomiResume {
    /// The runner to resume.
    pub runner: Entity,
    /// The return value of the command.
    pub input: Input,
}

impl YomiResume {
    /// Resumes with the unit value, for commands that return nothing.
    pub fn unit(runner: Entity) -> Self {
        Self {
            runner,
            input: Input::Return(Value::Unit),
        }
    }

    /// Resumes with a return value.
    pub fn with(runner: Entity, value: Value) -> Self {
        Self {
            runner,
            input: Input::Return(value),
        }
    }
}

/// An external event for `event.wait` (game → script).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiEvent {
    /// The runner to deliver to. `None` delivers to every runner that waits for `name`.
    pub runner: Option<Entity>,
    /// The event name.
    pub name: String,
    /// The event data (the form is up to the game).
    pub data: String,
}

/// The script reached `end`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiEnded {
    /// The runner that ended.
    pub runner: Entity,
}

/// The script stopped with an error.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiFailed {
    /// The runner that failed.
    pub runner: Entity,
    /// The message of the error.
    pub error: String,
}

/// Diagnostics of a failed compilation of the scripts.
#[derive(Message, Debug, Clone)]
pub struct YomiDiagnostics {
    /// The program that failed to compile (empty for the default program).
    pub program: String,
    /// The diagnostics, in the order of the modules.
    pub diagnostics: Vec<Diagnostic>,
}

/// The result of replacing the program of a running script.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiReloaded {
    /// The runner.
    pub runner: Entity,
    /// `Err` holds the reason; the runner goes on with the previous program.
    pub result: Result<(), String>,
}

/// The result of [`YomiRunner::load`](crate::YomiRunner::load).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiLoaded {
    /// The runner.
    pub runner: Entity,
    /// `Err` holds the reason; the runner is unchanged.
    pub result: Result<(), String>,
}

/// Switches the table of replacement bodies of a runner (game → script).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiSetTexts {
    /// The runner.
    pub runner: Entity,
    /// The table to use; `None` goes back to the original bodies.
    pub texts: Option<Handle<crate::YomiTexts>>,
}

/// The result of setting a table of replacement bodies (plugin → game).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct YomiTextsApplied {
    /// The runner.
    pub runner: Entity,
    /// `Err` holds the reason; the runner keeps the previous table.
    pub result: Result<(), String>,
}

/// A cue of the `audio` package, in the order written in the script.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct YomiAudioCue {
    /// The runner.
    pub runner: Entity,
    /// The cue.
    pub cue: AudioCue,
}

/// A cue of the `image` package, in the order written in the script.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct YomiImageCue {
    /// The runner.
    pub runner: Entity,
    /// The cue.
    pub cue: ImageCue,
}

/// A cue of the `screen` package, in the order written in the script.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct YomiScreenCue {
    /// The runner.
    pub runner: Entity,
    /// The cue.
    pub cue: ScreenCue,
}
