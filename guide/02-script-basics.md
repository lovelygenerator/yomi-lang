# 02 台本の基本

承認フローの台本に出てくる書き方を、小さな例で順に見ます。ここの例は、どれもそのまま `yomi check` に通ります。細かい決まりは、仕様書（[language/](../spec/language/01-lexical.md)）にあります。

## ラベルと `jump`

台本は、`label` の集まりです。`jump` で別のラベルへ移り、`end` で終わります。`jump` で移ると、いまのラベルの `let` の変数は捨てられます。

```yomi
label start {
    宿屋の主人が、にこやかに迎えてくれた。
    jump room
}

label room {
    部屋は静かだった。
    end
}
```

## 変数

変数は 3 種類です。

| 書き方 | 寿命 |
| --- | --- |
| `save var 名前: 型 = 値` | 台本の全体。**セーブに入る** |
| `persistent var 名前: 型 = 値` | 全セーブで共通。別のデータ（永続データ）に入る |
| `let 名前 = 値` | そのラベルの中だけ。`jump` で捨てられる |

文字列の中の `${式}` は、値を埋め込みます。

```yomi
save var gold: int = 100
persistent var visits: int = 0

label start {
    let name = "旅人"
    visits += 1
    gold -= 30
    ${name}は、残りの所持金${gold}ゴールドを数えた。
    end
}
```

承認フローが、申請（`request`）と金額（`amount`）を `save var` に置いているのは、承認を待つあいだに、`jump` でラベルをまたぎ、セーブされるからです。`let` の変数は、`jump` で消えます。

## 型・`Option`・`match`・`enum`

型は静的で、`int`・`float`・`bool`・`str`・`list<T>`・`map<K, V>`・`Option<T>` と、自分で宣言する `struct`・`enum` があります。`null` はありません。ないかもしれない値は `Option<T>`（`Some(x)` か `None`）で、`match` で取り出します。

```yomi
enum Mood { Good, Bad }

fn mood_of(level: int) -> Mood {
    if level >= 3 {
        return Mood.Good
    }
    return Mood.Bad
}

label start {
    let found: Option<int> = Some(3)
    match found {
        Some(n) => {
            見つかった：${n}
        }
        None => {
            見つからなかった。
        }
    }
    match mood_of(5) {
        Good => {
            機嫌がよさそうだ。
        }
        Bad => {
            機嫌が悪そうだ。
        }
    }
    end
}
```

`match` は、すべての場合を書かないと、コンパイルエラーになります。承認フローの `json.get_int(request, "amount")` は `Option<int>` を返し、金額が読めない申請（`None`）を、`match` で必ず扱うことになります。

## `fn` と `call`

`fn` は、計算のための関数です。停止点を含められません。承認フローの `approver_for` が、これです。

停止点を含む流れは、ラベルに引数と戻り値をつけて、`call` で呼びます。呼び出し元へ戻れるので、`jump` と違って、呼び出し元の `let` は残ります。

```yomi
label start {
    let ok = call pay(30)
    if ok {
        支払いを済ませた。
    }
    end
}

label pay(price: int) -> bool {
    if price > 50 {
        return false
    }
    return true
}
```

## 安定 ID

停止点にあたる行（本文・コマンドの行など）の終わりに、`#id` を付けられます。承認フローの `#waiting` や `#wait_decision` です。安定 ID は、セーブから再開するとき、台本を書き換えたあとでも、停止した位置を探し当てる手がかりになります（[04 保存して再開する](04-save-and-resume.md)）。手で付けなくても、`yomi assign-ids` が付けてくれます。

## コマンド

`@名前(引数)` は、コマンドの呼び出しです。コマンドは、Rust のホストが登録したもので、台本ができることは、登録されたコマンドだけです。承認フローの `@event.wait(...)` は、標準パッケージ `event` のコマンドで、ホストに値を返してもらうまで止まります（[05 外部イベントを待つ](05-waiting-for-events.md)）。標準パッケージを使うには、台本の先頭で `import std::event` のように書きます。

## 次へ

[03 Rust に組み込む](03-embedding.md)で、この台本を動かすホストを書きます。
