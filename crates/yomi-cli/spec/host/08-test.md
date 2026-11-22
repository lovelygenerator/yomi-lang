# 8. 分岐の探索

この章は、台本の分岐を自動で巡る**探索**のコア API を定める。探索は、選択と入力の答えをすべて試して、終わりに着く経路・エラーになる経路・着かない停止点を見つける。CLI の `yomi test`（host/06-cli.md の 6.17）は、この API と、経路ごとの表示を保存して比べる golden で作る。この章の Rust のコードは、形を示すためのもので、型の細部は rustdoc を正とする。

## 8.1 探索の API

```rust
fn explore(
    program: &Program,
    registry: &Registry,
    config: &ExploreConfig,
    answers: &mut dyn FnMut(&Yield) -> Option<Vec<Value>>,
) -> Result<ExploreReport, VmError>;

struct ExploreConfig {
    vm: VmConfig,         // VM の設定（開始ラベル・上限など）。`seed` は使わず、`seeds` で替える
    seeds: Vec<u64>,      // 乱数の種。既定は、`VmConfig` の既定の種 1 つ
    max_states: usize,    // 調べる状態の数の上限。既定は 100_000
}
```

- 種ごとに、`Vm::new` で開始ラベルから探索する。`Vm::new` が失敗したとき（開始ラベルがないなど）は、`Err`。`seeds` が空なら、既定の種 1 つとみなす。
- 永続データは、空から始める（`VmConfig::persistent` は使わない）。本文の差し替え表は、使わない。
- 探索は、決定的である。同じプログラム・登録表・設定・`answers` なら、同じ報告になる。

### 答えの候補

停止点ごとに、次の候補を、この順に 1 つずつ試す。

| 停止点 | 候補 |
| --- | --- |
| 選択（糖衣の選択。language/05-sugar.md の 5.5） | 渡した一覧の、すべての番号（先頭から）。`answers` は呼ばない |
| 戻り値が `()` のコマンド（素の行・引用行を含む） | `Value::Unit` の 1 つ。`answers` は呼ばない |
| そのほか | `answers` が返した値（返した順）。重なりは除く |

- `answers` が `None` か空を返した停止点は、**答えられない停止点**として報告し（8.2）、その先は探索しない。
- 候補を `resume` したときのエラー（型が合わない値・戻り値の検査の失敗）は、実行時エラーと同じく、エラーの経路として報告する。
- 標準パッケージのコマンドの候補は、`yomi_std::test_answers`（8.4）が返す。ホストは、自分のコマンドの候補を足した関数を渡す。

### 状態と打ち切り

- 停止点に着くたびに、**状態**を比べる。状態は、スナップショット（host/01-execution.md の 1.8）の内容から履歴を除いたものに、`persistent var` と通過済みの停止点を加えたもの。乱数の状態を含む。
- すでに着いた状態に着いたときは、その先を探索しない（経路の扱いは 8.3）。
- 状態の数が `max_states` に達したら、探索をやめ、報告の `truncated` を立てる。
- 1 回の `run` は、`VmConfig::max_steps_per_run` の上限を受ける。超えたときは、エラーの経路（`step_limit_exceeded`）として報告する。
- 探索は幅優先で、候補の順に進む（状態の数が増え続ける台本でも、浅い所の分岐を先に調べるため）。探索のあいだ、履歴は持たない（`VmConfig::history_limit` は 0 として動かす）。

## 8.2 報告

```rust
struct ExploreReport {
    paths: Vec<TestPath>,           // 終わりに着いた経路（8.3）
    errors: Vec<ErrorPath>,         // 実行時エラーに着いた経路
    unanswered: Vec<StuckPath>,     // 答えられない停止点で止まった経路
    unreached_stops: Vec<String>,   // 一度も着かなかった停止点
    unchosen_options: Vec<String>,  // 一度も選ばれなかった選択肢
    unreached_labels: Vec<String>,  // 一度も入らなかったラベル（"モジュール.ラベル"）
    states: usize,                  // 調べた状態の数
    truncated: bool,                // `max_states` で打ち切った
}

struct Step {
    stop: String,        // 停止点の名前（下）
    command: String,     // 止まったコマンドの名前
    kind: StopKind,      // 停止点の種類（host/04-save.md の 4.3）
    answer: Value,       // 渡した答え（選択は番号。0 から）
    option_id: Option<String>,  // 選択のとき、選んだ選択肢の安定 ID
}

struct TestPath  { name: String, seed: u64, steps: Vec<Step> }
struct ErrorPath { seed: u64, steps: Vec<Step>, error: VmError }
struct StuckPath { seed: u64, steps: Vec<Step>, stop: String, command: String }
```

