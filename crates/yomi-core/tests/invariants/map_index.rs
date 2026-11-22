//! The map index: comparing with a model (Vec), consistency of the index, independence of copies

use super::*;

/// Applies random operations (update, remove, clear, clone) to `MapData` and compares with a `Vec` model that keeps
/// insertion order. After each operation, checks that the index does not disagree with the order and that lookups give
/// the same results as the model. Copies are not affected by later operations on the original.
#[test]
fn map_index_matches_insertion_order_model_after_random_operations() {
    use yomi_core::MapData;

    fn key(rng: &mut Rng) -> Value {
        match rng.below(3) {
            0 => Value::Int(rng.range(0, 12)),
            1 => Value::Str(format!("k{}", rng.range(0, 12)).into()),
            _ => Value::Bool(rng.chance(1, 2)),
        }
    }

    fn check(m: &MapData, model: &[(Value, Value)], what: &str) {
        assert!(m.is_consistent(), "{what}: 索引が食い違っている");
        assert!(m.entries().eq(model.iter()), "{what}: 並びが違う");
        assert_eq!(m.len(), model.len(), "{what}: 数");
        for (k, v) in model {
            assert_eq!(m.get(k), Some(v), "{what}: 検索");
        }
    }

    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let mut m = MapData::new();
        let mut model: Vec<(Value, Value)> = Vec::new();
        // The copy, and the model at that time
        let mut kept: Vec<(MapData, Vec<(Value, Value)>)> = Vec::new();
        for step in 0..80 {
            let what = format!("seed={seed} step={step}");
            match rng.below(10) {
                0..=3 => {
                    // Update or insert (`m[k] = v`)
                    let (k, v) = (key(&mut rng), Value::Int(rng.range(0, 99)));
                    match model.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 = v.clone(),
                        None => model.push((k.clone(), v.clone())),
                    }
                    m = MapData::from_pairs({
                        // Update only through public entry points (replacing an existing pair and rebuilding keeps the position)
                        let mut pairs: Vec<_> = m.entries().cloned().collect();
                        pairs.push((k, v));
                        pairs
                    });
                }
                4..=6 => {
                    // Lookup (including missing keys)
                    let k = key(&mut rng);
                    let want = model.iter().find(|e| e.0 == k).map(|e| &e.1);
                    assert_eq!(m.get(&k), want, "{what}: 検索");
                    assert_eq!(
                        m.entries().position(|e| e.0 == k),
                        model.iter().position(|e| e.0 == k),
                        "{what}: 位置"
                    );
                    assert_eq!(m.contains_key(&k), want.is_some(), "{what}: 有無");
                }
                7 => kept.push((m.clone(), model.clone())),
                8 => {
                    // Equality of values ignores insertion order
                    let mut rev = model.clone();
                    rev.reverse();
                    assert_eq!(m, MapData::from_pairs(rev), "{what}: 順序を無視した等しさ");
                }
                _ => {
                    if rng.chance(1, 6) {
                        m = MapData::new();
                        model.clear();
                    }
                }
            }
            check(&m, &model, &what);
        }
        // Operations after copying do not affect the copy
        for (c, cm) in &kept {
            check(c, cm, &format!("seed={seed} 複製"));
        }
    }
}

