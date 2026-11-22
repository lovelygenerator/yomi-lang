# 4. セーブ

パッケージの状態スライス・永続スライスと、ホストが宣言した変数も、この章に含める（スライスの約束は host/03）。履歴の今の言語での引き直し（`render`・`history_text`）は、host/05-texts.md で定める。

この章は、セーブのバイト列の形と、保存・復元・ロールバックの約束を定める。方式は、スナップショット、安定 ID、位置の解決の順とする。

## 4.1 コアが行うこと・行わないこと

- コアは、**バイト列を作り、バイト列から復元する**。ファイルの読み書き、原子的な書き込み、オートセーブの時機、保存先の管理は、ホストが行う。
- セーブは、停止点で止まっているときだけ作れる（host/01-execution.md の 1.8）。
- セーブは、**2 種類**のバイト列に分ける。

| 種類 | 内容 | 使い方 |
| --- | --- | --- |
| セーブ | 1 つの停止点の状態（4.2） | 保存・ロードのたび |
| 永続データ | 通過済みの停止点、`persistent var`、パッケージの永続スライス（4.5） | 全セーブで共有。別の場所に保存する |

```rust
let bytes: Vec<u8> = vm.save()?;                                   // 停止中だけ呼べる
let (vm, report) = Vm::load(&program, &registry, cfg, &bytes)?;    // cfg: VmConfig
let persistent: Vec<u8> = vm.save_persistent();                    // いつでも呼べる
let report = vm.load_persistent(&persistent)?;
let snapshot = vm.snapshot()?;                                     // 停止中だけ。メモリ上の複製
vm.restore(&snapshot)?;                                            // ロールバック（4.7）
```

## 4.2 セーブの構成

セーブは、次の順に並ぶ。

| 欄 | 内容 |
| --- | --- |
| マジック | 固定の 4 バイト `YOMS` |
| 形式の版数 | `u32`（リトルエンディアン）。形式を変えたら上げる。いまは 12 |
| 本体 | `postcard` で直列化したデータ（4.3） |
| チェックサム | 先頭から本体の終わりまでの CRC-32（`u32`、リトルエンディアン） |

- マジック・形式の版数・チェックサムは、固定幅で、`to_le_bytes` / `from_le_bytes` で読み書きする。
- CRC-32 は、IEEE 802.3 の多項式（反転表現 `0xEDB88320`）で、初期値 `0xFFFFFFFF`、最後にビットを反転する。
- ロードは、マジック、形式の版数、チェックサムを、本体を読む前に確かめる。合わなければ、`LoadError`（4.8）。
- 知らない形式の版数は、`LoadError`（`UnsupportedVersion`）。読めるのは、いまの版数（12）だけで、古い版数（1〜11）のセーブは読めない。
- 形式の版数（本体の並びの版）と、スキーマの版（4.3。変数の宣言の版）は、別のものである。
- 永続データ（4.5）も、同じ並び（マジックは `YOMP`、版数は 5）。

## 4.3 本体

```rust
struct SaveBody {
    position: Position,                     // 4.4
    schema_version: u32,                    // 台本のスキーマの版（「スキーマ」）
    types: Vec<(Name, String)>,             // 宣言した `struct`・`enum`（`モジュール.型名` と、宣言の形を表す文字列）
    var_types: Vec<(Name, String)>,         // `vars`・`host_vars` の各変数の型（「スキーマ」）
    vars: Vec<(Name, SavedValue)>,          // モジュールの変数（`save var`）
    host_vars: Vec<(Name, SavedValue)>,     // ホストが宣言した `Kind::Save` の変数
    slices: Vec<SavedSlice>,                // パッケージの状態スライス
    locals: Vec<(u32, Name, SavedValue)>,   // いまのラベルのローカル変数（スロット、名前、値）
    frames: Vec<FrameBody>,                 // コールスタック
    visited: Vec<(Name, Name)>,             // 入ったラベル（モジュール、ラベル）
    once_done: Vec<(Name, Name, Name)>,     // 選ばれた once の選択肢（モジュール、ラベル、安定 ID）
    rng: [u64; 4],                          // 乱数の状態
    history: Vec<StopRecord>,               // 停止点の履歴
    rules: Vec<(Name, u32)>,                // パッケージの規則の版（パッケージ名、`Package::RULES_VERSION`）
}
```

