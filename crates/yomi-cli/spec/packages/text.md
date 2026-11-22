# パッケージ `text`（yomi-std）

文字列の操作。すべて、問い合わせ型コマンド。`import std::text` のあと、`text.len(s)` のように呼ぶ。パッケージ名は、`str`（型名で予約語）を避けて `text` とする。状態スライス・永続スライスは、持たない。

## 「文字」の数え方

1 文字は、**書記素クラスタ**（見た目の 1 文字。異体字・結合文字・ZWJ の絵文字・国旗を、1 文字とする）。

外部の依存を足さないため、数え方は、Unicode の書記素クラスタの規則（UAX #29）の**近似**を、`yomi-std` の中に持つ。1 文字に数えるのは、次のもの。

- `CR LF`（単独の `CR`・`LF`・制御文字は、1 文字ずつ）
- 基底の文字と、それに続く結合文字（U+0300 台・U+1AB0 台・U+1DC0 台・U+20D0 台・U+FE20 台・かなの結合用の濁点 U+3099・半濁点 U+309A）・異体字セレクタ（U+FE00〜FE0F・U+E0100〜E01EF）・肌の色の修飾（U+1F3FB〜1F3FF）・タグ文字（U+E0020〜E007F）・ZWNJ（U+200C）
- ZWJ（U+200D）は、前の文字にくっつく（前の文字が絵文字かは見ない）。ZWJ の次が、絵文字の範囲の文字のときは、その文字も同じ 1 文字に数える（範囲は、U+00A9・U+00AE・U+203C・U+2049・U+2190〜2BFF・U+3030・U+303D・U+1F000〜1FAFF）
- 国旗（地域指標の記号 U+1F1E6〜1F1FF の 2 つ。3 つ目からは、次の旗）

近似なので、次は、本物の規則どおりには数えない（本物より多く数えることが多い）。

- インド系の文字・タイ文字・アラビア文字・ヘブライ文字などの、上の範囲にない結合記号
- 初声・中声・終声の字母（ハングルの U+1100 台）の並び（現代の合成済みの音節は、1 文字）
- ZWJ の次が、絵文字の範囲の外の記号のとき（その記号は別の 1 文字になり、つながりとは見ない）。前の文字が絵文字でなくても、ZWJ の次が範囲の中の文字なら、つなげて数える

数え方の版は、`yomi_std::text::GRAPHEME_RULES_VERSION`（今は 1）で表す。数え方を直すときは、この値を上げる。セーブの形式の版数（`Package::VERSION` など）とは関係しない。`text` の `RULES_VERSION`（host/03-packages.md の 3.1。今は 1）は、数え方を含む `text` 全体の結果の規則の版で、数え方の版とは別に持つ。数え方を直したときと、ほかの規則（`trim` の空白・`upper`・`lower` の対象・`find` の位置の決め方など）を直したときに上げ、セーブに記録される。版が違うセーブを読むと、`LoadReport::changed_rules` に `text` が出る。

## 関数

文字列の連結は `${…}` で行う（`+` は持たない）。位置・幅・数は、すべて書記素クラスタの単位。

| 関数 | 戻り値 | 意味 |
| --- | --- | --- |
| `len(s: str)` | `int` | 書記素クラスタの数 |
| `to_int(s: str)` | `Option<int>` | 文字列を整数として読む。先頭に `+`・`-` を 1 つまで、あとは ASCII の数字が 1 つ以上。空白・桁の区切り・全角の数字・`int` の範囲の外は、`None` |
| `to_float(s: str)` | `Option<float>` | 文字列を小数として読む。先頭に `+`・`-` を 1 つまで、10 進の数字と小数点（`1.5`・`.5`・`2.`）と指数（`1e3`・`2.5E-2`）。`inf`・`NaN` などの名前、有限でない結果（`1e400`）、空白は `None`。`-0.0` は `0.0` |
| `contains(s: str, part: str)` | `bool` | `part` を含むか。`part` が空なら `true` |
| `starts_with(s: str, prefix: str)` | `bool` | `prefix` で始まるか。`prefix` が空なら `true` |
| `ends_with(s: str, suffix: str)` | `bool` | `suffix` で終わるか。`suffix` が空なら `true` |
| `trim(s: str)` | `str` | 前後の空白（Unicode の `White_Space`。全角スペースを含む）を取り除く |
| `replace(s: str, from: str, to: str)` | `str` | `from` の、重ならない一致を、前から順にすべて `to` に置き換える |
| `split(s: str, sep: str)` | `list<str>` | `sep` で区切る。`s` が空なら `[""]`。区切りが続くと、空の文字列の要素ができる |
| `join(parts: list<str>, sep: str = "")` | `str` | `parts` を `sep` でつなぐ。`parts` が空なら `""` |
| `slice(s: str, start: int, to: int)` | `str` | 書記素の位置 `start` から `to` の手前まで |
| `find(s: str, part: str)` | `Option<int>` | `part` が最初に現れる書記素の位置。なければ `None`。`part` が空なら `Some(0)` |
| `repeat(s: str, n: int)` | `str` | `s` を `n` 回くり返す |
| `pad_left(s: str, width: int, fill: str = " ")` | `str` | 書記素 `width` 個になるまで、左に `fill` をくり返す |
| `pad_right(s: str, width: int, fill: str = " ")` | `str` | 同じく、右に |
| `upper(s: str)` | `str` | ASCII の英字（`a`〜`z`）だけを大文字にする |
| `lower(s: str)` | `str` | ASCII の英字（`A`〜`Z`）だけを小文字にする |

