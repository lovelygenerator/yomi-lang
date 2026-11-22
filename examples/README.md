# サンプル

yomi の台本で書いた、通しのサンプルと、負荷用の台本です。`yomi` コマンドは、リポジトリのルートで `cargo build`（性能を見るときは `cargo build --release`）して作ります（`target/debug/yomi`・`target/release/yomi`）。

## approval

購入申請の承認フロー（約 80 行・1 モジュール）です。申請（JSON）を受け取り、金額で承認者を決め、承認を待ち（`event.wait`）、結果を知らせます。`json` パッケージで、イベントのデータを読みます。[ガイド](../guide/README.md)の通しの例です。

| ファイル | 内容 |
| --- | --- |
| `approval/approval.yomi` | 台本 |
| `../crates/yomi-std/examples/approval.rs` | ホスト（Rust）。承認待ちで保存し、イベントが届いたらロードして再開する |

```
printf 'submitted {"amount":120000}\napproved\n' | yomi run examples/approval/approval.yomi --auto
cargo run -p yomi-std --example approval
```

## ticket

問い合わせ対応の流れ（約 80 行・1 モジュール）です。問い合わせ（JSON）を受け取り、件名から緊急度を決めて受け付け、お客様の返信を待ちます（`event.wait`）。期限が切れたら、決まった回数まで催促し、それでも返信がなければ自動で閉じます。承認フローとの違いは、期限切れ（`TimedOut`）を繰り返しの一部として使うことです。

| ファイル | 内容 |
| --- | --- |
| `ticket/ticket.yomi` | 台本 |
| `../crates/yomi-std/tests/ticket_example.rs` | テスト。小さなホストが、待つたびにイベントか期限切れを返して、台本の動きを確かめる |

```
printf 'opened {"subject":"ログイン障害"}\n\n\n\n\n' | yomi run examples/ticket/ticket.yomi --auto
yomi test examples/ticket/ticket.yomi --event 'opened={"subject":"障害"}' --event 'opened={"subject":"質問"}' --event 'opened={}' --event replied= --event resolved=
```

## mini-game

見習い冒険者が、相棒と村で準備をして、森の祠へ向かう小さなゲーム（約 400 行・5 モジュール）です。会話・選択肢・買い物・ミニゲーム・クエストの結末を、台本で書いています。エンディングは 4 つあります。

| ファイル | 内容 |
| --- | --- |
| `main.yomi` | 開始（`start`）、名前の入力、村の広場（選択の繰り返し）、森の祠、結末の決定 |
| `chara.yomi` | 登場人物（identity）、相棒の信頼度と調子（`enum`）、練習場の的（`struct`）、ダメージの計算（`fn`） |
| `shop.yomi` | 所持金と持ち物（`map`）、道具屋の買い物（引数と戻り値のある `call`） |
| `game.yomi` | 弓の練習場（`random`・`while`・`Option`・`list`） |
| `ending.yomi` | 4 つのエンディング、クリアの記録（`persistent var`） |
| `en.texts` | 英語の差し替え表 |
| `playthroughs/` | 通しのプレイ（入力と期待する出力） |

### 遊ぶ

```
yomi run examples/mini-game/main.yomi --seed 7
```

停止点（本文の 1 行ごと）で止まります。Enter で次へ進み、選択肢は番号を入力します。`:save <ファイル>`・`:load <ファイル>`・`:back`・`:vars`・`:texts <表|off>`・`:repl`（REPL に入る。下の「試し打ち」）・`:quit` が使えます。`--seed` を省くと、起動した時刻から乱数の種を作ります。

エンディングの記録（`persistent var`）と通過済みの停止点は、`--persistent` で指定したファイルに残ります。続けて遊ぶと、2 周目の始めの文が変わり、条件が合えば 4 つ目のエンディングが見られます。

```
yomi run examples/mini-game/main.yomi --seed 7 --persistent /tmp/mini-game.bin
```

