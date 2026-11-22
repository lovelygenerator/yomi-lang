# 1. 実行モデル

この章の Rust のコードは、形を示すためのもので、型の細部は rustdoc を正とする。

この章は、ホスト（ゲーム側）が VM を動かす約束を定める。言語そのものの規則は、[language/](../language/) にある。

## 1.1 基本の形

- VM は、**同期・単一スレッド**で動く。`async` は使わない。
- `Program`・`Registry`・`Vm` は `Send + Sync` である。VM を作ったスレッドとは別のスレッドで動かしたり、`Program` と `Registry` を複数のスレッドの VM で共有したりできる（1 つの VM を同時に複数のスレッドから動かすことはできない。`run`・`resume` は `&mut self`）。そのため、登録するコマンドの関数・検証の関数と、パッケージの状態（`State`・`Persistent`）は、`Send + Sync` でなければならない。
- ホストは、VM を「次の停止点まで進める」。VM は、停止点で止まり、ホストに `Yield` を返す。
- ホストは、停止点の処理（表示、入力の待ち、時間の待ちなど）を行い、結果を `Input` として返して、VM を再開する。
- コアは、画面・音・時間・ファイルを持たない。停止点の意味は、束縛された停止型コマンド（language/05-sugar.md）と、それを実装するホスト・パッケージが与える。

```rust
let mut vm = Vm::new(program, &registry, VmConfig::default())?;
loop {
    match vm.run()? {                      // 次の停止点まで進める
        Yield::Stop { command, args, id } => {
            let result = host.handle(command, args);   // ホストの処理
            vm.resume(Input::Return(result))?;         // 戻り値を返して再開
        }
        Yield::End => break,
    }
}
```

## 1.2 `Yield`

```rust
#[non_exhaustive]
enum Yield {
    Stop { command: CommandId, args: Vec<Value>, id: Option<String> },
    End,
}
```

- `Stop`：停止型コマンドの呼び出しで止まった。
  - `command`：呼ばれた停止型コマンド（登録表の ID と名前）。
  - `args`：評価済みの引数（糖衣から展開されたものを含む。language/05-sugar.md の 5.2）。
  - `id`：行末の安定 ID（ない行は `None`）。
- `End`：`end` に達した、または、呼び出し元のない `return` に達した（language/04-labels-modules.md）。
- `Yield` と `Input` は、`#[non_exhaustive]` とする。後から、項目（ファイバーの識別子など）を足しても、ホストの `match` が壊れないようにする。
- `Say` / `Choice` / `Wait` のような、ジャンルの概念を表す形は持たない。

## 1.3 `Input`

```rust
#[non_exhaustive]
enum Input {
    Return(Value),      // 停止型コマンドの戻り値
}
```

- `Return` の値は、停止中のコマンドの戻り値の型に合っていなければならない。戻り値のないコマンドには `Value::Unit` を返す。
- 型が合わないときは、`resume` がエラーを返し、VM は停止したままにする（ホストは、正しい入力で、もう一度 `resume` できる）。
- 台本が戻り値を受けている停止点（`let name = @input.text(…)`・`name = @input.text(…)`。language/02-lines.md の 2.2a）では、`Return` の値が、その変数に入り、次の文から続く。変数に入るのは、`resume` のあとの `run` の中である（`resume` の直後は、`vm.variables()` に、まだ反映されない）。戻り値を受けていない停止点では、値は台本に使われない（履歴には残る。host/04-save.md の 4.3）。
- コアは、時間を扱わない。時間待ちも、ホストが時間を測り、終わったら `Return` で再開する。

## 1.4 `run` と `resume` の規則

- `run` は、`Yield` を返すか、実行時エラーを返す。停止中に `run` を呼ぶと、エラー。
- `resume` は、停止中にだけ呼べる。停止していないときに呼ぶと、エラー。
- `Yield::End` のあとに `run` を呼ぶと、エラー。台本を最初から動かすときは、新しい `Vm` を作る（またはセーブから復元する）。
- 同じ停止点に対して、`resume` を二度成功させることはできない。

## 1.4a 停止点の再取得

- `vm.current()` は、停止中の `Yield` を、そのまま返す（host/04-save.md の 4.6）。
- `vm.refresh_current()` は、停止中だけ呼べる。今の差し替え表で、本文を作り直した `Yield` を返す（式は評価し直さない。host/05-texts.md の 5.3）。
- `vm.set_texts(表)` は、いつでも呼べる。次の停止点から、差し替えが変わる（今の `Yield` は変わらない。host/05-texts.md の 5.3）。

