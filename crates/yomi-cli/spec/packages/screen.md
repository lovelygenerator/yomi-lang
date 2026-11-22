# パッケージ `screen`（yomi-std）

画面全体の効果（暗転・揺れ・フラッシュ）の指示。描くのは、ホストである。

## 考え方

- **覆い**（`cover`）は、画面全体を一色で覆った状態（暗転・白転など）。覆いは状態に残り、外すまで続く。ロード・ロールバックの後も、覆ったまま描き直す。
- **揺れ**と**フラッシュ**は、一度きりの出来事で、合図だけを出す（状態に残らない）。
- 色（`color`）は、ホストが解釈する文字列（`"black"`・`"#ffffff"` など）。強さ（`strength`）も、ホストが解釈する。コアは、意味を持たない。
- 画像（packages/image.md）の上に、覆い・フラッシュを重ねる。覆いの下の画像は、そのまま変えられる（暗転の間に背景を替えるなど）。

## 状態スライス

```rust
struct ScreenState {
    cover: Option<Cover>,    // 画面を覆っている色（覆っていなければ None）
    cues: Vec<ScreenCue>,    // 合図（直前の resume の後に足されたもの）
}
struct Cover { color: Str }
enum ScreenCue {
    FadeOut { color: Str, seconds: f64 },
    FadeIn { seconds: f64 },
    Shake { strength: f64, seconds: f64 },
    Flash { color: Str, seconds: f64 },
}
```

- `cover` が、永続的な部分。ホストは、ロード・ロールバックの後、これを読んで、描き直す。
- `cues` は、次の `run`・`resume` の始めと、ロード・ロールバックの直後に、空になる（`Package::clear_cues`。host/03-packages.md の 3.6）。合図は、状態の一部として、セーブ・スナップショットに入る。ホストは、停止点で `vm.package_state::<Screen>()` を読み、`cues` の合図に従って描き、次の `resume` の前に読み終える。
- 永続スライスは、持たない（`Persistent = ()`）。

## コマンド

| コマンド | 種類 | 引数 | 意味 |
| --- | --- | --- | --- |
| `screen.fade_out` | 副作用 | `color: str`, `seconds: float = 0.0` | 画面を、`seconds` 秒かけて、色で覆う（暗転など）。すでに覆っていれば、色を入れ替える。`seconds < 0` は実行時エラー |
| `screen.fade_in` | 副作用 | `seconds: float = 0.0` | 覆いを、`seconds` 秒かけて外す。覆っていなければ、何もしない（合図も出さない）。`seconds < 0` は実行時エラー |
| `screen.shake` | 副作用 | `strength: float`, `seconds: float` | 画面を、`seconds` 秒、揺らす（合図だけ）。`strength < 0`、`seconds < 0` は実行時エラー |
| `screen.flash` | 副作用 | `color: str`, `seconds: float` | 画面を、色で一瞬光らせる（合図だけ。`seconds` 秒で消える）。`seconds < 0` は実行時エラー |
| `screen.cover_color` | 問い合わせ | なし | 画面を覆っている色（`Option<str>`） |

- `= 値` は、引数の既定値。省いた引数は、既定値になる。名前つきで渡せる（language/03-statements.md の 3.5）。
- 数値の引数は、有限であること（`NaN`・`±∞` は実行時エラー）。実行時エラーは、末尾の「エラーコード」のコードを持ち、状態は変わらない。
- 効果の終わりを待つには、`@flow.wait(…)` を書く。`screen` は、待たない。

```
import std::screen
import std::image
import std::flow

label start {
  @screen.fade_out("black", 1.0)
  @flow.wait(1.0)
  @image.show("bg", "night", 0.0, 0.0, 0.0)
  @screen.fade_in(1.0)
  @screen.shake(0.5, 0.3)
}
```

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`screen.shake` など）、`arg` は引数の名前。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `screen.negative` | `command`, `arg` | `seconds`・`strength` が負 |
| `screen.not_finite` | `command`, `arg` | 数値の引数が `NaN`・`±∞` |