/// Applies random operations (push, insert, remove, clear, clone) to `ListData` and compares with a `Vec` model.
/// After each operation, checks that the contents and total size are the same as the model, and that copies do not change
/// with later operations.
#[test]
fn list_matches_model_after_random_operations_and_clones_are_independent() {
    use yomi_core::ListData;

    fn item(rng: &mut Rng) -> Value {
        match rng.below(3) {
            0 => Value::Int(rng.range(0, 99)),
            1 => Value::Str(format!("s{}", rng.range(0, 99)).into()),
            _ => Value::List(
                (0..rng.below(3))
                    .map(|_| Value::Int(rng.range(0, 9)))
                    .collect(),
            ),
        }
    }

    fn check(l: &ListData, model: &[Value], what: &str) {
        assert!(l.is_consistent(), "{what}: 大きさの合計が食い違っている");
        assert_eq!(l.as_slice(), model, "{what}: 中身が違う");
        assert_eq!(
            Value::List(l.clone()).approx_bytes(),
            24 + model.iter().map(Value::approx_bytes).sum::<usize>(),
            "{what}: 大きさ"
        );
    }

    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let mut l = ListData::new();
        let mut model: Vec<Value> = Vec::new();
        let mut kept: Vec<(ListData, Vec<Value>)> = Vec::new();
        for step in 0..80 {
            let what = format!("seed={seed} step={step}");
            match rng.below(10) {
                0..=3 => {
                    let v = item(&mut rng);
                    model.push(v.clone());
                    l.push(v);
                }
                4 => {
                    let v = item(&mut rng);
                    let at = rng.below(model.len() + 1);
                    model.insert(at, v.clone());
                    l.insert(at, v);
                }
                5 | 6 => {
                    if !model.is_empty() {
                        let at = rng.below(model.len());
                        assert_eq!(l.remove(at), model.remove(at), "{what}");
                    }
                }
                7 | 8 => kept.push((l.clone(), model.clone())),
                _ => {
                    if rng.chance(1, 6) {
                        l.clear();
                        model.clear();
                    }
                }
            }
            check(&l, &model, &what);
        }
        for (c, cm) in &kept {
            check(c, cm, &format!("seed={seed} 複製"));
        }
    }
}

/// With script operations (`m[k] = v`, `m.remove(k)`, `m.clear()`, copies), the contents match the model, and the index
/// does not disagree even after loading (a long sequence with many insertions and removals).
#[test]
fn long_map_operation_scripts_match_model_and_index_stays_consistent_after_save() {
    let reg = registry();
    for case in 0..case_count().min(300) {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed);
        let mut model = MapModel::default();
        let mut copies = 0;
        let mut src = String::from(MAP_HEAD);
        src.push_str("label start {\n");
        for _ in 0..60 {
            let (k, v) = (rng.range(0, 25), rng.range(0, 9));
            match rng.below(10) {
                0..=4 => {
                    src.push_str(&format!("  m[{k}] = {v}\n"));
                    model.upsert(k, v);
                }
                5..=7 => {
                    src.push_str(&format!("  m.remove({k})\n"));
                    model.remove(k);
                }
                8 => {
                    // Copy, then change only the copy
                    copies += 1;
                    src.push_str(&format!(
                        "  let c{copies} = m\n  c{copies}[{k}] = {v}\n  c{copies}.remove({v})\n"
                    ));
                }
                _ => {
                    if rng.chance(1, 8) {
                        src.push_str("  m.clear()\n");
                        model.m.clear();
                    }
                }
            }
        }
        src.push_str("  ${m} ${xs} #end\n}\n");
        let prog = compile_ok(&src, &reg);
        let mut vm = Vm::new(&prog, &reg, config()).unwrap();
        let y = vm
            .run()
            .unwrap_or_else(|e| panic!("seed={seed}: {:?}\n{src}", e.kind));
        let Yield::Stop { args, .. } = &y else {
            panic!("seed={seed}: 停止点のはず");
        };
        assert_eq!(
            args.first()
                .map(Value::to_text)
                .map(|t| t.trim().to_string()),
            Some(model.text()),
            "seed={seed}\n{src}"
        );
        let bytes = vm.save().unwrap();
        let (vm2, _) = Vm::load(&prog, &reg, config(), &bytes).unwrap();
        let Some(Value::Map(m)) = vm2
            .variables()
            .into_iter()
            .find(|v| v.name == "m")
            .map(|v| v.value)
        else {
            panic!("seed={seed}: map のはず");
        };
        assert!(m.is_consistent(), "seed={seed}: ロード後の索引");
        assert_eq!(vm2.save().unwrap(), bytes, "seed={seed}: セーブの往復");
    }
}
