# 06 JSON を扱う

外部から届くデータや、AI エージェントが返す結果は、たいてい JSON です。yomi には、再帰する型もジェネリクスもないので、JSON を木の値にはしません。JSON は**文字列のまま**持ち、`json` パッケージで、**パスを指定して**取り出します。

```yomi
import std::json

label start {
    let data = "{\"title\":\"モニター\",\"amount\":120000,\"items\":[{\"name\":\"arm\"}]}"
    match json.get_int(data, "amount") {
        Some(amount) => {
            金額は${amount}円です。
        }
        None => {
            金額が読めません。
        }
    }
    match json.get_str(data, "items[0].name") {
        Some(name) => {
            最初の品目は${name}です。
        }
        None => {
            品目がありません。
        }
    }
    end
}
```

## パス

| 書き方 | 意味 |
| --- | --- |
| `""` | 文書の全体 |
| `a.b` | オブジェクトのキー |
| `items[0]` | 配列の添字（0 から） |
| `["x.y"]` | `.` などを含むキー（引用符つき） |

つなげて書けます：`items[0].name`・`a["x.y"][2]`。

## 取り出す

取り出せないとき（JSON が正しくない・パスに値がない・型が違う）は、実行時エラーではなく `None` です。台本は、`match` で必ず扱うことになります。

| 関数 | 戻り値 | 意味 |
| --- | --- | --- |
| `json.get_str(s, path)` | `Option<str>` | 文字列の値 |
| `json.get_int(s, path)` | `Option<int>` | 整数の値（`1.5`・`1e3` は `None`） |
| `json.get_float(s, path)` | `Option<float>` | 数の値（整数も含む） |
| `json.get_bool(s, path)` | `Option<bool>` | 真偽の値 |
| `json.get_raw(s, path)` | `Option<str>` | 値を JSON の文字列として。配列・オブジェクトを取り出して、次の呼び出しへ渡せる |
| `json.kind(s, path)` | `Option<str>` | `"null"`・`"bool"`・`"number"`・`"string"`・`"array"`・`"object"` |
| `json.len(s, path)` | `Option<int>` | 配列の要素数、オブジェクトのキーの数 |
| `json.keys(s, path)` | `Option<list<str>>` | オブジェクトのキー（コードポイント順） |
| `json.has(s, path)` | `bool` | パスに値がある（`null` も値） |
| `json.valid(s)` | `bool` | 正しい JSON か |

## 書き換える

書き出しは最小限です。値は、JSON の文字列で渡します。文字列を入れるときは、`json.quote` で包みます。

```yomi
import std::json

label start {
    let doc = "{\"status\":\"pending\"}"
    match json.set(doc, "status", json.quote("approved")) {
        Some(updated) => {
            ${updated}
        }
        None => {
            更新できませんでした。
        }
    }
    end
}
```

- `json.set(s, path, value)` は、パスの値を置き換えた文書を返します。オブジェクトのキーなら、なければ足します。配列は、既存の添字を置き換え、長さと同じ添字なら末尾に足します。途中のキーは、自動では作りません。
- `json.remove(s, path)` は、値を消した文書を返します。
- 出力は、コンパクトな形で、キーはコードポイント順です（文書の順は保ちません）。

## 制限

- 入れ子は 128 段までです。超える文書は、正しくないものとして扱います（`None`・`false`）。
- `u64` を超える整数は、`float` として読みます（桁が落ちることがあります）。

決まりの全体は、[packages/json.md](../spec/packages/json.md) にあります。

## 次へ

[07 安全に動かす](07-running-safely.md)へ。
