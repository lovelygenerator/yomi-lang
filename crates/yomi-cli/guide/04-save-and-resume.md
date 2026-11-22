# 04 保存して再開する

yomi の特徴は、台本を**止まった点で保存し、あとで再開できる**ことです。保存できるのは、停止点（`vm.run()` が `Yield::Stop` を返した状態）だけです。

## セーブとロード

```rust
let bytes = vm.save()?;
```

`save` が返すのは、チェックサムつきの 1 つのバイト列です。位置・呼び出しの状態（コールスタック）・変数・乱数の状態・停止点の履歴・パッケージごとの状態が入ります。`persistent var` は、全セーブ共通の別のデータで、`save_persistent` で別に取り出します。

再開するときは、同じ台本と登録表から `Vm::load` で VM を作ります。別のプロセスでも構いません。

```rust
let (loaded, _report) = Vm::load(&program, &reg, config.clone(), &bytes)?;
vm = loaded;
```

ロードした VM は、保存したときと同じ停止点で止まっています。ホストは、その停止点が求める値を `resume` で返して、続きを実行します。

## `yomi run` で試す

承認フローを、承認待ちまで進めて保存し、別のプロセスで読み込んで再開します（`:save`・`:load` は、`yomi run` の対話コマンドです）。

```sh
printf '\nsubmitted {"amount":120000}\n\n:save wait.save\n:quit\n' | yomi run approval.yomi
```

```text
申請を待っています。
[event] submitted
課長の承認を待っています（120000 円）。
[event] approved, rejected (time limit 86400.0 s)
saved to wait.save
```

```sh
printf ':load wait.save\napproved\n\n' | yomi run approval.yomi
```

```text
申請を待っています。
loaded from wait.save
[event] approved, rejected (time limit 86400.0 s)
承認されました。購買部へ回します。
(end)
```

2 つ目のプロセスは、申請を受け取る前の状態から始まりますが、`:load` で、承認を待っている点へ移り、`approved` で再開しています。申請の金額や `request` は、セーブに入っていたものです。

## 台本を書き換えたあとでも再開できる

休眠しているセーブがあるあいだに、台本を直したくなります。yomi は、停止点の**安定 ID**（行末の `#id`）で、セーブの位置を探し当てます。ID が見つからないときは、ラベルと内容のハッシュ、近い位置、ラベルの先頭の順に、手がかりを変えて探します。どれでも合わなければ、セーブは無効になり、ロードは失敗します。

- ID のない停止点には、`yomi assign-ids 台本.yomi` が、自動の ID を書き足します。`--check` なら、書かずに、足りない行を報告します（CI に向きます）。
- 変数を足したときは、既定値で補います。型を変えたときは、移行（`migrations` ファイル）を書きます。決まりは、[host/04-save.md](../spec/host/04-save.md) と [language/07-migrate.md](../spec/language/07-migrate.md) にあります。
- 型の変わった変数があるとき、`LoadPolicy::Strict`（既定）はロードを失敗させ、`Lenient` は初期値に戻して続けます（`yomi run --lenient`）。

## 動いている VM の台本を差し替える

開発中は、台本を保存したら、いまの停止点のまま反映してほしいものです。`vm.reload(&新しい Program)` は、停止点・変数・コールスタック・履歴を、新しい台本へ引き継いだ VM を返します。位置が見つからなければ `Err` で、元の VM は変わりません。`yomi run --watch` は、これをファイルの変更で自動で行い、`:reload` は手動で行います。

## ロールバック

`vm.snapshot()` と `vm.restore()` は、メモリ上の複製で、1 つ前の停止点へ戻せます。バイト列を作らないので軽く、「戻る」操作に向きます。

## 次へ

休眠中のセーブは、外部のイベントで再開されます。[05 外部イベントを待つ](05-waiting-for-events.md)へ。
