//! Test-only packages (tests of `yomi-core` cannot depend on `yomi-std`, so small packages for checking the mechanisms
//! are placed here).
//!
//! - `Counter`: has state (`n`, `log`) and a persistent slice (`total`). Has effect, query and validated stop commands and
//!   host-declared variables.
//! - `Other`: has only state; it is depended on.
//! - `Mirror`: depends on `Counter` (reads its state). It does not declare `Other`, so reading it is a runtime error.
//! - [`VerV1`], [`VerV2`]: two definitions with the same name `ver` and different versions (for checking `migrate`).

#![allow(
    dead_code,
    reason = "テストの実行ファイルごとに、使う部分が違う（expect にすると、全部を使う実行ファイルで外れる）"
)]

pub mod rng;

use serde::{Deserialize, Serialize};
use yomi_core::{
    CommandError, Ctx, Identity, Kind, MigrateError, Package, PackageReg, Registry, Sugar,
    decode_slice,
};

/// The state slice of `Counter`.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct CounterState {
    pub n: i64,
    pub log: Vec<String>,
}

/// The persistent slice of `Counter`.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct CounterPersistent {
    pub total: i64,
}

/// The basic test package with state.
pub struct Counter;

impl Package for Counter {
    const NAME: &'static str = "counter";
    type State = CounterState;
    type Persistent = CounterPersistent;

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect(
            "counter.inc",
            |ctx: &mut Ctx<Counter>, by: i64| -> Result<(), CommandError> {
                let next = ctx
                    .state()
                    .n
                    .checked_add(by)
                    .ok_or_else(CommandError::integer_overflow)?;
                let s = ctx.state_mut();
                s.n = next;
                s.log.push(format!("inc {by}"));
                Ok(())
            },
        );
        reg.effect("counter.note", |ctx: &mut Ctx<Counter>, text: String| {
            ctx.state_mut().log.push(text);
        });
        reg.effect("counter.add_total", |ctx: &mut Ctx<Counter>, by: i64| {
            ctx.persistent_mut().total += by;
        });
        // Draw a random number, write it to the state, and also return the value (`@` statements discard the value)
        reg.effect("counter.roll", |ctx: &mut Ctx<Counter>, max: i64| -> i64 {
            let v = ctx.rng().range_inclusive(1, max);
            ctx.state_mut().n = v;
            v
        });
        reg.effect(
            "counter.fail",
            |_ctx: &mut Ctx<Counter>, message: String| -> Result<(), CommandError> {
                Err(CommandError::with_code(
                    "counter.failed",
                    [("message", message.into())],
                ))
            },
        );
        reg.query("counter.get", |ctx: &Ctx<Counter>| -> i64 { ctx.state().n });
        reg.query("counter.total", |ctx: &Ctx<Counter>| -> i64 {
            ctx.persistent().total
        });
        reg.query(
            "counter.describe",
            |ctx: &Ctx<Counter>, prefix: String| -> String { format!("{prefix}{}", ctx.state().n) },
        );
        reg.query(
            "counter.divide",
            |_ctx: &Ctx<Counter>, a: f64, b: f64| -> Result<f64, CommandError> {
                if b == 0.0 {
                    return Err(CommandError::division_by_zero());
                }
                Ok(a / b)
            },
        );
        // 6 arguments
        reg.query(
            "counter.six",
            |_ctx: &Ctx<Counter>,
             a: String,
             b: i64,
             c: bool,
             d: f64,
             e: String,
             f: i64|
             -> String { format!("{a}{b}{c}{d}{e}{f}") },
        );
        reg.stop_checked::<(i64,), (), _>("counter.wait", |(n,)| {
            if n < 0 {
                Err(CommandError::with_code(
                    "counter.negative_wait",
                    [("count", n.into())],
                ))
            } else {
                Ok(())
            }
        });
        reg.var::<i64>("counter.step", Kind::Save, 1);
        reg.var::<i64>("counter.best", Kind::Persistent, 0);
        reg.var::<bool>("counter.locked", Kind::Save, false)
            .read_only();
        reg.var::<String>("counter.title", Kind::Save, "無題".to_string());
    }
}

/// A package that has only state and is depended on.
pub struct Other;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct OtherState {
    pub value: i64,
}

impl Package for Other {
    const NAME: &'static str = "other";
    type State = OtherState;
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("other.set", |ctx: &mut Ctx<Other>, v: i64| {
            ctx.state_mut().value = v;
        });
        reg.query("other.get", |ctx: &Ctx<Other>| -> i64 { ctx.state().value });
    }
}

/// A package with the standard package mark (`Package::STD`) (imported with `import std::probe`).
pub struct Probe;

impl Package for Probe {
    const NAME: &'static str = "probe";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.query("probe.get", |_ctx: &Ctx<Probe>| -> i64 { 7 });
    }
}

/// A package that depends on `Counter`.
pub struct Mirror;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorState {
    pub copied: i64,
}