- `position`：保存した停止点（4.4）。
- `types`：セーブしたときの、`struct`・`enum` の宣言（台本で宣言した型と、登録表に登録した型。host/02-registry.md の 2.5）。型の名前と、宣言の形（フィールドの名前と型の並び・バリアントの名前と引数の型の並び）を表す文字列。ロードで、宣言が変わっていないか調べるのに使う（4.6）。書き方は、「スキーマ」のとおり。
- `schema_version`・`var_types`：保存したときの、変数のスキーマの版と、変数の型（下の「スキーマ」）。
- `vars`：`save var` の値。名前は、`モジュール名.変数名` と、モジュール名を付ける。**`persistent var` は入れない**（4.5）。値は、`int`・`float`・`bool`・`str` と、複合値（`list`・`map`・`Option`・`struct`・`enum`。入れ子を含む）。複合値は、後置の節の列（先に中身、後ろに親）として保存し、ロードは、再帰せずに（明示のスタックで）読む。入れ子の深さは 64 までで、超える・節の数が合わない値は、壊れているとみなす。移植性の規則に従う（整数は `i64`、`float` は有限の値だけ）。`struct` の値はフィールドを宣言の順に、`enum` の値はバリアントの名前と引数を持つ。`map` の値は、組の数 n のあとに、キーと値を挿入順に 2n 個の節として持つ（読むときに、キーの重なりは、壊れているとみなす）。
- `host_vars`：ホストが宣言した変数（host/02-registry.md の 2.7）のうち、`Kind::Save` のもの。名前は `パッケージ名.変数名`。値は、`vars` と同じ形（`list`・`map`・`Option`・登録した `struct`・`enum` も）。`Kind::Persistent` のものは、永続データに入れる（4.5）。
- `slices`：パッケージの状態スライス（host/03-packages.md の 3.2）。`SavedSlice { name, version, bytes }` の並びで、`name` はパッケージ名、`version` は保存したときの `Package::VERSION`、`bytes` は `State` を `postcard` で直列化したもの。登録の順に並べる。`State` が `()` のパッケージは、入れない。
- `locals`・フレームのローカル変数：関数の型（language/03-statements.md の 3.5）を含む型の変数は、値を入れない（停止点では使い終わっていることを、コンパイラが保証する）。ロードでは既定の値になり、型の変化も調べない。
- `frames`：コールスタックの各フレーム（底が最初の呼び出し元）。フレームは、呼び出し元のラベル（モジュール・ラベル）、呼び出し行の位置（4.4 の `hash`・`ordinal`。`call` の行は安定 ID を持たないので、`id` は持たない）、ローカル変数を持つ。`hash` の入力は、下の「ハッシュの入力」のとおり。
- `visited`・`once_done`：台本の `visited()` と、選択肢の `once`（language/05-sugar.md の 5.5、5.7）のための状態。`visited` は入ったラベルの集合、`once_done` は選ばれた `once` の選択肢の集合である。ロールバックで、元に戻る。ロードでは、停止点の引数を作り直す前に復元する。台本から消えたラベルのものは、捨てる。
- `rng`：乱数（language/03-statements.md の 3.8）の状態（`u64` が 4 つ）。保存するのは、**いまの停止点の引数を求め始めたときの状態**である。ロードは、停止点の引数を作り直す（4.6）。そのとき `${random(…)}` のように、引数の中で乱数を引く台本でも、保存したときと同じ値になり、作り直したあとの乱数の状態が、保存したときと同じになる。0 だけの状態は、壊れているとみなす。
- `history`：通過した停止点の記録（下）。新しい `history_limit` 件まで。ロールバック用のスナップショットは含めない。
- `rules`：登録されていた全パッケージの、規則の版（host/03-packages.md の 3.1 の `RULES_VERSION`）。登録の順。状態を持たないパッケージも入れる。ロードで、今の値と比べる（4.6）。
- 本体の欄の並びは、形式の版数ごとに固定する。
- 形式の版数は 12。版 2 でコールスタックを、版 3 で `visited`・`once_done` を、版 4 で `rng`・`history` を、版 5 で `host_vars`・`slices` を、版 6 で履歴の `ret`（停止点の戻り値）を、版 7 で複合値（`list`・`Option`・`struct`・`enum`）と型の宣言 `types` を、版 8 で `map` の値（`Map` の節）を、版 9 で履歴の `label`・`kind`（停止点のラベルと種類。本文の差し替えの引き直しに使う）を、版 10 で `rules`（パッケージの規則の版）を足し、版 11 で乱数を xoshiro256++（`rng` が `u64` 4 つ）に変え、版 12 で `schema_version`・`var_types` を足して、位置の `hash` の入力を標準書式に変えた（4.4）。`persistent var` と通過済みの停止点と永続スライスは、含まない。

### スキーマ

`schema_version`・`var_types`・`types` を合わせて、保存したときの変数の**スキーマ**と呼ぶ。セーブだけで、保存したときの変数の名前と型が分かる（台本がなくても）。

- **型の書き方**：`int`・`float`・`bool`・`str`、`list<T>`・`map<K, V>`・`Option<T>`。`struct`・`enum` は、`モジュール名.型名`（ホストが登録した型は `パッケージ名.型名`）。`,` のあとに空白を 1 つ置き、ほかの空白は置かない。関数の型は現れない（保存できない。language/03-statements.md の 3.5）。
- **宣言の形**（`types` の文字列）：`struct { 名前: 型, 名前: 型 }`、`enum { バリアント(型, 型), バリアント }`。フィールドとバリアントは、宣言の順。フィールドのない `struct` は `struct {}`。
- `var_types`：`vars` と `host_vars` の変数ごとに 1 つ（名前は同じ。並びも同じ）。ホストが宣言した変数も入れるが、ロードの検査と移行の対象は、台本の変数だけで、ホストが宣言した変数は、診断のために持つ（4.6）。
- ラベルのローカル変数とコールスタックのフレームは、スキーマに含めない（位置の解決で扱う。4.4）。
- **スキーマの版**（`schema_version`）：プロジェクト全体で 1 つの連番（`u32`）。台本の現在の版は、`migrations` モジュールの `migrate N`（language/07-migrate.md）のうち、最大の `N`。`migrate` が 1 つもなければ 1。コアは、版を `Program` から読めるようにする（`Program::schema_version()`）。
- 版は、`migrate` を適用する順序と、サポートの下限（`min_save_version`）を決める。版が同じでも、宣言が変わっていれば、ロードで検出する（4.6）。