## 1.5 実行時エラー

- 実行時エラーは、`Result` の `Err` で返す。位置（モジュール・ラベル・行）と、種類を持つ。
- 実行時エラーになるもの：命令数・呼び出しの深さ・値の大きさの上限（1.9）、整数のオーバーフロー、0 での割り算（`int` でも `float` でも）、`float` の演算の結果が `NaN`・`±∞` になること（`NonFiniteFloat`）、`to_int` の結果が `int` の範囲に収まらないこと（`FloatOutOfRange`）、`random` の最小が最大より大きいこと（`InvalidRandomRange`）、リストの添字の範囲外（`IndexOutOfRange`。引数 `index`・`len`。language/03-statements.md の 3.2・3.3）、`map` にないキーの読み出し・更新（`KeyNotFound`。引数 `key`。3.2・3.3）、再帰の深さの上限、すべての選択肢が一覧に含まれない選択（`NoChoiceAvailable`）、範囲外の選択の番号（`ChoiceOutOfRange`。language/05-sugar.md の 5.5）、中断（`Interrupted`。1.9a）、コマンドの関数が返した `Err`（02-registry.md の 2.4）など。
- エラーのあとの VM は、`run` を続けられない。ホストは、直前のスナップショット（1.8）から `restore` して続ける。
- コアは、`panic` しない（台本の誤りでは、`panic` に至らない）。

## 1.6 コマンドの呼び出し

- **副作用型・問い合わせ型**のコマンドは、VM が `run` の中で、登録された Rust の関数を直接呼ぶ。止まらない。
  - 関数は、自分のパッケージの状態スライスを読み書きする（副作用型）、または読むだけ（問い合わせ型）。
- **停止型**のコマンドは、VM が呼ばずに、`Yield::Stop` で止まる。処理はホストが行う。
- 登録表の形（コマンドの型、種類、関数の渡し方）は、[02-registry.md](02-registry.md)で定める。

## 1.7 決定性

- 同じ台本・同じ状態・同じ入力の列に対して、VM は、環境によらず同じ結果になる。
- コアは、時間・乱数（OS の）・ファイルを使わない。乱数は、状態を直列化できる、PRNG（xoshiro256++。状態は `u64` が 4 つ）である。種は `VmConfig::seed` で渡す（language/03-statements.md の 3.8）。

## 1.7a コードの形

- VM は、レジスタ式のコード（値をフレームのレジスタで受け渡す命令）を実行する。
- ラベル・`fn` 1 つが使うレジスタ（ローカル変数と、式の途中の値）は 65536 個まで。超えたら、コンパイルエラー（`too_many_registers`。language/06-diagnostics.md）。
- セーブは、位置をラベルと安定 ID で持つので、コードの形によらない（host/04-save.md）。スナップショットは、取ったのと同じプログラムにだけ戻せる。

## 1.8 セーブとの関係

- セーブできるのは、停止点で止まっているときだけである。
- 停止点で、ホストは `vm.snapshot()` で、VM の状態のスナップショットを取れる。スナップショットは、セーブの状態（`save var`・ローカル変数・コールスタック・`visited`・`once_done`・乱数・履歴・位置）と、全パッケージの状態スライスを含む。`persistent var` と通過済みの停止点（`seen`）は、含まない。
- `vm.restore(&snapshot)` で、同じ停止点まで復元できる（ロールバック）。復元した VM は、その停止点で止まっている。`persistent var` と通過済みの停止点は、戻らない。ホストは、`vm.current()` で停止点の `Yield` を得て、通常の停止点と同じように処理し、`resume` する（host/04-save.md の 4.7）。
- 停止点の履歴は、`vm.history()` で読める。停止点に達するたびに、記録される（host/04-save.md の 4.3）。今の言語で本文を引くのは、`vm.render`・`vm.history_text`（host/05-texts.md の 5.6）。
- 言語（差し替え表）は、セーブ・スナップショットの状態に含めない。ロールバックで戻した停止点は、戻すときの表で作り直す（host/05-texts.md の 5.3）。
- スナップショットの形、ロード時の位置の解決、版数は、[04-save.md](04-save.md) で定める。

## 1.9 実行の上限

台本の誤り（終わらないループ、終わらない再帰）で、ホストが止まらないように、VM は実行の上限を持つ。

