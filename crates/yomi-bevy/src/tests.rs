use std::time::Duration;

use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_time::{Time, Virtual};
use yomi_core::{Sugar, Value};

use super::*;

/// Everything the game side received, in order.
#[derive(Resource, Default)]
struct Seen {
    commands: Vec<YomiCommand>,
    ended: Vec<Entity>,
    failed: Vec<String>,
    diagnostics: usize,
    diagnostic_programs: Vec<String>,
    reloaded: Vec<Result<(), String>>,
    loaded: Vec<Result<(), String>>,
    audio: usize,
}

#[allow(clippy::too_many_arguments)]
fn collect(
    mut seen: ResMut<Seen>,
    mut commands: MessageReader<YomiCommand>,
    mut ended: MessageReader<YomiEnded>,
    mut failed: MessageReader<YomiFailed>,
    mut diagnostics: MessageReader<YomiDiagnostics>,
    mut reloaded: MessageReader<YomiReloaded>,
    mut loaded: MessageReader<YomiLoaded>,
    mut audio: MessageReader<YomiAudioCue>,
) {
    seen.commands.extend(commands.read().cloned());
    seen.ended.extend(ended.read().map(|e| e.runner));
    seen.failed.extend(failed.read().map(|e| e.error.clone()));
    for d in diagnostics.read() {
        seen.diagnostics += 1;
        seen.diagnostic_programs.push(d.program.clone());
    }
    seen.reloaded
        .extend(reloaded.read().map(|e| e.result.clone()));
    seen.loaded.extend(loaded.read().map(|e| e.result.clone()));
    seen.audio += audio.read().count();
}

fn app(source: &str) -> App {
    build_app(source, &["ui.say"])
}

fn build_app(source: &str, immediate: &[&str]) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
        .insert_resource(Time::<Virtual>::default())
        .init_resource::<Seen>()
        .add_plugins(
            YomiPlugin::new(|reg| {
                reg.stop::<(String,), ()>("ui.say")?;
                reg.stop::<(String,), i64>("ui.ask")?;
                reg.bind(Sugar::Plain, "ui.say")?;
                Ok(())
            })
            .script("main.yomi")
            .immediate(immediate.iter().copied()),
        )
        .add_systems(Update, collect.after(YomiSet));
    set_source(&mut app, source);
    app
}

fn set_source(app: &mut App, source: &str) {
    set_program_source(app, "", source);
}

fn set_program_source(app: &mut App, name: &str, source: &str) {
    let mut project = app.world_mut().resource_mut::<YomiProject>();
    let program = project
        .programs
        .iter_mut()
        .find(|p| p.name == name)
        .expect("the program is defined");
    program.modules = vec![("main".to_string(), source.to_string())];
    program.dirty = true;
}

/// One frame in which `seconds` of virtual time pass.
fn frame(app: &mut App, seconds: f64) {
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .advance_by(Duration::from_secs_f64(seconds));
    app.update();
}

fn spawn(app: &mut App) -> Entity {
    app.world_mut().spawn(YomiRunner::new().seed(1)).id()
}

fn seen(app: &App) -> &Seen {
    app.world().resource::<Seen>()
}

fn send<M: Message>(app: &mut App, m: M) {
    app.world_mut().write_message(m);
}

const ASK: &str = "label start {\n    こんにちは。\n    let n = @ui.ask(\"数は？\")\n    答え：${n}\n    end\n}\n";

#[test]
fn immediate_commands_do_not_stop_and_others_wait_for_the_game() {
    let mut app = app(ASK);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    let names: Vec<&str> = seen(&app)
        .commands
        .iter()
        .map(|c| c.command.as_str())
        .collect();
    assert_eq!(names, ["ui.say", "ui.ask"]);
    assert_eq!(
        seen(&app).commands[0].args,
        [Value::Str("こんにちは。".to_string().into())]
    );
    assert!(seen(&app).ended.is_empty());
    send(&mut app, YomiResume::with(runner, Value::Int(42)));
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), 3);
    assert_eq!(
        seen(&app).commands[2].args,
        [Value::Str("答え：42".to_string().into())]
    );
    assert_eq!(seen(&app).ended, [runner]);
}

