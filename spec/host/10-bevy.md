# 10. Bevy との連携（`yomi-bevy`）

Bevy のゲームに yomi の台本を組み込む。Bevy の 0.20 だけを対象にする。`yomi-bevy` の版は、対応する Bevy の版に合わせる（Bevy 0.20 用は `0.20.x`）。

- 台本の実行（`Vm`）は、エンティティの `Component`（`YomiRunner`）が持つ。台本 1 本の実行が、エンティティ 1 つ。会話・クエスト・NPC を、同時に複数動かせる。
- 台本のコマンドと再開のやりとりは、Bevy のメッセージ（`MessageWriter`・`MessageReader`）で行う。
- 台本のファイル（`.yomi`）は、Bevy の `Asset` として読み込む。ファイルが変わると、実行中の台本を `Vm::reload`（host/01-execution.md）で差し替える。
- 本文の表示・選択肢の UI など、ジャンル別の部品は含めない（ホストのゲームが作る）。

## 10.1 プラグインと登録

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(
        YomiPlugin::new(|reg: &mut Registry| {
            reg.stop::<(String,), ()>("ui.say")?;
            reg.bind(Sugar::Plain, "ui.say")?;
            Ok(())
        })
        .script("scripts/main.yomi")
        .script("scripts/shop.yomi")
        .immediate(["ui.say"]),
    );
