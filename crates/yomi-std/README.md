# yomi-std

yomi 言語の標準パッケージです（`flow`・`math`・`text`・`json`・`audio`・`image`・`screen`・`input`・`event`）。

```rust
let mut reg = yomi_core::Registry::new();
yomi_std::register_all(&mut reg)?; // 7 つをまとめて登録する
```

パッケージを選んで登録するときは、`yomi_core::Registry::package` を使います。音・画像・画面の効果は、状態と合図（`cues`）を持ちます。ホストは停止点で `Vm::package_state` から読んで、音を鳴らし、画を描きます。各パッケージの仕様は、リポジトリの `spec/packages/` にあります。

0.x の間は、パッケージのコマンドと状態の形が変わることがあります。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
