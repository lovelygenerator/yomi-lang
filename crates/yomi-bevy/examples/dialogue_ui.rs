//! Shows `assets/dialogue.yomi` in a window with `bevy_ui`.
//!
//! `cargo run --example dialogue_ui --features window` (in `crates/yomi-bevy`)
//!
//! Lines appear as they arrive. At a choice, press `1` or `2`. Edit and save `assets/dialogue.yomi`
//! while it runs: the script is recompiled and swapped in place, and the result is logged on the screen. Japanese text needs a system font
//! (the list in `FONT_FAMILIES`); edit it if none of them is installed.

use bevy::prelude::*;
use yomi_bevy::{
    YomiCommand, YomiDiagnostics, YomiEnded, YomiPlugin, YomiReloaded, YomiResume, YomiRunner,
    YomiSet,
};
use yomi_core::{Sugar, Value};

const FONT_FAMILIES: &str = "Yu Gothic, Meiryo, Hiragino Sans, Noto Sans CJK JP, sans-serif";
const VISIBLE_LINES: usize = 10;

#[derive(Component)]
struct LogText;

#[derive(Resource, Default)]
struct Screen {
    lines: Vec<String>,
    choice: Option<Choice>,
    ended: bool,
}

struct Choice {
    runner: Entity,
    options: Vec<String>,
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn(YomiRunner::new());
    commands.spawn((
        LogText,
        Text::new(""),
        TextFont {
            font: FontSource::families(FONT_FAMILIES),
            font_size: FontSize::Px(28.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(32),
            top: px(32),
            right: px(32),
            ..default()
        },
    ));
}

fn on_commands(
    mut commands: MessageReader<YomiCommand>,
    mut ended: MessageReader<YomiEnded>,
    mut reloaded: MessageReader<YomiReloaded>,
    mut diagnostics: MessageReader<YomiDiagnostics>,
    mut screen: ResMut<Screen>,
) {
    for r in reloaded.read() {
        match &r.result {
            Ok(()) => screen.lines.push("[reload] ok".into()),
            Err(reason) => screen
                .lines
                .push(format!("[reload] kept the old script: {reason}")),
        }
    }
    for d in diagnostics.read() {
        screen.lines.push(format!(
            "[compile error] {} diagnostic(s)",
            d.diagnostics.len()
        ));
    }
    for c in commands.read() {
        match (c.command.as_str(), c.args.as_slice()) {
            ("ui.say", [Value::Str(text)]) => screen.lines.push(text.to_string()),
            ("game.choose", args) => {
                screen.choice = Some(Choice {
                    runner: c.runner,
                    options: args.iter().map(Value::to_text).collect(),
                });
            }
            _ => {}
        }
    }
    if ended.read().next().is_some() {
        screen.ended = true;
    }
}

fn on_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<Screen>,
    mut resume: MessageWriter<YomiResume>,
) {
    let Some(choice) = &screen.choice else { return };
    let picked = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ]
    .iter()
    .take(choice.options.len())
    .position(|k| keys.just_pressed(*k));
    if let Some(index) = picked {
        let line = format!("> {}", choice.options[index]);
        resume.write(YomiResume::with(choice.runner, Value::Int(index as i64)));
        screen.choice = None;
        screen.lines.push(line);
    }
}

fn render(screen: Res<Screen>, mut text: Single<&mut Text, With<LogText>>) {
    if !screen.is_changed() {
        return;
    }
    let start = screen.lines.len().saturating_sub(VISIBLE_LINES);
    let mut out = screen.lines[start..].join("\n");
    if let Some(choice) = &screen.choice {
        for (i, option) in choice.options.iter().enumerate() {
            out.push_str(&format!("\n[{}] {}", i + 1, option));
        }
    }
    if screen.ended {
        out.push_str("\n(終わり)");
    }
    if text.0 != out {
        text.0 = out;
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            watch_for_changes_override: Some(true),
            ..default()
        }))
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
        .init_resource::<Screen>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (on_commands, on_keys, render).chain().after(YomiSet),
        )
        .run();
}