#[test]
fn a_runner_waits_for_flow_wait_in_virtual_time() {
    let mut app =
        app("import std::flow\nlabel start {\n    @flow.wait(2.0)\n    おわり。\n    end\n}\n");
    spawn(&mut app);
    frame(&mut app, 0.0);
    frame(&mut app, 1.5);
    assert!(seen(&app).commands.is_empty());
    frame(&mut app, 0.6);
    assert_eq!(seen(&app).commands.len(), 1);
    assert_eq!(seen(&app).ended.len(), 1);
}

const EVENT: &str = "import std::event\nlabel start {\n    let r = @event.wait([\"go\", \"stop\"], Some(10.0))\n    match r {\n        Arrived(name, data) => {\n            届いた：${name}\n        }\n        TimedOut => {\n            期限切れ。\n        }\n    }\n    end\n}\n";

#[test]
fn an_event_reaches_the_runner_that_waits_for_its_name() {
    let mut app = app(EVENT);
    let a = spawn(&mut app);
    let b = spawn(&mut app);
    frame(&mut app, 0.0);
    send(
        &mut app,
        YomiEvent {
            runner: Some(a),
            name: "go".into(),
            data: "{}".into(),
        },
    );
    send(
        &mut app,
        YomiEvent {
            runner: None,
            name: "other".into(),
            data: String::new(),
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).ended, [a]);
    assert_eq!(
        seen(&app).commands[0].args,
        [Value::Str("届いた：go".to_string().into())]
    );
    send(
        &mut app,
        YomiEvent {
            runner: None,
            name: "stop".into(),
            data: String::new(),
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).ended, [a, b]);
}

#[test]
fn an_event_wait_times_out_in_virtual_time() {
    let mut app = app(EVENT);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    frame(&mut app, 9.0);
    assert!(seen(&app).ended.is_empty());
    frame(&mut app, 1.5);
    assert_eq!(seen(&app).ended, [runner]);
    assert_eq!(
        seen(&app).commands[0].args,
        [Value::Str("期限切れ。".to_string().into())]
    );
}

#[test]
fn a_compile_error_is_reported_and_the_runner_does_not_start() {
    let mut app = app("label start {\n    @nothing.here()\n}\n");
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).diagnostics, 1);
    assert!(
        app.world()
            .get::<YomiRunner>(runner)
            .unwrap()
            .vm()
            .is_none()
    );
}

#[test]
fn a_new_program_replaces_the_one_of_a_stopped_runner() {
    let mut app = app(ASK);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    // The script is waiting at `ui.ask`; the text after it changes
    set_source(&mut app, &ASK.replace("答え：", "結果："));
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).reloaded, [Ok(())]);
    send(&mut app, YomiResume::with(runner, Value::Int(5)));
    frame(&mut app, 0.0);
    assert_eq!(
        seen(&app).commands.last().unwrap().args,
        [Value::Str("結果：5".to_string().into())]
    );
    // The command was not sent again
    assert_eq!(
        seen(&app)
            .commands
            .iter()
            .filter(|c| c.command == "ui.ask")
            .count(),
        1
    );
}

#[test]
fn a_program_that_does_not_compile_leaves_the_running_one_alone() {
    let mut app = app(ASK);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    set_source(&mut app, "label start {\n    @nothing.here()\n}\n");
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).diagnostics, 1);
    assert!(seen(&app).reloaded.is_empty());
    send(&mut app, YomiResume::with(runner, Value::Int(1)));
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).ended, [runner]);
}

