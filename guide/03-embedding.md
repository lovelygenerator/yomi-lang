# 03 Rust に組み込む

台本は、ホスト（Rust のアプリケーション）が動かします。台本は、ホストが登録したコマンドで止まり、ホストが値を返すと続きを実行します。このガイドのホストは、[`crates/yomi-std/examples/approval.rs`](../crates/yomi-std/examples/approval.rs) にあります。

```text
cargo run -p yomi-std --example approval
```

## 依存

```toml
[dependencies]
yomi-core = "0.1"
yomi-std = "0.1"
```

`yomi-core` が、コンパイラと VM とセーブです。`yomi-std` は、標準パッケージ（`event`・`json`・`text` など）です。公開前は、リポジトリのパスを指定します。

## 登録表

台本が使えるコマンドは、`Registry` に登録したものだけです。承認フローのホストは、本文を表示する 1 つのコマンドを自分で作り、標準パッケージをまとめて登録します。そして、素の行（`申請を待っています。` のような、飾りのない行）を、そのコマンドに束縛します。

```rust
let mut reg = Registry::new();
reg.stop::<(String,), ()>("ui.say")?;
yomi_std::register_all(&mut reg)?;
reg.bind(Sugar::Plain, "ui.say")?;
```

`reg.stop::<(String,), ()>` は、「`String` を 1 つ受け取り、何も返さない、停止型のコマンド」の登録です。型は、Rust の型から決まります。束縛がないと、台本に素の行を書いたときにコンパイルエラーになります。

## コンパイルと実行

```rust
let program = compile(source, "approval", &reg).map_err(|d| format!("{d:?}"))?;
let config = VmConfig::default().max_steps_per_run(Some(100_000));
let mut vm = Vm::new(&program, &reg, config.clone())?;
```

`compile` は、台本を型検査して `Program` にします。誤りは、診断の一覧で返ります。`Vm` は、`Program` と `Registry` を借りて動きます。`VmConfig` は、実行の上限などの設定です（[07 安全に動かす](07-running-safely.md)）。

`vm.run()` は、次の停止点か、台本の終わりまで進みます。

```rust
match vm.run()? {
    Yield::End => break,
    Yield::Stop { command, args, .. } => match reg.command(command).name.as_str() {
        "ui.say" => {
            if let Some(Value::Str(text)) = args.first() {
                shown.push(text.to_string());
            }
            vm.resume(Input::Return(Value::Unit))?;
        }
```

`Yield::Stop` は、止まったコマンド（`command`）と、評価済みの引数（`args`）です。ホストは、コマンドの名前で分け、処理をして、`vm.resume(Input::Return(値))` で戻り値を返します。値の型は、登録したコマンドの戻り値と合っていなければなりません。

## 次へ

停止点では、状態をバイト列に保存できます。[04 保存して再開する](04-save-and-resume.md)へ。