### 履歴の記録

```rust
struct StopRecord {
    label: String,               // 停止点を置いたラベル（"モジュール.ラベル"）
    kind: StopKind,              // 停止点の種類（Command・Plain・Quote・Choice。`@` のコマンドか、糖衣か）
    key: Option<String>,         // "モジュール.ラベル#id"（安定 ID のない停止点・選択は None）
    command: String,             // 止まったコマンドの名前（登録表の名前）
    args: Vec<Value>,            // 停止の知らせの引数（差し替え前の、原文の評価結果）
    values: Vec<Vec<String>>,    // 本文ごとの、`${式}` の評価済みの値
    ret: Option<Value>,          // 再開に渡された戻り値（再開前と、戻り値が `()` のときは None）
}
```

- 履歴は、停止点に達するたびに、1 件ずつ記録する（停止型コマンドの `@` の呼び出しも含む）。`vm.history()` が、古い順に返す（`&[Arc<StopRecord>]`。記録は、ロールバックのスナップショットと共有する）。
- 履歴は、**原文と評価済みの値**を持つ。差し替え後の訳文は持たない。言語を切り替えても、履歴を、今の言語で引き直せる（host/05-texts.md の 5.6）。
- `values` は、停止点の引数を求めるときに、評価した順に並ぶ本文ごとの、`${式}` の値（文字列にした値）の並びである。素の行・引用行は、本文が 1 つ。選択は、一覧に含めた選択肢ごとに 1 つ。停止型コマンドの呼び出しは、文字列リテラルの引数ごとに 1 つ（`${式}` がなければ空）。入れ子の文字列リテラルは、数えない。
- `ret` は、ホストが `resume(Input::Return(値))` で渡した値である。台本が受けていない停止点でも、残る。選択では、渡した一覧の番号（`int`）。まだ再開していない停止点（セーブした停止点を含む）と、戻り値が `()` の停止点は、`None`。セーブに入り、ロールバックで戻る（ロードした停止点を再開すると、その記録の `ret` が入る）。
- 件数の上限は、`VmConfig::history_limit`（既定 200）。超えた分は、古いものから捨てる。0 なら、記録しない。
- ロードは、保存した履歴のうち、新しい `history_limit` 件だけを残す。ロードで作り直した最後の停止点の引数が、保存したときと違うとき（`persistent var` や `seen` が変わっていたとき）は、最後の記録の `args` と `values` も、作り直した値にそろえる。
- 履歴の `args`（と `ret`）は、セーブの値と同じ形（複合値を含む。入れ子は 64 段まで）で保存する。
- `vm.render(&StopRecord) -> Vec<Value>`（今の差し替え表で、本文を作り直した引数を返す）と、`vm.history_text(&StopRecord) -> Option<String>` は、host/05-texts.md の 5.6 で定める。履歴は `label` と `kind` を持つので、ロードしたあとの履歴でも、選択を含めて、今の言語で引ける。

## 4.4 位置

```rust
struct Position {
    module: Name,
    label: Name,
    id: Option<String>,     // 安定 ID
    hash: u64,              // 内容のハッシュ（下の「ハッシュの入力」。訳文は含めない）
    ordinal: u32,           // ラベルの中の停止点の序数
}
```

- 位置は、「ラベル + 安定 ID」で持つ。`hash`（FNV-1a の 64 ビット）は、台本に書かれた原文から、書式の違い（空白・冗長な括弧など）を除いて作る（下の「ハッシュの入力」）。差し替え表（host/05-texts.md）の訳文は含めないので、言語を変えても、同じ停止点を探せる。バイトコードのオフセットは保存しない。
- 安定 ID のない停止点は、`hash` と `ordinal` で探す（ハイブリッド）。
- `choice` は 1 つの停止点で、安定 ID を持たない。`hash` の入力は、下の「ハッシュの入力」のとおり。復元では、`if` と `once` を、保存した状態で評価し直して、一覧を作り直す。作り直した一覧が空になるとき（台本が変わった）は、ラベルの先頭へ戻す（`LabelStart`）。

### ハッシュの入力

`hash` は、次の「部分」を順に混ぜて作る。各部分は UTF-8 のバイト列で、FNV-1a（初期値 `0xcbf29ce484222325`、乗数 `0x100000001b3`）に、部分のバイトを順に混ぜ、**各部分のあとに、バイト `0xFF` を 1 つ混ぜる**（部分の境目を区別するため）。

| 停止点の種類 | 部分（順） |
| --- | --- |
| 停止型コマンドの行 | ① コマンドの名前（登録された名前）② 引数の標準書式（language/03-statements.md の 3.9）を `, ` でつないだ文字列（引数がなければ空） |
| 素の行 | ① 束縛されたコマンドの名前 ② 本文 |
| 引用行 | ① 束縛されたコマンドの名前 ② 話者の `id`（identity がなければ空）③ 本文 |
| `choice` | ① 束縛されたコマンドの名前 ② 選択肢の本文を、書かれた順に 1 つずつ（選択肢ごとに 1 部分） |
| `call` | ① `call` ② 書かれたラベルの名前 ③ 引数の標準書式を `, ` でつないだ文字列 |

