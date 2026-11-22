# yomi 仕様書

> 詠めば、動く。

yomi は、Rust のアプリケーションに組み込む、途中で止めて保存・再開できるスクリプト言語です。この仕様書は、言語と、ホスト（組み込む側のアプリケーション）との約束を定めます。

状態: 草案。

## 構成

| 部 | 内容 |
| --- | --- |
| [language/](language/) | 言語仕様。字句、行の分類、文と式、型、ラベルとモジュール、診断 |
| [host/](host/) | ホスト連携仕様。`Yield` / `Input`、コマンド登録表、`Package`、状態、セーブの形式、本文の差し替え、コマンドライン、REPL |
| [packages/](packages/) | パッケージ仕様。yomi-std の `flow` `math` `text` `audio` `image` `screen` `input` `event`。用途別（会話の表示・UI など）のコマンドは、ホストのアプリケーションが登録する（yomi の側のライブラリには置かない） |

### language/ の章

| 章 | 内容 |
| --- | --- |
| [01-lexical.md](language/01-lexical.md) | 字句 |
| [02-lines.md](language/02-lines.md) | 行の分類 |
| [03-statements.md](language/03-statements.md) | 文と式（変数、型、演算子、制御、`fn`） |
| [04-labels-modules.md](language/04-labels-modules.md) | ラベルとモジュール（`label` `jump` `call` `import`） |
| [05-sugar.md](language/05-sugar.md) | 糖衣の展開（引用行、素の行、選択肢 → 核の呼び出し） |
| [06-diagnostics.md](language/06-diagnostics.md) | 型検査と診断（診断の形、重大度、近い綴りの判定） |
| [07-migrate.md](language/07-migrate.md) | セーブの移行（凍結スキーマ、`migrations` モジュール、`migrate`） |
| [08-format.md](language/08-format.md) | 整形（`yomi fmt` の結果の規則） |

### host/ の章

| 章 | 内容 |
| --- | --- |
| [01-execution.md](host/01-execution.md) | 実行モデル（`run` / `resume`、`Yield` / `Input`） |
| [02-registry.md](host/02-registry.md) | コマンド登録表と束縛 |
| [03-packages.md](host/03-packages.md) | パッケージと状態 |
| [04-save.md](host/04-save.md) | セーブ（バイト列の形、位置の解決、永続データ、ロード） |
| [05-texts.md](host/05-texts.md) | 本文の差し替え（多言語化） |
| [06-cli.md](host/06-cli.md) | コマンドライン（`yomi` の `run` `check` `decls` `assign-ids` `texts` `codes` `repl` `mcp` `lsp` `test`。引数・終了コード・入出力） |
| [07-repl.md](host/07-repl.md) | REPL（`yomi repl` と、`yomi run` の停止中の `:repl`。入力の判定、状態と有効範囲、`:apply`、台本の行、複数行入力、対話コマンド、コアの評価 API） |
| [08-test.md](host/08-test.md) | 分岐の探索（`explore`。答えの候補・状態・報告・経路） |
| [09-debug.md](host/09-debug.md) | デバッグ（行イベント・ブレークポイント・ステップ実行・呼び出しの確認） |
| [10-bevy.md](host/10-bevy.md) | Bevy との連携（`yomi-bevy`。プラグイン・台本のアセット・実行の `Component`・メッセージ・ホットリロード・セーブ） |

## 書き方

- 文法は EBNF で書く。記法は [01-lexical.md](language/01-lexical.md) の冒頭に示す。
- 規則は「〜する」「〜はエラー」「〜は警告」の形で書く。
