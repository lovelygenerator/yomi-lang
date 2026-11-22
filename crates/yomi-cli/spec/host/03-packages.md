# 3. パッケージと状態

この章は、パッケージの形と、パッケージの状態（状態スライス）の約束を定める。

## 3.1 パッケージ

パッケージは、コマンドと、それらが使う状態を、まとめて提供する単位である。

```rust
trait Package: Sized + 'static {
    const NAME: &'static str;
    const VERSION: u32 = 1;                          // 状態スライスの版数
    const DEPENDS: &'static [&'static str] = &[];    // 読む相手のパッケージ名（なければ空）
    const STD: bool = false;                         // 標準パッケージか（台本では `import std::名前`）
    type State: Clone + Serialize + DeserializeOwned + Default + Send + Sync + 'static;   // なければ ()
    const PERSISTENT_VERSION: u32 = 1;               // 永続スライスの版数
    const RULES_VERSION: u32 = 1;                    // コマンドの結果を決める規則の版
    type Persistent: Clone + Serialize + DeserializeOwned + Default + Send + Sync + 'static;  // なければ ()

    fn register(reg: &mut PackageReg<'_, Self>);     // コマンドと変数の登録
    fn clear_cues(state: &mut Self::State) {}        // 状態の中の合図を空にする（既定は何もしない。3.6）
    fn cue_len(state: &Self::State) -> usize { 0 }   // 状態の中の合図の数（既定は 0。書いた順の表に使う。3.6）
    fn migrate(old_version: u32, raw: &[u8]) -> Result<Self::State, MigrateError> { … }              // 既定は Err
    fn migrate_persistent(old_version: u32, raw: &[u8]) -> Result<Self::Persistent, MigrateError> { … } // 既定は Err
}

registry.package::<Audio>()?;   // 1 つを登録（失敗したら、登録表は変わらない）
registry.package_set(&packages![Audio, Image, Input])?;   // 順を問わずにまとめて登録（3.1 の最後の項目）
```

- `NAME` は、コマンド名の前半（`audio.bgm` の `audio`）と、状態スライスのキーになる。
- `VERSION` は、状態スライスの版数。状態の形を変えたら、上げる。
- `Persistent` は、セーブとは別に、全セーブで共有する、パッケージの状態（3.5）。持たないパッケージは、`()` にする。`migrate_persistent` と `PERSISTENT_VERSION` は、`migrate` と `VERSION` と同じ役割。
- `RULES_VERSION` は、コマンドの結果を決める規則（たとえば、文字の数え方）の版。状態の形とは別に、同じ引数に対する結果が変わる直し方をしたら、上げる。状態を持たないパッケージでも使う。セーブに記録し、ロードで違えば `LoadReport::changed_rules` に出す（host/04-save.md の 4.6。`migrate` は呼ばない）。
- `STD` は、標準パッケージか（既定は `false`）。`true` のパッケージは、台本では `import std::名前` と書く（`std` はモジュールの名前に使えない。language/04-labels-modules.md の 4.1・4.2）。ホストが作るパッケージは、`false` のまま、`import 名前` と書く。宣言ファイル（host/02-registry.md の 2.8）には、`true` のものだけ `"std": true` を書く。
- `DEPENDS` は、このパッケージのコマンドが、状態を**読む**相手のパッケージの名前（3.3）。
- `register` は、コマンドと変数を登録する（host/02-registry.md）。コマンド・変数の名前の前半は、`NAME` でなければならない（違うと `RegistryError::InvalidName`）。`reg` は `PackageReg<Self>` で、`effect`・`query`・`stop`・`stop_checked`・`stop_validated`・`var`・`ty`（型の登録。名前空間は `NAME`。host/02-registry.md の 2.5）を持つ。直前に登録したものへ、`doc`・`param_names`・`alias` を付けられる（2.4）。
- `migrate` は、旧い版数のセーブから読んだバイト列を、今の `State` に直す。版数が同じときは呼ばれない。既定の実装は `Err` を返す（版数を上げたら、書く）。`Err` なら、そのスライスは復元できず、ロードが `LoadError`（`PackageSlice`）になる（host/04-save.md の 4.6）。
- `MigrateError::new(メッセージ)`・`MigrateError::unsupported_version(版数)` で作る。バイト列は、`decode_slice::<T>(raw)`（`postcard`）で読む。パッケージが `postcard` に依存しなくてよいように、コアが `decode_slice`・`encode_slice` を公開する。
- `State`・`Persistent` が `()` のスライスは、セーブ・永続データに入れない。
- `clear_cues` は、状態の中の合図（3.6）を空にする。コアは、合図の中身を知らない。既定は何もしない（合図を持たないパッケージは、書かなくてよい）。
- `cue_len` は、状態の中の合図の数を返す（既定は 0）。VM は、コマンドの実行の前後でこの数を比べ、増えた分を、書いた順の表（3.6）に記録する。合図を持つパッケージは、`cues` の長さを返す。
- パッケージを登録するときの検査（`Registry::package`）：同じ名前の二重登録は `DuplicatePackage`、`DEPENDS` の相手が先に登録されていなければ `MissingDependency`、自分自身への依存は `DependencyCycle`。`package` は、依存先を先に登録する順に限る。失敗したとき、登録表は変わらない。
- **一括登録**（`Registry::package_set`。`packages![A, B, C]` が、型を消した項目の列を作る）：順を問わずに、まとめて登録する。`DEPENDS` を見て、依存先を先にして並べ替える（依存に関係のないものは、渡した順を保つ）。依存先は、同じ一括の中か、すでに登録済みのもの。依存先がどこにもなければ `MissingDependency`、依存が輪になれば（自分自身を含む）`DependencyCycle`、名前の重複は `DuplicatePackage`、名前・コマンドの名前の誤りは `InvalidName` など、`package` と同じエラー。失敗したとき、登録表は、一括の前から変わらない（全部登録するか、何もしないか）。`package::<P>()` は、1 つだけの一括と同じである。