- `= 値` は、引数の既定値。省いた引数は、既定値になる。名前つきで渡せる（`text.join(xs)`・`text.pad_left(s, 3, fill: "0")`。language/03-statements.md の 3.5）。

`contains`・`starts_with`・`ends_with`・`replace`・`split`・`find` は、コードポイントの列として比べる（正規化はしない）。書記素クラスタの途中から始まる一致も、一致とする。`replace` と `split` は、一致の位置でそのまま切るので、結合文字の手前に一致があれば、結合文字は後ろの部分に残る。

`trim` は、Rust の `str::trim` と同じ（Unicode の `White_Space`。U+3000 の全角スペース・U+00A0 は取り除き、U+200B のゼロ幅スペースは取り除かない）。

`upper`・`lower` は、ASCII の英字だけを変え、ほかの文字（全角の英字・`é`・`ß` など）はそのままにする。ロケールにも、Unicode の大文字小文字の規則にも依存しない。

### 書記素の位置

`slice` の `start`・`to` と、`find` の戻り値は、書記素クラスタの位置（先頭が 0。`len` と同じ数え方）。

- `slice` は、`0 <= start <= to <= len(s)` でなければならない。書記素クラスタの境目でだけ切る。`start == to` なら `""`。
- `find` が返すのは、最初の一致を含む書記素クラスタの位置。一致がクラスタの途中から始まるとき（たとえば、`e` に結合文字がついた 1 文字の、`e` の部分）は、そのクラスタの位置を返す。

### ホスト（Rust）から使う関数

`yomi-std` の `text` モジュールは、上と同じ数え方を、Rust の関数としても公開する。ホストが、台本の `len`・`slice`・`find` の位置を、表示の位置やバイト位置に直すときに使う。

| 関数 | 戻り値 | 意味 |
| --- | --- | --- |
| `grapheme_count(s: &str)` | `usize` | 書記素クラスタの数（`len` と同じ値） |
| `grapheme_offsets(s: &str)` | `Vec<usize>` | 各クラスタの先頭のバイト位置と、最後に `s.len()`。長さは「数 + 1」。`[i]` が台本の位置 `i` に当たる（空の文字列では `[0]`） |
| `graphemes(s: &str)` | `impl Iterator<Item = &str>` | クラスタを先頭から順に返す。連結すると `s` に戻る |

数え方を直すと（`GRAPHEME_RULES_VERSION` が上がると）、これらの結果も変わる。ホストは、この定数で数え方の版を確かめられる。

### 実行時エラー

次の場合は、実行時エラー（コードは、末尾の「エラーコード」）。

| 関数 | 条件 |
| --- | --- |
| `replace` | `from` が空 |
| `split` | `sep` が空 |
| `slice` | `start`・`to` が、上の範囲にない |
| `repeat` | `n` が負 |
| `pad_left`・`pad_right` | `width` が負。`fill` が書記素 1 つでない（空・2 つ以上） |
| `replace`・`join`・`repeat`・`pad_left`・`pad_right` | 結果が大きすぎる |

結果の大きさの上限は、16 MiB（UTF-8 のバイト数。コアの `max_value_bytes` の既定と同じ）。`replace`・`join`・`repeat`・`pad_*` は、結果を作る前に大きさを見積もって断るので、巨大な `n`・`width` でも、メモリを使い切らない。`repeat` は、`s` が空なら、`n` がどれだけ大きくても `""`。`pad_*` は、`width` が `len(s)` 以下なら、`s` をそのまま返す（`fill` の検査は、先に行う）。

### 例

```
import std::text

label start {
  let names = text.split("ゆみ,ひろ,さく", ",")
  ${text.join(names, "・")}
  ${text.pad_left("7", 3, "0")}
  ${text.slice("あいうえお", 1, 3)}
  ${text.replace("あいあ", "あ", "ー")}
}
```

表示は、順に `ゆみ・ひろ・さく`・`007`・`いう`・`ーいー`。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `text.empty_arg` | `command`, `arg` | `replace` の `from`、`split` の `sep` が空 |
| `text.fill_not_grapheme` | `command` | `pad_left`・`pad_right` の `fill` が書記素 1 つでない |
| `text.negative` | `command`, `arg` | `repeat` の `n`、`pad_left`・`pad_right` の `width` が負 |
| `text.result_too_large` | `command`, `limit` | 結果が大きすぎる。`limit` は上限（バイト） |
| `text.slice_range` | `start`, `end`, `len` | `slice` の範囲が `0 <= start <= to <= len` にない。`end` は `to` に渡された値、`len` は書記素の数 |
