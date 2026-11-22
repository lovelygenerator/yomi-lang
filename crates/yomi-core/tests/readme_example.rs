//! The host code example in the public README works (if the example is changed, update this too).

use yomi_core::{Input, Registry, Value, Vm, VmConfig, Yield, compile};

const SOURCE: &str = r#"
save var total: int = 0

label start {
  let n = @ask("いくつ？")
  total += n
  end
}
"#;

#[test]
fn stop_save_load_resume() -> Result<(), Box<dyn std::error::Error>> {
    let mut reg = Registry::new();
    reg.stop::<(String,), i64>("ask")?;
    let program = compile(SOURCE, "main", &reg).map_err(|d| format!("{d:?}"))?;

    let mut vm = Vm::new(&program, &reg, VmConfig::default())?;
    let Yield::Stop { args, .. } = vm.run()? else {
        unreachable!()
    };
    assert_eq!(args, vec![Value::Str("いくつ？".into())]);
    let bytes = vm.save()?;

    let (mut vm, _) = Vm::load(&program, &reg, VmConfig::default(), &bytes)?;
    vm.resume(Input::Return(Value::Int(42)))?;
    assert_eq!(vm.run()?, Yield::End);
    let t = vm
        .variables()
        .into_iter()
        .find(|v| v.name == "total")
        .unwrap();
    assert_eq!(t.value, Value::Int(42));
    Ok(())
}