- **本文**：停止点の種類ごとに、次のとおり。
  - 素の行：行頭のインデント（半角スペース・タブ）、行末の半角スペース・タブ、行末の安定 ID を除いた、行の文字。
  - 引用行・選択肢：`「` と `」` の間の文字。複数行の引用行は、ソース上の改行と、その直後の行頭の半角スペース・タブを除いて、1 つに結合した文字列（language/02-lines.md の 2.4。ホストへ渡る本文と同じ）。折り返しの位置や、続きの行のインデントを変えても、`hash` は変わらない。
  - 本文の文字は、書かれたまま（エスケープの `\` も、全角スペースも）。`=>` 以降は含めない。`${式}` の中の式は、標準書式に直し、`${` と `}` の内側に空白を入れない。それ以外の文字は、正規化しない。
- 識別子は NFC に直す（language/01-lexical.md の 1.5）。文字列リテラルの中身と本文の文字は、書かれたまま。
- コメントは、入力に含めない。
- 同じ台本の、空白だけを変えた書き直し（`@bg( "room" )` と `@bg("room")`、整数の桁区切りなど）では、`hash` は変わらない。文字列リテラルと本文の文字は、書かれたままなので、変えると `hash` が変わる。
- 標準書式の規則を変えると、`hash` の値が変わる。変えるときは、形式の版数（4.2）を上げる。

### 位置の解決

ロードのとき、台本の中で、保存した位置を、次の順に探す。

1. ラベル + 安定 ID が完全一致する停止点。
2. ラベル + `hash` が一致する停止点。
3. ラベル + `ordinal` が、最も近い停止点。
4. ラベルの先頭。
5. すべて不可（ラベルが存在しない）なら、`LoadError`。ホストは、そのセーブを無効にする（タイトルへ戻すなど）。

コールスタックのフレームは、呼び出しの位置を、1 から 3（ID は使わない）の順に解決する。次のときは、コールスタックを捨てて、保存した現在のラベルの先頭へ移る（`LabelStart`）。

- フレームのラベルが台本にない。
- フレームのローカル変数が、名前・型・個数で合わない。
- 呼び出しの位置が見つからない。
- 呼び出しの位置の呼び出し先が、1 つ上のフレーム（なければ現在のラベル）と合わない。1 つ上のラベルが `call` でだけ呼べるラベルなら、呼び出し先と同じでなければならない。そうでなければ、呼び出し先の戻り値が、なくてはならない。

- 解決のしかた（どの番号で見つかったか）は、`LoadReport` で返す（4.6）。
- 2 と 3 で、一致する停止点が複数あるときは、保存した `ordinal` に近い停止点を選ぶ。距離が同じなら、添字の小さいほう。
- 保存したモジュール名が、台本のどのモジュールにもないときは、`LoadError`（`ModuleMismatch`）。

## 4.5 永続データ

- 永続データは、`persistent var` の値と、通過済みの停止点（安定 ID の集合）である。セーブとは別のバイト列とし、全セーブで共有する。パッケージの**永続スライス**（host/03-packages.md の 3.5）も、パッケージ名・版数・バイト列として入る。
- 構成は、セーブと同じ（4.2。マジック `YOMP`、形式の版数 5、本体、チェックサム）。

```rust
struct PersistentBody {
    schema_version: u32,             // 台本のスキーマの版（4.3）
    types: Vec<(Name, String)>,      // 宣言した `struct`・`enum`（4.3 と同じ）
    var_types: Vec<(Name, String)>,  // `vars`・`host_vars` の各変数の型（4.3 と同じ）
    vars: Vec<(Name, SavedValue)>,   // `persistent var`（名前は `モジュール名.変数名`）
    host_vars: Vec<(Name, SavedValue)>,   // ホストが宣言した `Kind::Persistent` の変数（名前は `パッケージ名.変数名`）
    slices: Vec<SavedSlice>,         // パッケージの永続スライス（登録の順。`Persistent` が `()` のものは入れない）
    seen: Vec<String>,               // 通過済みの停止点（`モジュール.ラベル#id`。昇順）
}
```

- ホストが宣言した `Kind::Persistent` の変数（host/02-registry.md の 2.7）も、`host_vars` に入る。永続スライスは、`slices` に、セーブの `slices`（4.3）と同じ形で入る。
- 永続データも、セーブと同じスキーマ（4.3）を持ち、ロードのときに、セーブとは別に、同じ規則（4.6 の「スキーマの検査と移行」。`LoadPolicy` も同じ）で検査して、`migrate` を適用する。`PersistentReport` に、`from_schema_version`・`to_schema_version`・`migrated_vars`・`migrate_failed_vars` が加わる（意味は `LoadReport` と同じ）。
- 永続スライスの規則は、状態スライスと同じ（4.6）。登録されていないパッケージのものは無視し（`ignored_slices`）、永続データになかったものは `Persistent::default()` にし（`defaulted_slices`）、版数が違えば `migrate_persistent` を通す（`migrated_slices`）。`migrate_persistent` が `Err` のとき、または壊れたバイト列のときは、`LoadError`（`PackageSlice`）で、VM は変わらない。
- `save_persistent` は、直列化できないスライスがあっても、失敗しない（そのスライスは入れない）。セーブ（`save`）は、直列化できないスライスがあると、実行時エラー（`SliceSerialize`）を返す。
- `persistent var` の型に使う `struct`・`enum` の宣言が変わったときも、台本の変数と同じ規則（4.6 の「スキーマの検査と移行」）で扱う。`Strict` は `LoadError`、`Lenient` は、その変数を初期値に戻す（`changed_types`・`type_changed_vars`。`PersistentReport`）。
- ホストが宣言した変数の互換は、台本の変数と同じ規則（追加は初期値、消えたものは捨てる、型が変わったものは初期値）で、`added_vars`・`removed_vars`・`type_changed_vars`（名前は `パッケージ名.変数名`）で知らせる。
- ホストは、永続データを、セーブとは別の場所に保存する。ロード・セーブの順序や、同時に書く場合の整合は、ホストの責任とする。
- 通過済みの停止点は、`モジュール名.ラベル名#id` をキーに持つ。台本の `seen()` が読む（language/05-sugar.md の 5.7）。
- `persistent var` の宣言が変わったとき（追加、消えた、型の変更）の扱いは、セーブの変数と同じ規則だが、位置は戻らない（永続データは位置を持たない）。**追加された変数は、宣言の初期値になる**。型が変わった変数は、`migrate` で直せなければ、`Strict` は `LoadError`、`Lenient` は宣言の初期値になる。消えた変数は、捨てる。`PersistentReport`（`added_vars`・`removed_vars`・`type_changed_vars`・`ignored_slices`・`defaulted_slices`・`migrated_slices`。変数の名前は `モジュール名.変数名`、スライスはパッケージ名）で知らせる。

