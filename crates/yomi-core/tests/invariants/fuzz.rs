//! Tests: mutated scripts and random input

use super::*;

pub(super) const SPECIAL: &[char] = &[
    '{', '}', '「', '」', '\\', '$', '#', '@', '"', '(', ')', ',', ':', '=', '/', '*', '%', '+',
    '-', '<', '>', '!', '&', '|', '　', ' ', '\t', '\n', '\r', '\u{feff}', '0', '9', 'a', 'あ',
    '𠮷', '\u{0}',
];

pub(super) const FRAGMENTS: &[&str] = &[
    "fn f(x: int) -> int {",
    "fn g() {",
    "return f(1)",
    "return ",
    "f(f(1))",
    "main.f(2)",
    "@f(3)",
    "label ",
    "start",
    " {",
    "}",
    "\n",
    "\n  ",
    "let ",
    "if ",
    "else",
    "else if ",
    "jump ",
    "end",
    "save var ",
    ": int = ",
    "@game.pause()",
    "@ui.say(",
    "${",
    "}",
    "\"",
    "99999999999999999999",
    " #id",
    "// ",
    "「",
    "」",
    "import ",
    "while ",
    "break",
    "continue",
    "1.5",
    "0.0 ",
    "1_.5",
    "to_int(",
    "to_float(",
    "call ",
    "choice ",
    "+= ",
    "== ",
    "&& ",
];

pub(super) fn random_text(rng: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..rng.below(40) {
        match rng.below(3) {
            0 => s.push(*rng.pick(SPECIAL)),
            1 => s.push_str(rng.pick(FRAGMENTS)),
            _ => s.push(char::from_u32(rng.range(0x20, 0x3100) as u32).unwrap_or('?')),
        }
    }
    s
}

/// Breaks a valid script in only 1 to 3 places.
pub(super) fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    for _ in 0..(1 + rng.below(3)) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(7) {
            0 => {
                chars.remove(at);
            }
            1 => chars[at] = *rng.pick(SPECIAL),
            2 => chars.insert(at, *rng.pick(SPECIAL)),
            3 => {
                for (i, c) in rng.pick(FRAGMENTS).chars().enumerate() {
                    chars.insert(at + i, c);
                }
            }
            4 => {
                // Delete a line
                let start = chars[..at]
                    .iter()
                    .rposition(|&c| c == '\n')
                    .map_or(0, |i| i + 1);
                let end = chars[at..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(chars.len(), |i| at + i + 1);
                chars.drain(start..end);
            }
            5 => chars.truncate(at),
            _ => {
                // Duplicate a line
                let start = chars[..at]
                    .iter()
                    .rposition(|&c| c == '\n')
                    .map_or(0, |i| i + 1);
                let end = chars[at..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(chars.len(), |i| at + i + 1);
                let line: Vec<char> = chars[start..end].to_vec();
                for (i, c) in line.into_iter().enumerate() {
                    chars.insert(end + i, c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

/// Compiles and checks the shape of the result.
pub(super) fn compile_checked(src: &str, reg: &Registry) -> Option<Program> {
    match compile(src, "main", reg) {
        Ok(p) => Some(p),
        Err(diags) => {
            assert!(!diags.is_empty(), "Err なのに診断がない: {src:?}");
            let keys: Vec<(u32, u32)> = diags.iter().map(|d| (d.line, d.col)).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "診断が位置の順でない: {src:?}");
            for d in &diags {
                assert!(d.line >= 1 && d.col >= 1, "位置が 0: {src:?}");
                // The public specification of diagnostics (severity and argument names per code) matches the actual diagnostics
                assert_eq!(d.severity, d.code.severity(), "{}: 重大度", d.code.as_str());
                for (name, _) in &d.args {
                    assert!(
                        d.code.arg_names().contains(name),
                        "{}: 仕様にない引数 {name}: {src:?}",
                        d.code.as_str()
                    );
                }
            }
            None
        }
    }
}

/// The whole flow for mutated scripts and random input. If `run` is true, runs the resulting script.
pub(super) fn fuzz_inputs(run: bool) {
    let reg = registry();
    let mut ok = 0usize;
    let n = case_count();
    for case in 0..n {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0xF00D);
        let valid = gen_script(&mut Rng::new(seed));
        let inputs = [
            random_text(&mut rng),
            mutate(&valid, &mut rng),
            mutate(&valid, &mut rng),
            mutate(&mutate(&valid, &mut rng), &mut rng),
        ];
        for src in inputs {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let Some(prog) = compile_checked(&src, &reg) else {
                    return false;
                };
                if run {
                    if let Ok(mut vm) = Vm::new(&prog, &reg, config()) {
                        let t = trace(&mut vm, &reg, 600);
                        // Runtime errors are returned as `Err` (with a code)
                        for s in &t {
                            if let Err(code) = s {
                                assert!(!code.is_empty());
                            }
                        }
                    }
                    check_save_restore(&prog, &reg, "変形した台本");
                }
                true
            }));
            match r {
                Ok(true) => ok += 1,
                Ok(false) => {}
                Err(p) => {
                    eprintln!("--- パニックした入力（seed={seed}）\n{src:?}\n{src}");
                    std::panic::resume_unwind(p);
                }
            }
        }
    }
    // Even after mutation, some input still compiles (to confirm the mutation is not too strong)
    if n >= 100 {
        assert!(ok > 0, "コンパイルできた入力が 1 つもない");
    }
}

#[test]
fn compile_never_panics_on_any_input() {
    fuzz_inputs(false);
}

#[test]
fn compiled_scripts_never_panic_within_limits() {
    fuzz_inputs(true);
}

#[test]
fn compile_does_not_panic_on_empty_short_or_special_line_starts() {
    let reg = registry();
    let mut inputs: Vec<String> = vec![
        String::new(),
        "\n".into(),
        "\r\n".into(),
        "\u{feff}".into(),
        "}".into(),
        "{".into(),
        "label".into(),
        "label start".into(),
        "label start {".into(),
        "label start {\n".into(),
        "label start {\n  \"".into(),
        "label start {\n  ${".into(),
        "label start {\n  ${}".into(),
        "label start {\n  \\".into(),
        "label start {\n  @".into(),
        "label start {\n  @game.pause(".into(),
        "label start {\n  @game.pause() #".into(),
        "label start {\n  if".into(),
        "label start {\n  if true {\n  } else".into(),
        "label start {\n  if true {\n  } else {".into(),
        "label start {\n  let".into(),
        "label start {\n  let x".into(),
        "label start {\n  let x =".into(),
        "label start {\n  x +=".into(),
        "save".into(),
        "save var".into(),
        "save var x".into(),
        "save var x:".into(),
        "save var x: int".into(),
        "save var x: int =".into(),
        "label start {\n  ${99999999999999999999999}\n}".into(),
        "label start {\n  ${\"\\".into(),
    ];
    // All truncations
    let sample = "save var n: int = 1\nlabel start {\n  let a = n + 1\n  if a > 1 {\n    あ${a}「\\」 #x\n  } else {\n    @game.give(\"a\", 2)\n  }\n  jump start\n}\n";
    for end in 0..=sample.len() {
        if sample.is_char_boundary(end) {
            inputs.push(sample[..end].to_string());
        }
    }
    for src in inputs {
        let r = catch_unwind(AssertUnwindSafe(|| {
            let _ = compile(&src, "main", &reg);
        }));
        assert!(r.is_ok(), "パニック: {src:?}");
    }
}
