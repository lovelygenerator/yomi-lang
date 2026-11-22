# パッケージ `json`（yomi-std）

JSON の文字列から値を取り出す・書き換える。JSON は `str` のまま扱い、`save var` にそのまま置ける。木構造の型は持たない。

状態スライス・永続スライスは、持たない（`State = ()`、`Persistent = ()`）。すべて問い合わせ型で、同じ引数なら同じ結果を返す。

## パス

値の場所を、文字列で指定する。

| 書き方 | 意味 |
| --- | --- |
| `""`（空） | 文書の全体 |
| `a`・`a.b` | オブジェクトのキー。`.` でつなぐ。キーは、`.`・`[`・`]`・`"` を含まない、空でない文字列 |
| `[0]` | 配列の添字（0 から。10 進の数字だけ） |
| `["a.b"]` | キーを引用符で書く。`.` などを含むキー・空のキーに使う。中のエスケープは JSON と同じ |

- つなげて書く：`items[0].name`・`a["x.y"][2]`。先頭のキーの前に `.` は付けない。
- 形が正しくないパス（`a..b`・`a[`・`[-1]`・`[x]`）は、どの値にも当たらない（`None`・`false`）。エラーにはしない。

## 関数

`s` は JSON の文字列、`path` は上のパス。次のときは、すべての `Option` の関数が `None` を返す：`s` が不正な JSON（深さが 128 を超えるものを含む）、パスに当たる値がない、値の型が違う。

| 関数 | 戻り値 | 意味 |
| --- | --- | --- |
| `valid(s: str)` | `bool` | `s` が、ちょうど 1 つの正しい JSON の値か（前後の空白は許す） |
| `has(s: str, path: str)` | `bool` | パスに値がある（`null` も値）。`s` が不正なら `false` |
| `kind(s: str, path: str)` | `Option<str>` | 値の種類。`"null"`・`"bool"`・`"number"`・`"string"`・`"array"`・`"object"` |
| `get_str(s: str, path: str)` | `Option<str>` | 文字列の値 |
| `get_int(s: str, path: str)` | `Option<int>` | 整数の値。`1.0`・`1e3` のように小数点や指数を含む書き方、`int` の範囲の外は `None` |
| `get_float(s: str, path: str)` | `Option<float>` | 数の値（整数も含む）。`float` で有限でないものは `None` |
| `get_bool(s: str, path: str)` | `Option<bool>` | 真偽の値 |
| `get_raw(s: str, path: str)` | `Option<str>` | 値を JSON の文字列にしたもの（出力の形は下）。配列・オブジェクトを取り出して、別の呼び出しの `s` に渡せる |
| `len(s: str, path: str)` | `Option<int>` | 配列の要素数、またはオブジェクトのキーの数。それ以外の種類は `None` |
| `keys(s: str, path: str)` | `Option<list<str>>` | オブジェクトのキー（コードポイント順）。オブジェクト以外は `None` |
| `quote(s: str)` | `str` | `s` を JSON の文字列のリテラルにする（前後の `"` を含む）。`"`・`\`・制御文字（U+0000〜U+001F）だけをエスケープし、ほかはそのまま |
| `set(s: str, path: str, value: str)` | `Option<str>` | パスの値を、JSON の文字列 `value` で置き換えた文書を返す |
| `remove(s: str, path: str)` | `Option<str>` | パスの値を消した文書を返す |

### `set` と `remove` の規則

- `value` は、正しい JSON の値でなければならない（`valid` と同じ）。違えば `None`。文字列を入れるときは、`json.quote` で包む：`json.set(doc, "name", json.quote("太郎"))`。
- パスの最後の要素が、オブジェクトのキーなら、なければ足す。配列の添字なら、`0 <= 添字 < 長さ` の既存の要素を置き換え、`添字 == 長さ` なら末尾に足す（それより大きい添字は `None`）。途中の要素は、すでにある値でなければならない（途中を自動では作らない）。当てはまらなければ `None`。
- パスが `""` の `set` は、文書の全体を `value` に置き換える。`remove` は `None`。
- `remove` は、キーまたは添字の要素を取り除く（配列では、後ろの要素が前へ詰まる）。なければ `None`。

### 出力の形

`get_raw`・`set`・`remove` が返す文書は、コンパクトな形：要素の間に空白と改行を入れない。オブジェクトのキーは、コードポイント順に並べる。キーが重なる入力は、あとのものが勝つ。数は、整数はそのまま、小数は、読み戻して同じ値になる最短の書き方。`u64` を超える整数と、`float` で表せない桁の多い数は、`float` として読む（桁が落ちうる）。

## 使い方

```
import std::event
import std::json

identity 受付 = "受付"

label start {
    let r = @event.wait(["order"], None) #wait_order
    match r {
        Arrived(name, data) => {
            match json.get_str(data, "items[0].name") {
                Some(n) => 受付「${n} を受け付けました。」 #accepted
                None => 受付「注文の形式が正しくありません。」 #invalid
            }
            end
        }
        TimedOut => end
    }
}
```

- 戻り値の `Option` は、`match` で分ける（language/03-statements.md の 3.6）。

## エラーコード

実行時エラーは、ない（取り出せないときは `None`・`false`）。