```rust
let bytes: Vec<u8> = vm.save_persistent();                 // 停止中でなくても呼べる
let report: PersistentReport = vm.load_persistent(&bytes)?;
```

- `load_persistent` は、`persistent var` と通過済みの停止点を、**置き換える**（永続データになかった `persistent var` は、初期値になる）。セーブの状態（`save var`・位置・履歴・乱数）は変えない。壊れたバイト列は `LoadError` を返し、VM は変わらない。
- 停止中に呼ぶと、停止点の引数を、新しい `persistent var` と `seen` で作り直す（`${式}` や `if` が、それらを読む台本のため）。作り直せなかった（実行時エラーになった）ときは、`LoadError`（`StopArgsFailed`）を返し、VM は変わらない。
- **ロードの順序**：セーブを先に読んでから永続データを読んでも（`Vm::load` のあとに `load_persistent`）、永続データを先に入れてからセーブを読んでも（`VmConfig::persistent_data` に永続データを渡して、`Vm::load`）、結果は同じである（引数の作り直しで、実行時エラーにならないかぎり）。作り直しで実行時エラーになりうる台本（`persistent var` で割るなど）は、後者（永続データを先に渡す）にする。
- `VmConfig::persistent_data(bytes)` は、`Vm::new` と `Vm::load` が、`persistent var` と通過済みの停止点を入れるときに使う。`Vm::new` は、読めなければ実行時エラー（`PersistentData`）を返す。`Vm::load` は、`LoadReport::persistent` に報告を入れる。
- `VmConfig::seen_points`・`Vm::seen_points()`・`Vm::set_seen_points()` は、通過済みの停止点だけを出し入れする、低い水準の口である。永続データを読むと、通過済みの停止点は、永続データのもので置き換わる（`VmConfig::seen_points` と `VmConfig::persistent_data` を両方渡したときは、永続データが優先する）。
- **セーブには、`persistent var` も通過済みの停止点も含めない**。セーブを読んでも、それらは変わらない。ロールバックでも、元に戻らない（4.7）。

## 4.6 ロード

```rust
struct LoadReport {
    resolved_by: ResolvedBy,         // Id / Hash / Ordinal / LabelStart。停止点とフレームの解決のうち、最も弱いもの（フレームは、最も良くて Hash）
    added_vars: Vec<Name>,           // 既定値で補った変数（`save var`）
    removed_vars: Vec<Name>,         // 台本から消えていて、捨てた変数（`save var`）
    type_changed_vars: Vec<Name>,    // 型が変わっていた変数（`save var`。ラベルの先頭へ戻る）
    changed_types: Vec<Name>,        // 宣言が変わっていた `struct`・`enum`（`モジュール.型名`。それを含む型も）
    type_changed_locals: Vec<Name>,  // 型が変わっていたローカル変数（`モジュール.ラベル.変数`。ラベルの先頭へ戻る）
    ignored_slices: Vec<Name>,       // 登録されていないので無視した状態スライス（パッケージ名）
    defaulted_slices: Vec<Name>,     // セーブになかったので、既定値にした状態スライス
    migrated_slices: Vec<Name>,      // `migrate` を通した状態スライス
    changed_rules: Vec<Name>,        // 規則の版が、セーブしたときと違うパッケージ
    from_schema_version: u32,        // セーブのスキーマの版（4.3）
    to_schema_version: u32,          // 台本のスキーマの版
    migrated_vars: Vec<Name>,        // `migrate` で変換した変数（`save var`。`rename` した変数は、新しい名前）
    migrate_failed_vars: Vec<Name>,  // `migrate` の変換が失敗し、`Lenient` で初期値にした変数（ラベルの先頭へ戻る）
    persistent: Option<PersistentReport>,   // `VmConfig::persistent_data` を渡したときの、永続データの報告
}
```

### ロードの順序

`Vm::load` は、次の順に進む。どこで失敗しても、`LoadError` を返し、渡されたバイト列も既存の VM も変わらない。

