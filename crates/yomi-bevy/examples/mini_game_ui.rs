//! Plays `examples/mini-game` in a window with `bevy_ui`.
//!
//! `cargo run --example mini_game_ui --features window` (in `crates/yomi-bevy`)
//!
//! Space or Enter advances a line; at a choice, press the number. F5 saves in memory, F9 loads it back, F2 switches between Japanese and English (`en.texts`).
//! Text input and confirmations are answered with their defaults (there is no text field here).
//! The pictures and sounds are drawn from the state of the `image` and `audio` packages, not from the cues.

use bevy::prelude::*;
use yomi_bevy::{
    YomiCommand, YomiDiagnostics, YomiEnded, YomiFailed, YomiLoaded, YomiPlugin, YomiResume,
    YomiRunner, YomiSet, YomiSetTexts, YomiTexts, YomiTextsApplied,
};
use yomi_core::{Identity, Sugar, Value};
use yomi_std::{audio::Audio, image::Image};

const FONT_FAMILIES: &str = "Yu Gothic, Meiryo, Hiragino Sans, Noto Sans CJK JP, sans-serif";
const VISIBLE_LINES: usize = 8;

#[derive(Component)]
struct ScreenText;

enum Waiting {
    Nothing,
    Advance(Entity),
    Choice(Entity, Vec<String>),
}

#[derive(Resource)]
struct Screen {
    lines: Vec<String>,
    waiting: Waiting,
    ended: bool,
    saved: Option<Vec<u8>>,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            waiting: Waiting::Nothing,
            ended: false,
            saved: None,
        }
    }
}

/// The English table, and whether it is the one in use.
#[derive(Resource)]
struct English {
    texts: Handle<YomiTexts>,
    on: bool,
}

fn setup(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(English {
        texts: server.load("en.texts"),
        on: false,
    });
    commands.spawn(Camera2d);
    commands.spawn(YomiRunner::new().start("main.start").seed(7));
    commands.spawn((
        ScreenText,
        Text::new(""),
        TextFont {
            font: FontSource::families(FONT_FAMILIES),
            font_size: FontSize::Px(24.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(32),
            top: px(24),
            right: px(32),
            ..default()
        },
    ));
}

fn on_commands(
    mut commands: MessageReader<YomiCommand>,
    mut loaded: MessageReader<YomiLoaded>,
    mut ended: MessageReader<YomiEnded>,
    mut failed: MessageReader<YomiFailed>,
    mut diagnostics: MessageReader<YomiDiagnostics>,
    mut applied: MessageReader<YomiTextsApplied>,
    mut resume: MessageWriter<YomiResume>,
    mut screen: ResMut<Screen>,
) {
    for l in loaded.read() {
        // The plugin sends the waiting command again after a load
        screen.waiting = Waiting::Nothing;
        screen.ended = false;
        screen.lines.push(match &l.result {
            Ok(()) => "[load] ok".to_string(),
            Err(reason) => format!("[load] failed: {reason}"),
        });
    }
    for c in commands.read() {
        match (c.command.as_str(), c.args.as_slice()) {
            ("ui.say", [Value::Str(text)]) => {
                show_line(&mut screen, c.runner, text.to_string());
            }
            ("ui.say_as", [who, Value::Str(text)]) => {
                // The body already holds its own quotation marks
                show_line(&mut screen, c.runner, format!("{}{}", speaker(who), text));
            }
            ("ui.choose", [Value::List(texts), _ids]) => {
                let options = texts.iter().map(Value::to_text).collect();
                screen.waiting = Waiting::Choice(c.runner, options);
            }
            ("input.text", [prompt, Value::Str(default), _]) => {
                screen
                    .lines
                    .push(format!("({} → {})", prompt.to_text(), default));
                resume.write(YomiResume::with(c.runner, Value::Str(default.clone())));
            }
            ("input.confirm", [prompt]) => {
                screen.lines.push(format!("({} → はい)", prompt.to_text()));
                resume.write(YomiResume::with(c.runner, Value::Bool(true)));
            }
            ("input.integer", [prompt, Value::Int(default), ..]) => {
                screen
                    .lines
                    .push(format!("({} → {})", prompt.to_text(), default));
                resume.write(YomiResume::with(c.runner, Value::Int(*default)));
            }
            _ => {}
        }
    }
    for a in applied.read() {
        if let Err(reason) = &a.result {
            screen.lines.push(format!("[texts] failed: {reason}"));
        }
    }
    if ended.read().next().is_some() {
        screen.ended = true;
    }
    for f in failed.read() {
        screen.lines.push(format!("[error] {}", f.error));
    }
    for d in diagnostics.read() {
        screen.lines.push(format!(
            "[compile error] {} diagnostic(s)",
            d.diagnostics.len()
        ));
    }
}

/// A line the runner is already waiting on comes again with another text (the language changed): replace it.
fn show_line(screen: &mut Screen, runner: Entity, line: String) {
    match (&screen.waiting, screen.lines.last_mut()) {
        (Waiting::Advance(e), Some(last)) if *e == runner => *last = line,
        _ => screen.lines.push(line),
    }
    screen.waiting = Waiting::Advance(runner);
}

fn speaker(who: &Value) -> String {
    match who {
        Value::Option(Some(inner)) => match &**inner {
            Value::Identity(identity) => identity.name.clone(),
            other => other.to_text(),
        },
        _ => String::new(),
    }
}

fn on_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<Screen>,
    mut resume: MessageWriter<YomiResume>,
    mut runners: Query<(Entity, &mut YomiRunner)>,
    mut english: ResMut<English>,
    mut set_texts: MessageWriter<YomiSetTexts>,
) {
    match &screen.waiting {
        Waiting::Advance(runner) => {
            if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) {
                resume.write(YomiResume::unit(*runner));
                screen.waiting = Waiting::Nothing;
            }
        }
        Waiting::Choice(runner, options) => {
            let digits = [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
            ];
            let picked = digits
                .iter()
                .take(options.len())
                .position(|k| keys.just_pressed(*k));
            if let Some(index) = picked {
                let line = format!("> {}", options[index]);
                resume.write(YomiResume::with(*runner, Value::Int(index as i64)));
                screen.waiting = Waiting::Nothing;
                screen.lines.push(line);
            }
        }
        Waiting::Nothing => {}
    }
    for (entity, mut runner) in &mut runners {
        if keys.just_pressed(KeyCode::F2) {
            english.on = !english.on;
            set_texts.write(YomiSetTexts {
                runner: entity,
                texts: english.on.then(|| english.texts.clone()),
            });
        }
        if keys.just_pressed(KeyCode::F5) {
            match runner.save() {
                Ok(bytes) => {
                    screen.lines.push(format!("[save] {} bytes", bytes.len()));
                    screen.saved = Some(bytes);
                }
                Err(reason) => screen.lines.push(format!("[save] failed: {reason}")),
            }
        }
        if keys.just_pressed(KeyCode::F9) {
            match screen.saved.clone() {
                Some(bytes) => runner.load(bytes),
                None => screen.lines.push("[load] nothing saved".to_string()),
            }
        }
    }
}