## 3.2 状態スライス

- パッケージの状態は、パッケージごとに 1 つの**状態スライス**（`State`）に持つ。
- 状態スライスは、コアから見ると、「パッケージ名 → （版数、直列化したバイト列）」の汎用の入れ物である。コアは、中身を解釈しない。
- 中身の直列化は、`serde` と `postcard` で行う。パッケージは、`State` に `Serialize` と `Deserialize` を実装する。
- ロードのとき（host/04-save.md の 4.6）：
  - セーブにあって、登録されていないパッケージのスライスは、無視する（`LoadReport::ignored_slices`）。
  - 登録されていて、セーブにないパッケージのスライスは、`State::default()` で補う（`defaulted_slices`）。
  - 版数がセーブと今で違うときは、`migrate` を呼ぶ（`migrated_slices`）。
- 状態は、台本の変数とは別のものである。台本は、問い合わせ型コマンドを通してだけ、状態を読む（language/02-lines.md の 2.2）。

## 3.3 コマンドから見える状態

```rust
reg.effect("audio.bgm", |ctx: &mut Ctx<Audio>, name: String| { ctx.state_mut().bgm = Some(name); });
reg.query("audio.playing", |ctx: &Ctx<Audio>| -> bool { ctx.state().bgm.is_some() });
```

- コマンドの関数は、**自分のパッケージの状態スライス**を、`Ctx<P>` を通して受け取る。副作用型は `&mut Ctx<P>`（`state()`・`state_mut()`）、問い合わせ型は `&Ctx<P>`（`state()`）。書けるのは、自分のスライスだけである。
- ほかのパッケージの状態は、`DEPENDS` に宣言した相手のものだけ、**読める**。`ctx.dep::<Q>()` が、相手の状態への `&` を `Result<&Q::State, CommandError>` で返す。`Q` が `DEPENDS` にないとき（宣言のし忘れ）は、`Err`（実行時エラー）になる。
- 依存の宣言は、パッケージの層を表す。**循環する依存は、作れない**（登録のとき、輪を `DependencyCycle` で断る。3.1）。
- 宣言した相手が、登録済みでも、同じ一括の中にもないときは、登録のエラー（`MissingDependency`）。
- 複数のパッケージにまたがるコマンドは、両方に依存する上位のパッケージに置く。上位のパッケージは、自分のスライスに書き、下位のパッケージのスライスは読むだけである。
- `Ctx<P>` の中身は、状態への参照（自分のスライス、依存先のスライス、永続スライス）と、乱数（下）だけである。時間・ファイルは持たない。
- **乱数**：副作用型の関数は、`ctx.rng()` で、コアの PRNG（`Rng`。`range_inclusive(min, max)` など）への `&mut` を借りる。台本の `random`（language/03-statements.md の 3.8）と、同じ PRNG である。問い合わせ型の関数は、`&Ctx` しか受け取らないので、`ctx.rng()` を使えない（コンパイルエラー）。`min > max` の範囲を渡したときは、`min` を返す。
- PRNG の状態は、セーブに含める（host/04-save.md の 4.3）。ロールバックで、元に戻る。同じ状態から、同じ順に使えば、同じ値が出る。