#[test]
fn a_save_loads_into_another_runner_and_sends_the_command_again() {
    let mut app = app(ASK);
    let first = spawn(&mut app);
    frame(&mut app, 0.0);
    let bytes = app
        .world()
        .get::<YomiRunner>(first)
        .unwrap()
        .save()
        .unwrap();
    let second = spawn(&mut app);
    app.world_mut()
        .get_mut::<YomiRunner>(second)
        .unwrap()
        .load(bytes);
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).loaded, [Ok(())]);
    let asks: Vec<Entity> = seen(&app)
        .commands
        .iter()
        .filter(|c| c.command == "ui.ask")
        .map(|c| c.runner)
        .collect();
    assert_eq!(asks, [first, second]);
    send(&mut app, YomiResume::with(second, Value::Int(7)));
    frame(&mut app, 0.0);
    assert_eq!(
        seen(&app).commands.last().unwrap().args,
        [Value::Str("答え：7".to_string().into())]
    );
}

#[test]
fn save_is_refused_unless_the_script_is_stopped() {
    let runner = YomiRunner::new();
    assert!(runner.save().is_err());
}

#[test]
fn a_resume_for_a_runner_that_does_not_wait_is_ignored() {
    let mut app = app("import std::flow\nlabel start {\n    @flow.wait(5.0)\n    end\n}\n");
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    send(&mut app, YomiResume::unit(runner));
    frame(&mut app, 0.0);
    assert!(seen(&app).ended.is_empty());
}

#[test]
fn audio_cues_come_out_in_the_frame_they_were_made() {
    let mut app = app(
        "import std::audio\nimport std::flow\nlabel start {\n    @audio.se(\"bell\")\n    @flow.wait(1.0)\n    end\n}\n",
    );
    spawn(&mut app);
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).audio, 1);
}

#[test]
fn restart_runs_the_script_from_the_start_label_again() {
    let mut app = app("label start {\n    はじまり。\n    end\n}\n");
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    app.world_mut()
        .get_mut::<YomiRunner>(runner)
        .unwrap()
        .restart();
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), 2);
    assert_eq!(seen(&app).ended.len(), 2);
}

#[test]
fn a_script_file_is_loaded_as_an_asset_and_runs() {
    let dir = std::env::temp_dir().join(format!("yomi-bevy-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.yomi"),
        "label start {\n    ファイルから。\n    end\n}\n",
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..Default::default()
        },
    ))
    .insert_resource(Time::<Virtual>::default())
    .init_resource::<Seen>()
    .add_plugins(
        YomiPlugin::new(|reg| {
            reg.stop::<(String,), ()>("ui.say")?;
            reg.bind(Sugar::Plain, "ui.say")?;
            Ok(())
        })
        .script("main.yomi")
        .immediate(["ui.say"]),
    )
    .add_systems(Update, collect.after(YomiSet));
    app.world_mut().spawn(YomiRunner::new());
    for _ in 0..200 {
        frame(&mut app, 0.0);
        if !seen(&app).ended.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(seen(&app).ended.len(), 1);
    assert_eq!(
        seen(&app).commands[0].args,
        [Value::Str("ファイルから。".to_string().into())]
    );
    std::fs::remove_dir_all(&dir).ok();
}

const LOCALIZED: &str =
    "label start {\n    朝 #a\n    let n = @ui.ask(\"数は？\")\n    答え：${n} #b\n    end\n}\n";

fn table(entries: &[(&str, &str)]) -> YomiTexts {
    let mut t = yomi_core::TextTable::new("en");
    for (k, v) in entries {
        t.insert(*k, *v).unwrap();
    }
    YomiTexts { table: t }
}

fn add_texts(app: &mut App, texts: YomiTexts) -> Handle<YomiTexts> {
    app.world_mut()
        .resource_mut::<Assets<YomiTexts>>()
        .add(texts)
}

#[derive(Resource, Default)]
struct Applied(Vec<Result<(), String>>);

fn collect_applied(mut applied: ResMut<Applied>, mut r: MessageReader<YomiTextsApplied>) {
    applied.0.extend(r.read().map(|m| m.result.clone()));
}

