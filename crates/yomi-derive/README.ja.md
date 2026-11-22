# yomi-derive

[English](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-derive/README.md) | 日本語

yomi 言語の `#[derive(YomiType)]` です。Rust の `struct`・`enum` を、台本のコマンドの引数・戻り値にできる型にします。直接は使わず、`yomi-core` が再公開している `yomi_core::YomiType` を使ってください（生成するコードは `::yomi_core` を参照します）。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