```rust
struct VmConfig {
    max_steps_per_run: Option<u64>,   // 1 回の run で実行できる命令の数
    max_call_depth: u32,              // 台本の呼び出しの深さ
    start: String,                    // 開始ラベル。`モジュール.ラベル`（モジュールが 1 つなら、ラベルだけでもよい）。既定は `start`
    history_limit: usize,             // 停止点の履歴の件数の上限。既定は 200（0 なら記録しない。host/04-save.md の 4.3）
    seed: u64,                        // 乱数の種。既定は固定の値
    seen_points: Vec<String>,         // 通過済みの停止点（`モジュール.ラベル#id`。`seen` が読む。host/04-save.md の 4.5）
    load_policy: LoadPolicy,          // ロードの方針。`Strict`（既定）か `Lenient`（host/04-save.md の 4.6）
    persistent: Option<Vec<u8>>,      // 永続データ（host/04-save.md の 4.5）。あれば、VM を作る・ロードするときに入れる
    max_value_bytes: usize,           // 1 つの値の大きさ（1.9）。既定は 16 MiB
    max_total_bytes: Option<usize>,   // 変数に保持された値の合計の大きさ（1.9）。既定は `Some(64 MiB)`、`None` は上限なし
    interrupt: Option<InterruptHandle>, // 中断のトークン（1.9a）。`None` なら、VM が自分で作る
}
```

- **命令数の上限**：`run` を呼んでから、次の停止点（または `End`）に着くまでに、実行できる命令の数。超えると、実行時エラー（`StepLimitExceeded`）。`run` を呼ぶたびに、数え直す。
- 命令は、バイトコードの命令を、1 つずつ数える。コマンド（Rust の関数）の呼び出しは、1 命令として数える。関数の中の時間は、数えない（ホストの責任）。
- 数えるのは、実行した命令の数であり、時間ではない。環境によらず、同じ位置で超える（1.7）。
- `max_steps_per_run` が `None` のときは、上限なし。既定は `Some(50_000_000)`。`fn` の中で実行した命令も数える（language/03-statements.md の 3.5）。
- **呼び出しの深さ**：`call` の入れ子（コールスタックのフレームの数）と、`fn` の呼び出しの入れ子を合わせた数の上限。`jump` は、フレームを積まない。超えると、実行時エラー（`CallDepthExceeded`）。既定は 256。VM は、`call` も `fn` の呼び出しも Rust の再帰で実行しないので、上限を上げても、ホストのスタックは溢れない。
- **値の大きさ**：1 つの値の大きさが `max_value_bytes` を超えると、実行時エラー（`ValueTooLarge`）。値を作ったとき（文字列の連結、`list`・`map`・`Option`・`struct`・`enum` を作る・要素を足す・フィールドや要素へ代入する、`range` が作る列など（`range` は、列を作る前に大きさを見る））に検査する。入れ子の値は、中身を含めた大きさで数える（language/03-statements.md の 3.7）。
- **値の合計**：変数（モジュールの変数、ホストが宣言した変数、ラベルと `fn` のローカル変数・引数）に保持された値の大きさの合計が `max_total_bytes` を超えると、実行時エラー（`MemoryLimitExceeded`。名前は `memory_limit_exceeded`）。合計が増える操作（代入・`let`・更新するメソッド・`for` の進み・呼び出しの引数など）のあとに検査する。実装は、増分で数え、ロードや復元のあとは数え直す。
- 値の大きさは、環境によらない**論理の大きさ**で数える（実際のメモリの使用量ではない）。内容を共有する値（`Rc` など）も、共有を数えず、別々のものとして数える。
  - `int`・`float`・`bool`・戻り値なし：8
  - `str`：24 + UTF-8 のバイト数
  - `list<T>`：24 + 要素の大きさの合計
  - `Option<T>`：`None` は 8、`Some(x)` は 8 + `x` の大きさ
  - `struct`・`enum` の値：24 + フィールド（引数）の大きさの合計
  - `map<K, V>`：24 + (キーの大きさ + 値の大きさ) の合計
- `max_total_bytes` が `None` のときは、上限なし。既定は、`max_value_bytes` が 16 MiB、`max_total_bytes` が 64 MiB。
- 上限を超えたあとの VM の扱いは、1.5 のとおり。

## 1.9a 中断

別のスレッドから、実行中の `run` を止める手段。命令数の上限（1.9）と違い、どこで止まるかは決まらない（環境と時間による）。

```rust
#[derive(Clone)]
struct InterruptHandle { /* … */ }   // Send + Sync

impl InterruptHandle {
    fn new() -> Self;
    fn interrupt(&self);              // 中断を求める
    fn is_interrupted(&self) -> bool;
    fn reset(&self);                  // 求めを取り消す
}