英語の差し替え表で遊ぶには、`--texts` を付けます。

```
yomi run examples/mini-game/main.yomi --seed 7 --texts examples/mini-game/en.texts
```

`en.texts` は、本文・選択肢の文に加えて、次の 2 つも訳します。

- **identity の表示名**（話者名）：鍵は `identity:モジュール名.名前`（`identity:chara.相棒`）。引用行の話者名が英語になります。
- **入力の促し**（`@input.text(...)`・`@input.integer(...)`・`@input.confirm(...)` の `prompt`）：台本が文字列リテラルで書き、行に ID を付けているとき（`#wlh37x` など）、その行の鍵（`main.register#wlh37x`）で訳せます。`default` は訳しません。

訳せないもの：式の中の `相棒.name`、変数に入れた `Identity`、`fn`・データ中の文字列リテラル（`struct` のフィールドの景品名・持ち物の名前など）、ID のない行・変数で渡した促し。これらは日本語のまま出ます。

### 試し打ち（REPL）

`yomi repl` は、台本の変数・`fn`・型・ラベルの上で、式の評価や文の実行を試せる対話の環境です。試した結果は、既定では台本に影響しません（規則は `spec/host/07-repl.md`）。

```
$ yomi repl examples/mini-game/main.yomi --seed 7
yomi> turns
5
yomi> chara.mood_of(7)
Mood.Confident
yomi> let ok = call shop.buy("携帯食")
[audio] se coin
道具屋の主人「毎度あり！　携帯食、250ゴールドね」

ok = true
yomi> shop.money
950
yomi> :vars
```

- 式は値を、`let` は `名前 = 値` を表示します。`{ }` のブロックや複数行の引用行は、閉じるまで `...> ` で続けて入力できます（空行も入力の一部。途中で捨てるには `:cancel`）。
- 素の行・引用行・`choice`・`call ラベル(引数)` は停止点になり、`yomi run` と同じ表示・同じ入力（Enter、選択は番号、`input.*` は値）で進みます。
- 診断とエラーは標準エラーに `<repl>:行:桁: error: …` の形で出ます。実行時エラーになった入力は取り消され、REPL は続きます。
- コマンド：`:help`・`:vars`・`:defs`・`:type <式>`・`:reset`・`:exit`・`:quit`・`:cancel`・`:abort`（停止点の途中）。
- 台本なし（`yomi repl`）でも、`import std::math` のように標準パッケージを試せます。`--persistent` は読むだけです。

`yomi run` の停止中に `:repl` と打つと、その停止点の環境の複製の上で REPL に入ります。`:exit` で同じ停止点へ戻り、`:apply` で変えた変数（`save var` と停止点のローカル変数。`persistent var` は対象外）を台本へ反映できます。

```
$ yomi run examples/mini-game/main.yomi --seed 7
…
:repl
(entering the REPL on a copy of this stop point: :exit returns here, :help lists the commands)
yomi> turns = 1
yomi> :apply
main.turns = 1
yomi> :exit
```

### 自動プレイ（`--auto`）

`--auto` を付けると、本文（素の行・引用行）と `flow.wait`・`flow.wait_input` の停止点では、入力を読まずに進みます。標準入力から 1 行読むのは、**選択**と `input.*`（名前の入力・数の入力・y/n）の停止点だけです。台本の回帰テストの入力を、本文の数を数えずに書けます。

選択肢の行には、安定 ID が出ます（`--auto` のときは常に。`--auto` なしで出すには `--show-ids`）。選択は、番号のほかに、**選択肢の安定 ID**（`#` つき）で答えられます。`if`・`once` で選択肢が増えたり消えたりしても、答えがずれません。

```
$ yomi run examples/mini-game/main.yomi --seed 7 --auto
…
1: 「道具屋で買い物をする」 #ewtm2h
2: 「弓の練習場に行く」 #archery
3: 「村の長老に挨拶する」 #elder
…
```

