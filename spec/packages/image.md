# パッケージ `image`（yomi-std）

キャラクター・背景・アイコンなど、画像を表示する部品の指示。描くのは、ホストである。

## 考え方

- 画像は、**枠**（`slot`、名前つきの置き場）に置く。背景は `"bg"`、キャラクターは `"left"` や `"partner"` のように、台本の書き手が決める。コアは、意味を持たない。
- 座標は、ホストが決める座標系の `float`（0.0〜1.0 の割合にするなど、ホストの約束）。
- 重なりの順は、重なりの値（`layer`。整数で、小さいほど奥。既定 0）の順。同じ値なら、枠を足した順（あとから足したものが、手前）。
- 枠の画像は、拡大率（`scale`。1.0 が元の大きさ）・左右の反転（`flip`）・不透明度（`alpha`。0.0〜1.0）を持つ。拡大・反転の中心は、ホストが決める。

## 状態スライス

```rust
struct ImageState {
    items: Vec<Item>,        // 表示中のもの。足した順
    cues: Vec<ImageCue>,     // 合図（直前の resume の後に足されたもの）
}
struct Item {
    slot: Str, name: Str, x: f64, y: f64,
    layer: i64,              // 重なりの値。既定 0
    scale: f64,              // 拡大率。既定 1.0
    flip: bool,              // 左右の反転。既定 false
    alpha: f64,              // 不透明度。既定 1.0
}
enum ImageCue {
    Show { slot: Str, fade: f64 },
    Move { slot: Str, seconds: f64 },
    Hide { slot: Str, fade: f64 },
    Clear { fade: f64 },
    Layer { slot: Str },
    Scale { slot: Str, seconds: f64 },
    Flip { slot: Str },
    Alpha { slot: Str, seconds: f64 },
}
```

- `items` が、永続的な部分。ホストは、ロード・ロールバックの後、これを読んで、描き直す。
- `cues` は、次の `run`・`resume` の始めと、ロード・ロールバックの直後に、空になる（`Package::clear_cues`。host/03-packages.md の 3.6）。合図は、状態の一部として、セーブ・スナップショットに入る。ホストは、停止点で `vm.package_state::<Image>()` を読み、`cues` の合図に従って描き、次の `resume` の前に読み終える。位置・画像の名前・重なりの値・変形は、`items` から引く（合図は、枠の名前と、時間だけを持つ）。
- 永続スライスは、持たない（`Persistent = ()`）。
- 状態スライスの版は 2（`Package::VERSION`）。版 1 のセーブは、重なりの値と変形を既定の値として読む（`Package::migrate`）。

## コマンド

| コマンド | 種類 | 引数 | 意味 |
| --- | --- | --- | --- |
| `image.show` | 副作用 | `slot: str`, `name: str`, `x: float = 0.0`, `y: float = 0.0`, `fade: float = 0.0` | 枠に画像を置く。枠にすでにあれば、画像と位置を入れ替える（重なりの値・変形・順は変えない）。`fade < 0` は実行時エラー |
| `image.move` | 副作用 | `slot: str`, `x: float`, `y: float`, `seconds: float = 0.0` | 枠の画像の位置を変える。枠が空なら、実行時エラー。`seconds` が負なら、実行時エラー |
| `image.hide` | 副作用 | `slot: str`, `fade: float = 0.0` | 枠の画像を消す。枠が空なら、何もしない（合図も出さない）。`fade < 0` は実行時エラー |
| `image.clear` | 副作用 | `fade: float = 0.0` | すべての枠を空にする。すでに空なら、何もしない（合図も出さない）。`fade < 0` は実行時エラー |
| `image.layer` | 副作用 | `slot: str`, `layer: int` | 枠の重なりの値を変える（すぐに）。枠が空なら、実行時エラー |
| `image.scale` | 副作用 | `slot: str`, `scale: float`, `seconds: float = 0.0` | 枠の画像の拡大率を、`seconds` 秒かけて変える。枠が空、`scale <= 0`、`seconds < 0` なら、実行時エラー |
| `image.flip` | 副作用 | `slot: str`, `flip: bool` | 枠の画像を、左右に反転するかを変える（すぐに）。枠が空なら、実行時エラー |
| `image.alpha` | 副作用 | `slot: str`, `alpha: float`, `seconds: float = 0.0` | 枠の画像の不透明度を、`seconds` 秒かけて変える。枠が空、`alpha` が 0.0〜1.0 の外、`seconds < 0` なら、実行時エラー |
| `image.shown` | 問い合わせ | `slot: str` | 枠に画像があるか（`bool`） |
| `image.name` | 問い合わせ | `slot: str` | 枠の画像の名前（`Option<str>`） |

- `= 値` は、引数の既定値。省いた引数は、既定値になる。名前つきで渡せる（language/03-statements.md の 3.5）。
- 数値の引数（`x`・`y`・`fade`・`seconds`・`scale`・`alpha`）は、有限であること（`NaN`・`±∞` は実行時エラー）。`fade`・`seconds` は 0 以上。実行時エラーは、末尾の「エラーコード」のコードを持ち、状態は変わらない。
- フェード・移動は、ホストが、合図を見て、時間をかけて描く。台本は、終わりを待たない。待つなら、`@flow.wait(…)` を書く。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `image.negative` | `command`, `arg` | `fade`・`seconds` が負 |
| `image.not_finite` | `command`, `arg` | 数値の引数が `NaN`・`±∞` |
| `image.not_positive` | `command`, `arg` | `scale` が 0 以下 |
| `image.out_of_unit_range` | `command`, `arg` | `alpha` が 0.0〜1.0 の外 |
| `image.slot_empty` | `command`, `slot` | `image.move`・`image.layer`・`image.scale`・`image.flip`・`image.alpha` の枠が空 |
