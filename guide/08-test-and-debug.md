# 08 テストとデバッグ

## `yomi test`：分岐を全部たどる

`yomi test` は、台本の分岐（選択肢・入力・待ち）を、すべて試し、どこへ着くかを調べます。

```sh
yomi test examples/approval/approval.yomi
```

```text
States examined: 3
Paths that reach the end: 1
Stop points never reached:
  approval.wait_approval#waiting
  approval.wait_approval#wait_decision
  approval.wait_approval#rejected
  approval.wait_approval#rejected_plain
  approval.wait_approval#expired
  approval.approved#approved
Labels never entered:
  approval.wait_approval
  approval.approved
```

**着かない停止点がある**と報告されています。理由は、`yomi test` が `@event.wait` を、既定では**データが空のイベント**で試すからです。承認フローは、申請の JSON から金額を読むので、空のデータでは `invalid` で終わり、承認待ちの先へ進みません。

イベントのデータは、`--event 名前=データ` で指定します。同じ名前を繰り返すと、その数だけ分岐として試します。

```sh
yomi test examples/approval/approval.yomi \
  --event 'submitted={"amount":500}' \
  --event 'submitted={"amount":120000}' \
  --event 'submitted={"amount":600000}'
```

```text
States examined: 13
Paths that reach the end: 7
Stop points never reached:
  approval.review#invalid
  approval.wait_approval#rejected
```

名前にデータを指定すると、空のデータは試しません（空も試すなら `--event 名前=` と書きます）。残った「着かない停止点」は、指定した金額では起こらない経路（`invalid`）などです。

データを細かく比べたいときは、`yomi run --auto` に、入力を流して、出力を比べます（CI に入れられます）。

```sh
printf 'submitted {"amount":120000}\napproved\n' | yomi run approval.yomi --auto > actual.txt
diff expected.txt actual.txt
```

`yomi test` では、ほかに次のことができます。

- 経路ごとの表示を、`tests/golden/` の golden と比べます。`--update` で書き直します。
- `--require-coverage` で、着かない停止点・選ばれない選択肢があると失敗にできます。
- `--seeds N` で、乱数の種を変えて探索します。
- エラーに至る経路は、再現する答えの列つきで報告され、終了コードは 1 です。

## `yomi debug`：行ごとに止めて見る

```sh
yomi debug approval.yomi
```

開始ラベルの最初の行で止まります。一時停止中は、`step`（`s`）・`next`（`n`）・`finish`・`continue`（`c`）・`break 行`（`b`）・`vars`・`bt`・`list`・`quit` などのコマンドを、そのまま書きます。台本の停止点では、`yomi run` と同じに答えを書き、デバッガのコマンドは `:` を付けます。承認待ちの行に止めて、変数を見る例です。

```text
-> approval:25 (start)
25 |     申請を待っています。 #idle
breakpoint 1 at approval:55
申請を待っています。
[event] submitted
-> approval:55 (wait_approval)
55 |     ${approver_for(amount)}の承認を待っています（${amount} 円）。 #waiting
```

`vars` で `request` と `amount` を、`bt` で呼び出しの並びを見られます。ブレークポイントを置いた行（`break 55`）の手前で止まります。

## `yomi dap`：エディタから

`yomi dap` は、Debug Adapter Protocol のサーバーです。DAP に対応したエディタから、`yomi dap` を起動すれば、ブレークポイント・ステップ・変数の表示を、エディタの画面で使えます。停止点への答えは、デバッグコンソールで入力します。決まりは、[host/06-cli.md](../spec/host/06-cli.md) の `yomi dap` の節にあります。

## `--watch`：書き換えながら動かす

```sh
yomi run approval.yomi --watch
```

台本を保存すると、停止点を保ったまま、新しい台本に差し替えます。コンパイルエラーのときは、診断を出して、いまの実行を続けます。`:reload` で手動でも差し替えられます。

## 次の一歩

- 台本の書き方の決まり：[`spec/language/`](../spec/language/01-lexical.md)
- ホストとの連携の決まり：[`spec/host/`](../spec/host/01-execution.md)
- 標準パッケージ：[`spec/packages/`](../spec/packages/event.md)
