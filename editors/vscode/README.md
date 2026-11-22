# yomi の VS Code 拡張

`.yomi` ファイルのシンタックスハイライトと、Language Server（`yomi lsp`）への接続。

## 使い方

1. `yomi` コマンドを PATH に置く（`cargo install --path crates/yomi-cli`）。
2. この拡張のディレクトリで `npm install` する（`vscode-languageclient` が入る）。
3. VS Code で、このディレクトリを開いて F5 で試す（または `vsce package` で作った `.vsix` を入れる）。

## 設定

| 設定 | 内容 |
| --- | --- |
| `yomi.server.path` | `yomi` コマンドのパス（既定は `yomi`） |
| `yomi.server.decls` | ホストの登録表の宣言ファイル（`yomi decls` が書く JSON）。空なら組み込みの登録表 |

Language Server の機能は、診断・補完・定義へ移動・参照・ホバー・名前の変更・同名の強調・アウトライン・ワークスペースのシンボル検索。