1. マジック・形式の版数・チェックサムの検査（4.2）。本体を読む。
2. スキーマの版の検査と、スキーマの比較・移行（下の「スキーマの検査と移行」。メモリ上で行う）。
3. パッケージのスライスの復元（`migrate`。4.6 の「スライス」）。
4. 位置の解決（4.4）。
5. VM の復元（停止点の引数の作り直し）。

### ロードの方針（`LoadPolicy`）

```rust
enum LoadPolicy { Strict, Lenient }      // 既定は Strict
```

- `VmConfig::load_policy` で選ぶ（host/01-execution.md の 1.9）。`Vm::load`、`Vm::new`（`persistent_data` を渡したとき）、`load_persistent` に効く。
- スキーマが合わない（下）とき、`Strict` は、`LoadError` を返す。`Lenient` は、その変数を台本の初期値にし、位置をラベルの先頭へ戻して、続行する（報告に出す）。
- `Strict` でも `Lenient` でも変わらない扱いは、「スキーマの検査と移行」に書く。

### スキーマの検査と移行

保存したスキーマ（4.3）と、台本の現在のスキーマを、変数ごとに比べる。対象は、`save var`（永続データでは `persistent var`）である。ホストが宣言した変数とローカル変数は、対象としない（下）。

1. **版の検査**。セーブの `schema_version` を `s`、台本の版を `v` とする。
   - `s` > `v`：`LoadError`（`SaveFromNewerVersion`）。古い台本での、新しいセーブの読み込み（ダウングレード）はしない。
   - `s` < 台本の `min_save_version`（language/07-migrate.md。書かれていなければ 1）：`LoadError`（`SaveVersionUnsupported`）。
   - どちらも、`LoadPolicy` によらない。
2. **移行**。`s` < `v` のとき、`migrate s+1`、`migrate s+2`、…、`migrate v` を、この順に、保存した変数の集まりに適用する（`rename`・`convert`・`drop`。language/07-migrate.md）。`migrate` のない版は、何もしない版として進む。`convert` を通した変数と、`rename` した変数は、`migrated_vars` に出る。
3. **比較**。移行したあとの各変数を、台本の宣言と比べる。比べるのは、**展開した型**である（`struct`・`enum` は、`types` の宣言の形で置き換える。再帰する型は、2 回目の出現を名前のままにする）。`struct`・`enum` の宣言が変わると、それを含む変数も、型が違うものになる（`changed_types`）。
   - 台本にあって、セーブにない変数：台本の初期値で補う（`added_vars`）。
   - セーブにあって、台本にない変数：捨てる（`removed_vars`）。
   - 型が同じ変数：そのまま使う。
   - 型が違う変数（**スキーマの不一致**）：`Strict` は、`LoadError`（`SchemaMismatch`）。`Lenient` は、その変数を台本の初期値にし、位置をラベルの先頭へ戻す（`type_changed_vars`・`changed_types`）。
4. **`convert` の実行時エラー**（0 による割り算・整数の桁あふれなど）：`Strict` は、`LoadError`（`MigrateFailed`）。`Lenient` は、その変数を台本の初期値にし、位置をラベルの先頭へ戻す（`migrate_failed_vars`）。

- `SchemaMismatch` は、不一致の変数を**すべて**持つ。1 件は、変数の名前・保存した型・台本の型（どちらも展開した型）である。`MigrateFailed` も、失敗した変数をすべて持つ。
- **ホストが宣言した変数**（`Kind::Save`・`Kind::Persistent`）は、台本の `migrate` の対象にならず、`LoadPolicy` にもよらない。追加・消えた・型が変わったときの扱いは、従来どおり（追加は初期値、消えたものは捨てる、型が変わったものは初期値にして位置をラベルの先頭へ戻す。`added_vars`・`removed_vars`・`type_changed_vars`）。状態スライスの移行は、パッケージの `migrate`（host/03-packages.md の 3.2）が行う。
- **ローカル変数**は、スキーマに含めない。名前・型・個数が合わないときは、`LoadPolicy` によらず、位置をラベルの先頭へ戻す（下の「変数の互換」）。
- ロードしたあとのセーブ（`vm.save()`）は、台本の現在のスキーマで書かれる。変換後のセーブを元のファイルに書き戻すかどうかは、ホストが決める。コアは、渡されたバイト列に書かない。ロードの前にバックアップを取りたいホストは、元のバイト列を保っておく。