/// `ui.say` waits for the game, so a plain line is a stop whose body the table can change.
fn texts_app(source: &str) -> App {
    let mut app = build_app(source, &[]);
    app.init_resource::<Applied>()
        .add_systems(Update, collect_applied.after(YomiSet));
    app
}

fn text_of(c: &YomiCommand) -> String {
    c.args[0].to_text()
}

#[test]
fn a_stopped_body_is_sent_again_when_the_table_changes_it() {
    let mut app = texts_app(LOCALIZED);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    assert_eq!(text_of(&seen(&app).commands[0]), "朝");
    let en = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(en),
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), 2);
    assert_eq!(text_of(&seen(&app).commands[1]), "Morning");
    assert_eq!(app.world().resource::<Applied>().0, [Ok(())]);
    // The same table again changes nothing
    let again = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(again),
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), 2);
    // Back to the originals
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: None,
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), 3);
    assert_eq!(text_of(&seen(&app).commands[2]), "朝");
}

#[test]
fn a_command_without_a_body_is_not_sent_again() {
    let mut app = texts_app(LOCALIZED);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    send(&mut app, YomiResume::unit(runner));
    frame(&mut app, 0.0);
    assert_eq!(text_of(seen(&app).commands.last().unwrap()), "数は？");
    let count = seen(&app).commands.len();
    let en = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(en),
        },
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).commands.len(), count);
}

#[test]
fn the_table_applies_to_the_stops_after_the_switch() {
    let mut app = texts_app("label start {\n    朝 #a\n    夜 #b\n    end\n}\n");
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    let en = add_texts(&mut app, table(&[("main.start#b", "Night")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(en),
        },
    );
    frame(&mut app, 0.0);
    send(&mut app, YomiResume::unit(runner));
    frame(&mut app, 0.0);
    assert_eq!(text_of(seen(&app).commands.last().unwrap()), "Night");
}

#[test]
fn a_table_that_cannot_be_set_keeps_the_previous_one() {
    let mut app = texts_app(LOCALIZED);
    let runner = spawn(&mut app);
    frame(&mut app, 0.0);
    let good = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(good),
        },
    );
    frame(&mut app, 0.0);
    // `${5}` is more than the originals hold
    let bad = add_texts(&mut app, table(&[("main.start#a", "Bad ${5}")]));
    send(
        &mut app,
        YomiSetTexts {
            runner,
            texts: Some(bad),
        },
    );
    frame(&mut app, 0.0);
    let applied = &app.world().resource::<Applied>().0;
    assert_eq!(applied.len(), 2);
    assert!(applied[0].is_ok() && applied[1].is_err());
    assert_eq!(seen(&app).commands.len(), 2);
    assert_eq!(text_of(seen(&app).commands.last().unwrap()), "Morning");
}

#[test]
fn a_runner_started_with_a_table_uses_it_from_the_first_stop() {
    let mut app = texts_app(LOCALIZED);
    let en = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    app.world_mut().spawn(YomiRunner::new().seed(1).texts(en));
    frame(&mut app, 0.0);
    assert_eq!(text_of(&seen(&app).commands[0]), "Morning");
    assert_eq!(seen(&app).commands.len(), 1);
}

#[test]
fn a_changed_table_file_is_set_again() {
    let mut app = texts_app(LOCALIZED);
    let en = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    app.world_mut()
        .spawn(YomiRunner::new().seed(1).texts(en.clone()));
    frame(&mut app, 0.0);
    app.world_mut()
        .resource_mut::<Assets<YomiTexts>>()
        .insert(&en, table(&[("main.start#a", "Good morning")]))
        .unwrap();
    frame(&mut app, 0.0);
    frame(&mut app, 0.0);
    assert_eq!(text_of(seen(&app).commands.last().unwrap()), "Good morning");
}

