# yomi

> 詠めば、動く。

yomi（詠み）は、Rust のアプリケーションに組み込む、静的型付きの小さなスクリプト言語です。実行を途中で止めて、その状態をバイト列に保存し、あとで再開できます。台本を書き換えたあとのセーブからでも再開できます。

## 特徴

- **止めて、保存して、再開できる**：台本は、ホストが登録した停止型のコマンドで止まり、ホストが値を返すと続きを実行します。止まった点では、コールスタック・変数・乱数の状態をまとめて 1 つのバイト列にセーブでき、メモリ上の複製（ロールバック）も作れます。台本を書き換えても、行末の安定 ID で、セーブの位置を合わせ直します。
- **静的型**：局所的な型推論つきの静的型です。型の誤りや未定義の名前は、実行する前に見つかります。エディタにも同じ診断が出ます。
- **決定的**：乱数の状態はセーブに含まれ、コアは時刻を読みません。同じ台本と同じ入力からは、いつも同じ結果になります（リプレイ、テストの再現）。
- **サンドボックス**：台本ができることは、ホストが登録したコマンドだけです。1 回の実行のステップ数・値の大きさ・メモリの合計・呼び出しの深さに上限があります。
- **Rust から使いやすい**：Rust の関数やクロージャを、そのままコマンドとして登録できます。引数と戻り値の型は、シグネチャから決まります。独自の `struct`・`enum` は `#[derive(YomiType)]` で渡せ、失敗は `Result` で返します。
- **safe Rust だけで書かれている**：すべてのクレートが `#![forbid(unsafe_code)]` です。GC はなく（値は参照カウント）、C のツールチェーンも要りません。
- **スレッドをまたげる**：`Program`・`Registry`・`Vm` は `Send + Sync` です。コンパイルした台本と登録表を複数のスレッドで共有し、VM をワーカースレッドやゲームエンジンの中で動かせます。
- **国際化**：識別子に日本語などを使えます（UAX #31）。文字列は書記素の単位で数えます。表示する本文は、安定 ID をキーにして、言語ごとに差し替えられます。

## 使い道の例

- 会話・台本：ビジュアルノベル、ゲームの会話・クエスト・チュートリアル
- ユーザーが書くスクリプト：MOD、ルールの設定、自動化
- AI エージェントが書くスクリプト：検査してから、上限つきで動かす
- 途中で人の入力を待つ、長く続く手続き

## 例

台本（`main.yomi`）:

```
save var total: int = 0

label start {
  let n = @ask("いくつ？")
  total += n
  end
}
```

ホスト（Rust）:

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

## 会話・台本のための糖衣

会話を書くための糖衣を持っています。使うときは、ホストが停止型のコマンドに束縛します。

```
identity 店主 = "道具屋の主人"
save var favor: int = 0

label start {
    店の奥から、店主が顔を出した。
    店主「いらっしゃい。今日は何を探しているんだい？」 #a3f9k2
    choice {
        「依頼の話を聞く」 #quest => { favor += 1; jump quest }
        「また来る」       #leave => jump village
    }
}
```

- 素の行（地の文）・引用行（`名前「本文」`）・選択肢（`choice { … }`）は、それぞれホストの停止型コマンドの呼び出しになります。
- 行末の `#id` は、セーブの位置と、多言語化の本文の差し替えに使う安定 ID です（`yomi assign-ids` で書き足せます）。

## 状態

開発中で、0.x の間は、言語の文法・API・セーブの形式が変わることがあります。

- `crates/yomi-core`：言語のコア（解析・型検査・VM・セーブ）
- `crates/yomi-std`：標準パッケージ（`flow` `math` `text` `json` `audio` `image` `screen` `input` `event`）
- `crates/yomi-cli`：`yomi` コマンド（`check`・`run`・`repl`・`assign-ids` など）。エディタ向けの Language Server（`yomi lsp`）と、AI エージェント向けの MCP サーバー（`yomi mcp`）も持つ
- `crates/yomi-derive`：コマンドの引数・戻り値の型の derive
- `crates/yomi-bevy`：Bevy 0.20 のプラグイン（台本の実行・メッセージでのやりとり・`.yomi` のアセットとホットリロード）。Bevy の依存が重いので別のワークスペースで、crates.io には出していません。`cd crates/yomi-bevy && cargo test`。窓つきの例は `cargo run --example dialogue_ui --features window`（小さな会話）と `cargo run --example mini_game_ui --features window`（`examples/mini-game` を遊べる）
- `editors/vscode`：VS Code 拡張（シンタックスハイライトと `yomi lsp` への接続）
- `guide/`：ガイド（Rust に組み込む開発者向けの入門。購入申請の承認フローを例に、台本・ホスト・セーブ・イベント待ち・テストを順にたどる）
- `spec/`：仕様書（言語・ホスト連携・パッケージ）
- `examples/`：サンプルの台本

## 使い方

Rust 1.88 以上（edition 2024）が要ります。

```toml
[dependencies]
yomi-core = "0.1"
yomi-std = "0.1"   # 標準パッケージを使うとき
```

```bash
cargo install yomi-cli   # `yomi` コマンド
```

## ビルド

```bash
cargo build --release
cargo test
./target/release/yomi --help
```

## エディタ・AI エージェントから使う

- `yomi lsp`：診断・補完・定義へ移動・参照・ホバー・名前の変更を、LSP で提供します。VS Code は [editors/vscode](editors/vscode) の拡張から使えます。
- `yomi mcp`：台本の検査・予約語と診断コードの一覧・仕様書の閲覧・サンドボックスでの実行を、MCP で提供します。クライアントの設定に `{ "command": "yomi", "args": ["mcp"] }` を書きます。
- どちらも、ビルドの feature（`lsp`・`mcp`。既定で有効）で切り離せます。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べます。