- `steps` は、開始から、その経路で渡した答えの列（`Value::Unit` の答えも含む）。同じ種と同じ答えの列で動かせば、同じ所に着く（再現の手順になる）。
- **停止点の名前**：安定 ID があれば `モジュール.ラベル#id`。なければ `モジュール.ラベル:行`（行は 1 から）。選択肢の名前も同じ（選択肢の行）。
- `unreached_stops`・`unchosen_options`・`unreached_labels` は、すべての種を合わせて決める。台本の順に並べる。`truncated` のときは、着かなかったのが打ち切りのためかもしれない（報告はするが、確かではない）。
- `errors` は、同じ停止点・同じ種類のエラーにつき 1 つ（最初に見つけた経路）。`unanswered` も、同じ停止点につき 1 つ。

## 8.3 経路

- 経路は、開始から終わり（`Yield::End`）までの答えの列である。
- 各状態から終わりへの**続き**は、最も短いもの（同じ長さなら、候補の順で先のもの）である。終わりに着く続きがない状態へ向かう候補は、経路に入れない（そこから先のエラーなどは、`errors` と `unanswered` で報告している）。
- `paths` は、終わりに着く続きのある状態から出るどの候補も、少なくとも 1 つの経路が通るように選ぶ。経路は、開始からその候補までの最短の道、その候補、終わりまでの続き、である。経路の数を少なくするため、あとに見つけた候補から順に選び、すでに選んだ経路が通った候補は、選ばない。結果は、候補を見つけた順に並べる。
- **経路の名前**：経路の中の選択の答えを、選んだ選択肢の安定 ID（なければ `c` と、一覧の 1 始まりの番号）で表し、`-` でつなぐ。選択がなければ `main`。
  - `seeds` が 2 つ以上なら、先頭に `s<種>-` を付ける。
  - 同じ名前が重なれば、探索の順で、2 つめから `~2`・`~3` … を付ける。
  - 200 文字を超えるときは、先頭の 180 文字に、`~` と、名前全体の FNV-1a（64 ビット）の 16 桁の 16 進数（小文字）を続ける。
- 名前は、ファイル名に使える文字（`a-z`・`0-9`・`_`・`-`・`~`）だけになる。

## 8.4 標準パッケージの答え

```rust
fn test_answers(registry: &Registry, stop: &Yield) -> Option<Vec<Value>>;   // yomi_std
```

| コマンド | 候補（重なりは除く） |
| --- | --- |
| `input.text` | `default`、空の文字列、`a` を `max_len` 個並べた文字列（`max_len` が 100000 を超えるときは、100000 個） |
| `input.integer` | `default`、`min`、`max` |
| `input.confirm` | `true`、`false` |
| `event.wait` | `names` の順に、名前ごとに `Arrived(名前, データ)`。データは、`--event`（6.17）で名前に指定したもの（指定の順）。指定がなければ空の文字列 1 つ。`timeout` が `Some` なら、最後に `TimedOut` |
| そのほか | `None` |

- コマンドの名前を調べるため、登録表を受け取る。
- 名前ごとのデータは、`test_answers_with_events(registry, stop, events)`（`events` は `(名前, データ)` の組の並び）で渡す。`test_answers` は、`events` が空のときと同じ。その停止点の `names` にない名前の指定は使わない。名前に指定があるとき、空のデータの候補は含めない（空も試すなら `名前=` と指定する）。
- 戻り値が `()` のコマンド（`flow.wait` など）は、8.1 のとおり、コアが答える。