#[test]
fn the_table_stays_after_a_reload_and_a_load() {
    let mut app = texts_app(LOCALIZED);
    let en = add_texts(&mut app, table(&[("main.start#a", "Morning")]));
    let runner = app
        .world_mut()
        .spawn(YomiRunner::new().seed(1).texts(en))
        .id();
    frame(&mut app, 0.0);
    // A reload with an unchanged text keeps the translation and sends nothing again
    set_source(&mut app, &format!("{LOCALIZED}\n"));
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).reloaded, [Ok(())]);
    assert_eq!(seen(&app).commands.len(), 1);
    // A load: the stop comes with the translation
    let bytes = app
        .world()
        .entity(runner)
        .get::<YomiRunner>()
        .unwrap()
        .save()
        .unwrap();
    app.world_mut()
        .entity_mut(runner)
        .get_mut::<YomiRunner>()
        .unwrap()
        .load(bytes);
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).loaded, [Ok(())]);
    assert_eq!(text_of(seen(&app).commands.last().unwrap()), "Morning");
}

/// Two programs apart from each other, and no default one.
fn two_programs(a: &str, b: &str) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
        .insert_resource(Time::<Virtual>::default())
        .init_resource::<Seen>()
        .add_plugins(
            YomiPlugin::new(|reg| {
                reg.stop::<(String,), ()>("ui.say")?;
                reg.bind(Sugar::Plain, "ui.say")?;
                Ok(())
            })
            .program("a", ["a.yomi"])
            .program("b", ["b.yomi"])
            .immediate(["ui.say"]),
        )
        .add_systems(Update, collect.after(YomiSet));
    set_program_source(&mut app, "a", a);
    set_program_source(&mut app, "b", b);
    app
}

const A: &str = "label start {\n    あ\n    @ui.say(\"待つ\")\n    end\n}\n";
const B: &str = "label start {\n    い\n    end\n}\n";

#[test]
fn each_runner_runs_the_program_it_chose() {
    let mut app = two_programs(A, B);
    app.world_mut()
        .spawn(YomiRunner::new().program("a").seed(1));
    app.world_mut()
        .spawn(YomiRunner::new().program("b").seed(1));
    frame(&mut app, 0.0);
    let texts: Vec<String> = seen(&app).commands.iter().map(text_of).collect();
    assert_eq!(texts, ["あ", "待つ", "い"]);
    assert_eq!(seen(&app).ended.len(), 2);
}

#[test]
fn a_program_compiles_and_reloads_apart_from_the_others() {
    let mut app = two_programs(
        "import std::flow\nlabel start {\n    @flow.wait(10.0)\n    end\n}\n",
        "import std::flow\nlabel start {\n    @flow.wait(10.0)\n    end\n}\n",
    );
    app.world_mut()
        .spawn(YomiRunner::new().program("a").seed(1));
    app.world_mut()
        .spawn(YomiRunner::new().program("b").seed(1));
    frame(&mut app, 0.0);
    // A broken source in `b` only reports the diagnostics of `b`
    set_program_source(&mut app, "b", "label start {\n    ???\n");
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).diagnostic_programs, ["b"]);
    assert!(seen(&app).reloaded.is_empty());
    // A change of `b` reloads only the runner of `b`
    set_program_source(
        &mut app,
        "b",
        "import std::flow\nlabel start {\n    @flow.wait(10.0)\n    うしろ\n    end\n}\n",
    );
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).reloaded, [Ok(())]);
}

#[test]
fn a_runner_of_an_unknown_program_fails() {
    let mut app = two_programs(A, B);
    app.world_mut()
        .spawn(YomiRunner::new().program("nope").seed(1));
    // The default program has no scripts in this app
    app.world_mut().spawn(YomiRunner::new().seed(1));
    frame(&mut app, 0.0);
    assert_eq!(seen(&app).failed.len(), 2);
    assert!(seen(&app).failed[0].contains("unknown program `nope`"));
    assert!(seen(&app).failed[1].contains("default program"));
}

#[test]
#[should_panic(expected = "defined twice")]
fn a_program_name_cannot_be_used_twice() {
    let _ = YomiPlugin::new(|_| Ok(()))
        .program("a", ["a.yomi"])
        .program("a", ["b.yomi"]);
}
