# 9. デバッグ

この章は、台本をソースの行の単位で追い、止め、調べるための、コアの約束を定める。ブレークポイント・ステップ実行・呼び出しの確認・トレースの土台である。コマンドライン（`yomi debug`）と Debug Adapter Protocol のサーバー（`yomi dap`）は、この上に作る（別の章）。この章の Rust のコードは、形を示すためのもので、型の細部は rustdoc を正とする。

## 9.1 考え方

- 通常の実行（`run`・`resume`）は、この章の機能を使わない。**デバッグ用の実行（`run_debug`）を使ったときだけ**、行の切り替わりごとに、ホストが渡した**フック**が呼ばれる。通常の実行の速さは変わらない。
- 止める・止めないは、フックが決める。ブレークポイント・ステップ・トレースは、フックの作り方の違いである。コアは、よく使う組み合わせを `Debugger` として用意する（9.6）。
- 止まる単位は、**ソースの行**である。`fn`・ラムダ式・高階メソッド（`map` など）の本体の中も、行ごとに追える。
- デバッグ用の実行が止まるのは、停止点（`Yield::Stop`）とは別の、**一時停止**である。一時停止中は、セーブ・スナップショット・`reload` はできない（停止点でだけできる）。

## 9.2 行イベントとフック

```rust
trait DebugHook {
    fn on_line(&mut self, event: &LineEvent) -> HookAction;
}

struct LineEvent<'a> {
    module: &'a str,       // 行があるモジュール
    unit: &'a str,         // 行があるラベルまたは `fn` の名前（コンパイラが作る関数は、`<` で始まる名前）
    kind: UnitKind,        // Label / Fn
    line: u32,             // 1 から数える行番号
    depth: usize,          // 呼び出しの深さ（9.3）
}

enum HookAction { Continue, Pause }
```

**行イベント**は、命令を実行する直前に、次のときに起こる。

- 直前に実行した命令と、**ラベルまたは `fn` が違う**、または**行が違う**。
- 初めて実行するとき（直前の命令がないとき）。

次の場合は、起こらない。

- 一時停止から続けるとき、止まった行の最初の命令（もう知らせたため）。
- 停止点（`Yield::Stop`）から `resume` して `run_debug` を続けるとき、停止点の命令と同じ行の、続きの命令（戻り値を受ける代入など）。

次のあとは、「直前の命令」を忘れる（続きの最初の命令で、必ず起こる）。`run`（通常の実行）・`restore`・`load`・`reload`。

- 行のない命令（コンパイラが足した命令。行番号が 0）では、起こらない。
- `fn` から戻った直後は、呼び出した行（呼び出し元の、行の途中）で、もう一度起こる。ラベルの終わり（暗黙の `end`）は、ラベルを宣言した行の行イベントを起こす。
- フックが `Pause` を返すと、その命令を実行せずに、`run_debug` が `DebugYield::Paused` を返す。`Continue` なら、そのまま実行する。
- 1 行の中で何度も繰り返す処理（1 行に書いた `while` など）は、行が変わらないので、続けて起こらない。

## 9.3 呼び出しの深さ

`depth` は、呼び出しの入れ子の数である。ラベルの `call` と、`fn`・ラムダ式の呼び出し（高階メソッドの本体を含む）を、1 つずつ数える。最初のラベルの中は 0。

- ステップ実行（9.6）は、`depth` の比べで、「中へ入る・飛ばす・出る」を決める。
- `jump` は、深さを変えない。

## 9.4 デバッグ用の実行

```rust
enum DebugYield {
    Stop(Yield),     // 通常の実行と同じ停止点（`Yield::Stop`）、または終わり（`Yield::End`）
    Paused,          // フックが一時停止を求めた
}

impl Vm {
    fn run_debug(&mut self, hook: &mut dyn DebugHook) -> Result<DebugYield, VmError>;
    fn is_paused(&self) -> bool;
}
```

- `run_debug` は、`run` と同じく、実行を始める（`Ready`）ときと、一時停止から続けるときに呼べる。停止中（`resume` を待っている）に呼ぶと、`run` と同じエラー（`AlreadyStopped`）。
- 停止点で `Yield::Stop` を受けたら、ホストは、通常どおり `resume` してから、また `run_debug` を呼ぶ。
- 一時停止中に、`run`（通常の実行）を呼ぶと、フックなしで、続きを実行する。一時停止中の `resume`・`save`・`snapshot`・`reload` は、`NotStopped`。
- 実行の上限（命令数）・中断（`InterruptHandle`）は、デバッグ用の実行でも、通常と同じように効く。一時停止のあいだの時間は、数えない。
- 実行時エラーは、通常と同じ（VM は、続けられない状態になる）。

