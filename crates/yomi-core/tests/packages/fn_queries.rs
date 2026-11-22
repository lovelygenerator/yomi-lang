//! fn: query commands and host-declared variables can be read, but side effects and writes cannot

use super::*;

pub(super) fn codes_of(src: &str, reg: &Registry) -> Vec<&'static str> {
    match compile(src, "main", reg) {
        Ok(_) => Vec::new(),
        Err(d) => d.iter().map(|d| d.code.as_str()).collect(),
    }
}

#[test]
fn fn_can_read_query_commands_and_host_variables() {
    let reg = full_registry();
    let src = "import counter\nfn peek(k: int) -> int {\n  return counter.get() + counter.step + k\n}\nlabel start {\n  @counter.inc(5)\n  ${peek(1)} #a\n}\n";
    let prog = compile_ok(src, &reg);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => {
            // counter.get() = 5, the initial value of counter.step = 1, k = 1
            assert_eq!(args.last(), Some(&Value::Str("7".into())));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn effect_commands_and_variable_writes_in_fn_are_errors() {
    let reg = full_registry();
    // Effect and stop commands
    assert_eq!(
        codes_of(
            "import counter\nfn f() {\n  @counter.inc(1)\n}\nlabel start {\n}\n",
            &reg
        ),
        ["not_allowed_in_fn"]
    );
    assert_eq!(
        codes_of(
            "import counter\nfn f() {\n  @counter.wait()\n}\nlabel start {\n}\n",
            &reg
        ),
        ["not_allowed_in_fn"]
    );
    assert_eq!(
        codes_of(
            "import counter\nfn f() -> int {\n  let r = @counter.roll(6)\n  return r\n}\nlabel start {\n}\n",
            &reg
        ),
        ["not_allowed_in_fn"]
    );
    // Writing to host-declared variables
    assert_eq!(
        codes_of(
            "import counter\nfn f() {\n  counter.step = 3\n}\nlabel start {\n}\n",
            &reg
        ),
        ["assign_in_fn"]
    );
    assert_eq!(
        codes_of(
            "import counter\nfn f() {\n  counter.step += 3\n}\nlabel start {\n}\n",
            &reg
        ),
        ["assign_in_fn"]
    );
    // Query commands cannot be called in an `@` statement (as before)
    assert_eq!(
        codes_of(
            "import counter\nfn f() {\n  @counter.get()\n}\nlabel start {\n}\n",
            &reg
        ),
        ["invalid_command_use"]
    );
}
