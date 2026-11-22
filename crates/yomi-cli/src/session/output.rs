//! Writing stop points, cues and variables to the output.

use super::*;

/// Writes variables one per line as `name = value` (values in the display form where `str` is quoted even inside nesting;
/// section 7.2.4 of spec/host/07-repl.md). When `qualify` is true, shows module variables as `module.name`.
pub(crate) fn write_vars<O: Write>(
    out: &mut O,
    vars: &[Variable],
    qualify: bool,
) -> io::Result<()> {
    for v in vars {
        let value = v.value.to_repl_text();
        match &v.module {
            Some(m) if qualify => writeln!(out, "{m}.{} = {value}", v.name)?,
            _ => writeln!(out, "{} = {value}", v.name)?,
        }
    }
    Ok(())
}

/// Writes one line to standard output (the text is made from an FTL key).
pub(crate) fn say<O: Write>(
    loc: &Localizer,
    out: &mut O,
    id: &str,
    args: Option<&FluentArgs>,
) -> io::Result<()> {
    writeln!(out, "{}", loc.fmt(id, args))
}

/// Prints sound and image cues to standard output, one line each (`[audio] ...`, `[image] ...`).
///
/// Cues are in the state and are emptied at the start of `run` / `resume` and right after loading or restoring.
pub(crate) fn write_cues<O: Write>(
    loc: &Localizer,
    out: &mut O,
    audio: Option<&<Audio as Package>::State>,
    image: Option<&<Image as Package>::State>,
    screen: Option<&<Screen as Package>::State>,
) -> io::Result<()> {
    if let Some(state) = audio {
        for cue in &state.cues {
            let (key, args) = match cue {
                AudioCue::Play {
                    channel,
                    name,
                    volume,
                    fade,
                } => {
                    let mut a = one_arg("channel", channel);
                    a.set("name", name.clone());
                    a.set("volume", volume.to_string());
                    a.set("fade", fade.to_string());
                    ("run-cue-audio-play", a)
                }
                AudioCue::Stop { channel, fade } => {
                    let mut a = one_arg("channel", channel);
                    a.set("fade", fade.to_string());
                    ("run-cue-audio-stop", a)
                }
                AudioCue::Volume {
                    channel,
                    volume,
                    fade,
                } => {
                    let mut a = one_arg("channel", channel);
                    a.set("volume", volume.to_string());
                    a.set("fade", fade.to_string());
                    ("run-cue-audio-volume", a)
                }
                AudioCue::Se { name } => ("run-cue-audio-se", one_arg("name", name)),
                AudioCue::Voice { name } => ("run-cue-audio-voice", one_arg("name", name)),
                AudioCue::SeStop => ("run-cue-audio-se-stop", FluentArgs::new()),
                AudioCue::VoiceStop => ("run-cue-audio-voice-stop", FluentArgs::new()),
            };
            writeln!(out, "{}", loc.fmt(key, Some(&args)))?;
        }
    }
    if let Some(state) = image {
        for cue in &state.cues {
            let (key, args) = match cue {
                ImageCue::Show { slot, fade } => {
                    let mut a = one_arg("slot", slot);
                    // Images to show are taken from the state
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set("name", item.map(|i| i.name.clone()).unwrap_or_default());
                    a.set("x", item.map(|i| i.x.to_string()).unwrap_or_default());
                    a.set("y", item.map(|i| i.y.to_string()).unwrap_or_default());
                    a.set("fade", fade.to_string());
                    ("run-cue-image-show", a)
                }
                ImageCue::Move { slot, seconds } => {
                    let mut a = one_arg("slot", slot);
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set("x", item.map(|i| i.x.to_string()).unwrap_or_default());
                    a.set("y", item.map(|i| i.y.to_string()).unwrap_or_default());
                    a.set("seconds", seconds.to_string());
                    ("run-cue-image-move", a)
                }
                ImageCue::Hide { slot, fade } => {
                    let mut a = one_arg("slot", slot);
                    a.set("fade", fade.to_string());
                    ("run-cue-image-hide", a)
                }
                ImageCue::Clear { fade } => {
                    ("run-cue-image-clear", one_arg("fade", &fade.to_string()))
                }
                // Stacking values and transforms are taken from the state
                ImageCue::Layer { slot } => {
                    let mut a = one_arg("slot", slot);
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set(
                        "layer",
                        item.map(|i| i.layer.to_string()).unwrap_or_default(),
                    );
                    ("run-cue-image-layer", a)
                }
                ImageCue::Scale { slot, seconds } => {
                    let mut a = one_arg("slot", slot);
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set(
                        "scale",
                        item.map(|i| i.scale.to_string()).unwrap_or_default(),
                    );
                    a.set("seconds", seconds.to_string());
                    ("run-cue-image-scale", a)
                }
                ImageCue::Flip { slot } => {
                    let mut a = one_arg("slot", slot);
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set("flip", item.map(|i| i.flip.to_string()).unwrap_or_default());
                    ("run-cue-image-flip", a)
                }
                ImageCue::Alpha { slot, seconds } => {
                    let mut a = one_arg("slot", slot);
                    let item = state.items.iter().find(|i| i.slot == *slot);
                    a.set(
                        "alpha",
                        item.map(|i| i.alpha.to_string()).unwrap_or_default(),
                    );
                    a.set("seconds", seconds.to_string());
                    ("run-cue-image-alpha", a)
                }
            };
            writeln!(out, "{}", loc.fmt(key, Some(&args)))?;
        }
    }
    if let Some(state) = screen {
        for cue in &state.cues {
            let (key, args) = match cue {
                ScreenCue::FadeOut { color, seconds } => {
                    let mut a = one_arg("color", color);
                    a.set("seconds", seconds.to_string());
                    ("run-cue-screen-fade-out", a)
                }
                ScreenCue::FadeIn { seconds } => (
                    "run-cue-screen-fade-in",
                    one_arg("seconds", &seconds.to_string()),
                ),
                ScreenCue::Shake { strength, seconds } => {
                    let mut a = one_arg("strength", &strength.to_string());
                    a.set("seconds", seconds.to_string());
                    ("run-cue-screen-shake", a)
                }
                ScreenCue::Flash { color, seconds } => {
                    let mut a = one_arg("color", color);
                    a.set("seconds", seconds.to_string());
                    ("run-cue-screen-flash", a)
                }
            };
            writeln!(out, "{}", loc.fmt(key, Some(&args)))?;
        }
    }
    Ok(())
}

