# yomi-cli

[English](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-cli/README.md) | 日本語

yomi 言語の `yomi` コマンドです。`check`（実行せずに検査）・`run`（停止点ごとに止めながら実行）・`repl`（式・文・宣言を試す）のほか、エディタ向けの Language Server（`yomi lsp`）と、AI エージェント向けの MCP サーバー（`yomi mcp`）があります。`assign-ids`（安定 ID を書き足す）・`decls`（宣言ファイル）・`codes`（診断コードの一覧）・`texts`（本文の差し替え表）もあります。`yomi --help` で一覧を見られます。`lsp`・`mcp` は feature（既定で有効）で切り離せます。表示は英語と日本語です。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
