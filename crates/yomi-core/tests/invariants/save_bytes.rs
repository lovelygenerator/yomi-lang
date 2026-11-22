//! Tests: bytes of saves

use super::*;

/// Makes some saves at stop points.
pub(super) fn some_saves(
    prog: &Program,
    reg: &Registry,
    rng: &mut Rng,
    max: usize,
) -> Vec<Vec<u8>> {
    let mut vm = Vm::new(prog, reg, config()).unwrap();
    let mut out = Vec::new();
    for _ in 0..200 {
        match step(&mut vm) {
            Ok(y @ Yield::Stop { .. }) => {
                if rng.chance(1, 4) {
                    out.push(vm.save().unwrap());
                    if out.len() >= max {
                        break;
                    }
                }
                vm.resume(Input::Return(answer(reg, &y))).unwrap();
            }
            _ => break,
        }
    }
    out
}

#[test]
fn saves_from_same_state_are_same_bytes() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        // Run the same script twice and save at the same stop point
        let (mut a, mut b) = (
            Vm::new(&prog, &reg, config()).unwrap(),
            Vm::new(&prog, &reg, config()).unwrap(),
        );
        for i in 0..100 {
            let (sa, sb) = (step(&mut a), step(&mut b));
            assert_eq!(sa, sb, "seed={seed}");
            let Ok(y @ Yield::Stop { .. }) = sa else {
                break;
            };
            assert_eq!(
                a.save().unwrap(),
                b.save().unwrap(),
                "seed={seed} 停止点 {i}"
            );
            let v = answer(&reg, &y);
            a.resume(Input::Return(v.clone())).unwrap();
            b.resume(Input::Return(v)).unwrap();
        }
    }
}

#[test]
fn broken_saves_error_without_panic() {
    let reg = registry();
    for case in 0..case_count() {
        let seed = base_seed() + case as u64;
        let mut rng = Rng::new(seed ^ 0xABCD);
        let src = gen_script(&mut Rng::new(seed));
        let prog = compile_ok(&src, &reg);
        for bytes in some_saves(&prog, &reg, &mut rng, 3) {
            // A 1-bit flip is always caught (CRC-32)
            for _ in 0..8 {
                let mut broken = bytes.clone();
                let pos = rng.below(broken.len());
                broken[pos] ^= 1 << rng.below(8);
                let r = catch_unwind(AssertUnwindSafe(|| {
                    Vm::load(&prog, &reg, config(), &broken).is_err()
                }));
                assert_eq!(r.ok(), Some(true), "seed={seed} 位置 {pos}");
            }
            // Replace 1 byte (only when the value changes)
            let mut broken = bytes.clone();
            let pos = rng.below(broken.len());
            broken[pos] = broken[pos].wrapping_add(1 + rng.below(255) as u8);
            assert!(
                Vm::load(&prog, &reg, config(), &broken).is_err(),
                "seed={seed} 位置 {pos}"
            );
            // Truncation, empty, extra trailing bytes
            for len in [0, 1, 4, 8, 11, bytes.len() - 1] {
                assert!(
                    Vm::load(&prog, &reg, config(), &bytes[..len]).is_err(),
                    "seed={seed} 長さ {len}"
                );
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(Vm::load(&prog, &reg, config(), &longer).is_err());
            // Break the body and recompute the checksum: an error is fine, but it must not panic.
            // When it can be read, continuing to run does not panic.
            for _ in 0..8 {
                let mut body = bytes[..bytes.len() - 4].to_vec();
                let pos = 8 + rng.below(body.len() - 8);
                body[pos] ^= 1 << rng.below(8);
                let crc = crc32(&body);
                body.extend_from_slice(&crc.to_le_bytes());
                let r = catch_unwind(AssertUnwindSafe(|| {
                    if let Ok((mut vm, _)) = Vm::load(&prog, &reg, config(), &body) {
                        let _ = vm.variables();
                        let _ = vm.save();
                        let _ = trace(&mut vm, &reg, 300);
                    }
                }));
                assert!(
                    r.is_ok(),
                    "seed={seed}: チェックサムを付け直した壊れたセーブでパニック（位置 {pos}）"
                );
            }
        }
    }
}

#[test]
fn loading_random_bytes_does_not_panic() {
    let reg = registry();
    let prog = compile_ok("label start {\n  あ #a\n  い\n}\n", &reg);
    let mut rng = Rng::new(base_seed() ^ 0x77);
    for _ in 0..case_count() * 4 {
        let len = rng.below(64);
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
        if rng.chance(1, 2) && bytes.len() >= 8 {
            bytes[..4].copy_from_slice(b"YOMS");
            bytes[4..8].copy_from_slice(&4u32.to_le_bytes());
        }
        if rng.chance(1, 2) && bytes.len() >= 12 {
            let n = bytes.len() - 4;
            let crc = crc32(&bytes[..n]);
            bytes[n..].copy_from_slice(&crc.to_le_bytes());
        }
        let r = catch_unwind(AssertUnwindSafe(|| {
            let _ = Vm::load(&prog, &reg, config(), &bytes);
        }));
        assert!(r.is_ok(), "パニック: {bytes:?}");
    }
}
