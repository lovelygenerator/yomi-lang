//! The `screen` package: instructions for whole-screen effects (fade to color, shake, flash) (`spec/packages/screen.md`).
//!
//! Drawing is the host's job. Commands only change the state ([`ScreenState`]) and add one-time events as cues
//! ([`ScreenCue`]). `cover` (whether the screen is covered with a single color, as in a fade to black) is the state for
//! redrawing after a load or rollback, and `cues` holds only those added after the last `resume` (emptied at the start of
//! the next `run` / `resume` and right after loading or restoring). Shakes and flashes are only cues and do not remain in
//! the state.

use serde::{Deserialize, Serialize};
use yomi_core::{CommandError, Ctx, Package, PackageReg, Value};

use crate::check::non_negative;

/// The `screen` package. It has no persistent slice.
pub struct Screen;

/// The color covering the screen (fade to black, fade to white and so on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cover {
    /// The color (`"black"`, `"#ffffff"` and so on; interpreted by the host).
    pub color: String,
}

/// A one-time screen event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScreenCue {
    /// Covers the screen with a color over `seconds` seconds (fade to black and so on).
    FadeOut {
        /// The color.
        color: String,
        /// Duration in seconds (0 or more).
        seconds: f64,
    },
    /// Removes the cover over `seconds` seconds.
    FadeIn {
        /// Duration in seconds (0 or more).
        seconds: f64,
    },
    /// Shakes the screen for `seconds` seconds.
    Shake {
        /// The strength of the shake (0 or more; interpreted by the host).
        strength: f64,
        /// Duration in seconds (0 or more).
        seconds: f64,
    },
    /// Flashes the screen with a color for a moment (disappears in `seconds` seconds).
    Flash {
        /// The color.
        color: String,
        /// Duration in seconds (0 or more).
        seconds: f64,
    },
}

/// The state slice of `screen`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenState {
    /// The color covering the screen (`None` if not covered).
    pub cover: Option<Cover>,
    /// Cues (those added after the last `resume`).
    pub cues: Vec<ScreenCue>,
}

impl Package for Screen {
    const NAME: &'static str = "screen";
    const STD: bool = true;
    type State = ScreenState;
    type Persistent = ();

    fn clear_cues(state: &mut ScreenState) {
        state.cues.clear();
    }

    fn cue_len(state: &ScreenState) -> usize {
        state.cues.len()
    }

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect(
            "screen.fade_out",
            |ctx: &mut Ctx<Screen>, color: String, seconds: f64| {
                non_negative("screen.fade_out", "seconds", seconds)?;
                let s = ctx.state_mut();
                // If already covered, replace the color
                s.cover = Some(Cover {
                    color: color.clone(),
                });
                s.cues.push(ScreenCue::FadeOut { color, seconds });
                Ok::<(), CommandError>(())
            },
        )
        .doc("画面を、seconds 秒かけて色で覆う（暗転など。覆っていれば色を入れ替える）")
        .param_names(&["color", "seconds"])
        .defaults(&[("seconds", Value::Float(0.0))]);
        reg.effect("screen.fade_in", |ctx: &mut Ctx<Screen>, seconds: f64| {
            non_negative("screen.fade_in", "seconds", seconds)?;
            let s = ctx.state_mut();
            // If not covered, do nothing
            if s.cover.take().is_some() {
                s.cues.push(ScreenCue::FadeIn { seconds });
            }
            Ok::<(), CommandError>(())
        })
        .doc("画面の覆いを、seconds 秒かけて外す（覆っていなければ何もしない）")
        .param_names(&["seconds"])
        .defaults(&[("seconds", Value::Float(0.0))]);
        reg.effect(
            "screen.shake",
            |ctx: &mut Ctx<Screen>, strength: f64, seconds: f64| {
                non_negative("screen.shake", "strength", strength)?;
                non_negative("screen.shake", "seconds", seconds)?;
                ctx.state_mut()
                    .cues
                    .push(ScreenCue::Shake { strength, seconds });
                Ok::<(), CommandError>(())
            },
        )
        .doc("画面を、seconds 秒、揺らす（合図だけ。strength はホストが解釈する）")
        .param_names(&["strength", "seconds"]);
        reg.effect(
            "screen.flash",
            |ctx: &mut Ctx<Screen>, color: String, seconds: f64| {
                non_negative("screen.flash", "seconds", seconds)?;
                ctx.state_mut()
                    .cues
                    .push(ScreenCue::Flash { color, seconds });
                Ok::<(), CommandError>(())
            },
        )
        .doc("画面を、色で一瞬光らせる（合図だけ。seconds 秒で消える）")
        .param_names(&["color", "seconds"]);
        reg.query(
            "screen.cover_color",
            |ctx: &Ctx<Screen>| -> Option<String> {
                ctx.state().cover.as_ref().map(|c| c.color.clone())
            },
        )
        .doc("画面を覆っている色（覆っていなければ None）")
        .param_names(&[]);
    }
}
