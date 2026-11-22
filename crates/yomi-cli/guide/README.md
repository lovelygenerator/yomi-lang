# yomi ガイド

> 詠めば、動く。

yomi を、Rust のアプリケーションに組み込んで使うための入門です。台本（yomi のスクリプト）の書き方から、Rust 側のホストの書き方、セーブと再開、外部イベント待ち、安全な実行、テストとデバッグまでを、1 つの例で順にたどります。

通しの例は、**購入申請の承認フロー**です。申請（JSON）を受け取り、金額で承認者を決め、承認を待ち（休眠して、セーブに残し）、結果を知らせます。

| 章 | 内容 |
| --- | --- |
| [01 はじめに](01-getting-started.md) | `yomi` コマンドを作り、承認フローを動かす |
| [02 台本の基本](02-script-basics.md) | ラベル・変数・型・`match`・`fn`・`call` |
| [03 Rust に組み込む](03-embedding.md) | 登録表・コンパイル・VM・停止点 |
| [04 保存して再開する](04-save-and-resume.md) | セーブ・ロード・安定 ID・台本の書き換え |
| [05 外部イベントを待つ](05-waiting-for-events.md) | `event.wait`・期限・休眠中のセーブから待ち条件を読む |
| [06 JSON を扱う](06-json.md) | `json` パッケージ |
| [07 安全に動かす](07-running-safely.md) | 実行の上限・中断・検査してから動かす |
| [08 テストとデバッグ](08-test-and-debug.md) | `yomi test`・`yomi debug`・`yomi dap`・`--watch` |

## 例のファイル

- 台本：[`examples/approval/approval.yomi`](../examples/approval/approval.yomi)
- ホスト（Rust）：[`crates/yomi-std/examples/approval.rs`](../crates/yomi-std/examples/approval.rs)（`cargo run -p yomi-std --example approval`）

本文のコードは、台本は `yomi check` に通るもの、Rust はこのホストの抜き出しです。テストが、実装とずれていないことを確かめています。

## 前提

Rust を書ける人を想定します。yomi は 0.x で、文法・API・セーブの形式が変わることがあります。決まりの全体は、仕様書（[`spec/`](../spec/language/01-lexical.md)）にあります。このガイドは、仕様書を読む前に、全体の流れをつかむためのものです。