```
yomi run examples/mini-game/main.yomi --seed 7 --auto < examples/mini-game/playthroughs/end_good.auto
```

- 答えの行は、番号（`2`）でも ID（`#archery`）でもよく、混ぜられます。`input.*` の停止点の答えは、これまでどおりの 1 行です（入力が尽きたら、既定値）。
- 一覧にない ID・選択の答えにならない入力（範囲外の番号・数でない文字列・空行）・選択の前に入力が尽きたときは、続けずに、標準エラーにメッセージを出して、終了コード 1 で終わります（先の答えがずれて、気づかないまま進むのを防ぎます）。`--auto` なしでは、これまでどおり、番号を求め直します。
- `:save <ファイル>`・`:load <ファイル>`・`:back`・`:texts <表|off>`・`:vars`・`:repl`・`:quit` は、入力を読む停止点（選択と `input.*`）で使えます。

`playthroughs/` の `.auto` は、`--auto` の標準入力です（`.answers` と違い、本文のための空行は要りません）。`# `（`#` と空白）で始まる行はコメントで、テストが除いてから渡します（`#ID` の行は、`#` のすぐ後ろが空白でないので、コメントではありません）。

| 名前 | 内容 |
| --- | --- |
| `end_good.auto` | `end_good` と同じ選び方を、選択はすべて安定 ID で書いたもの。出力（`.auto.out`）から選択肢の ID を除くと、`end_good.out` と同じになります。英語の表（`--texts`）でも、同じ ID で答えられます（`.auto.en.out`） |
| `end_normal.auto` | `end_normal` と同じ選び方を、番号と ID を混ぜて書き、途中で `:vars` を使ったもの（`.auto.out`） |

### 検査・ツール

```
yomi check examples/mini-game/main.yomi                       # 警告も診断も出ない
yomi assign-ids --check examples/mini-game                    # 安定 ID の付け忘れがない
yomi assign-ids examples/mini-game --seed 20261118            # ID のない行に ID を付ける
yomi texts check examples/mini-game/main.yomi examples/mini-game/en.texts
yomi texts template examples/mini-game/main.yomi --text-language en   # 差し替え表のひな形
```

`once` を付けた選択肢にも、`assign-ids` が ID を付けます（`once` には ID が要りますが、ID を書き忘れたコンパイルエラーだけなら、直せます）。このサンプルの `once` の ID（`#archery` など）は、`en.texts` の鍵になるので、わかりやすい名前を手で書いています。入力の促しの行（`@input.*`）にも、`assign-ids` が ID を付けます（`require_text_ids` を有効にしている場合は、ID がないとコンパイルエラーになります）。

### 通しのプレイ

`playthroughs/` に、エンディングごとのプレイを置いています。

| 名前 | エンディング | 内容 |
| --- | --- | --- |
| `end_good` | 封印の夜 | 名前を打ち直し、回復薬・携帯食・護符を買い、長老に挨拶して、弓の練習をして、約束する |
| `end_normal` | 帰り道 | 護符を買わず、池で釣りをして、お金が足りない買い物もする |
| `end_lost` | 霧の森で | すぐ森へ行って、約束しない |
| `end_promise` | 次の依頼も | `end_good` を見たあと、同じ選び方をする（同じ永続データのファイルで続けて遊ぶ） |

| 拡張子 | 内容 |
| --- | --- |
| `.answers` | 答えだけを書いたもの（選択の番号・入力した文字列・y/n。`#` で始まる行はコメント。空行は Enter だけの答え） |
| `.in` | `yomi run` の標準入力そのもの。`.answers` に、本文の停止点で進める空行を補ったもの |
| `.auto` | `--auto` の標準入力（選択の番号・`#ID`・`input.*` の答え・対話コマンド）。下の「自動プレイ」を参照 |
| `.out` | 期待する出力（日本語） |
| `.en.out` | 英語の差し替え表を使ったときの、期待する出力 |
| `.auto.out`・`.auto.en.out` | `.auto` で自動プレイしたときの、期待する出力（選択肢の行に ID が出る） |

