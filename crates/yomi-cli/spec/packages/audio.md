# パッケージ `audio`（yomi-std）

チャンネルで鳴らし続ける音（BGM・環境音など）と、効果音・ボイスの指示。音を鳴らすのは、ホストである。

## 状態スライス

```rust
struct AudioState {
    channels: Vec<Channel>,  // 鳴らし続けているチャンネル（鳴らし始めた順）
    cues: Vec<AudioCue>,     // 合図（直前の resume の後に足されたもの）
}
struct Channel { channel: Str, name: Str, volume: f64 }
enum AudioCue {
    Play { channel: Str, name: Str, volume: f64, fade: f64 },
    Stop { channel: Str, fade: f64 },
    Volume { channel: Str, volume: f64, fade: f64 },
    Se { name: Str },
    Voice { name: Str },
    SeStop,
    VoiceStop,
}
```

- `channels` が、永続的な部分。ホストは、ロード・ロールバックの後、これを読んで、鳴らし直す。
- `cues` は、次の `run`・`resume` の始めと、ロード・ロールバックの直後に、空になる（`Package::clear_cues`。host/03-packages.md の 3.6）。合図は、状態の一部として、セーブ・スナップショットに入る。ホストは、停止点で `vm.package_state::<Audio>()` を読み、`cues` の合図を再生して、次の `resume` の前に読み終える。
- 永続スライスは、持たない（`Persistent = ()`）。音量の設定など、ホストの設定は、ホストが持つ。
- 状態スライスの版は 3（`Package::VERSION`）。版 1・2 のセーブは、BGM を `"bgm"` のチャンネルに（音量は BGM の音量）、ループをそのあとのチャンネルにして読む（`Package::migrate`。BGM が鳴っていて、`"bgm"` という名前のループもあれば、BGM を残す）。旧い版の合図は、読んで捨てる（ロードの直後に空になるため）。

## コマンド

| コマンド | 種類 | 引数 | 意味 |
| --- | --- | --- | --- |
| `audio.play` | 副作用 | `channel: str`, `name: str`, `volume: float = 1.0`, `fade: float = 0.0` | チャンネルで、音を鳴らし続ける。すでに鳴っていれば、素材と音量を入れ替える（順は変えない）。`fade` 秒で、入れ替える。`volume` が 0.0〜1.0 の外、または `fade < 0` なら、実行時エラー |
| `audio.stop` | 副作用 | `channel: str`, `fade: float = 0.0` | チャンネルの音を止める。鳴っていなければ、何もしない（合図も出さない）。`fade < 0` は実行時エラー |
| `audio.volume` | 副作用 | `channel: str`, `volume: float`, `fade: float = 0.0` | チャンネルの音量を変える。鳴っていなければ、実行時エラー。`volume` が 0.0〜1.0 の外、または `fade < 0` なら、実行時エラー |
| `audio.playing` | 問い合わせ | `channel: str` | チャンネルで鳴らしている音の名前（`Option<str>`） |
| `audio.se` | 副作用 | `name: str` | 効果音を 1 回鳴らす（合図だけ） |
| `audio.voice` | 副作用 | `name: str` | ボイスを 1 回鳴らす（合図だけ） |
| `audio.se_stop` | 副作用 | なし | 鳴っている効果音を、すべて止める（合図だけ） |
| `audio.voice_stop` | 副作用 | なし | 鳴っているボイスを止める（合図だけ） |

- `= 値` は、引数の既定値。省いた引数は、既定値になる。名前つきで渡せる（`@audio.play("bgm", "town", fade: 1.5)`。language/03-statements.md の 3.5）。
- チャンネルの名前（`channel`）は、台本の書き手が決める（`"rain"`・`"crowd"` など）。BGM は、慣習として `"bgm"` のチャンネルで鳴らす（ホストが、チャンネルの名前で、音の種類を分けてよい）。`name` は、ホストが、素材に対応づける。コアは、意味を持たない。
- 数値の引数（`fade`・`volume`）は、有限であること（`NaN`・`±∞` は実行時エラー）。実行時エラーは、末尾の「エラーコード」のコードを持ち、状態は変わらない。
- `audio.playing` は、台本では `Option<str>` で、`${audio.playing("bgm")}` と書くと `Some(名前)`・`None` になる（`match` で取り出せる。language/03-statements.md の 3.6）。
- 効果音・ボイスの終わりを待つには、`@flow.wait(…)` か、ホスト側の停止型コマンドを、台本で書く。`audio` は、待たない。

## エラーコード

コマンドの実行時エラーは、コード（`パッケージ名.エラー名`）と、名前つきの引数を持つ（文面は持たない。表示は、ホストの責務）。VM の実行時エラーとしては、`command_failed`（language/06-diagnostics.md の 6.6）で、`VmErrorKind::CommandFailed { code, args }` からコードと引数を取り出せる。位置は VM が付け、状態は変わらない。引数の `command` は、コマンドの名前（`audio.play` など）、`arg` は引数の名前、小数の値は、文字列で渡す。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `audio.channel_empty` | `command`, `channel` | `audio.volume` のチャンネルが鳴っていない |
| `audio.negative` | `command`, `arg` | `fade` が負 |
| `audio.not_finite` | `command`, `arg` | 数値の引数が `NaN`・`±∞` |
| `audio.out_of_unit_range` | `command`, `arg` | `volume` が 0.0〜1.0 の外 |
