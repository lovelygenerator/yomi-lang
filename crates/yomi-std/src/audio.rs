//! The `audio` package: instructions for sounds played continuously on channels (BGM, ambient sounds and so on), sound
//! effects and voices (`spec/packages/audio.md`).
//!
//! Playing sounds is the host's job. Commands only change the state ([`AudioState`]) and add one-time events as cues
//! ([`AudioCue`]). The host reads `Vm::package_state::<Audio>()` at a stop point. `channels` is the state for playing
//! sounds again after a load or rollback, and `cues` holds only those added after the last `resume` (emptied at the start
//! of the next `run` / `resume` and right after loading or restoring). By convention, BGM is played on the `"bgm"`
//! channel.

use serde::{Deserialize, Serialize};
use yomi_core::{CommandError, Ctx, MigrateError, Package, PackageReg, Value, decode_slice};

use crate::check::{non_negative, unit_range};

/// The `audio` package. It has no persistent slice.
pub struct Audio;

/// A sound kept playing on a named channel (BGM, ambient sounds and so on).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    /// The name of the channel (decided by the script writer; `"bgm"`, `"rain"` and so on).
    pub channel: String,
    /// The name of the asset (the host maps it to an asset).
    pub name: String,
    /// Volume (0.0 to 1.0).
    pub volume: f64,
}

