# yomi-core

> 詠めば、動く。

yomi 言語のコア（解析・型検査・VM・セーブ）です。yomi は、Rust のアプリケーションに組み込む、静的型付きの小さなスクリプト言語です。実行を途中で止めて、その状態をバイト列に保存し、あとで再開できます。

```rust
use yomi_core::{Input, Registry, Value, Vm, VmConfig, Yield, compile};

let mut reg = Registry::new();
reg.stop::<(String,), i64>("ask")?; // 台本が止まって、ホストに整数を尋ねるコマンド
let program = compile(SOURCE, "main", &reg).map_err(|d| format!("{d:?}"))?;

let mut vm = Vm::new(&program, &reg, VmConfig::default())?;
let Yield::Stop { args, .. } = vm.run()? else { unreachable!() }; // `@ask` で止まる
let bytes = vm.save()?; // 止まった点を保存する

// あとで（別のプロセスでも）再開する
let (mut vm, _) = Vm::load(&program, &reg, VmConfig::default(), &bytes)?;
vm.resume(Input::Return(Value::Int(42)))?;
assert_eq!(vm.run()?, Yield::End);
```

- 標準パッケージ（`flow`・`math`・`text`・`json`・`audio`・`image`・`screen`・`input`・`event`）は `yomi-std` です。
- `yomi` コマンド（検査・実行・REPL・Language Server・MCP サーバー）は `yomi-cli` です。
- 言語とホスト連携の仕様書は、リポジトリの `spec/` にあります。

0.x の間は、言語の文法・API・セーブの形式が変わることがあります。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