- ロードの直後に、全パッケージの合図（host/03-packages.md の 3.6）を空にする（セーブに入っていた合図は、再生されない）。
- ロードに成功すると、VM は、復元した停止点で止まった状態になる。ただし、解決のしかたが `LabelStart`（4.4 の 4）のときは、停止していない。`vm.current()` は `None`（`vm.is_stopped()` は偽）で、ホストは `vm.run()` で、ラベルの先頭から実行する。ホストは、`vm.current()` で、その停止点の `Yield` を得て、通常の停止点と同じように処理し、`resume` する。
- **変数の互換**：台本の宣言を調べる。対象は `save var` と、ホストが宣言した `Kind::Save` の変数と、ローカル変数である（`persistent var` と `Kind::Persistent` の変数は、4.5）。`added_vars` などには、ホストが宣言した変数も、`パッケージ名.変数名` で入る。
  - 台本に追加された変数は、既定値で補う（`added_vars`）。
  - 台本から消えた変数は、捨てる（`removed_vars`）。`save var` を `persistent var` に変えたときも、セーブの値は捨てる。
  - 型が変わった変数（`migrate` で直らないもの）は、`LoadPolicy` に従う（上の「スキーマの検査と移行」）。`Lenient` では、位置を、ラベルの先頭へ戻し、その変数を台本の初期値にする（`type_changed_vars`）。`struct`・`enum` の宣言が変わったとき（フィールド・バリアントの名前・個数・型・順序の変更）も、その型（と、それを `list`・`Option`・ほかの型の中に含む型）の値を持つ変数は、型が変わったものとして扱い、変わった型は `changed_types` に出る。保存した値が、いまの型に合わないとき（壊れている）は、`LoadPolicy` によらず、`LoadError`（壊れている）。
  - `for` は、隠しのローカル変数（回す列のコピーと次の位置。名前は `$` で始まる）に状態を持つので、ループの途中のセーブもローカル変数として保存・復元される。隠し変数は、`Vm::variables()`・CLI の `:vars` の表示と `type_changed_locals` に出ない。
  - ローカル変数も同じ。保存したときと、名前・型・個数が 1 つでも合わないときは、位置を、ラベルの先頭へ戻す。`type_changed_locals` に出るのは、現在のラベルと、コールスタックの各フレームのラベルの、名前が同じで型が変わったものだけで、名前や個数が変わっただけのときは、出ない（`resolved_by` が `LabelStart` になる）。停止点の引数を再評価できないときも、ラベルの先頭へ戻す。
- **戻り値を受ける停止点**：停止点が `let 名前 = @コマンド(…)`・`名前 = @コマンド(…)` のとき、ロードのあとの `resume` の値も、同じ変数に入る（位置は、停止点の ID・ハッシュ・序数から決まる）。位置を決められず、ラベルの先頭へ戻るときは、その `let`・代入を、最初から実行し直す（変数は、ラベルの先頭からの実行で、また受ける）。
- **停止点の引数の作り直し**：ロードは、停止点の引数を、台本から作り直す。乱数は、セーブの `rng`（引数を求め始めたときの状態）から始める。
- **スライス**：host/03-packages.md の 3.2 に従う。登録されていないパッケージのスライスは無視し、セーブになかったものは既定値にし、版数が違えば `migrate` を通す。`migrate` が `Err` のとき、または壊れたバイト列のときは、`LoadError`（`PackageSlice`。値はパッケージ名）。
- **規則の版**：セーブの `rules` と、登録されているパッケージの `RULES_VERSION` を比べ、違うパッケージを `changed_rules` に入れる（登録の順）。ロードは続け、今の規則で動く（同じ引数のコマンドでも、セーブ前と結果が違うことがある）。セーブにないパッケージ・登録されていないパッケージは、比べない。
- 永続データを先に入れてから（`VmConfig::persistent_data`）セーブを読んでも、スライスの結果は変わらない。永続スライスは、セーブからは復元しない。
- 実行時にソースをロードするので、ロードに使う `program` は、セーブしたときと違ってよい（台本が更新されている場合）。

## 4.7 ロールバック

- ロールバックは、停止点ごとのスナップショットを溜めて、戻る機能である。ホストが、停止点ごとに `vm.snapshot()` を取り、リングに溜め、戻るときに `vm.restore(&snapshot)` を呼ぶ。

```rust
let snapshot: Snapshot = vm.snapshot()?;     // 停止中だけ。停止していなければ NotStopped
vm.restore(&snapshot)?;                      // その停止点で止まった状態に戻る

let mut log = SnapshotLog::new(200);         // 件数の上限つきのリング
log.push(vm.snapshot()?);                    // 上限を超えたら、古いものから捨てる
let previous = log.pop_back();               // いちばん新しいものを取り出す
```

- スナップショットは、メモリ上の複製で、バイト列にはしない（ディスクに書くのは、セーブ）。同じプログラムの VM にだけ戻せる。別のプログラムのスナップショットは、`restore` が `SnapshotMismatch` を返し、VM は変わらない。
- 複合値（`list`・`struct` など）のスナップショットは、取ったあとの更新の影響を受けない（`list`・`map` は、中身を共有していて、更新するときに、共有している分だけ写す。値としては、複製と同じ）。
- **戻すもの**は、セーブの状態（`save var`・ホストが宣言した `Kind::Save` の変数・パッケージの状態スライス・ローカル変数・コールスタック・`visited`・`once_done`・乱数・履歴・位置）。戻したあとの続きの実行は、保存してから読み込んで復元したときと同じになる。
- **戻さないもの**は、`persistent var`・ホストが宣言した `Kind::Persistent` の変数・パッケージの永続スライスと、通過済みの停止点（`seen`）。ロールバックで戻っても、一度通過した停止点は、通過済みのままで、`persistent var` は、進んだ値のままである（language/05-sugar.md の 5.7）。
- ロールバックの直後に、全パッケージの合図（host/03-packages.md の 3.6）を空にする（スナップショットに入っていた合図は、再生されない）。
- `restore` は、実行時エラーのあと（`run` を続けられない状態）でも、呼べる。復元した VM は、スナップショットの停止点で止まっている。`vm.current()` は、そのときの `Yield` である。
- `SnapshotLog` の `capacity` が 0 のときは、何も持たない。

## 4.8 エラー

`LoadError` は、次の場合に返る。

