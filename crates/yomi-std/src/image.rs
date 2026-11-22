//! The `image` package: instructions for components that show images such as characters, backgrounds and icons
//! (`spec/packages/image.md`).
//!
//! Images are placed in named slots (`slot`). Drawing is the host's job. Commands only change the state ([`ImageState`])
//! and add one-time events as cues ([`ImageCue`]). `items` is the state for redrawing after a load or rollback, and `cues`
//! holds only those added after the last `resume` (emptied at the start of the next `run` / `resume` and right after
//! loading or restoring).
//!
//! The stacking order is by the stacking value (`layer`; smaller is further back), and for equal values, the order the
//! slots were added (later ones in front).

use serde::{Deserialize, Serialize};
use yomi_core::{CommandError, Ctx, MigrateError, Package, PackageReg, Value, decode_slice};

use crate::check::{finite, non_negative, positive, unit_range};

/// The `image` package. It has no persistent slice.
pub struct Image;

/// One image being shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// The name of the slot.
    pub slot: String,
    /// The name of the asset (the host maps it to an asset).
    pub name: String,
    /// The x position (in a coordinate system decided by the host).
    pub x: f64,
    /// The y position (in a coordinate system decided by the host).
    pub y: f64,
    /// The stacking value (smaller is further back; default 0).
    pub layer: i64,
    /// The scale (greater than 0; default 1.0).
    pub scale: f64,
    /// Whether it is flipped horizontally (default `false`).
    pub flip: bool,
    /// Opacity (0.0 to 1.0; default 1.0).
    pub alpha: f64,
}

impl Item {
    /// An image placed in a slot for the first time (transforms have default values).
    fn new(slot: String, name: String, x: f64, y: f64) -> Self {
        Self {
            slot,
            name,
            x,
            y,
            layer: 0,
            scale: 1.0,
            flip: false,
            alpha: 1.0,
        }
    }
}

/// A one-time image event.
///
/// The cues of version 1 are the first 4. New variants are added at the end (so cues in saves of older versions can be
/// read as they are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageCue {
    /// An image was placed in a slot (appears over `fade` seconds).
    Show {
        /// The name of the slot.
        slot: String,
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// The image in a slot was moved (over `seconds` seconds).
    Move {
        /// The name of the slot.
        slot: String,
        /// Movement duration in seconds (0 or more).
        seconds: f64,
    },
    /// The image in a slot was removed (disappears over `fade` seconds).
    Hide {
        /// The name of the slot.
        slot: String,
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// All slots were emptied (disappear over `fade` seconds).
    Clear {
        /// Fade duration in seconds (0 or more).
        fade: f64,
    },
    /// The stacking value of a slot was changed (immediately).
    Layer {
        /// The name of the slot.
        slot: String,
    },
    /// The scale of the image in a slot was changed (over `seconds` seconds).
    Scale {
        /// The name of the slot.
        slot: String,
        /// Duration of the change in seconds (0 or more).
        seconds: f64,
    },
    /// Whether the image in a slot is flipped horizontally was changed (immediately).
    Flip {
        /// The name of the slot.
        slot: String,
    },
    /// The opacity of the image in a slot was changed (over `seconds` seconds).
    Alpha {
        /// The name of the slot.
        slot: String,
        /// Duration of the change in seconds (0 or more).
        seconds: f64,
    },
}

/// An image of version 1 (no transforms). Used only when reading saves of an older version.
#[derive(Serialize, Deserialize)]
struct ItemV1 {
    slot: String,
    name: String,
    x: f64,
    y: f64,
}

/// The state slice of version 1.
#[derive(Serialize, Deserialize)]
struct ImageStateV1 {
    items: Vec<ItemV1>,
    cues: Vec<ImageCue>,
}

/// The error when a slot is empty.
fn slot_empty(command: &'static str, slot: String) -> CommandError {
    CommandError::with_code(
        "image.slot_empty",
        [("command", command.into()), ("slot", slot.into())],
    )
}

/// The state slice of `image`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageState {
    /// What is being shown (in the order the slots were added). The drawing order is by `layer`, smallest first, and for
    /// equal values, this order (later ones in front).
    pub items: Vec<Item>,
    /// Cues (those added after the last `resume`).
    pub cues: Vec<ImageCue>,
}