/// A one-time sound event.
///
/// New variants are added at the end (so cues in saves of older versions can be read as they are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioCue {
    /// Keeps a sound playing on a channel (replaces it if one is playing; over `fade` seconds).
    Play {
        /// The name of the channel.
        channel: String,
        /// The name of the asset.
        name: String,
        /// Volume (0.0 to 1.0).
        volume: f64,
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// Stops the sound of a channel over `fade` seconds.
    Stop {
        /// The name of the channel.
        channel: String,
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// Changes the volume of a channel over `fade` seconds.
    Volume {
        /// The name of the channel.
        channel: String,
        /// Volume (0.0 to 1.0).
        volume: f64,
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// Plays a sound effect once.
    Se {
        /// The name of the asset.
        name: String,
    },
    /// Plays a voice once.
    Voice {
        /// The name of the asset.
        name: String,
    },
    /// Stops all sound effects that are playing.
    SeStop,
    /// Stops the voice that is playing.
    VoiceStop,
}

/// The state slice of `audio`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioState {
    /// Channels kept playing (in the order they started).
    pub channels: Vec<Channel>,
    /// Cues (those added after the last `resume`).
    pub cues: Vec<AudioCue>,
}

/// Types for reading the state of older versions (1 and 2). Cues become empty right after a load, so they are read and
/// discarded.
mod old {
    use serde::Deserialize;

    /// BGM of version 1 (name only).
    #[derive(Deserialize)]
    pub(super) struct Bgm {
        pub(super) name: String,
    }

    /// Loop playback of version 2 (channel, name, volume).
    #[derive(Deserialize)]
    pub(super) struct Loop {
        pub(super) channel: String,
        pub(super) name: String,
        pub(super) volume: f64,
    }

    /// Cues of versions 1 and 2 (version 1 uses only the first 5).
    #[expect(dead_code, reason = "旧い版のバイト列を読むために、並びと形だけが要る")]
    #[derive(Deserialize)]
    pub(super) enum Cue {
        /// Starts BGM (with a fade duration).
        BgmStart {
            name: String,
            fade: f64,
        },
        /// Stops BGM.
        BgmStop {
            fade: f64,
        },
        /// Changes the BGM volume.
        BgmVolume {
            volume: f64,
            fade: f64,
        },
        /// Plays a sound effect.
        Se {
            name: String,
        },
        /// Plays a voice.
        Voice {
            name: String,
        },
        LoopStart {
            channel: String,
            name: String,
            volume: f64,
            fade: f64,
        },
        /// Stops the loop sound of a channel.
        LoopStop {
            channel: String,
            fade: f64,
        },
        LoopVolume {
            channel: String,
            volume: f64,
            fade: f64,
        },
        SeStop,
        VoiceStop,
    }

    /// The state of version 1 (BGM, BGM volume, cues).
    #[expect(dead_code, reason = "旧い版のバイト列を読むために、並びと形だけが要る")]
    #[derive(Deserialize)]
    pub(super) struct V1 {
        pub(super) bgm: Option<Bgm>,
        pub(super) bgm_volume: f64,
        pub(super) cues: Vec<Cue>,
    }

    /// The state of version 2 (BGM, BGM volume, cues, loops).
    #[expect(dead_code, reason = "旧い版のバイト列を読むために、並びと形だけが要る")]
    #[derive(Deserialize)]
    pub(super) struct V2 {
        pub(super) bgm: Option<Bgm>,
        pub(super) bgm_volume: f64,
        pub(super) cues: Vec<Cue>,
        pub(super) loops: Vec<Loop>,
    }
}

/// Turns the BGM of an older version into the `"bgm"` channel and puts it before the loops.
fn upgrade(bgm: Option<old::Bgm>, bgm_volume: f64, loops: Vec<old::Loop>) -> AudioState {
    let mut channels: Vec<Channel> = bgm
        .map(|b| Channel {
            channel: "bgm".into(),
            name: b.name,
            volume: bgm_volume,
        })
        .into_iter()
        .collect();
    for l in loops {
        // Older versions could also make a loop named "bgm". In that case, keep the BGM
        if channels.iter().all(|c| c.channel != l.channel) {
            channels.push(Channel {
                channel: l.channel,
                name: l.name,
                volume: l.volume,
            });
        }
    }
    AudioState {
        channels,
        cues: Vec::new(),
    }
}

/// The error when a channel is empty.
fn channel_empty(command: &'static str, channel: String) -> CommandError {
    CommandError::with_code(
        "audio.channel_empty",
        [("command", command.into()), ("channel", channel.into())],
    )
}

impl Package for Audio {
    const NAME: &'static str = "audio";
    const STD: bool = true;
    /// Version 2 added loops. Version 3 unified BGM and loops into the channel mechanism.
    const VERSION: u32 = 3;
    type State = AudioState;
    type Persistent = ();

    fn migrate(old_version: u32, raw: &[u8]) -> Result<AudioState, MigrateError> {
        match old_version {
            1 => {
                let s: old::V1 = decode_slice(raw)?;
                Ok(upgrade(s.bgm, s.bgm_volume, Vec::new()))
            }
            2 => {
                let s: old::V2 = decode_slice(raw)?;
                Ok(upgrade(s.bgm, s.bgm_volume, s.loops))
            }
            v => Err(MigrateError::unsupported_version(v)),
        }
    }

    fn clear_cues(state: &mut AudioState) {
        state.cues.clear();
    }

    fn cue_len(state: &AudioState) -> usize {
        state.cues.len()
    }

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect(
            "audio.play",
            |ctx: &mut Ctx<Audio>, channel: String, name: String, volume: f64, fade: f64| {
                unit_range("audio.play", "volume", volume)?;
                non_negative("audio.play", "fade", fade)?;
                let s = ctx.state_mut();
                // If the channel is already playing, replace the asset and volume (the order does not change)
                match s.channels.iter_mut().find(|c| c.channel == channel) {
                    Some(c) => {
                        c.name = name.clone();
                        c.volume = volume;
                    }
                    None => s.channels.push(Channel {
                        channel: channel.clone(),
                        name: name.clone(),
                        volume,
                    }),
                }
                s.cues.push(AudioCue::Play {
                    channel,
                    name,
                    volume,
                    fade,
                });
                Ok::<(), CommandError>(())
            },
        )
        .doc("チャンネルで音を鳴らし続ける（鳴っていれば入れ替える。fade 秒でフェードする）")
        .param_names(&["channel", "name", "volume", "fade"])
        .defaults(&[("volume", Value::Float(1.0)), ("fade", Value::Float(0.0))]);
        reg.effect(
            "audio.stop",
            |ctx: &mut Ctx<Audio>, channel: String, fade: f64| {
                non_negative("audio.stop", "fade", fade)?;
                let s = ctx.state_mut();
                // If nothing is playing, do nothing
                if let Some(i) = s.channels.iter().position(|c| c.channel == channel) {
                    s.channels.remove(i);
                    s.cues.push(AudioCue::Stop { channel, fade });
                }
                Ok::<(), CommandError>(())
            },
        )
        .doc("チャンネルの音を止める（鳴っていなければ何もしない）")
        .param_names(&["channel", "fade"])
        .defaults(&[("fade", Value::Float(0.0))]);
        reg.effect(
            "audio.volume",
            |ctx: &mut Ctx<Audio>, channel: String, volume: f64, fade: f64| {
                unit_range("audio.volume", "volume", volume)?;
                non_negative("audio.volume", "fade", fade)?;
                let s = ctx.state_mut();
                let Some(c) = s.channels.iter_mut().find(|c| c.channel == channel) else {
                    return Err(channel_empty("audio.volume", channel));
                };
                c.volume = volume;
                s.cues.push(AudioCue::Volume {
                    channel,
                    volume,
                    fade,
                });
                Ok(())
            },
        )
        .doc("チャンネルの音量（0.0〜1.0）を fade 秒かけて変える（鳴っていなければ実行時エラー）")
        .param_names(&["channel", "volume", "fade"])
        .defaults(&[("fade", Value::Float(0.0))]);
        reg.query(
            "audio.playing",
            |ctx: &Ctx<Audio>, channel: String| -> Option<String> {
                ctx.state()
                    .channels
                    .iter()
                    .find(|c| c.channel == channel)
                    .map(|c| c.name.clone())
            },
        )
        .doc("チャンネルで鳴らしている音の名前（鳴っていなければ None）")
        .param_names(&["channel"]);
        reg.effect("audio.se", |ctx: &mut Ctx<Audio>, name: String| {
            ctx.state_mut().cues.push(AudioCue::Se { name });
        })
        .doc("効果音を 1 回鳴らす")
        .param_names(&["name"]);
        reg.effect("audio.voice", |ctx: &mut Ctx<Audio>, name: String| {
            ctx.state_mut().cues.push(AudioCue::Voice { name });
        })
        .doc("ボイスを 1 回鳴らす")
        .param_names(&["name"]);
        reg.effect("audio.se_stop", |ctx: &mut Ctx<Audio>| {
            ctx.state_mut().cues.push(AudioCue::SeStop);
        })
        .doc("鳴っている効果音を、すべて止める（合図だけ）")
        .param_names(&[]);
        reg.effect("audio.voice_stop", |ctx: &mut Ctx<Audio>| {
            ctx.state_mut().cues.push(AudioCue::VoiceStop);
        })
        .doc("鳴っているボイスを止める（合図だけ）")
        .param_names(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yomi_core::encode_slice;

    /// Types for writing the state of older versions (the same layout as `old`).
    #[derive(Serialize)]
    struct Bgm {
        name: String,
    }
    #[derive(Serialize)]
    struct Loop {
        channel: String,
        name: String,
        volume: f64,
    }
    #[derive(Serialize)]
    #[expect(
        dead_code,
        reason = "バリアントの並びを旧い版と同じにするため、使わないものも置く"
    )]
    enum Cue {
        BgmStart {
            name: String,
            fade: f64,
        },
        BgmStop {
            fade: f64,
        },
        BgmVolume {
            volume: f64,
            fade: f64,
        },
        Se {
            name: String,
        },
        Voice {
            name: String,
        },
        LoopStart {
            channel: String,
            name: String,
            volume: f64,
            fade: f64,
        },
    }
    #[derive(Serialize)]
    struct V1 {
        bgm: Option<Bgm>,
        bgm_volume: f64,
        cues: Vec<Cue>,
    }
    #[derive(Serialize)]
    struct V2 {
        bgm: Option<Bgm>,
        bgm_volume: f64,
        cues: Vec<Cue>,
        loops: Vec<Loop>,
    }

    fn ch(channel: &str, name: &str, volume: f64) -> Channel {
        Channel {
            channel: channel.into(),
            name: name.into(),
            volume,
        }
    }

    #[test]
    fn version_1_bgm_becomes_bgm_channel_and_cues_are_dropped() {
        let old = V1 {
            bgm: Some(Bgm {
                name: "town".into(),
            }),
            bgm_volume: 0.5,
            cues: vec![Cue::Se {
                name: "door".into(),
            }],
        };
        let raw = encode_slice(&old).unwrap();
        let state = Audio::migrate(1, &raw).unwrap();
        assert_eq!(state.channels, vec![ch("bgm", "town", 0.5)]);
        assert!(state.cues.is_empty());
        assert!(Audio::migrate(4, &raw).is_err());
    }

    #[test]
    fn version_2_bgm_and_loop_merge_into_channel() {
        let old = V2 {
            bgm: None,
            bgm_volume: 0.3,
            cues: vec![Cue::LoopStart {
                channel: "rain".into(),
                name: "rain_soft".into(),
                volume: 0.4,
                fade: 1.0,
            }],
            loops: vec![
                Loop {
                    channel: "rain".into(),
                    name: "rain_soft".into(),
                    volume: 0.4,
                },
                Loop {
                    channel: "bgm".into(),
                    name: "x".into(),
                    volume: 1.0,
                },
            ],
        };
        let raw = encode_slice(&old).unwrap();
        let state = Audio::migrate(2, &raw).unwrap();
        // If BGM is not playing, a loop named "bgm" also stays as is
        assert_eq!(
            state.channels,
            vec![ch("rain", "rain_soft", 0.4), ch("bgm", "x", 1.0)]
        );
        let old = V2 {
            bgm: Some(Bgm {
                name: "town".into(),
            }),
            bgm_volume: 0.3,
            cues: Vec::new(),
            loops: vec![Loop {
                channel: "bgm".into(),
                name: "x".into(),
                volume: 1.0,
            }],
        };
        let state = Audio::migrate(2, &encode_slice(&old).unwrap()).unwrap();
        assert_eq!(state.channels, vec![ch("bgm", "town", 0.3)]);
    }
}
