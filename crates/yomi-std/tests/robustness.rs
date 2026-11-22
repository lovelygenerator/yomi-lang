//! Robustness tests: broken input (scripts, saves, persistent data, declaration files, text tables) does not panic.
//!
//! Breaks scripts and bytes randomly with a fixed random generator and tries compiling, running and loading. Being
//! returned as an error is fine; only a panic is a failure. The number of iterations is changed with the environment
//! variable `YOMI_FUZZ_ITERS` (small by default), and the seed with `YOMI_FUZZ_SEED`.
//! For exploration: `YOMI_FUZZ_ITERS=20000 cargo test --release -p yomi-std --test robustness`.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use yomi_core::{
    CompileOptions, Identity, Input, Registry, Sugar, TextTable, Value, Vm, VmConfig, Yield,
    compile_modules_with_options,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn seed() -> u64 {
    std::env::var("YOMI_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn iters(default: usize) -> usize {
    std::env::var("YOMI_FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("ui.say_as")
        .unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
        .unwrap();
    yomi_std::register_all(&mut r).unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.bind(Sugar::Quote, "ui.say_as").unwrap();
    r.bind(Sugar::Choice, "ui.choose").unwrap();
    r
}

fn mini_game_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mini-game")
}

/// All modules of the mini game `(name, source)`.
fn mini_game() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(mini_game_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "yomi").then(|| {
                (
                    p.file_stem().unwrap().to_string_lossy().into_owned(),
                    fs::read_to_string(&p).unwrap(),
                )
            })
        })
        .collect();
    v.sort();
    v
}

/// The golden scripts of std (single module).
fn golden_scripts() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "yomi").then(|| fs::read_to_string(&p).unwrap())
        })
        .collect();
    v.sort();
    v
}

const TOKENS: &[&str] = &[
    "{",
    "}",
    "(",
    ")",
    "「",
    "」",
    "@",
    "label ",
    "jump ",
    "call ",
    "let ",
    "save var ",
    "persistent var ",
    "fn ",
    "if ",
    "else ",
    "while ",
    "for ",
    "in ",
    "match ",
    "choice ",
    "=>",
    "${",
    "#id",
    "import ",
    "return ",
    "break",
    "continue",
    "struct ",
    "enum ",
    "map ",
    "[",
    "]",
    ",",
    ":",
    ".",
    "::",
    "=",
    "==",
    "+",
    "-",
    "*",
    "/",
    "%",
    "!",
    "&&",
    "||",
    "<",
    ">",
    "0",
    "-1",
    "9223372036854775807",
    "1.5",
    "\"",
    "\"x\"",
    "\n",
    "  ",
    "\u{3000}",
    "こんにちは",
    "\u{200d}",
    "\u{0301}",
    "Some(",
    "None",
    "speaker ",
    "end",
    "once",
    "seen(",
    "visited(",
];

