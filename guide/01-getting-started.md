# 01 はじめに

## `yomi` コマンドを作る

リポジトリのルートで、次を実行します。

```sh
cargo build -p yomi-cli
```

`target/debug/yomi` ができます。以降は、これを `yomi` と書きます。性能を見るときは `--release` を付けます。

## 最初の台本

台本は、`label` の中に、文を書きます。何も飾らない行（素の行）は、本文です。

```yomi
label start {
    こんにちは、yomi。
}
```

これを `hello.yomi` に保存して、検査と実行をします。

```sh
yomi check hello.yomi
yomi run hello.yomi
```

`yomi check` は、実行せずに、型の誤りや未定義の名前を見つけます。`yomi run` は、停止点（ここでは本文の 1 行ごと）で止まり、Enter で進みます。起動の既定のラベルは `start` です。

## 承認フローを動かす

このガイドの通しの例です。全体を先に見ておきます。わからない書き方は、あとの章で順に説明します。

```yomi
// 購入申請の承認フロー。
// 申請（JSON）が届くと、金額で承認者を決めて、承認を待ち、結果を知らせる。
// 実行: yomi run examples/approval/approval.yomi
import std::event
import std::json

// この金額（円）以下の申請は、承認を待たずに通す
save var auto_limit: int = 10000
// 承認を待つ時間（秒）
save var deadline: float = 86400.0
// 処理中の申請（JSON のまま持つ）
save var request: str = ""
// 処理中の申請の金額（円）
save var amount: int = 0

// 金額から、承認者を決める
fn approver_for(price: int) -> str {
    if price > 500000 {
        return "部長"
    }
    return "課長"
}

label start {
    申請を待っています。 #idle
    let r = @event.wait(["submitted"]) #wait_submit
    match r {
        Arrived(name, data) => {
            request = data
            jump review
        }
        TimedOut => {
            end
        }
    }
}

label review {
    match json.get_int(request, "amount") {
        Some(a) => {
            amount = a
        }
        None => {
            申請の形式が正しくありません。 #invalid
            end
        }
    }
    if amount <= auto_limit {
        jump approved
    }
    jump wait_approval
}

label wait_approval {
    ${approver_for(amount)}の承認を待っています（${amount} 円）。 #waiting
    let r = @event.wait(["approved", "rejected"], Some(deadline)) #wait_decision
    match r {
        Arrived(name, data) => {
            if name == "approved" {
                jump approved
            }
            match json.get_str(data, "reason") {
                Some(reason) => {
                    却下されました。理由：${reason} #rejected
                }
                None => {
                    却下されました。 #rejected_plain
                }
            }
            end
        }
        TimedOut => {
            期限が切れました。申請者に知らせます。 #expired
            end
        }
    }
}

label approved {
    承認されました。購買部へ回します。 #approved
    end
}
```

`yomi run` は、`@event.wait` で止まると、届いたイベントを `名前 データ` の 1 行で受け付けます。申請が届き、課長が承認する流れを、入力をまとめて流して動かします。

```sh
printf 'submitted {"amount":120000}\napproved\n' | yomi run examples/approval/approval.yomi --auto
```

```text
申請を待っています。
[event] submitted
課長の承認を待っています（120000 円）。
[event] approved, rejected (time limit 86400.0 s)
承認されました。購買部へ回します。
(end)
```

金額が 10000 円以下なら、承認を待たずに通ります。`rejected {"reason":"予算超過"}` を送れば、理由つきの却下になります。

## 次へ

[02 台本の基本](02-script-basics.md)で、この台本に出てくる書き方を順に見ます。