/// Prints the contents of a stop point to standard output.
///
/// `flow` waits do not actually wait; they only show what the point waits for (Enter goes to the next).
pub(crate) fn write_stop<O: Write>(
    loc: &Localizer,
    reg: &Registry,
    out: &mut O,
    show_ids: bool,
    command: CommandId,
    args: &[Value],
) -> io::Result<()> {
    match reg.command(command).name.as_str() {
        WAIT => {
            let seconds = args.first().map(Value::to_text).unwrap_or_default();
            return say(loc, out, "run-wait", Some(&one_arg("seconds", &seconds)));
        }
        WAIT_INPUT => return say(loc, out, "run-wait-input", None),
        INPUT_TEXT => {
            if let [Value::Str(prompt), Value::Str(default), Value::Int(max_len)] = args {
                let mut a = one_arg("prompt", prompt);
                a.set("default", default.to_string());
                a.set("max_len", *max_len);
                return say(loc, out, "run-input-text", Some(&a));
            }
        }
        INPUT_INT => {
            if let [
                Value::Str(prompt),
                Value::Int(default),
                Value::Int(min),
                Value::Int(max),
            ] = args
            {
                let mut a = one_arg("prompt", prompt);
                a.set("default", *default);
                a.set("min", *min);
                a.set("max", *max);
                return say(loc, out, "run-input-int", Some(&a));
            }
        }
        EVENT_WAIT => {
            if let [Value::List(items), timeout] = args {
                let names = items
                    .iter()
                    .map(Value::to_text)
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut a = one_arg("names", &names);
                return match timeout {
                    Value::Option(Some(seconds)) => {
                        a.set("seconds", seconds.to_text());
                        say(loc, out, "run-event-wait-timeout", Some(&a))
                    }
                    _ => say(loc, out, "run-event-wait", Some(&a)),
                };
            }
        }
        INPUT_CONFIRM => {
            let prompt = args.first().map(Value::to_text).unwrap_or_default();
            return say(
                loc,
                out,
                "run-input-confirm",
                Some(&one_arg("prompt", &prompt)),
            );
        }
        _ => {}
    }
    match args {
        [Value::List(items), ids] => {
            for (i, item) in items.iter().enumerate() {
                match choice_id(ids, i).filter(|_| show_ids) {
                    Some(id) => writeln!(out, "{}: {} #{id}", i + 1, item.to_text())?,
                    None => writeln!(out, "{}: {}", i + 1, item.to_text())?,
                }
            }
        }
        [Value::Option(who), Value::Str(text)] => match who.as_deref() {
            Some(Value::Identity(identity)) => {
                writeln!(out, "{}{text}", identity.name)?;
            }
            _ => writeln!(out, "{text}")?,
        },
        _ => {
            let text = args.first().map(Value::to_text).unwrap_or_default();
            writeln!(out, "{text}")?;
        }
    }
    Ok(())
}

/// Makes Fluent arguments with one argument.
pub(crate) fn one_arg(name: &'static str, value: &str) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    args.set(name, value.to_string());
    args
}
