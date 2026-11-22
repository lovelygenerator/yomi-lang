# yomi-core

[English](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-core/README.md) | 日本語

yomi 言語のコア（解析・型検査・VM・セーブ）です。yomi は、Rust のアプリケーションに組み込む、静的型付きの小さなスクリプト言語です。実行を途中で止めて、その状態をバイト列に保存し、あとで再開できます。

- 標準パッケージ（`flow`・`math`・`text`・`json`・`audio`・`image`・`screen`・`input`・`event`）は `yomi-std` です。
- `yomi` コマンド（検査・実行・REPL・Language Server・MCP サーバー）は `yomi-cli` です。
- 言語とホスト連携の仕様書は、リポジトリの `spec/` にあります。

0.x の間は、言語の文法・API・セーブの形式が変わることがあります。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