fn render(
    screen: Res<Screen>,
    runners: Query<&YomiRunner>,
    mut text: Single<&mut Text, With<ScreenText>>,
) {
    let mut out = String::new();
    if let Some(vm) = runners.iter().next().and_then(YomiRunner::vm) {
        if let Some(audio) = vm.package_state::<Audio>() {
            for c in &audio.channels {
                out.push_str(&format!("♪ {}: {}  ", c.channel, c.name));
            }
        }
        if let Some(image) = vm.package_state::<Image>() {
            for i in &image.items {
                out.push_str(&format!("[{}: {}] ", i.slot, i.name));
            }
        }
    }
    out.push_str("\n\n");
    let start = screen.lines.len().saturating_sub(VISIBLE_LINES);
    out.push_str(&screen.lines[start..].join("\n"));
    match &screen.waiting {
        Waiting::Choice(_, options) => {
            for (i, option) in options.iter().enumerate() {
                out.push_str(&format!("\n[{}] {}", i + 1, option));
            }
        }
        Waiting::Advance(_) => out.push_str("\n▼"),
        Waiting::Nothing => {}
    }
    if screen.ended {
        out.push_str("\n(終わり)");
    }
    out.push_str("\n\nSpace/Enter: 次へ  数字: 選ぶ  F2: 日本語/English  F5: セーブ  F9: ロード");
    // Setting it every frame would lay the text out again every frame
    if text.0 != out {
        text.0 = out;
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "../../examples/mini-game".into(),
            watch_for_changes_override: Some(true),
            ..default()
        }))
        .add_plugins(
            YomiPlugin::new(|reg| {
                reg.stop::<(String,), ()>("ui.say")?;
                reg.stop::<(Option<Identity>, String), ()>("ui.say_as")?;
                reg.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")?;
                reg.bind(Sugar::Plain, "ui.say")?;
                reg.bind(Sugar::Quote, "ui.say_as")?;
                reg.bind(Sugar::Choice, "ui.choose")?;
                Ok(())
            })
            .script("chara.yomi")
            .script("shop.yomi")
            .script("game.yomi")
            .script("ending.yomi")
            .script("main.yomi"),
        )
        .init_resource::<Screen>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (on_commands, on_keys, render).chain().after(YomiSet),
        )
        .run();
}
