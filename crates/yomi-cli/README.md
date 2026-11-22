# yomi-cli

yomi 言語の `yomi` コマンドです。

```bash
cargo install yomi-cli
yomi check main.yomi     # 実行せずに検査する
yomi run main.yomi       # 停止点ごとに止めながら実行する
yomi repl                # 式・文・宣言を試す
```

- `yomi lsp`：エディタ向けの Language Server（診断・補完・定義へ移動・参照・ホバー・名前の変更）
- `yomi mcp`：AI エージェント向けの MCP サーバー（検査・予約語と診断コードの一覧・仕様書の閲覧・サンドボックスでの実行）。クライアントの設定に `{ "command": "yomi", "args": ["mcp"] }` を書きます。
- ほかに `assign-ids`（安定 ID を書き足す）・`decls`（宣言ファイル）・`codes`（診断コードの一覧）・`texts`（本文の差し替え表）があります。`yomi --help` で一覧を見られます。
- `lsp`・`mcp` は feature（既定で有効）で切り離せます（`cargo install yomi-cli --no-default-features`）。
- 表示は英語と日本語です（`--lang`、または環境変数 `YOMI_CLI_LANG`・`LANG` など）。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
