//! Calls across modules

use super::*;

pub(super) fn compile_mods(mods: &[(&str, &str)]) -> Result<Program, Vec<&'static str>> {
    compile_modules(mods, &registry()).map_err(|d| d.iter().map(|d| d.code.as_str()).collect())
}

#[test]
fn fn_of_another_module_can_be_called_with_module_name() {
    let util = "fn double(x: int) -> int {\n  return x * 2\n}\nfn quad(x: int) -> int {\n  return double(double(x))\n}\nsave var base: int = 100\nfn plus_base(x: int) -> int {\n  return x + base\n}\n";
    let main = "import util\nlabel start {\n  ${util.double(4)} ${util.quad(2)} ${util.plus_base(1)}\n  @util.double(1)\n  let z = @util.quad(1)\n  ${z}\n}\n";
    let prog = compile_mods(&[("main", main), ("util", util)]).unwrap();
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    assert_eq!(run_all(&mut vm), ["8 8 101", "4", "<end>"]);
}

#[test]
fn can_call_with_own_module_name_prefix() {
    let src =
        "fn double(x: int) -> int {\n  return x * 2\n}\nlabel start {\n  ${main.double(4)}\n}\n";
    assert_eq!(texts(src), ["8", "<end>"]);
}

#[test]
fn fn_of_unimported_module_cannot_be_called() {
    let util = "fn double(x: int) -> int {\n  return x * 2\n}\n";
    let main = "label start {\n  let a = util.double(1)\n  @util.double(1)\n}\n";
    assert_eq!(
        compile_mods(&[("main", main), ("util", util)]).unwrap_err(),
        ["module_not_imported", "module_not_imported"]
    );
    // A name alone cannot call a `fn` of another module
    let main = "import util\nlabel start {\n  let a = double(1)\n}\n";
    assert_eq!(
        compile_mods(&[("main", main), ("util", util)]).unwrap_err(),
        ["unknown_function"]
    );
}

#[test]
fn fn_missing_from_module_is_unknown_function() {
    let util = "fn double(x: int) -> int {\n  return x * 2\n}\n";
    let main = "import util\nlabel start {\n  let a = util.doubel(1)\n  @util.nothing()\n}\n";
    let d = compile_modules(&[("main", main), ("util", util)], &registry()).unwrap_err();
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["unknown_function", "unknown_function"]
    );
    assert_eq!(d[0].arg_str("name"), Some("util.doubel"));
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["util.double".to_string()][..])
    );
}

#[test]
fn fns_can_call_each_other_across_cyclic_imports() {
    let a = "import b\nfn even(n: int) -> bool {\n  if n == 0 {\n    return true\n  }\n  return b.odd(n - 1)\n}\nlabel start {\n  ${even(10)} ${b.odd(7)}\n}\n";
    let b = "import a\nfn odd(n: int) -> bool {\n  if n == 0 {\n    return false\n  }\n  return a.even(n - 1)\n}\n";
    let prog = compile_mods(&[("a", a), ("b", b)]).unwrap();
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("a.start")).unwrap();
    assert_eq!(run_all(&mut vm), ["true true", "<end>"]);
}

#[test]
fn seen_in_fn_of_another_module_reads_that_fns_module() {
    let util = "fn first() -> bool {\n  return visited(\"intro\")\n}\nlabel intro {\n}\n";
    let main =
        "import util\nlabel start {\n  ${util.first()}\n  jump util.intro\n}\nlabel after {\n}\n";
    let prog = compile_mods(&[("main", main), ("util", util)]).unwrap();
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    // Has not yet entered `intro` of `util` (not `intro` of `main`)
    assert_eq!(run_all(&mut vm), ["false", "<end>"]);
}
