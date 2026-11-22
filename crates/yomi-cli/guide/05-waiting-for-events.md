# 05 外部イベントを待つ

承認や通知の到着のように、いつ来るかわからない外部のイベントを待つ手続きは、数時間から数日、休眠することがあります。yomi では、これを標準パッケージ `event` の停止型コマンド `@event.wait` で書きます。

## 台本から

```yomi
import std::event

label start {
    承認を待っています。 #waiting
    let r = @event.wait(["approved", "rejected"], Some(86400.0)) #wait_decision
    match r {
        Arrived(name, data) => {
            ${name} が届きました。 #arrived
        }
        TimedOut => {
            期限が切れました。 #expired
        }
    }
    end
}
```

- 1 つ目の引数は、待つイベントの名前の一覧です。どれかが届くまで止まります。
- 2 つ目の引数は、期限（秒）で、`Some(秒)` か `None`（期限なし）です。省くと `None` です。
- 戻り値は `event.Outcome` で、`Arrived(名前, データ)`（届いたイベントの名前と、データの文字列）か `TimedOut` です。`match` で分けます。
- データの形は、ホストが決めます。コアとパッケージは解釈しません。JSON の文字列にして渡すと、台本で `json` パッケージで取り出せます（[06 JSON を扱う](06-json.md)）。

## ホストから

`@event.wait` で止まると、`Yield::Stop` が返ります。待っているあいだは、ふつうの停止点なので、保存できます。ホストは、保存して休眠させ、イベントが届いたら、ロードして値を返します。

```rust
"event.wait" => {
    // The script is waiting. Save it and read the wait condition from the save, as a host does
    // for a script that sleeps for a long time.
    let bytes = vm.save()?;
    let wait = pending_stop(&bytes)?.expect("the save is at a stop point");
    waits.push(format!("{} {:?}", wait.command, wait.args));
    // ... time passes, the process may exit. Later, the event arrives and the host resumes from the save.
    let Some((name, data)) = events.next() else {
        break;
    };
    let (loaded, _report) = Vm::load(&program, &reg, config.clone(), &bytes)?;
    vm = loaded;
    let outcome = Outcome::Arrived(name.to_string(), data.to_string());
    vm.resume(Input::Return(outcome.into_value()))?;
}
```

`pending_stop(&bytes)` は、VM を作らずに、セーブから、待っている停止点（ラベル・安定 ID・コマンド・引数）を読みます。多くの休眠中のセーブから、「どのイベントを待っているか」を引いて、届いたイベントの宛先を決めるのに使います。承認フローのホストは、次のように、待ち条件を出力します。

```text
[wait] event.wait [List([Str("submitted")]), Option(None)]
[wait] event.wait [List([Str("approved"), Str("rejected")]), Option(Some(Float(86400.0)))]
```

## 期限はホストが測る

yomi のコアは、時計を持ちません。期限は、ホストが測り、切れたら `Outcome::TimedOut` を返します。実時間でも、テストのための仮想時間でも構いません。待ち始めた時刻は、セーブに入りません。ロードしたあとで、期限を数え直すか、ホストが別に覚えた時刻から測るかは、ホストが決めます。

ホストが返す値は、検査されます。`names` にない名前の `Arrived` や、期限のない待ちへの `TimedOut` は、`resume` が実行時エラーにします（`event.unknown_event`・`event.unexpected_timeout`）。

## `yomi run` で

`yomi run` は、`名前 データ` の 1 行で、イベントを届けます。データは省けます。期限のある待ちでは、空行が `TimedOut` です。

```text
approved {"approver":"tanaka"}
```

詳しくは、[packages/event.md](../spec/packages/event.md) にあります。

## 次へ

イベントのデータは、たいてい JSON です。[06 JSON を扱う](06-json.md)へ。