/// Breaks it while keeping character (`char`) boundaries.
fn mutate_text(rng: &mut Rng, src: &str) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    for _ in 0..1 + rng.below(3) {
        if chars.is_empty() {
            chars.push('a');
        }
        let at = rng.below(chars.len());
        match rng.below(6) {
            0 => {
                let n = (1 + rng.below(40)).min(chars.len() - at);
                chars.drain(at..(at + n).min(chars.len()));
            }
            1 | 2 => {
                let t: Vec<char> = TOKENS[rng.below(TOKENS.len())].chars().collect();
                for (i, c) in t.into_iter().enumerate() {
                    chars.insert((at + i).min(chars.len()), c);
                }
            }
            3 => {
                chars[at] = TOKENS[rng.below(TOKENS.len())]
                    .chars()
                    .next()
                    .unwrap_or('x')
            }
            4 => {
                // Duplicate a line
                let start = chars[..at]
                    .iter()
                    .rposition(|&c| c == '\n')
                    .map_or(0, |i| i + 1);
                let end = chars[at..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(chars.len(), |i| at + i);
                let line: Vec<char> = chars[start..end].to_vec();
                for (i, c) in line.into_iter().chain(['\n']).enumerate() {
                    chars.insert((end + 1 + i).min(chars.len()), c);
                }
            }
            _ => {
                // Cut out a range and move it elsewhere
                let n = (1 + rng.below(30)).min(chars.len() - at);
                let piece: Vec<char> = chars.drain(at..(at + n).min(chars.len())).collect();
                let to = rng.below(chars.len() + 1);
                for (i, c) in piece.into_iter().enumerate() {
                    chars.insert((to + i).min(chars.len()), c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

fn mutate_bytes(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    let mut b = src.to_vec();
    for _ in 0..1 + rng.below(4) {
        if b.is_empty() {
            b.push(0);
        }
        let at = rng.below(b.len());
        match rng.below(6) {
            0 => b[at] ^= 1 << rng.below(8),
            1 => b[at] = rng.next() as u8,
            2 => b.truncate(at),
            3 => {
                let n = 1 + rng.below(8);
                for _ in 0..n {
                    b.insert(at.min(b.len()), rng.next() as u8);
                }
            }
            4 => {
                let n = (1 + rng.below(8)).min(b.len() - at);
                b.drain(at..at + n);
            }
            _ => b[at] = [0x00, 0xff, 0x7f, 0x80][rng.below(4)],
        }
    }
    b
}

fn crc32(data: &[u8]) -> u32 {
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

/// Breaks only the body inside the outer frame (4-byte marker, 4-byte version, body, 4-byte CRC-32) and recomputes the
/// CRC (without a matching CRC, it never gets as far as reading the body). Once every 5 times, breaks the frame as well.
fn mutate_container(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    if rng.below(5) == 0 || src.len() < 12 {
        return mutate_bytes(rng, src);
    }
    let body = &src[8..src.len() - 4];
    let mut out = src[..8].to_vec();
    out.extend(mutate_bytes(rng, body));
    let crc = crc32(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Advances by returning values of matching types at stop points, up to `steps` times. At each stop point, also tries
/// whether it can be saved and read back.
fn drive(vm: &mut Vm<'_>, reg: &Registry, steps: usize, with_save: bool) {
    for _ in 0..steps {
        match vm.run() {
            Ok(Yield::Stop { command, .. }) => {
                if with_save {
                    let _ = vm.save();
                    let _ = vm.snapshot();
                }
                let name = reg.command(command).name.clone();
                let v = match name.as_str() {
                    "ui.choose" => Value::Int(0),
                    "input.text" => Value::Str("a".into()),
                    "input.integer" => Value::Int(1),
                    "input.confirm" => Value::Bool(true),
                    _ => Value::Unit,
                };
                if vm.resume(Input::Return(v)).is_err() {
                    return;
                }
            }
            Ok(_) | Err(_) => return,
        }
    }
}

/// Runs on a thread with an 8 MiB stack (so deep nesting is not affected by the 2 MiB default of tests).
fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

fn report(kind: &str, panics: &[String]) {
    if !panics.is_empty() {
        for p in panics.iter().take(3) {
            eprintln!("--- {kind}: パニックした入力 ---\n{p}\n");
        }
        panic!("{kind}: {} 件のパニック", panics.len());
    }
}

#[test]
fn broken_scripts_never_panic_on_compile_or_run() {
    on_big_stack(|| {
        let reg = registry();
        let game = mini_game();
        let goldens = golden_scripts();
        let n = iters(150);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed());
        let mut panics = Vec::new();
        let mut compiled = 0usize;
        for i in 0..n {
            // Half the time break one module of the mini game, otherwise break golden scripts
            let (names, srcs): (Vec<String>, Vec<String>) = if i % 2 == 0 {
                let k = rng.below(game.len());
                let mut mods = game.clone();
                mods[k].1 = mutate_text(&mut rng, &mods[k].1);
                mods.into_iter().unzip()
            } else {
                let k = rng.below(goldens.len());
                let s = mutate_text(&mut rng, &goldens[k]);
                (vec!["main".to_string()], vec![s])
            };
            let modules: Vec<(&str, &str)> = names
                .iter()
                .map(String::as_str)
                .zip(srcs.iter().map(String::as_str))
                .collect();
            let r = catch_unwind(AssertUnwindSafe(|| {
                if let Ok((prog, _)) =
                    compile_modules_with_options(&modules, &reg, &CompileOptions::default())
                {
                    let cfg = VmConfig::default()
                        .max_steps_per_run(Some(50_000))
                        .start("main.start");
                    if let Ok(mut vm) = Vm::new(&prog, &reg, cfg) {
                        drive(&mut vm, &reg, 80, i % 5 == 0);
                    }
                    return true;
                }
                false
            }));
            if matches!(r, Ok(true)) {
                compiled += 1;
            }
            if r.is_err() {
                panics.push(srcs.join("\n=====\n"));
            }
        }
        eprintln!("コンパイルが通った（壊しても通った）: {compiled}/{n}");
        report("台本", &panics);
    });
}

/// Advances the mini game a few moves and makes the bytes of a save and persistent data.
fn sample_saves(reg: &Registry) -> (Vec<Vec<u8>>, Vec<u8>) {
    let game = mini_game();
    let modules: Vec<(&str, &str)> = game.iter().map(|(n, s)| (n.as_str(), s.as_str())).collect();
    let (prog, _) =
        compile_modules_with_options(&modules, reg, &CompileOptions::default()).expect("通る");
    let cfg = VmConfig::default().start("main.start");
    let mut vm = Vm::new(&prog, reg, cfg).unwrap();
    let mut saves = Vec::new();
    for step in 0..60 {
        match vm.run() {
            Ok(Yield::Stop { command, .. }) => {
                if step % 6 == 0 {
                    saves.push(vm.save().expect("保存できる"));
                }
                let name = reg.command(command).name.clone();
                let v = match name.as_str() {
                    "ui.choose" => Value::Int(step % 2),
                    "input.text" => Value::Str("a".into()),
                    "input.integer" => Value::Int(1),
                    "input.confirm" => Value::Bool(true),
                    _ => Value::Unit,
                };
                vm.resume(Input::Return(v)).expect("再開できる");
            }
            _ => break,
        }
    }
    (saves, vm.save_persistent())
}

#[test]
fn broken_saves_and_persistent_data_never_panic_on_load_or_continue() {
    on_big_stack(|| {
        let reg = registry();
        let game = mini_game();
        let modules: Vec<(&str, &str)> =
            game.iter().map(|(n, s)| (n.as_str(), s.as_str())).collect();
        let (prog, _) =
            compile_modules_with_options(&modules, &reg, &CompileOptions::default()).unwrap();
        let (saves, persistent) = sample_saves(&reg);
        assert!(!saves.is_empty());
        let n = iters(400);
        let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ seed());
        let mut panics = Vec::new();
        let (mut loaded, mut p_loaded) = (0usize, 0usize);
        for i in 0..n {
            let base = &saves[rng.below(saves.len())];
            let bytes = mutate_container(&mut rng, base);
            let r = catch_unwind(AssertUnwindSafe(|| {
                let cfg = VmConfig::default().max_steps_per_run(Some(50_000));
                if let Ok((mut vm, _)) = Vm::load(&prog, &reg, cfg, &bytes) {
                    drive(&mut vm, &reg, 30, false);
                    return true;
                }
                false
            }));
            if matches!(r, Ok(true)) {
                loaded += 1;
            }
            if r.is_err() {
                panics.push(format!("セーブ: {bytes:?}"));
            }
            // Persistent data
            let pbytes = mutate_container(&mut rng, &persistent);
            let r = catch_unwind(AssertUnwindSafe(|| {
                let cfg = VmConfig::default()
                    .start("main.start")
                    .persistent_data(pbytes.clone());
                if let Ok(mut vm) = Vm::new(&prog, &reg, cfg) {
                    drive(&mut vm, &reg, 20, false);
                    let _ = vm.load_persistent(&pbytes);
                    return true;
                }
                false
            }));
            if matches!(r, Ok(true)) {
                p_loaded += 1;
            }
            if r.is_err() {
                panics.push(format!("永続データ: {pbytes:?}"));
            }
            let _ = i;
        }
        eprintln!(
            "読み込めた（壊れていても受け入れた）: セーブ {loaded}/{n}、永続データ {p_loaded}/{n}"
        );
        report("セーブ・永続データ", &panics);
    });
}

#[test]
fn broken_declaration_files_and_text_tables_do_not_panic() {
    on_big_stack(|| {
        let reg = registry();
        let decls = reg.export_decls();
        let table = fs::read_to_string(mini_game_dir().join("en.texts")).unwrap();
        let game = mini_game();
        let modules: Vec<(&str, &str)> =
            game.iter().map(|(n, s)| (n.as_str(), s.as_str())).collect();
        let (prog, _) =
            compile_modules_with_options(&modules, &reg, &CompileOptions::default()).unwrap();
        let n = iters(300);
        let mut rng = Rng(0x2545_F491_4F6C_DD1D ^ seed());
        let mut panics = Vec::new();
        for _ in 0..n {
            let d = mutate_text(&mut rng, &decls);
            let t = mutate_text(&mut rng, &table);
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = Registry::from_decls(&d);
                if let Ok(tab) = TextTable::from_file(&t) {
                    let _ = tab.check(&prog);
                }
            }));
            if r.is_err() {
                panics.push(format!("宣言:\n{d}\n=====\n表:\n{t}"));
            }
        }
        report("宣言ファイル・差し替え表", &panics);
    });
}
