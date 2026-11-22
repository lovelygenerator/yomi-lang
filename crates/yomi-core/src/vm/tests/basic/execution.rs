//! Basics

use super::*;

#[test]
fn prints_plain_lines_in_order() {
    let out = texts("label start {\n  おはよう\n  こんにちは\n}\n");
    assert_eq!(out, vec!["おはよう", "こんにちは"]);
}

#[test]
fn branching_with_variables_if_and_jump() {
    let src = "save var n: int = 0\nlabel start {\n  n += 1\n  if n < 3 {\n    ${n}回目\n    jump start\n  } else {\n    おわり\n  }\n}\n";
    assert_eq!(texts(src), vec!["1回目", "2回目", "おわり"]);
}

#[test]
fn save_and_restore_give_same_continuation() {
    let src = "save var n: int = 0\nlabel start {\n  let m = 10\n  a ${n} ${m} #p1\n  n += 1\n  b ${n} #p2\n}\n";
    let reg = registry();
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    assert!(matches!(y, Yield::Stop { .. }));
    vm.resume(Input::Return(Value::Unit)).unwrap();
    let y2 = vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(vm2.current(), Some(&y2));
    vm2.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(vm2.run().unwrap(), Yield::End);
}

#[test]
fn broken_saves_cannot_be_loaded() {
    let reg = registry();
    let prog = compile("label start {\n  a\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let mut bytes = vm.save().unwrap();
    let n = bytes.len();
    bytes[n - 6] ^= 0xff;
    let e = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert_eq!(e.kind, LoadErrorKind::ChecksumMismatch);
}

#[test]
fn integer_overflow_and_division_by_zero_are_runtime_errors() {
    let reg = registry();
    let prog = compile(
        "save var n: int = 9223372036854775807\nlabel start {\n  n += 1\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::IntegerOverflow);
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::Poisoned);
    let prog = compile(
        "save var n: int = 0\nlabel start {\n  n = 1 / n\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::DivisionByZero);
}

#[test]
fn exceeding_instruction_limit_is_error() {
    let reg = registry();
    let prog = compile("label start {\n  jump start\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(
        &prog,
        &reg,
        VmConfig::default().max_steps_per_run(Some(1000)),
    )
    .unwrap();
    assert_eq!(vm.run().unwrap_err().kind, VmErrorKind::StepLimitExceeded);
}

#[test]
fn save_with_mismatched_locals_returns_to_label_start() {
    let reg = registry();
    let v1 = compile(
        "label start {\n  let m = 10\n  値 ${m} #p1\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&v1, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // Editing the script changed the order of local variables
    let v2 = compile(
        "label start {\n  let k = 1\n  let m = 10\n  値 ${m} #p1\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let (mut vm2, report) = Vm::load(&v2, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert!(vm2.current().is_none());
    // It runs again from the start of the label, so the values are correct
    let Yield::Stop { args, .. } = vm2.run().unwrap() else {
        panic!("停止点で止まる");
    };
    assert_eq!(args[0].to_text(), "値 10");
}

#[test]
fn save_whose_args_cannot_be_reevaluated_also_returns_to_label_start() {
    let reg = registry();
    let v1 = compile(
        "label start {\n  let m = 10\n  値 ${m} #p1\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&v1, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let v2 = compile(
        "label start {\n  let m = 10\n  値 ${100 / (m - 10)} #p1\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let (_, report) = Vm::load(&v2, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
}

#[test]
fn vm_restored_at_label_start_is_not_stopped() {
    let reg = registry();
    let prog = compile("label start {\n  a\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(!vm.is_stopped());
    vm.run().unwrap();
    assert!(vm.is_stopped());
}

/// Runs the script to the end and returns the kind of runtime error (`None` if there is no error).
pub(super) fn run_error(src: &str) -> Option<VmError> {
    let reg = registry();
    let prog = compile(src, "main", &reg).expect("コンパイルできる");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    loop {
        match vm.run() {
            Ok(Yield::Stop { .. }) => vm.resume(Input::Return(Value::Unit)).unwrap(),
            Ok(Yield::End) => return None,
            Err(e) => return Some(e),
        }
    }
}

#[test]
fn float_arithmetic_comparison_and_to_string() {
    let src = "label start {
  let a = 0.1 + 0.2
  ${a} ${1.0} ${2.5 * 2.0} ${7.0 / 2.0} ${1.0 - 3.5} ${-a}
  ${1.5 < 2.5} ${2.5 <= 2.5} ${3.0 > 3.0} ${0.5 >= 1.0} ${1.5 == 1.5} ${0.0 != 0.0}
}
";
    assert_eq!(
        texts(src),
        vec![
            "0.30000000000000004 1.0 5.0 3.5 -2.5 -0.30000000000000004",
            "true true false false true false"
        ]
    );
}

#[test]
fn float_to_string_never_uses_exponent_and_integers_get_decimal_point() {
    let src = "label start {
  ${1000000.0 * 1000000.0 * 1000000.0 * 1000000.0}
  ${0.000001 * 0.000001}
  ${123456789.125}
}
";
    assert_eq!(
        texts(src),
        vec![
            "1000000000000000000000000.0",
            "0.000000000001",
            "123456789.125"
        ]
    );
}

#[test]
fn negative_zero_results_become_positive_zero() {
    let src = "save var z: float = -0.0
label start {
  let a = 0.0 * -1.0
  let b = -0.0
  let c = -(0.0)
  ${z} ${a} ${b} ${c} ${a == 0.0}
}
";
    assert_eq!(texts(src), vec!["0.0 0.0 0.0 0.0 true"]);
    // The bits of the value are not negative zero either
    let reg = registry();
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    for v in vm.variables() {
        if let Value::Float(x) = v.value {
            assert_eq!(x.to_bits(), 0.0f64.to_bits(), "{}", v.name);
        }
    }
}

#[test]
fn float_division_by_zero_is_runtime_error() {
    let e = run_error("label start {\n  let z = 0.0\n  let x = 1.0 / z\n}\n").unwrap();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
    assert_eq!(e.line, Some(3));
    let e = run_error("label start {\n  let z = 0.0\n  let x = 0.0 / z\n}\n").unwrap();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
    let e = run_error("save var x: float = 1.0\nlabel start {\n  x /= 0.0\n}\n").unwrap();
    assert_eq!(e.kind, VmErrorKind::DivisionByZero);
}

#[test]
fn float_operations_resulting_in_infinity_are_runtime_errors() {
    // 1e10 to the 31st power is 1e310, which does not fit in f64
    let product = vec!["x"; 31].join(" * ");
    let src = format!("label start {{\n  let x = 10000000000.0\n  let y = {product}\n}}\n");
    let e = run_error(&src).unwrap();
    assert_eq!(e.kind, VmErrorKind::NonFiniteFloat);
    assert_eq!(e.line, Some(3));
    // It exceeds by repetition
    let e = run_error(
        "save var x: float = 1.0\nlabel start {\n  while true {\n    x *= 1000.0\n  }\n}\n",
    )
    .unwrap();
    assert_eq!(e.kind, VmErrorKind::NonFiniteFloat);
    // Also with addition and subtraction (twice the largest finite value)
    let big = format!("17976931348623157{}.0", "0".repeat(292));
    let src = format!("label start {{\n  let a = {big}\n  let b = a + a\n}}\n");
    assert_eq!(run_error(&src).unwrap().kind, VmErrorKind::NonFiniteFloat);
    let src = format!("label start {{\n  let a = {big}\n  let b = -a - a\n}}\n");
    assert_eq!(run_error(&src).unwrap().kind, VmErrorKind::NonFiniteFloat);
}

#[test]
fn too_large_float_literals_are_not_numbers() {
    let big = format!("1{}.0", "0".repeat(400));
    let reg = registry();
    let src = format!("label start {{\n  let a = {big}\n}}\n");
    let d = compile(&src, "main", &reg).unwrap_err();
    assert_eq!(d[0].code, crate::DiagCode::InvalidNumber);
}

#[test]
fn to_int_truncates_toward_zero() {
    let src = "label start {
  ${to_int(2.9)} ${to_int(-2.9)} ${to_int(0.5)} ${to_int(-0.5)} ${to_int(5.0)}
  ${to_int(9223372036854775808.0 - 1024.0)} ${to_int(-9223372036854775808.0)}
}
";
    assert_eq!(
        texts(src),
        vec!["2 -2 0 0 5", "9223372036854774784 -9223372036854775808"]
    );
}

#[test]
fn to_int_out_of_range_is_runtime_error() {
    // 2^63 does not fit in i64
    let e = run_error("label start {\n  let x = to_int(9223372036854775808.0)\n}\n").unwrap();
    assert_eq!(e.kind, VmErrorKind::FloatOutOfRange);
    let e = run_error("label start {\n  let x = to_int(-9223372036854777856.0)\n}\n").unwrap();
    assert_eq!(e.kind, VmErrorKind::FloatOutOfRange);
}

#[test]
fn to_float_rounds_to_nearest() {
    let src = "label start {
  ${to_float(3)} ${to_float(-7)} ${to_float(9007199254740993)} ${to_float(9007199254740995)}
}
";
    assert_eq!(
        texts(src),
        vec!["3.0 -7.0 9007199254740992.0 9007199254740996.0"]
    );
}

#[test]
fn runtime_errors_have_module_label_and_line() {
    let e = run_error("label start {\n  jump b\n}\nlabel b {\n  let x = 1 / 0\n}\n").unwrap();
    assert_eq!(e.module.as_deref(), Some("main"));
    assert_eq!(e.label.as_deref(), Some("b"));
    assert_eq!(e.line, Some(5));
    assert_eq!(e.to_string(), "division_by_zero (label main.b, line 5)");
    // Misuse has no position
    let reg = registry();
    let prog = compile("label start {\n  a\n}\n", "main", &reg).unwrap();
    let vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let e = vm.save().unwrap_err();
    assert_eq!((e.module, e.label, e.line), (None, None, None));
}

#[test]
fn while_repeats_until_condition_is_false() {
    let src = "save var n: int = 0
label start {
  while n < 3 {
    n += 1
    ${n}
  }
  終わり ${n}
}
";
    assert_eq!(texts(src), vec!["1", "2", "3", "終わり 3"]);
}

#[test]
fn break_and_continue_affect_innermost_loop() {
    let src = "label start {
  let i = 0
  while i < 3 {
    i += 1
    let j = 0
    while true {
      j += 1
      if j == 2 {
        continue
      }
      if j > 3 {
        break
      }
      ${i}${j}
    }
  }
}
";
    assert_eq!(texts(src), vec!["11", "13", "21", "23", "31", "33"]);
}

#[test]
fn jump_inside_loop_exits_loop() {
    let src = "save var n: int = 0
label start {
  while true {
    n += 1
    if n == 3 {
      jump 次
    }
    ${n}
  }
}
label 次 {
  抜けた ${n}
}
";
    assert_eq!(texts(src), vec!["1", "2", "抜けた 3"]);
}

#[test]
fn infinite_loops_stop_at_instruction_limit() {
    let reg = registry();
    let prog = compile("label start {\n  while true {\n  }\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(
        &prog,
        &reg,
        VmConfig::default().max_steps_per_run(Some(10_000)),
    )
    .unwrap();
    let e = vm.run().unwrap_err();
    assert_eq!(e.kind, VmErrorKind::StepLimitExceeded);
    assert_eq!(e.line, Some(2));
}

#[test]
fn saving_and_restoring_at_stop_point_in_loop_continues_identically() {
    let src = "save var total: float = 0.5
label start {
  let i = 0
  while i < 4 {
    i += 1
    total += 0.25
    回 ${i} 合計 ${total} #t
    if i == 2 {
      continue
    }
  }
  おわり ${total}
}
";
    let reg = registry();
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    // The third stop point
    for _ in 0..2 {
        vm.run().unwrap();
        vm.resume(Input::Return(Value::Unit)).unwrap();
    }
    let y = vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let (mut vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(vm2.current(), Some(&y));
    let mut rest = Vec::new();
    for vm in [&mut vm, &mut vm2] {
        let mut texts = Vec::new();
        vm.resume(Input::Return(Value::Unit)).unwrap();
        while let Yield::Stop { args, .. } = vm.run().unwrap() {
            texts.push(args[0].to_text());
            vm.resume(Input::Return(Value::Unit)).unwrap();
        }
        rest.push(texts);
    }
    assert_eq!(rest[0], rest[1]);
    assert_eq!(rest[0], vec!["回 4 合計 1.5", "おわり 1.5"]);
}

#[test]
fn float_save_variables_survive_save_and_restore() {
    let src = "save var x: float = 1.5
save var y: float = -0.1
label start {
  x += 0.25
  y *= 3.0
  値 ${x} ${y} #p
}
";
    let reg = registry();
    let prog = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let before = vm.variables();
    let bytes = vm.save().unwrap();
    let (vm2, report) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(vm2.variables(), before);
    assert_eq!(before[0].value, Value::Float(1.75), "x は 1.5 + 0.25");
    assert_eq!(vm2.save().unwrap(), bytes);
}

#[test]
fn float_variable_with_changed_type_resets_and_returns_to_label_start() {
    let reg = registry();
    let v1 = compile(
        "save var x: int = 4\nlabel start {\n  値 ${x} #p\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&v1, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // `x` changed from int to float
    let v2 = compile(
        "save var x: float = 2.5\nlabel start {\n  値 ${x} #p\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let (vm2, report) = Vm::load(
        &v2,
        &reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
        &bytes,
    )
    .unwrap();
    assert_eq!(report.type_changed_vars, vec!["main.x"]);
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(vm2.variables()[0].value, Value::Float(2.5));
}

/// Replaces 8 bytes of the body of a save and recomputes the checksum.
pub(super) fn patch_save(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut body = bytes[..bytes.len() - 4].to_vec();
    let at = body
        .windows(from.len())
        .position(|w| w == from)
        .expect("置き換える値がある");
    body[at..at + from.len()].copy_from_slice(to);
    let crc = crate::vm::save::crc32(&body);
    body.extend_from_slice(&crc.to_le_bytes());
    body
}

#[test]
fn save_with_nan_or_infinity_is_broken() {
    let reg = registry();
    let prog = compile(
        "save var x: float = 1.5\nlabel start {\n  値 ${x} #p\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    let one_and_a_half = 1.5f64.to_le_bytes();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let broken = patch_save(&bytes, &one_and_a_half, &bad.to_le_bytes());
        let e = Vm::load(&prog, &reg, VmConfig::default(), &broken).unwrap_err();
        assert_eq!(e.kind, LoadErrorKind::Corrupt, "{bad}");
    }
    // Negative zero is read as positive zero
    let zeroed = patch_save(&bytes, &one_and_a_half, &(-0.0f64).to_le_bytes());
    let (vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &zeroed).unwrap();
    let Value::Float(x) = vm2.variables()[0].value else {
        panic!("float のはず")
    };
    assert_eq!(x.to_bits(), 0.0f64.to_bits());
}

#[test]
fn nan_cannot_be_passed_as_stop_command_return_value() {
    let mut reg = registry();
    reg.stop::<(), f64>("game.measure").unwrap();
    let prog = compile("label start {\n  @game.measure()\n}\n", "main", &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    for bad in [f64::NAN, f64::INFINITY] {
        let e = vm.resume(Input::Return(Value::Float(bad))).unwrap_err();
        assert_eq!(e.kind, VmErrorKind::NonFiniteFloat);
        // It is misuse, so it stays stopped
        assert!(vm.is_stopped());
    }
    vm.resume(Input::Return(Value::Float(-0.0))).unwrap();
    assert_eq!(vm.run().unwrap(), Yield::End);
    // Passing an int is a type error
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let e = vm.resume(Input::Return(Value::Int(1))).unwrap_err();
    assert!(matches!(e.kind, VmErrorKind::BadInput { .. }));
}

#[test]
fn integer_comparison_conditions_branch_correctly_per_operator_with_constants_and_variables() {
    let ops = ["<", "<=", ">", ">=", "==", "!="];
    let mut body = String::new();
    for op in ops {
        body += &format!("    if x {op} 1 {{\n      T\n    }} else {{\n      F\n    }}\n");
        body += &format!("    if x {op} y {{\n      T\n    }} else {{\n      F\n    }}\n");
    }
    body += "    if x > 0 && x < 2 {\n      T\n    } else {\n      F\n    }\n";
    body += "    if x < 0 || x >= 2 {\n      T\n    } else {\n      F\n    }\n";
    let src = format!(
        "label start {{\n  let y = 1\n  let x = -1\n  while x <= 2 {{\n{body}    x += 1\n  }}\n}}\n"
    );
    let mut expected = Vec::new();
    for x in -1i64..=2 {
        let results = [x < 1, x <= 1, x > 1, x >= 1, x == 1, x != 1];
        for r in results {
            expected.push(r);
            expected.push(r);
        }
        expected.push((1..2).contains(&x));
        expected.push(!(0..2).contains(&x));
    }
    let expected: Vec<&str> = expected
        .iter()
        .map(|&b| if b { "T" } else { "F" })
        .collect();
    assert_eq!(texts(&src), expected);
}

#[test]
fn while_comparison_conditions_stop_at_boundary() {
    let src = "label start {\n  let i = 0\n  while i < 3 {\n    ${i}\n    i += 1\n  }\n  let j = 3\n  while j > 0 {\n    ${j}\n    j -= 1\n  }\n  let k = 0\n  let n = 2\n  while k <= n {\n    ${k}\n    k += 1\n  }\n}\n";
    assert_eq!(
        texts(src),
        vec!["0", "1", "2", "3", "2", "1", "0", "1", "2"]
    );
}
