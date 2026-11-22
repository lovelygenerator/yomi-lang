# パッケージ `input`（yomi-std）

プレイヤーからの値の入力。すべて停止型コマンドで、値を戻す。入力の画面を出して、値を集めるのは、ホストである。値は、ホストが `Input::Return` で渡す。

状態スライス・永続スライスは、持たない（`State = ()`、`Persistent = ()`）。

| コマンド | 引数 | 戻り値 | 意味 |
| --- | --- | --- | --- |
| `input.text` | `prompt: str`, `default: str`, `max_len: int` | `str` | 文字列の入力。`max_len` は、書記素クラスタ（`text.len` と同じ数え方）の上限。`max_len <= 0` は、実行時エラー |
| `input.integer` | `prompt: str`, `default: int`, `min: int`, `max: int` | `int` | 整数の入力。`min > max` と、`default` が範囲の外なのは、実行時エラー |
| `input.confirm` | `prompt: str` | `bool` | はい・いいえ |

- `prompt` と `default` は、ホストが入力の画面に出す。コアは、意味を持たない。
- `prompt` は、差し替え表（host/05-texts.md の 5.2a）で訳せる。台本が `prompt` を文字列リテラルで書き、その行に安定 ID を付けたとき（`let name = @input.text("名前は？", "", 8) #name`）、キー `モジュール.ラベル#name` の訳文が、停止の知らせの `prompt` に入る。`default` は、訳さない。
- 引数の制約（`max_len <= 0`・`min > max`・`default` が範囲の外）は、止まる前に実行時エラー（`input.max_len_not_positive`・`input.range_reversed`・`input.default_out_of_range`）。
- ホストが返す値が、制約（`max_len`・範囲）を満たさないとき、コアは、`resume` で実行時エラー（`input.text_too_long`・`input.integer_out_of_range`。位置は VM が付ける）にする（登録の戻り値の検査。host/02-registry.md の `stop_validated`）。そのあと、VM は、実行時エラーのあとの状態になる（スナップショットから戻せる）。制約を満たす入力にさせるのは、ホストの責務である。
- コマンド名は `input.integer`（`input.int` ではない）。`int` は型の名前で、台本で読み違えやすいので避けている。
- 台本は、`let`・`=` の右辺に `@` で書いて、戻り値を変数に受ける（language/02-lines.md の 2.2a）。受けないとき（`@input.text(…)` だけの文）は、値は台本に使われず、ホストの `Input::Return` の側にだけある。
- キー・ボタンの現在の状態を、台本から読む手段は、持たない。

## 使い方

```
import std::input

label start {
  let name = @input.text("名前は？", "", 8)
  let age = @input.integer("年齢は？", 10, 0, 120)
  let ok = @input.confirm("${name}さんで、よろしいですか？")
  ${name}さん（${age}歳）
}
```

- `let name = @input.text("名前は？", "", 8)` は、停止点で止まり、ホストが返した文字列を `name` に入れて、次の文へ進む。受ける変数の型は、戻り値の型（`input.text` は `str`、`input.integer` は `int`、`input.confirm` は `bool`）。
- 受けた値は、本文の `${name}` や、条件式で使える。
- 値が制約を破っていれば、変数に入る前に、実行時エラーになる（上）。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `input.default_out_of_range` | `default`, `min`, `max` | `input.integer` の `default` が `min`〜`max` の外 |
| `input.integer_out_of_range` | `value`, `min`, `max` | ホストが返した整数が `min`〜`max` の外。`value` は返された値 |
| `input.max_len_not_positive` | `max_len` | `input.text` の `max_len` が 1 未満 |
| `input.range_reversed` | `min`, `max` | `input.integer` の `min` が `max` より大きい |
| `input.text_too_long` | `max_len`, `len` | ホストが返した文字列が `max_len` を超える。`len` は書記素の数 |