impl Vm {
    fn interrupt_handle(&self) -> InterruptHandle;   // この VM が見るトークンの複製
}
```

- VM は、トークンを 1 つ見る。`VmConfig::interrupt` で渡せば、それを見る（複数の VM で、1 つのトークンを共有できる）。渡さなければ、VM が自分で作る。`Vm::load` も、`VmConfig` に従う。`restore` は、トークンを替えない。
- `run` の間、VM は、次の**確認点**でトークンを見る。中断が求められていれば、実行時エラー `Interrupted`（コード `interrupted`）を返す。
  - `run` を始めたとき。
  - 命令を実行している間、一定の命令数ごと（間隔は実装が決め、1024 命令以下）。`fn` の中も含む。
  - 副作用型・問い合わせ型のコマンドの関数から戻ったとき。
- 停止点で止まっている VM は、トークンを見ない。止まっている間に中断を求めたときは、次の `run` の始めで `Interrupted` になる。
- 中断の求めは、VM が `Interrupted` を返しても、取り消されない。ホストが `reset` を呼ぶまで、そのトークンを見る VM の `run` は、すべて `Interrupted` になる（共有したトークンで、まとめて止められる）。
- `Interrupted` のあとの VM は、ほかの実行時エラーのあとと同じ（1.5）。ホストは、`reset` してから、スナップショットから `restore` して続ける。
- コマンドの関数は、`Ctx` から中断の求めを読める（host/02-registry.md の 2.4）。長い処理の途中で求めを見つけたら、`CommandError::interrupted()` を返す。VM は、これを `CommandFailed` ではなく、`Interrupted` として返す。
- コアは、時間の期限を持たない。期限は、ホストが見張りのスレッドなどで時間を測り、期限が来たら `interrupt` を呼ぶ（1.10）。
- 中断は、決定性（1.7）の外にある。中断で止まった `run` は、エラーを返すだけで、停止点の履歴・セーブ・スナップショットには、何も残さない。
- `Repl`（host/07-repl.md の 7.8）も、同じトークンを持つ（`repl.interrupt_handle()`）。中断は、ほかの実行時エラーと同じく、その入力を取り消す。

## 1.10 時間と非同期 IO（ホストでの扱い）

コアは、同期・単一スレッドで、時間も非同期 IO も持たない。必要なときは、ホストが次のように受ける。

**非同期 IO**

- IO を伴う処理は、停止型コマンドとして登録する。
- VM が `Yield::Stop { command, args, id }` を返したら、ホストが IO を始める（VM は止まったまま）。完了したら、`vm.resume(Input::Return(値))` で再開する。失敗は `Result` で返す（host/02-registry.md）。
- VM は IO の実行状態を持たない。待ち中の VM は通常の停止点なので、そのままセーブできる。
- 待ち中にセーブしたとき、セーブには IO の進行状態も結果も入らない。ロード後の扱いは、コマンドごとにホストが決め、コマンドの仕様に書く。

| 方式 | 内容 | 向く処理 |
| --- | --- | --- |
| 再発行 | ロード後、停止点で同じコマンドをもう一度実行する | 読み取り・冪等な処理 |
| 結果の記録 | 完了した結果をパッケージの状態スライスに保存し、ロード後はそれを返す | 副作用のある処理・再実行できない処理 |

**外部イベント**

- 外部のイベント（承認・メッセージの到着など）を待つときも、停止型コマンドで受ける。標準パッケージの `event.wait`（packages/event.md）を使える。
- 待ち中は通常の停止点なので、セーブして VM を捨て、イベントが届いたらロードして `resume` できる。多くのセーブから、待っているコマンドと引数を、ロードせずに読むには、`pending_stop`（host/04-save.md の 4.9）を使う。

**時間**

- 待ちは `@flow.wait(秒)` で、ホストが測る（packages/flow.md）。
- 実行の期限（実時間）は、コアに持たせない。ホストが測り、期限が来たら中断する（1.9a）。
- 実時間（`deltaTime` など）を台本に渡さない。リプレイ・ロールバック・golden テストの決定性（1.7）を保つため。
- 台本から時間の値が要るときは、ホストが仮想時間を問い合わせ型コマンドで返す。仮想時間はパッケージの状態スライスに入れ、セーブに含める（host/03-packages.md の 3.2）。実時間から仮想時間への進め方は、ホストが決める。
- リプレイでは、実時間ではなく、記録した仮想時間を使う。

**持たないもの**

- 非同期の Future・タスク・並行実行を、VM に持たせない。
- 実行環境（tokio など）にコアが依存しない。
