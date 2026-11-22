//! Limits

use super::*;

#[test]
fn too_large_map_is_runtime_error() {
    let reg = registry();
    let src = "label start {\n  let m: map<int, str> = map {}\n  let i = 0\n  while true {\n    m[i] = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n    i += 1\n  }\n}\n";
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(1 << 12)).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
    // Literals are checked too
    let prog = compile_ok(
        "label start {\n  let m = map { 1: \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\", 2: \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\" }\n}\n",
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().max_value_bytes(100)).unwrap();
    assert_eq!(run_all(&mut vm), ["<value_too_large>"]);
}

#[test]
fn deeply_nested_for_and_map_run_on_1_mib_stack() {
    crate::tests::on_small_stack(|| {
        // Nesting of for (63 levels of the 64 block levels, excluding label)
        let mut src = String::from("label start {\n");
        for i in 0..60 {
            src.push_str(&format!("  for v{i} in [1] {{\n"));
        }
        src.push_str("  深い\n");
        for _ in 0..60 {
            src.push_str("  }\n");
        }
        src.push_str("}\n");
        let t = texts(&src);
        assert_eq!(t, ["深い", "<end>"]);
        // map<int, map<int, …>> up to the nesting limit of types
        let depth = 15;
        let ty = format!("{}int{}", "map<int, ".repeat(depth), ">".repeat(depth));
        let lit = format!("{}1{}", "map { 1: ".repeat(depth), " }".repeat(depth));
        let src = format!("save var a: {ty} = {lit}\nlabel start {{\n  一 #a\n  ${{a}}\n}}\n");
        let reg = registry();
        let prog = compile_ok(&src);
        let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
        vm.run().unwrap();
        let bytes = vm.save().unwrap();
        let snap = vm.snapshot().unwrap();
        let (mut vm2, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
        assert_eq!(var(&vm2, "a"), var(&vm, "a"));
        vm.restore(&snap).unwrap();
        let t = run_all(&mut vm2);
        assert_eq!(t.last().map(String::as_str), Some("<end>"));
    });
}
