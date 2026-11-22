//! map and for: a model of insertion order, and the continuation is the same after saving and restoring

use super::*;

/// A Rust-side model of map and list. Does the same operations as the script at the same time and compares the output.
#[derive(Default)]
pub(super) struct MapModel {
    pub(super) m: Vec<(i64, i64)>,
    pub(super) xs: Vec<i64>,
}

impl MapModel {
    pub(super) fn upsert(&mut self, k: i64, v: i64) {
        match self.m.iter_mut().find(|e| e.0 == k) {
            Some(e) => e.1 = v,
            None => self.m.push((k, v)),
        }
    }

    pub(super) fn remove(&mut self, k: i64) {
        self.m.retain(|e| e.0 != k);
    }

    /// The same output as `${m} ${xs}` in the script.
    pub(super) fn text(&self) -> String {
        let m = if self.m.is_empty() {
            "map {}".to_string()
        } else {
            let parts: Vec<String> = self.m.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            format!("map {{ {} }}", parts.join(", "))
        };
        let xs: Vec<String> = self.xs.iter().map(i64::to_string).collect();
        format!("{m} [{}]", xs.join(", "))
    }
}

pub(super) const MAP_HEAD: &str = "save var m: map<int, int> = map {}
save var xs: list<int> = []
save var names: map<str, list<int>> = map {}
";

/// Makes a valid script that uses map and for, and the bodies that should appear at each stop point.
pub(super) fn gen_map_script(rng: &mut Rng) -> (String, Vec<String>) {
    let mut src = String::from(MAP_HEAD);
    src.push_str("label start {\n");
    let mut model = MapModel::default();
    let mut expect = Vec::new();
    let mut ids = 0;
    let n = 4 + rng.below(14);
    for _ in 0..n {
        match rng.below(12) {
            0 | 1 => {
                let (k, v) = (rng.range(0, 6), rng.range(-5, 9));
                src.push_str(&format!("  m[{k}] = {v}\n"));
                model.upsert(k, v);
            }
            2 => {
                let k = rng.range(0, 6);
                src.push_str(&format!("  m.remove({k})\n"));
                model.remove(k);
            }
            3 => {
                let (k, d) = (rng.range(0, 6), rng.range(1, 4));
                src.push_str(&format!("  if m.has({k}) {{\n    m[{k}] += {d}\n  }}\n"));
                if let Some(e) = model.m.iter_mut().find(|e| e.0 == k) {
                    e.1 += d;
                }
            }
            4 | 5 => {
                if model.xs.len() < 8 {
                    let v = rng.range(0, 6);
                    src.push_str(&format!("  xs.push({v})\n"));
                    model.xs.push(v);
                }
            }
            6 => {
                // It iterates the copy taken at the start, so changing xs in the body does not change the number of iterations
                let copy = model.xs.clone();
                let grow = copy.len() * 2 <= 8;
                src.push_str("  for x in xs {\n");
                src.push_str(
                    "    if m.has(x) {\n      m[x] += 1\n    } else {\n      m[x] = 1\n    }\n",
                );
                if grow {
                    src.push_str("    xs.push(x)\n");
                }
                src.push_str("  }\n");
                for x in copy {
                    match model.m.iter_mut().find(|e| e.0 == x) {
                        Some(e) => e.1 += 1,
                        None => model.m.push((x, 1)),
                    }
                    if grow {
                        model.xs.push(x);
                    }
                }
            }
            7 => {
                let (a, b) = (rng.range(-1, 3), rng.range(0, 5));
                src.push_str(&format!("  for i in range({a}, {b}) {{\n"));
                src.push_str("    if i == 2 {\n      continue\n    }\n");
                src.push_str("    m[i] = i * 2\n");
                src.push_str("  }\n");
                for i in a..b {
                    if i != 2 {
                        model.upsert(i, i * 2);
                    }
                }
            }
            8 => {
                if rng.chance(1, 4) {
                    src.push_str("  m.clear()\n");
                    model.m.clear();
                }
            }
            9 => {
                // Lists inside values are also independent in copies
                let k = rng.pick(&["a", "b", "案内人"]);
                ids += 1;
                src.push_str(&format!(
                    "  names[\"{k}\"] = [m.len()]\n  let t{ids} = names\n  t{ids}[\"{k}\"] = []\n"
                ));
            }
            10 => {
                // Insert including the edges of the range (0 and `len`)
                if model.xs.len() < 8 {
                    let (i, v) = (rng.below(model.xs.len() + 1), rng.range(0, 6));
                    src.push_str(&format!("  xs.insert({i}, {v})\n"));
                    model.xs.insert(i, v);
                }
            }
            _ => {
                ids += 1;
                src.push_str(&format!("  ${{m}} ${{xs}} #s{ids}\n"));
                expect.push(model.text());
            }
        }
    }
    ids += 1;
    src.push_str(&format!("  ${{m}} ${{xs}} #s{ids}\n}}\n"));
    expect.push(model.text());
    (src, expect)
}

#[test]
fn map_and_for_scripts_print_same_as_insertion_order_model() {
    let reg = registry();
    let (mut with_for, mut with_remove, mut with_insert) = (0, 0, 0);
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let (src, expect) = gen_map_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let Ok(mut vm) = Vm::new(&prog, &reg, config()) else {
            panic!("seed={seed}: 作れない");
        };
        let mut got = Vec::new();
        loop {
            let y = vm
                .run()
                .unwrap_or_else(|e| panic!("seed={seed}: {:?}\n{src}", e.kind));
            match y {
                Yield::End => break,
                Yield::Stop { ref args, .. } => {
                    if let Some(Value::Str(t)) = args.first() {
                        got.push(t.trim().to_string());
                    }
                    let v = answer(&reg, &y);
                    vm.resume(Input::Return(v)).expect("再開できる");
                }
                _ => {}
            }
        }
        assert_eq!(got, expect, "seed={seed}\n{src}");
        with_for += src.contains("for ") as usize;
        with_remove += src.contains(".remove(") as usize;
        with_insert += src.contains(".insert(") as usize;
    }
    let n = case_count();
    assert!(
        with_for * 3 >= n && with_remove * 3 >= n && with_insert * 3 >= n,
        "{with_for} {with_remove} {with_insert} / {n}"
    );
}

#[test]
fn map_and_for_scripts_continue_identically_after_save_and_restore() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let (src, _) = gen_map_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        let what = format!("map seed={seed}");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}

/// It can be saved and restored even in the middle of a `for` body (at a stop point inside the body).
#[test]
fn saving_in_for_body_restores_same_continuation() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let k = rng.range(1, 6);
        let src = format!(
            "save var m: map<int, int> = map {{}}\n\
             label start {{\n  for i in range(0, {k}) {{\n    for j in range(0, 2) {{\n      m[i] = j\n      ${{m}} ${{i}}${{j}} #s{k}\n    }}\n  }}\n  ${{m}} #end\n}}\n"
        );
        let prog = compile_ok(&src, &reg);
        let what = format!("for seed={seed}");
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| check_save_restore(&prog, &reg, &what))) {
            eprintln!("--- 失敗した台本（{what}）\n{src}");
            std::panic::resume_unwind(p);
        }
    }
}
