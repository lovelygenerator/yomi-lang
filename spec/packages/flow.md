# パッケージ `flow`（yomi-std）

流れの待ち。すべて停止型コマンド。時間を測るのも、入力を受けるのも、ホストである（host/01-execution.md の 1.3）。

状態スライス・永続スライスは、持たない（`State = ()`、`Persistent = ()`）。

| コマンド | 引数 | 意味 |
| --- | --- | --- |
| `flow.wait` | `seconds: float` | `seconds` 秒、止まる。`seconds` が負なら、止まらずに実行時エラー（`flow.negative_seconds`）。`0` は、停止点を作る（セーブできる点になる。ホストは、待たずに `resume` してよい） |
| `flow.wait_input` | なし | 何かの入力があるまで止まる。「入力」の種類（クリック・キー・タッチ）は、ホストが決める |

- 台本では、`import std::flow` のあと、`@flow.wait(1.5) #pause` のように書く。安定 ID は、行末に付ける。引数の `float` は、`1.5` のように、小数点つきで書く（`int` を渡すと `type_mismatch`。`to_float` で変換する）。
- 負の秒数の検査は、登録の検証（host/02-registry.md の 2.4 の `stop_checked`）で行う。VM は、停止の知らせを返す前に検査し、`Err` なら止まらない。
- ホストは、`Yield::Stop` のコマンド名（`flow.wait`・`flow.wait_input`）で、処理を分ける。`yomi run`（CLI）は、実際には待たず、何を待つ点かだけを示して、Enter で進む。
- 待ちを、飛ばせるか（スキップ・オート）は、ホストが決める。飛ばすときは、すぐに `resume` を呼ぶ。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `flow.negative_seconds` | `seconds` | `flow.wait` の `seconds` が負 |