## 9.5 呼び出しの確認

```rust
struct FrameInfo {
    module: String,
    name: String,        // ラベルまたは `fn` の名前
    kind: UnitKind,
    line: u32,           // そのフレームがいま実行している行（上のフレームを呼んでいる行）
}

impl Vm {
    fn call_stack(&self) -> Result<Vec<FrameInfo>, VmError>;
    fn frame_variables(&self, frame: usize) -> Result<Vec<Variable>, VmError>;
}
```

- どちらも、**一時停止中**と、**停止点で止まっているとき**に呼べる。それ以外は `NotStopped`。
- `call_stack` は、いちばん内側のフレームから外側へ並べる（`frame` の 0 が、いま実行しているところ）。ラベルの `call` と、`fn`・ラムダ式の呼び出しが、同じ並びに入る。停止点で止まっているときの 0 番目の行は、その停止点の行。
- `frame_variables` は、そのフレームの**ローカル変数**（引数を含む。名前が `$` で始まる隠しの変数と、関数の型の変数は除く）を、`Variable`（`local` は真）で返す。モジュール変数・ホストの変数は、`Vm::variables()` の既存の結果を使う。`frame` が範囲の外なら、空の `Vec` を返す。
- 一時停止中の値は、実行の途中のもので、まだ代入されていない変数は、型の初期値か、前の値のままである。

## 9.6 `Debugger`（ブレークポイントとステップ）

```rust
struct Debugger { /* … */ }
enum StepMode { Continue, In, Over, Out }

impl Debugger {
    fn new() -> Self;
    fn set_breakpoints(&mut self, module: &str, lines: &[u32]);   // そのモジュールの分を置き換える。空で消す
    fn set_mode(&mut self, mode: StepMode);
    fn pause_handle(&self) -> PauseHandle;       // 別のスレッドから、次の行で一時停止を求める
    fn set_trace(&mut self, f: Option<Box<dyn FnMut(&LineEvent) + Send>>);
}
impl DebugHook for Debugger { /* … */ }
```

`Debugger` は、行イベントごとに、次のとおり判定する。上から順に見て、当てはまれば `Pause`。

1. 一時停止の要求がある（`PauseHandle::request`。要求は、止まったときに消える）。
2. その行（モジュールと行番号）に、ブレークポイントがある。
3. `StepMode` が次のとおり。
   - `In`：次の行イベントで止まる。
   - `Over`：`depth` が、**最後に止まったときの深さ以下**の、次の行イベントで止まる。
   - `Out`：`depth` が、**最後に止まったときの深さより小さい**、次の行イベントで止まる。
   - `Continue`：止まらない（既定）。

- ステップの基準になる「最後に止まったときの深さ」は、`Debugger` が一時停止のたびに覚える。まだ止まっていないときは、`In` は最初の行で止まり、`Over`・`Out` は `In` と同じに扱う。
- 止まったら、`StepMode` は `Continue` に戻る。続けてステップしたいときは、ホストが `set_mode` を呼んでから `run_debug` を呼ぶ。
- ブレークポイントは、**そのブレークポイントの行に、実行できる命令がなければ、止まらない**（空行・コメント・`}` の行など）。どの行で止まれるかは、`Debugger::breakable_lines(program, module)` で調べられる（9.7）。
- `set_trace` のフックは、行イベントごとに呼ばれる（止めるかどうかとは別）。トレースの出力に使う。
- 停止点のある行で、ステップが止まった場合、続けると、その停止点で `DebugYield::Stop` になる。

## 9.7 ブレークできる行

```rust
impl Debugger {
    fn breakable_lines(program: &Program, module: &str) -> Vec<u32>;
}
```

- そのモジュールの、実行できる命令を持つ行を、小さい順に返す（ラベルと `fn` の両方）。重複はない。
- ホストは、設定されたブレークポイントを、これで、一番近い後ろの行に寄せたり、無効として知らせたりできる。`Debugger` 自身は、寄せない。

## 9.8 `reload` との関係

- `reload`（host/04-save.md の 4.10）は、停止点でだけ呼べる。一時停止中は呼べない。
- `reload` のあと、ブレークポイントの行番号は、そのままである（行がずれたときの調整は、ホストが `set_breakpoints` で行う）。

## 9.9 エラー

- `call_stack`・`frame_variables`・`run_debug` の誤った呼び方は、`VmError`（`NotStopped`・`AlreadyStopped`・`Ended`・`Poisoned`）で返る。コードは既存のものである（host/01-execution.md）。