- マジックが違う、形式の版数が知らないもの、チェックサムが合わない、本体が壊れている（`float` の値が `NaN`・`±∞` のセーブ、入れ子が深すぎる複合値、節の数が合わない複合値も、壊れているとみなす。値として現れないため。乱数の状態が 0 だけのセーブも同じ）。
- ラベルが存在せず、位置を決められない（4.4）。
- 永続データを、セーブとして（またはセーブを永続データとして）読もうとした（マジックが違う）。
- `load_persistent` が、停止点の引数を作り直せなかった（`StopArgsFailed`）。
- パッケージのスライスを復元できなかった（`PackageSlice`。壊れている・`migrate` が `Err`。状態スライス・永続スライスとも）。
- セーブのスキーマの版が、台本の版より新しい（`SaveFromNewerVersion`。保存した版と台本の版を持つ）。
- セーブのスキーマの版が、台本の `min_save_version` より古い（`SaveVersionUnsupported`。保存した版と下限を持つ）。
- `Strict` で、スキーマが合わない変数がある（`SchemaMismatch`。変数の名前・保存した型・台本の型の並びを持つ。すべての変数を、まとめて返す）。
- `Strict` で、`migrate` の変換が実行時エラーになった（`MigrateFailed`。変数の名前の並びを持つ）。

- `LoadError` は、種類（コード）と、引数を持つ。文面は持たない。
- コアは、ロードが失敗したとき、既存の VM の状態を壊さない（`Vm::load` は、新しい VM を返すだけである。`load_persistent` は、失敗したら、VM を元のままにする）。

## 4.9 待っている停止点の読み出し

```rust
fn pending_stop(save: &[u8]) -> Result<Option<PendingStop>, LoadError>;

struct PendingStop {
    label: String,          // 停止点を置いたラベル（"モジュール.ラベル"）
    key: Option<String>,    // "モジュール.ラベル#id"（安定 ID のない停止点・選択は None）
    command: String,        // 止まっているコマンドの名前（保存したときの登録表の名前）
    args: Vec<Value>,       // 停止の知らせの引数（原文の評価結果）
    schema_version: u32,    // セーブのスキーマの版（4.3）
}
```

- セーブから、保存した停止点で止まっているコマンドと引数を、台本・登録表・VM なしで読む。外部イベントを待つセーブ（host/01-execution.md の 1.10）を多く持つホストが、どのセーブを再開するかを決めるのに使う。
- 読むのは、履歴（4.3）の最後の記録である（保存した停止点の記録は、履歴の最後にある）。履歴を記録していない（`history_limit` が 0 の）セーブは、`Ok(None)`。
- マジック・形式の版数・チェックサム・本体の検査は、ロード（4.2）と同じで、合わなければ `LoadError`。スキーマの検査・移行・位置の解決は、行わない。永続データを渡すと、`LoadError`（マジックが違う）。
- `args` は、保存したときの値である。ロードすると、停止点の引数は作り直される（4.6）ので、`persistent var` などで変わることがある。

## 4.10 ホットリロード

```rust
impl Vm {
    fn reload(&self, program: &Program) -> Result<(Vm, LoadReport), ReloadError>;
}

enum ReloadError {
    NotStopped,                     // 停止していない
    Load(LoadError),                // 4.8 の理由で、引き継げない
    PositionLost(ResolvedBy),       // 位置を、ラベルの先頭へ落とす以外に解決できない
}
```

- 台本を直したとき、止まっている位置・変数・コールスタックを保ったまま、新しい `Program` へ差し替える。停止中にだけ呼べる（停止していなければ `NotStopped`）。
- 新しい `Vm` は、いまの `Vm` から、停止点の位置・コールスタック・ローカル変数・`save var`・ホストが宣言した変数・パッケージの状態スライス・履歴・乱数・入ったラベルと選び済みの `once`・`persistent var`・パッケージの永続スライス・通過済みの停止点を引き継ぐ。登録表・`VmConfig`（中断のトークンを含む）は、いまの `Vm` と同じ。
- 位置・フレームの解決、スキーマの検査と移行は、ロード（4.4・4.6）と同じ規則で、新しい `Program` に対して行う。保存したスキーマの版は、いまの `Vm` の `Program` のもの。`LoadPolicy` も `VmConfig` のものに従う。
- ロードと違い、**位置かフレームがラベルの先頭へ落ちるとき**（`LabelStart`。`Lenient` でスキーマが合わないときも含む）は、成功にせず、`PositionLost` を返す。`Vm` が変わるのは、停止点を保てたときだけである。
- 成功した `Vm` は、必ず停止点で止まっている（`vm.current()` は `Some`）。停止点の引数は、新しい台本から作り直す（4.6）ので、本文を直したあとの内容になる。ホストは、`vm.current()` を、通常の停止点と同じに処理し直す。差し替え表を使っているホストは、新しい `Vm` に設定し直す（host/05-texts.md の 5.8）。
- `LoadReport` は、ロードのときと同じ（`resolved_by` は `Id`・`Hash`・`Ordinal` のどれか）。`Ordinal` は、近い位置を推した結果で、行を直しすぎたときに、別の行へずれることがある。
- 失敗しても、いまの `Vm` は変わらない（`&self`）。ホストは、そのまま続けられる。
- 全パッケージの合図（host/03-packages.md の 3.6）は、ロードと同じで、新しい `Vm` では空になる。ホストは、`reload` の前に、合図を取り出しておく。
