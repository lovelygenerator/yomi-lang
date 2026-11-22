//! Runs `assets/dialogue.yomi` without a window: prints the lines, answers the choice, and exits at the end.
//!
//! `cargo run --example dialogue` (in `crates/yomi-bevy`)

use std::time::Duration;

use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_time::TimePlugin;
use yomi_bevy::{YomiCommand, YomiEnded, YomiPlugin, YomiResume, YomiRunner, YomiSet};
use yomi_core::{Sugar, Value};

fn game(
    mut commands: MessageReader<YomiCommand>,
    mut resume: MessageWriter<YomiResume>,
    mut ended: MessageReader<YomiEnded>,
    mut exit: MessageWriter<AppExit>,
) {
    for c in commands.read() {
        match (c.command.as_str(), c.args.as_slice()) {
            ("ui.say", [Value::Str(text)]) => println!("{text}"),
            ("game.choose", args) => {
                let first = args.first().map(Value::to_text).unwrap_or_default();
                println!("[choice] {args:?} -> 0 ({first})");
                resume.write(YomiResume::with(c.runner, Value::Int(0)));
            }
            _ => {}
        }
    }
    if ended.read().next().is_some() {
        exit.write(AppExit::Success);
    }
}

fn main() {
    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            AssetPlugin::default(),
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)),
        ))
        .add_plugins(
            YomiPlugin::new(|reg| {
                reg.stop::<(String,), ()>("ui.say")?;
                reg.stop::<(String, String), i64>("game.choose")?;
                reg.bind(Sugar::Plain, "ui.say")?;
                Ok(())
            })
            .script("dialogue.yomi")
            .immediate(["ui.say"]),
        )
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(YomiRunner::new());
        })
        .add_systems(Update, game.after(YomiSet))
        .run();
}