impl Package for Mirror {
    const NAME: &'static str = "mirror";
    const DEPENDS: &'static [&'static str] = &["counter"];
    type State = MirrorState;
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect(
            "mirror.copy",
            |ctx: &mut Ctx<Mirror>| -> Result<(), CommandError> {
                let n = ctx.dep::<Counter>()?.n;
                ctx.state_mut().copied = n;
                Ok(())
            },
        );
        reg.query(
            "mirror.double",
            |ctx: &Ctx<Mirror>| -> Result<i64, CommandError> {
                ctx.dep::<Counter>()?
                    .n
                    .checked_mul(2)
                    .ok_or_else(CommandError::integer_overflow)
            },
        );
        reg.query("mirror.copied", |ctx: &Ctx<Mirror>| -> i64 {
            ctx.state().copied
        });
        // `Other` is not declared in DEPENDS, so it is a runtime error
        reg.query(
            "mirror.peek_other",
            |ctx: &Ctx<Mirror>| -> Result<i64, CommandError> { Ok(ctx.dep::<Other>()?.value) },
        );
    }
}

/// `ver` version 1.
pub struct VerV1;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerStateV1 {
    pub count: i64,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerPersistentV1 {
    pub visits: i64,
}

impl Package for VerV1 {
    const NAME: &'static str = "ver";
    type State = VerStateV1;
    type Persistent = VerPersistentV1;

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("ver.bump", |ctx: &mut Ctx<VerV1>| {
            ctx.state_mut().count += 1;
        });
        reg.effect("ver.visit", |ctx: &mut Ctx<VerV1>| {
            ctx.persistent_mut().visits += 1;
        });
    }
}

/// `ver` version 2 (the shape of the state changed). Migrates from version 1.
pub struct VerV2;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerStateV2 {
    pub label: String,
    pub count: i64,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerPersistentV2 {
    pub visits: i64,
    pub note: String,
}

impl Package for VerV2 {
    const NAME: &'static str = "ver";
    const VERSION: u32 = 2;
    const PERSISTENT_VERSION: u32 = 2;
    type State = VerStateV2;
    type Persistent = VerPersistentV2;

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("ver.bump", |ctx: &mut Ctx<VerV2>| {
            ctx.state_mut().count += 1;
        });
        reg.effect("ver.visit", |ctx: &mut Ctx<VerV2>| {
            ctx.persistent_mut().visits += 1;
        });
        reg.query("ver.tag", |ctx: &Ctx<VerV2>| -> String {
            ctx.state().label.clone()
        });
    }

    fn migrate(old_version: u32, raw: &[u8]) -> Result<Self::State, MigrateError> {
        match old_version {
            1 => {
                let old: VerStateV1 = decode_slice(raw)?;
                Ok(VerStateV2 {
                    label: "旧版から".to_string(),
                    count: old.count,
                })
            }
            v => Err(MigrateError::unsupported_version(v)),
        }
    }

    fn migrate_persistent(old_version: u32, raw: &[u8]) -> Result<Self::Persistent, MigrateError> {
        match old_version {
            1 => {
                let old: VerPersistentV1 = decode_slice(raw)?;
                Ok(VerPersistentV2 {
                    visits: old.visits,
                    note: "旧版から".to_string(),
                })
            }
            v => Err(MigrateError::unsupported_version(v)),
        }
    }
}

/// The state slice of `Cued` (has cues `cues`).
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct CuedState {
    /// Cues (emptied by `clear_cues`).
    pub cues: Vec<String>,
    /// The number of times cues were added (the persistent part that is not emptied).
    pub count: i64,
}

/// A test package that holds cues (one-time events) in its state.
pub struct Cued;

impl Package for Cued {
    const NAME: &'static str = "cued";
    type State = CuedState;
    type Persistent = ();

    fn clear_cues(state: &mut CuedState) {
        state.cues.clear();
    }

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("cued.cue", |ctx: &mut Ctx<Cued>, name: String| {
            let s = ctx.state_mut();
            s.cues.push(name);
            s.count += 1;
        });
        reg.query("cued.count", |ctx: &Ctx<Cued>| -> i64 { ctx.state().count });
        // A stop command with return value checks: accepts only integers up to `max`
        reg.stop_validated::<(i64,), i64, _, _>(
            "cued.ask",
            |(max,)| {
                if *max < 0 {
                    Err(CommandError::with_code(
                        "cued.negative_max",
                        [("max", (*max).into())],
                    ))
                } else {
                    Ok(())
                }
            },
            |(max,), value| {
                if value <= max {
                    Ok(())
                } else {
                    Err(CommandError::with_code(
                        "cued.over_max",
                        [("max", (*max).into()), ("value", (*value).into())],
                    ))
                }
            },
        );
    }
}

/// The standard stop commands and sugar bindings (the same as `golden.rs`).
pub fn host_registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    r.stop::<(String, i64), ()>("game.give").unwrap();
    r.stop::<(i64,), i64>("game.roll").unwrap();
    r.stop::<(), ()>("game.pause").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

/// A registry with all test packages registered (`counter`, `other`, `mirror`).
pub fn full_registry() -> Registry {
    let mut r = host_registry();
    register_test_packages(&mut r, &["counter", "other", "mirror"]);
    r
}

/// Registers test packages chosen by name, in dependency order.
pub fn register_test_packages(r: &mut Registry, names: &[&str]) {
    let has = |n: &str| names.contains(&n);
    if has("counter") {
        r.package::<Counter>().unwrap();
    }
    if has("other") {
        r.package::<Other>().unwrap();
    }
    if has("mirror") {
        r.package::<Mirror>().unwrap();
    }
    if has("probe") {
        r.package::<Probe>().unwrap();
    }
}

/// `full_registry` with `cued` added.
pub fn cued_registry() -> Registry {
    let mut r = full_registry();
    r.package::<Cued>().unwrap();
    r
}

/// CRC-32 (IEEE). Used when tests recompute the check value of the outer frame of saves.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}
