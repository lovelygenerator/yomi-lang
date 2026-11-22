# yomi-core

English | [日本語](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-core/README.ja.md)

> 詠めば、動く。 (*Recite it, and it runs.*)

The core of the yomi language: parsing, type checking, the VM and saves. yomi is a small, statically typed scripting language that you embed in a Rust application. A running script can stop in the middle, have its state saved as bytes, and resume later.

```rust
use yomi_core::{Input, Registry, Value, Vm, VmConfig, Yield, compile};

let mut reg = Registry::new();
reg.stop::<(String,), i64>("ask")?; // a command that stops the script and asks the host for an integer
let program = compile(SOURCE, "main", &reg).map_err(|d| format!("{d:?}"))?;

let mut vm = Vm::new(&program, &reg, VmConfig::default())?;
let Yield::Stop { args, .. } = vm.run()? else { unreachable!() }; // stops at `@ask`
let bytes = vm.save()?; // save the stop point

// Later (even in another process), resume
let (mut vm, _) = Vm::load(&program, &reg, VmConfig::default(), &bytes)?;
vm.resume(Input::Return(Value::Int(42)))?;
assert_eq!(vm.run()?, Yield::End);
```

- The standard packages (`flow`, `math`, `text`, `json`, `audio`, `image`, `screen`, `input`, `event`) are in `yomi-std`.
- The `yomi` command (checking, running, REPL, Language Server, MCP server) is in `yomi-cli`.
- The specification of the language and the host integration is in `spec/` of the [repository](https://github.com/lovelygenerator/yomi-lang) (written in Japanese).

While the version is 0.x, the grammar, the API and the save format may change.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