`.in` は、そのまま標準入力に渡せます（種は 7）。

```
yomi run examples/mini-game/main.yomi --seed 7 < examples/mini-game/playthroughs/end_good.in
```

`yomi run` は、停止点ごとに標準入力を 1 行読むので、選択や入力の答えの前に、本文の数だけ空行が要ります。そのため、答えだけを書く `.answers` から、`.in` を作ります（テストが作ります）。

### テストと、プレイスルーの更新

```
cargo test -p yomi-cli --test mini_game
```

台本・`.answers`・`en.texts` を変えたら、次で、`.in`・`.out`・`.en.out` を書き直します。**書き直したあと、差分を目で確かめてから**コミットしてください。

```
BLESS=1 cargo test -p yomi-cli --test mini_game
git diff examples/mini-game/playthroughs
```

## bench

性能の目安を見るための、負荷用の台本です。1 本ごとにディレクトリを分けています（`yomi run` は、起動ファイルと同じディレクトリの `.yomi` をすべてモジュールとして読むため）。

| ディレクトリ | 内容 | 出力 |
| --- | --- | --- |
| `loop_int` | 10 万回のループで整数の計算 | `結果: 45580` |
| `list_update` | 要素数 1 万の `list` への追加・全要素の更新・合計 | `要素数: 10000、合計: 100000000` |
| `map_update` | 要素数 1 万の `map` への追加・更新・削除・合計 | `要素数: 9000、合計: 135009000` |
| `list_scale` | 要素数 4 万の `list` への追加・添字読み・`len`・全要素の更新・合計（要素数にほぼ線形） | `要素数: 40000、読み: 2399980000、合計: 1600000000` |
| `map_scale` | 要素数 4 万の `map` への挿入・更新・読み・合計（要素数にほぼ線形） | `要素数: 40000、合計: 800020000` |
| `text_join_split` | 文字列 1 万個の `join`・`split`・`replace`・`pad_left` | `結果: 10000 個、全体 69999 文字、置換後 69999 文字` |
| `recursion` | 再帰の `fn`（`fib(24)`、深さ 200 の足し算を 500 回） | `fib(24) = 46368、…` |
| `stops` | 停止点（素の行）を 2 万回通る（停止点ごとの履歴・ロールバックの記録の重さ） | `行 0` 〜 `行 19999`、`合計: 199990000` |
| `struct_field` | 10 万文字の `str` を持つ `struct` の、整数のフィールドを 5 万回読む | `結果: 150000、本文 100000 文字` |
| `str_copy` | 10 万文字の `str` を、ローカル変数へ 2 万回代入する | `結果: 20000、100000 文字` |
| `struct_copy` | 10 万文字の `str` を持つ `struct` を、`fn` の引数に 2 万回渡す | `結果: 80000` |

```
cargo build --release
time ./target/release/yomi run examples/bench/loop_int/main.yomi < /dev/null
```

全部をまとめて、何回か測った最小と中央値を出すには、`tools/bench.sh` を使います（release でビルドしてから測る）。変更の前後を比べるときは、前のバイナリを `YOMI` で指定します。

```
tools/bench.sh                       # 全部を 5 回ずつ
tools/bench.sh -n 10 stops           # 回数と台本を指定する
YOMI=/tmp/yomi-before tools/bench.sh  # 別の yomi で測る
```

テストで、11 本をまとめて動かし、結果が決まった値になることと、かかった時間（標準エラー）を見られます。時間は、合否に使いません。

```
cargo test --release -p yomi-cli --test mini_game -- --nocapture 負荷用
```

時間は、マシンと負荷によって変わる、暫定の目安です。
