# パッケージ `math`（yomi-std）

数値の基本関数。すべて、問い合わせ型コマンド（副作用・乱数・状態の変更なし）。`import std::math` のあと、`math.abs(x)` のように呼ぶ。

## 規則

- 型の多重定義がないので、`int` 用と `float` 用は、別の名前にする。`float` 用には `_f` を付ける。
- 結果が `int` の範囲を超えるとき（`IntegerOverflow`）、`float` の結果が `NaN`・`±∞` になるとき（`NonFiniteFloat`）、`float` が `int` の範囲に収まらないとき（`FloatOutOfRange`）は、実行時エラー（language/03-statements.md の 3.2、3.8）。
- そのほかの失敗（`clamp` の `lo > hi`、`pow` の `exp < 0`）は、パッケージのエラーコード（末尾の「エラーコード」）を持つ実行時エラー。
- 状態スライス・永続スライスは、持たない（`State = ()`、`Persistent = ()`）。

## 関数

| 関数 | 戻り値 | 意味 |
| --- | --- | --- |
| `abs(x: int)` / `abs_f(x: float)` | `int` / `float` | 絶対値。`int` の最小値は、`IntegerOverflow` |
| `min(a: int, b: int)` / `min_f(a: float, b: float)` | 同じ型 | 小さいほう |
| `max(a: int, b: int)` / `max_f(a: float, b: float)` | 同じ型 | 大きいほう |
| `clamp(x: int, lo: int, hi: int)` / `clamp_f(x: float, lo: float, hi: float)` | 同じ型 | `lo` 以上 `hi` 以下に収める。`lo > hi` は実行時エラー（`math.bounds_reversed`） |
| `pow(base: int, exp: int)` / `pow_f(base: float, exp: float)` | 同じ型 | べき乗。`pow` は、`exp < 0`（`math.pow_negative_exp`）と、オーバーフロー（`IntegerOverflow`）が実行時エラー（`pow(0, 0)` は 1）。`pow_f` は、結果が有限でなければ `NonFiniteFloat` |
| `floor(x: float)` | `int` | 切り下げ。範囲外は `FloatOutOfRange` |
| `ceil(x: float)` | `int` | 切り上げ。範囲外は `FloatOutOfRange` |
| `wrapping_add(a: int, b: int)` / `wrapping_sub(a: int, b: int)` / `wrapping_mul(a: int, b: int)` | `int` | 足し算・引き算・掛け算。範囲を超えたら、64 ビットの 2 の補数で折り返す（実行時エラーにしない。ハッシュの計算など） |
| `round(x: float)` | `int` | 四捨五入（半分は、0 から遠いほうへ。`round(2.5)` は 3、`round(-2.5)` は -3）。範囲外は `FloatOutOfRange` |

「範囲」は、`-9223372036854775808.0` 以上 `9223372036854775808.0` 未満（`to_int` と同じ）。

台本の `float` は、常に有限（`NaN`・`±∞` にならない）ので、`min_f`・`max_f`・`clamp_f` の `NaN` の扱いは定めない。

`float` から `int` へ、0 に向かって切り捨てるときは、コアの組み込み関数 `to_int` を使う（language/03-statements.md の 3.8）。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `math.bounds_reversed` | `command`, `lo`, `hi` | `clamp`・`clamp_f` の `lo` が `hi` より大きい。`command` は `math.clamp` か `math.clamp_f` |
| `math.pow_negative_exp` | `exp` | `pow` の `exp` が負 |