impl Package for Image {
    const NAME: &'static str = "image";
    const STD: bool = true;
    /// Version 2 added the stacking value and transforms (`layer`, `scale`, `flip`, `alpha`) to `Item`.
    const VERSION: u32 = 2;
    type State = ImageState;
    type Persistent = ();

    fn migrate(old_version: u32, raw: &[u8]) -> Result<ImageState, MigrateError> {
        match old_version {
            1 => {
                let old: ImageStateV1 = decode_slice(raw)?;
                Ok(ImageState {
                    items: old
                        .items
                        .into_iter()
                        .map(|i| Item::new(i.slot, i.name, i.x, i.y))
                        .collect(),
                    cues: old.cues,
                })
            }
            v => Err(MigrateError::unsupported_version(v)),
        }
    }

    fn clear_cues(state: &mut ImageState) {
        state.cues.clear();
    }

    fn cue_len(state: &ImageState) -> usize {
        state.cues.len()
    }

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect(
            "image.show",
            |ctx: &mut Ctx<Image>, slot: String, name: String, x: f64, y: f64, fade: f64| {
                finite("image.show", "x", x)?;
                finite("image.show", "y", y)?;
                non_negative("image.show", "fade", fade)?;
                let s = ctx.state_mut();
                // If the slot already has one, replace the image and position (the stacking order and transforms do not change)
                match s.items.iter_mut().find(|i| i.slot == slot) {
                    Some(item) => {
                        item.name = name;
                        item.x = x;
                        item.y = y;
                    }
                    None => s.items.push(Item::new(slot.clone(), name, x, y)),
                }
                s.cues.push(ImageCue::Show { slot, fade });
                Ok::<(), CommandError>(())
            },
        )
        .doc("枠に画像を置く（枠にすでにあれば入れ替える）。fade 秒でフェードする")
        .param_names(&["slot", "name", "x", "y", "fade"])
        .defaults(&[
            ("x", Value::Float(0.0)),
            ("y", Value::Float(0.0)),
            ("fade", Value::Float(0.0)),
        ]);
        reg.effect(
            "image.move",
            |ctx: &mut Ctx<Image>, slot: String, x: f64, y: f64, seconds: f64| {
                finite("image.move", "x", x)?;
                finite("image.move", "y", y)?;
                non_negative("image.move", "seconds", seconds)?;
                let s = ctx.state_mut();
                let Some(item) = s.items.iter_mut().find(|i| i.slot == slot) else {
                    return Err(slot_empty("image.move", slot));
                };
                item.x = x;
                item.y = y;
                s.cues.push(ImageCue::Move { slot, seconds });
                Ok(())
            },
        )
        .doc("枠の画像を、seconds 秒かけて (x, y) へ動かす（枠が空なら実行時エラー）")
        .param_names(&["slot", "x", "y", "seconds"])
        .defaults(&[("seconds", Value::Float(0.0))]);
        reg.effect(
            "image.hide",
            |ctx: &mut Ctx<Image>, slot: String, fade: f64| {
                non_negative("image.hide", "fade", fade)?;
                let s = ctx.state_mut();
                // If the slot is empty, do nothing
                if let Some(i) = s.items.iter().position(|i| i.slot == slot) {
                    s.items.remove(i);
                    s.cues.push(ImageCue::Hide { slot, fade });
                }
                Ok::<(), CommandError>(())
            },
        )
        .doc("枠の画像を消す（枠が空なら何もしない）")
        .param_names(&["slot", "fade"])
        .defaults(&[("fade", Value::Float(0.0))]);
        reg.effect("image.clear", |ctx: &mut Ctx<Image>, fade: f64| {
            non_negative("image.clear", "fade", fade)?;
            let s = ctx.state_mut();
            // If already empty, do nothing
            if !s.items.is_empty() {
                s.items.clear();
                s.cues.push(ImageCue::Clear { fade });
            }
            Ok::<(), CommandError>(())
        })
        .doc("すべての枠を空にする（すでに空なら何もしない）")
        .param_names(&["fade"])
        .defaults(&[("fade", Value::Float(0.0))]);
        reg.effect(
            "image.layer",
            |ctx: &mut Ctx<Image>, slot: String, layer: i64| {
                let s = ctx.state_mut();
                let Some(item) = s.items.iter_mut().find(|i| i.slot == slot) else {
                    return Err(slot_empty("image.layer", slot));
                };
                item.layer = layer;
                s.cues.push(ImageCue::Layer { slot });
                Ok(())
            },
        )
        .doc("枠の重なりの値を変える（小さいほど奥。同じ値なら足した順。枠が空なら実行時エラー）")
        .param_names(&["slot", "layer"]);
        reg.effect(
            "image.scale",
            |ctx: &mut Ctx<Image>, slot: String, scale: f64, seconds: f64| {
                positive("image.scale", "scale", scale)?;
                non_negative("image.scale", "seconds", seconds)?;
                let s = ctx.state_mut();
                let Some(item) = s.items.iter_mut().find(|i| i.slot == slot) else {
                    return Err(slot_empty("image.scale", slot));
                };
                item.scale = scale;
                s.cues.push(ImageCue::Scale { slot, seconds });
                Ok(())
            },
        )
        .doc("枠の画像の拡大率を、seconds 秒かけて変える（1.0 が元の大きさ。枠が空なら実行時エラー）")
        .param_names(&["slot", "scale", "seconds"])
        .defaults(&[("seconds", Value::Float(0.0))]);
        reg.effect(
            "image.flip",
            |ctx: &mut Ctx<Image>, slot: String, flip: bool| {
                let s = ctx.state_mut();
                let Some(item) = s.items.iter_mut().find(|i| i.slot == slot) else {
                    return Err(slot_empty("image.flip", slot));
                };
                item.flip = flip;
                s.cues.push(ImageCue::Flip { slot });
                Ok(())
            },
        )
        .doc("枠の画像を、左右に反転するかを変える（枠が空なら実行時エラー）")
        .param_names(&["slot", "flip"]);
        reg.effect(
            "image.alpha",
            |ctx: &mut Ctx<Image>, slot: String, alpha: f64, seconds: f64| {
                unit_range("image.alpha", "alpha", alpha)?;
                non_negative("image.alpha", "seconds", seconds)?;
                let s = ctx.state_mut();
                let Some(item) = s.items.iter_mut().find(|i| i.slot == slot) else {
                    return Err(slot_empty("image.alpha", slot));
                };
                item.alpha = alpha;
                s.cues.push(ImageCue::Alpha { slot, seconds });
                Ok(())
            },
        )
        .doc("枠の画像の不透明度（0.0〜1.0）を、seconds 秒かけて変える（枠が空なら実行時エラー）")
        .param_names(&["slot", "alpha", "seconds"])
        .defaults(&[("seconds", Value::Float(0.0))]);
        reg.query("image.shown", |ctx: &Ctx<Image>, slot: String| -> bool {
            ctx.state().items.iter().any(|i| i.slot == slot)
        })
        .doc("枠に画像があるか")
        .param_names(&["slot"]);
        reg.query(
            "image.name",
            |ctx: &Ctx<Image>, slot: String| -> Option<String> {
                ctx.state()
                    .items
                    .iter()
                    .find(|i| i.slot == slot)
                    .map(|i| i.name.clone())
            },
        )
        .doc("枠の画像の名前（枠が空なら None）")
        .param_names(&["slot"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yomi_core::encode_slice;

    #[test]
    fn version_1_state_reads_as_default_transform_image() {
        let old = ImageStateV1 {
            items: vec![ItemV1 {
                slot: "bg".into(),
                name: "room".into(),
                x: 0.5,
                y: 0.25,
            }],
            cues: vec![ImageCue::Show {
                slot: "bg".into(),
                fade: 1.0,
            }],
        };
        let raw = encode_slice(&old).unwrap();
        let state = Image::migrate(1, &raw).unwrap();
        assert_eq!(
            state.items,
            vec![Item::new("bg".into(), "room".into(), 0.5, 0.25)]
        );
        assert_eq!(state.items[0].scale, 1.0);
        assert_eq!(state.items[0].alpha, 1.0);
        assert_eq!(state.cues.len(), 1);
        assert!(Image::migrate(3, &raw).is_err());
    }
}