## 3.4 スナップショットと状態

- 停止点のスナップショットは、VM の状態と、全パッケージの状態スライスを含む（host/01-execution.md の 1.8）。
- ロールバックのために、スナップショットを履歴に残す。状態スライスは、`State: Clone` の `clone()` で複製する。
- 複製が重い状態（大きな表、画像の情報など）を持つパッケージは、`State` の中で、内容を共有する持ち方（`Rc` など）を使って、`clone()` を軽くする。
- 共有を使った `State` も、直列化（セーブ）は、共有を展開した値として行う。

## 3.5 永続スライス

- パッケージは、`State`（セーブごと）のほかに、`Persistent`（全セーブで共有）を持てる（例：音量の設定）。
- 永続スライスは、永続データ（host/04-save.md の 4.5）に、パッケージ名をキーに、版数と、直列化したバイト列で入る。ロードの規則は、状態スライスと同じ（欠けたら既定値、余分は無視、版数が違えば `migrate_persistent`）。
- コマンドの関数は、`ctx.persistent()`（読む。問い合わせ型でも使える）と `ctx.persistent_mut()`（書く。副作用型だけ）で、**自分のパッケージの**永続スライスを使える。依存先の永続スライスは、読めない。
- 永続スライスは、スナップショットに含めず、ロールバックでも、元に戻らない。
- 永続スライスを書いたことは、コアからホストへ知らせない。ホストは、停止点ごとに `vm.save_persistent()` を呼ぶなど、保存の時機を自分で決める（host/04-save.md の 4.5）。

## 3.6 ホストが状態を読む

- ホストは、停止点（`Yield::Stop`）で、パッケージの状態を読んで、画面などに反映する。入口は、`vm.package_state::<P>()`（`Option<&P::State>`）と、`vm.package_persistent::<P>()`（`Option<&P::Persistent>`）。`P` が登録されていなければ `None`。
- 一度きりの出来事は、状態の中の「合図」の列（`cues`）に入る。合図は、直前の `resume` の後に実行したコマンドが足したものだけを持つ。
- 空にするのは、VM である。`Package::clear_cues`（3.1）を、**全パッケージについて**、次の場面で呼ぶ。
  - `run`・`resume` の始め（実行を再開する前。`resume` は、入力を検査して受け入れたあと。受け入れられなかった `resume` は、何も変えない）
  - ロード（`Vm::load`）・ロールバック（`Vm::restore`）の直後（停止点の引数を作り直す前）
- 合図は、状態の一部である。セーブにもスナップショットにも、ほかの状態と同じように入る（特別扱いしない）。ただし、ロード・ロールバックの直後に空になるので、古い合図は、再生されない。合図でない部分（表示中のもの・鳴らしているもの）は、戻る。
- 合図は、パッケージごとの `cues` に入るが、パッケージをまたいだ順（台本で書いた順）は、`vm.cue_order()` で分かる。`CueRef { package, index }`（`package` は `Package::NAME`、`index` は、そのパッケージの `cues` の位置）の列を、足された順に返す。表は、`clear_cues` と同じ場面で空になり、セーブ・スナップショットには入らない。コマンドが足した合図だけを記録する（途中の合図を消すパッケージには向かない）。
- ホストは、停止点（`Yield::Stop`、または `Yield::End`）の直後に、`vm.package_state::<P>()` で、状態と合図を読む。次の `resume` の前に、読み終える（`resume` が空にする）。
- ホストは、状態を書き換えない（読むだけ）。書くのは、コマンドだけである。

## 3.7 標準パッケージの状態と合図

- `audio`・`image`（yomi-std）が、合図を使う（packages/audio.md、packages/image.md）。状態スライスに `cues` を持ち、`clear_cues` で空にする。永続スライスは持たない。
- `audio`・`image`・`screen` は、`cue_len` を実装する（書いた順の表に出る）。
- 合図の列の形（型）は、パッケージごとに決める。コアは、型を知らない（型を失わないため）。
- `input` は、状態を持たない。入力の値は、停止型コマンドの戻り値で受ける（packages/input.md、host/02-registry.md の `stop_validated`）。
