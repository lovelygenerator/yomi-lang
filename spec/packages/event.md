# パッケージ `event`（yomi-std）

外部のイベント（承認・メッセージ・通知の到着など）を待つ。停止型コマンドで、イベントを受けて値を返すのは、ホストである（host/01-execution.md の 1.10）。

状態スライス・永続スライスは、持たない（`State = ()`、`Persistent = ()`）。

## 型

パッケージが登録する enum（host/02-registry.md の 2.5）。

```
enum event.Outcome {
    Arrived(str, str),   // 届いたイベントの名前と、データ
    TimedOut,            // 期限が切れた
}
```

- 台本では、`import std::event` のあと、コマンドと同じく型も `event.Outcome` で使える。`match` のパターンは、バリアントの名前（`Arrived(name, data)`・`TimedOut`）で書く（language/03-statements.md の 3.6）。
- データの形（JSON の文字列など）は、ホストが決める。コアとパッケージは、解釈しない。

## コマンド

| コマンド | 引数 | 戻り値 | 意味 |
| --- | --- | --- | --- |
| `event.wait` | `names: list<str>`, `timeout: Option<float> = None` | `event.Outcome` | `names` のどれかのイベントが届くまで止まる。`timeout` が `Some(秒)` なら、その秒数で期限が切れる。`None` は、期限なし |

- `names` は 1 つ以上で、どれも空でない文字列で、重ならない。`timeout` は 0 以上（`Some(0.0)` は、ホストがすぐに期限切れを返してよい）。違えば、止まる前に実行時エラー（`event.no_names`・`event.empty_name`・`event.duplicate_name`・`event.negative_timeout`。登録の検証。host/02-registry.md の 2.4 の `stop_checked`）。
- ホストが返す値は、次を満たさなければならない。満たさないとき、`resume` が実行時エラー（`event.unknown_event`・`event.unexpected_timeout`。登録の戻り値の検査。host/02-registry.md の 2.4 の `stop_validated`）。
  - `Arrived` の名前は、`names` のどれか。
  - `TimedOut` は、`timeout` が `Some` のときだけ。
- 期限は、ホストが測る。実時間でなく、仮想時間で測ってもよい（host/01-execution.md の 1.10）。期限の起点（待ち始めた時刻）は、セーブに入らない。ロードのあとの期限の扱い（数え直す・待ち始めの時刻をホストが別に持つ）は、ホストが決める。
- 待ち中は通常の停止点なので、セーブできる。ロードした VM は、同じ待ち（同じコマンドと引数）で止まっていて、`vm.current()` で読める。イベントが届いていれば、そのまま `resume` できる（再発行の方式。host/01-execution.md の 1.10）。待っているセーブの、コマンドと引数（イベントの名前・期限）は、ロードせずに `pending_stop` で読める（host/04-save.md の 4.9）。
- 台本では、戻り値を `let` で受けて、`match` で分ける。安定 ID は、行末に付ける。

## 使い方

```
import std::event

identity 依頼人 = "依頼人"

label start {
    依頼人「承認が届いたら、出発しよう。」 #irai
    let r = @event.wait(["approved", "rejected"], Some(3600.0)) #wait_approval
    match r {
        Arrived(name, data) => {
            if name == "approved" {
                jump 出発
            }
            依頼人「却下された。理由: ${data}」 #rejected
            end
        }
        TimedOut => jump 期限切れ
    }
}

label 出発 {
    依頼人「出発する。」 #go
    end
}

label 期限切れ {
    依頼人「返事がなかった。」 #late
    end
}
```

- `@event.wait(["approved"])` のように、`timeout` を省くと、期限なしで待つ。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `event.duplicate_name` | `name` | `names` に、同じ名前が 2 つ以上ある |
| `event.empty_name` | `index` | `names` の `index` 番目（0 から）が、空の文字列 |
| `event.negative_timeout` | `timeout` | `timeout` が負 |
| `event.no_names` | なし | `names` が空 |
| `event.unexpected_timeout` | なし | `timeout` が `None` なのに、ホストが `TimedOut` を返した |
| `event.unknown_event` | `name` | ホストが返した `Arrived` の名前が、`names` にない |