```

- `YomiPlugin::new(f)` の `f` は、`Registry` に、ゲームのコマンド・型・糖衣の束縛を登録する関数（`Result<(), RegistryError>` を返す）。標準パッケージ（`yomi_std::register_all`）は、`f` の前に登録される（`without_std()` で外せる）。登録に失敗したら、プラグインの組み立てで panic する。
- 登録表は 1 つのアプリに 1 つで、すべての実行が共有する。
- `.script(path)` で、台本のファイルを既定のプログラムに足す（`AssetServer` のパス）。モジュール名は、ファイル名の拡張子を除いたもの（`scripts/shop.yomi` は `shop`）。モジュール名が重なる、または識別子でないと、読み込みの失敗として扱う。
- `.program(name, paths)` で、名前つきのプログラムを足す（10.2）。`name` は空でなく、重ならない（違うと panic する）。
- `.immediate(names)` で、**止まらずに返すコマンド**を指定する（10.3）。

## 10.2 台本のアセットとコンパイル

- `.yomi` は、`YomiScript`（`Asset`）として読み込む（本文は UTF-8）。
- プログラムは、既定のもの（`.script` で足したファイル。名前は空）と、名前つきのもの（`.program`）がある。プログラムごとに、足したファイルが**すべて**読み込めたとき、全体を 1 つのプログラム（`compile_modules`）にコンパイルする。コンパイルした `Program` は、そのプログラムを選んだ実行が共有する。同じファイルを、複数のプログラムに入れてよい（モジュール名の重なりは、プログラムの中でだけ調べる）。
- コンパイルに失敗したら、`YomiDiagnostics { program, diagnostics }` のメッセージを出す（`program` は、既定のプログラムでは空）。すでに動いている、そのプログラムの実行は、前のプログラムで続ける。まだそのプログラムがなければ、その実行は始まらない。
- プログラムのいずれかのファイルが変わった（`AssetEvent::Modified`）ら、そのプログラムだけ、すべてを集め直してコンパイルし直す。成功したら、そのプログラムの実行へ、`Vm::reload` を試す（10.7）。ほかのプログラムの実行には、何も起こらない。
- 読み込みの失敗（ファイルがない・UTF-8 でない）は、Bevy の `AssetLoadFailedEvent` のままで、プラグインは別の通知を出さない。

## 10.3 実行（`YomiRunner`）

```rust
commands.spawn(YomiRunner::new().start("main.start").seed(7));
```

- `YomiRunner::program(name)` で、使うプログラムを選ぶ（選ばなければ既定のプログラム）。実行を作るときに選び、あとから変えない。定義にない名前（または、台本のない既定のプログラム）を選んだ実行は、`YomiFailed` で止まる。
- `YomiRunner` を持つエンティティができると、その実行が使うプログラムが用意でき次第、`Vm` を作って、開始ラベル（既定は `start`。host/01-execution.md）から実行を始める。
- 実行は、`Update` の `YomiSet`（システムセット）の中で進める。1 フレームの中で、止まるところ（待ち）まで、次を繰り返す。`vm.run()` が返した結果に応じて、
  - `Yield::End`：`YomiEnded { runner }` を出して、実行を終える（`YomiRunner` は残る。`restart`・`load` で動かし直せる）。
  - `Yield::Stop`：コマンドの名前で分ける（下）。
  - 実行時エラー：`YomiFailed { runner, error }` を出して、実行を止める。
- 1 フレームで続ける回数は、`YomiPlugin::max_steps_per_frame`（既定 1000 回の `run`）まで。超えたら、続きは次のフレームにする。
- 停止型のコマンドは、次のように扱う。

| コマンド | 扱い |
| --- | --- |
| `flow.wait` | プラグインが待つ。時間は、Bevy の仮想時間（`Time<Virtual>`。一時停止・倍速に従う）で測る。経ったら再開する。`0` は、すぐ再開する |
| `event.wait` | プラグインが待つ。`YomiEvent`（10.5）が届くか、期限（`timeout`。仮想時間）が切れるまで待ち、`event.Outcome` で再開する |
| `.immediate` で指定したもの | `YomiCommand` を出して、**すぐ**、戻り値なし（`()`）で再開する（戻り値が `()` のコマンドだけ指定できる。違うとプラグインの組み立てで panic する） |
| それ以外（`flow.wait_input`・`input.*`・ゲームのコマンド） | `YomiCommand { runner, command, args, id }` を出して、**止まる**。ゲームが `YomiResume`（10.4）を出すまで待つ |

- メッセージは、停止点に着いたフレームの `YomiSet` の中で書く。ゲームのシステムは、`YomiSet` の後（`after`）で読めば、同じフレームで受け取れる。

## 10.4 メッセージ

| メッセージ | 向き | 内容 |
| --- | --- | --- |
| `YomiCommand { runner: Entity, command: String, args: Vec<Value>, id: Option<String> }` | 台本 → ゲーム | 停止型コマンドの呼び出し。`id` は停止点の安定 ID |
| `YomiResume { runner: Entity, input: Input }` | ゲーム → 台本 | 止まっている実行を再開する。`input` は `Input::Return(値)`（host/01-execution.md）。止まっていない実行への `YomiResume` は無視する（`warn!` を出す）。値の型が合わなければ、実行時エラー（`YomiFailed`） |
| `YomiEvent { runner: Option<Entity>, name: String, data: String }` | ゲーム → 台本 | `event.wait` へ届けるイベント。`runner` が `None` なら、`name` を待っているすべての実行へ。`Some` なら、その実行だけへ |
| `YomiEnded { runner }` | 台本 → ゲーム | 実行が終わった |
| `YomiFailed { runner, error: String }` | 台本 → ゲーム | 実行時エラーで止まった（メッセージは、診断と同じ文面） |
| `YomiDiagnostics { program: String, diagnostics: Vec<Diagnostic> }` | プラグイン → ゲーム | コンパイルの診断（10.2）。`program` は、既定のプログラムでは空 |
| `YomiReloaded { runner, result: Result<(), String> }` | プラグイン → ゲーム | 差し替えの結果（10.7） |
| `YomiLoaded { runner, result: Result<(), String> }` | プラグイン → ゲーム | `YomiRunner::load` の結果（10.8） |
| `YomiSetTexts { runner: Entity, texts: Option<Handle<YomiTexts>> }` | ゲーム → 台本 | 本文の差し替え表を切り替える。`None` は原文（10.10） |
| `YomiTextsApplied { runner, result: Result<(), String> }` | プラグイン → ゲーム | `YomiSetTexts` の結果（10.10） |
| `YomiAudioCue { runner, cue }`・`YomiImageCue { runner, cue }`・`YomiScreenCue { runner, cue }` | 台本 → ゲーム | 標準パッケージの合図（10.6） |

- `Value`・`Input`・`Diagnostic` は、`yomi_core` の型をそのまま使う。
- `YomiResume` は、停止中の `YomiCommand` に対して**1 回だけ**出す。メッセージは、Bevy の規則で、2 フレームで捨てられるので、ゲームは、毎フレーム読むシステムで受ける。

## 10.5 外部イベント

- ゲームは、`YomiEvent` を出して、`event.wait` で待っている実行へ、イベントを届ける。
- `name` が、待っている `names` のどれでもなければ、その実行には届かない（捨てる。ほかの実行や、あとの待ちには持ち越さない）。
- 届いた実行は、`event.Outcome` の `Arrived(name, data)` で再開する。期限が切れたら `TimedOut`。

## 10.6 標準パッケージの状態と合図

- `audio`・`image`・`screen` は、コマンドを実行しても止まらず、状態の中の合図（`cues`）に足す（host/03-packages.md の 3.6）。プラグインは、実行が止まるたび（`YomiCommand` を出す前、`event.wait`・`flow.wait` に入る前、終わる前）に、`vm.cue_order()` の順で、合図を `YomiAudioCue`・`YomiImageCue`・`YomiScreenCue` として出す。台本で書いた順に出る。
- 状態そのもの（表示中の背景・鳴っている音など）は、`YomiRunner::vm()` から `package_state::<P>()` で読める（読むだけ）。
- ロードと巻き戻しの直後は、合図は空になる（古い合図は再生されない）。表示中の状態は、ゲームが `package_state` から作り直す。

## 10.7 ホットリロード

- 10.2 のコンパイルが成功したら、プラグインは、そのプログラムを使っている、止まっているすべての実行へ `Vm::reload` を試す。結果は、実行ごとに `YomiReloaded` で知らせる。
- `reload` が失敗した実行は、前のプログラムで続ける（その実行だけは、新しいプログラムを使わない）。失敗の理由は、`reload` のエラーの文面。
- 差し替えは、止まっている実行に対して、そのフレームの `YomiSet` の中で、実行を進める前に行う。
- 差し替えたあと、止まっていたコマンドは、そのまま `YomiResume` を待つ（`YomiCommand` を出し直さない）。`flow.wait`・`event.wait` は、待ち時間を数え直す。

## 10.8 セーブとロード

- `YomiRunner::save() -> Result<Vec<u8>, SaveError>`：実行が**止まっている間**（`YomiCommand` を出して待っている間、または `flow.wait`・`event.wait` の間）だけ呼べる。それ以外は `Err`。永続データは、`YomiRunner::save_persistent()`（host/04-save.md の 4.5）。
- `YomiRunner::load(&mut self, bytes: Vec<u8>)`：セーブからのロードを頼む。ロードは次の更新で行い、結果は `YomiLoaded` で知らせる（失敗した実行は変わらない）。ロードした実行は、保存したときの停止点で止まっている。待ち条件（コマンド・引数）は、セーブから読み直して、同じメッセージ（`YomiCommand`）を**出し直す**。`flow.wait`・`event.wait` は、待ち時間を数え直す。
- 方針（`LoadPolicy`）は、`YomiPlugin::load_policy` で指定する（既定は `Strict`）。永続データは、ロードのとき、その実行がいま持っているものを引き継ぐ。
- `YomiRunner::restart()`：開始ラベルからやり直す（永続データは保つ）。次の更新で行う。
- 巻き戻し（`snapshot`・`restore`）は、`YomiRunner::vm_mut()` から、コアの API を直接使う。

## 10.9 プログラムの寿命

`Vm` は、`Program` と `Registry` を参照で持つので、`Component` に置くため、プラグインは、`Registry` と、コンパイルに成功した `Program` を、アプリが終わるまで解放しない（リークさせて持つ）。差し替え（10.7）のたびに、前の `Program` が 1 つ残る（ゲームの開発中の、手で数回の保存を想定している）。

## 10.10 本文の差し替え表

- 差し替え表のファイル（`.texts`。host/05-texts.md の 5.7）は、Bevy の `Asset`（`YomiTexts`。拡張子 `texts`）として読む。形式の誤りは、アセットの読み込みの失敗になる。
- 実行は、表を 1 つ持てる（`YomiRunner::texts(handle)` で起動時に指定する。既定は、なし＝原文）。切り替えは、メッセージ `YomiSetTexts { runner, texts }` で行い、結果は `YomiTextsApplied` で知らせる。`texts` が `None` なら、原文に戻す。
- 表の読み込みが終わっていなければ、終わるのを待って、設定する。設定できない（`${n}` の番号の誤りなど）ときは、前の表のままで、`YomiTextsApplied` に理由を入れる。
- 停止中に切り替えたとき、今の停止点の本文を、新しい表で作り直し（`Vm::refresh_current`）、引数が変わっていれば、同じ停止点の `YomiCommand` を出し直す。本文を持たない停止点（`flow.wait` など）と、変わらなかった本文は、出し直さない。出し直した `YomiCommand` にも、`YomiResume` は 1 回だけ出す（元の `YomiCommand` への `YomiResume` は、まだ出していないものとして数える）。
- 表のファイルが変わったとき（`file_watcher`）は、その表を使っているすべての実行へ、同じ手順で設定し直す。
- 台本の差し替え（10.7）とロード（10.8）のあとも、実行が持つ表を設定し直す（言語はセーブに入らない）。停止中の実行は、設定し直したあとの本文が、出していた `YomiCommand` と違っていれば、`YomiCommand` を出し直す（ロードは、設定し直した表で、止まった行を出す）。
- 表の検査（足りない訳・余った訳）は、プラグインは行わない。必要なら、ゲームが `TextTable::check` を呼ぶ。

## 10.11 対象外

- 実行時にプログラムを足すこと、実行中にプログラムを切り替えること。プログラムは、プラグインの組み立てで決め、実行は、作るときに選ぶ。
- 本文・選択肢の表示（UI）の部品。窓つきの例（`examples/dialogue_ui.rs`。機能 `window`）が、`bevy_ui` での表示の一例。
- Web（`wasm32`）での `AssetServer` 経由の読み込みの確認。
