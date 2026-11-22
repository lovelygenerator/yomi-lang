# 2. コマンド登録表と束縛

この章は、ホストが、コマンドを登録する約束と、糖衣を停止型コマンドへ束縛する約束を定める。

## 2.1 登録表

- 台本から呼べるコマンドは、ホストが、**登録表**（`Registry`）に型つきで登録したものだけである（language/02-lines.md の 2.2）。
- コンパイラは、登録表を入力に受け取り、台本の型検査に使う（language/06-diagnostics.md）。VM は、同じ登録表を使って実行する。
- 登録表は、`Vm` を作る前に組み立てる。実行中に、コマンドを足したり消したりしない。

## 2.2 コマンドの名前

- コマンドの名前は、`パッケージ名.コマンド名` の形である（例：`audio.play`）。どちらも識別子である。パッケージ名は、language/01-lexical.md の予約語（`label`・`list` など）にできない（使うと `RegistryError::InvalidName`）。コマンド名の部分は、予約語でもよい（例：`input.int`）。
- 識別子の規則は、台本と同じである（language/01-lexical.md の 1.5）。`XID_Start`（か `_`）で始まり `XID_Continue` が続く、UAX #31 の識別子で、**NFC に直してある**こと。絵文字・記号・空白類を含む名前や、NFC に直っていない名前（例：`か` + 濁点で書いた `が`）は、`RegistryError::InvalidName`（台本の識別子は NFC に直して読むので、NFC でない名前は、台本から引けないため）。コマンド・変数・型・パッケージ・別名・フィールド・バリアントの名前、宣言ファイル（2.8）の名前に、同じ規則を使う。予約語は、`名前空間.名前` の後ろの名前（コマンド・変数・型・別名）に使える。名前空間・パッケージ名・フィールド・バリアントの名前、名前空間のないコマンドの名前には使えない。パッケージに属さない停止型の名前（`reg.stop`）は、`名前空間.名前` か、名前空間のない識別子 1 つ。
- パッケージのコマンドは、`Package::register`（host/03-packages.md）の中で登録する。名前の前半が、そのパッケージの `NAME` と違うと、`RegistryError::InvalidName`。
- 台本からは、`import パッケージ` のあと、副作用型・停止型は `@audio.play("bgm", "x")` のように、問い合わせ型は `math.abs(x)` のように、式の中で書く（language/02-lines.md、language/04-labels-modules.md の 4.2）。
- パッケージに属さない停止型（`reg.stop`。2.4）は、`import` なしで呼べる（`@ui.say_as(...)` のような、糖衣の束縛先）。
- 同じ名前を、二度登録するとエラー（`RegistryError::DuplicateCommand`）。
- コマンドを別のパッケージへ移したときは、旧名の**別名**を登録できる。別名は、同じコマンドを指す。
  - 登録：`reg.effect(…).alias("sound.bgm")`（パッケージの中。直前に登録したコマンドの別名）、または `registry.alias("sound.bgm", "audio.bgm")`。旧名は `名前空間.名前`（識別子）で、別のパッケージの名前空間でもよい。1 つのコマンドに、いくつでも付けられる。
  - 旧名が、別のコマンド・変数・型・別名の名前と重なると `RegistryError::DuplicateCommand`、旧名の形が正しくないと `InvalidName`、指す先が（正式な名前の）コマンドでないと `UnknownCommand`。別名が指せるのは正式な名前だけ（別名の別名はない）。
  - 台本は、旧名で呼べる。コンパイルは通り、警告 `deprecated_command_name`（引数 `name`＝書かれた旧名、`new`＝今の名前）が出る。実行は、今の名前のコマンドと同じ。停止点の位置のハッシュにも、今の名前を使うので、旧名から今の名前へ書き換えても、セーブの位置はずれない。
  - パッケージのコマンドの旧名は、旧名の名前空間を `import` して使う（`import sound`。名前空間は、旧名が登録されていれば、パッケージが残っていなくても `import` できる）。パッケージに属さない停止型の旧名は、`import` なしで呼べる。

## 2.3 コマンドの種類

| 種類 | 呼ぶと | 登録するもの |
| --- | --- | --- |
| 副作用型 | VM が関数を呼ぶ。自分のパッケージの状態を変える。止まらない | 型つきの関数 |
| 問い合わせ型 | VM が関数を呼ぶ。状態を読むだけで、変えない。値を返す | 型つきの関数 |
| 停止型 | VM が止まり、`Yield::Stop` を返す。処理はホスト | 型だけ（関数は持たない） |

- 種類は、登録の呼び出しで決まる（2.4）。台本から、種類を指定する書き方はない。
- 問い合わせ型は、同じ状態なら同じ値を返さなければならない。

## 2.4 登録の書き方

```rust
// パッケージの中（`Package::register`。reg は `&mut PackageReg<Self>`）
// 副作用型：引数の型は、関数のシグネチャから導く
reg.effect("audio.bgm", |ctx: &mut Ctx<Audio>, name: String| {
    ctx.state_mut().bgm = Some(name);
});

// 問い合わせ型：戻り値の型も導く
reg.query("sprite.shown", |ctx: &Ctx<Sprite>, who: String| -> bool {
    ctx.state().shown(&who)
});

// 検証つきの停止型：停止の知らせを返す前に、評価済みの引数を確かめる
reg.stop_checked::<(f64,), (), _>("flow.wait", |(seconds,)| {
    if seconds < 0.0 { Err(CommandError::with_code("flow.negative_seconds", [("seconds", seconds.to_string().into())])) } else { Ok(()) }
});

// 戻り値の検査もつく停止型：ホストが `Input::Return` で返した値が制約を破れば、`resume` が実行時エラーを返す
reg.stop_validated::<(String, String, i64), String, _, _>(
    "input.text",
    |(_, _, max_len)| if *max_len <= 0 { Err(CommandError::with_code("input.max_len_not_positive", [("max_len", (*max_len).into())])) } else { Ok(()) },   // 引数の検査
    |(_, _, max_len), value| if count(value) > *max_len { Err(CommandError::with_code("input.text_too_long", [("max_len", (*max_len).into()), ("len", count(value).into())])) } else { Ok(()) },   // 戻り値の検査
);

// 直前に登録したコマンドへ、説明・引数の名前・旧名を付ける（宣言ファイルに書き出す。2.8）
reg.effect("audio.bgm", |ctx: &mut Ctx<Audio>, name: String| { /* … */ })
    .doc("BGM を再生する")
    .param_names(&["name"])
    .alias("sound.bgm");

// パッケージに属さない停止型：型だけを宣言する（`Registry` に直接）
let mut reg = Registry::new();
reg.stop::<(String,), ()>("ui.say")?;
reg.stop::<(Option<Identity>, String), ()>("ui.say_as")?;
reg.stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")?;

// パッケージの中の停止型（型だけ。処理はホスト）
reg.stop::<(String,), ()>("audio.bgm_file");
```

- コマンドの引数と戻り値の型は、Rust の関数（または型引数）から、自動で導く。シグネチャを別に書かない。引数は 0〜6 個。引数・戻り値に `struct`・`enum`・`map` を使うときは、使う型が登録済みで（2.5）、`map` のキーが language/03-statements.md の 3.1 の規則を満たしていなければならない（登録のエラー `UnknownType`・`InvalidMapKey`・`TypeTooDeep`）。
- `.doc(text)`・`.param_names(&[…])`・`.alias(old)` は、直前に登録したコマンドに付く（なければ `RegistryError::NoCommand`）。`Registry` に直接登録したものには、`registry.doc(名前, text)`・`registry.param_names(コマンド, &[…])`・`registry.alias(旧名, コマンド)` を使う。`param_names` の個数は引数と同じ（違うと `InvalidParams`）。引数の既定値は、`.defaults(&[(名前, 値)])`（`registry.param_defaults(コマンド, &[…])`）で付ける。先に `param_names` で名前を付けておき、値は引数の型に合い、既定値のある引数は末尾に並ばなければならない（違うと `InvalidParams`）。既定値のある引数は、台本の呼び出しで省ける（language/03-statements.md の 3.5）。`doc` は、コマンド・変数・型に付けられる。
- `.text_param(index)`（`Registry` に直接登録したものは `registry.text_param(コマンド, index)`）は、直前に登録した停止型コマンドの `str` の引数 1 つを、差し替え表で訳せる引数にする（入力の促しなど。host/05-texts.md の 5.2a）。コマンドがなければ `UnknownCommand`、`index` が範囲の外か `str` でないと `InvalidParams`。宣言ファイル（2.8）には書き出されない。宣言ファイルから作った登録表では、訳せる引数の印は付かない。
- `Ctx<P>` は、そのコマンドのパッケージ `P` の状態スライスへの入口である（[03-packages.md](03-packages.md)）。副作用型の関数は `&mut Ctx<P>`、問い合わせ型は `&Ctx<P>` を受け取る。
- `ctx.interrupted()` は、VM が見ている中断のトークン（host/01-execution.md の 1.9a）に、中断が求められているかを返す。長い処理は、途中で確かめ、求められていたら `Err(CommandError::interrupted())` を返す。VM は、これを `CommandFailed` ではなく、実行時エラー `Interrupted` として返す。
- 問い合わせ型の戻り値は、`()` にできない（登録が `RegistryError::QueryWithoutValue`）。副作用型が `()` 以外を返すときは、`@` の文が、値を捨てる（台本は、`let`・`=` の右辺に `@` で書いて、値を受けられる。language/02-lines.md の 2.2a）。
- 引数の数と型が、台本の呼び出しに合わないときは、コンパイルエラー（`arg_count`・`type_mismatch`）。種類に合わない使い方（副作用型・停止型を式の中で、問い合わせ型を `@` の文で、戻り値を受けるときの `()` を返すコマンド）は、`invalid_command_use`（language/06-diagnostics.md）。
- 停止型にだけ、行末の安定 ID（`#id`）を付けられる。副作用型・問い合わせ型には付けられない（`invalid_stable_id`）。
- 停止型は、登録のときに、検証の関数を付けられる（`stop_checked`）。VM は、停止の知らせ（`Yield::Stop`）を返す前に、評価済みの引数で呼び、`Err` なら止まらずに実行時エラーにする（`CommandFailed { code, args }`）。
- 停止型は、**戻り値の検査**も付けられる（`stop_validated`。引数の検査と一緒に登録する）。ホストが返した値の型が合ったあと、台本へ渡す前に、止まったときの引数と返された値で呼び、`Err` なら、`resume` が実行時エラー（`CommandFailed { code, args }`。位置は VM が付ける）を返す。そのあと、VM は、実行時エラーのあとの状態になる（`run` を続けられず、スナップショットから `restore` できる）。型が合わない値は、従来どおり `BadInput`（VM は停止点のまま）。
- 登録の失敗（名前の二重・名前の前半の誤りなど）は、`Registry::package`（まとめるときは `Registry::package_set`）が `Err` で返す。そのとき、登録表は変わらない。
- 副作用型・問い合わせ型の関数は、戻り値を `Result<T, CommandError>` にできる（`T` は 2.5 の型）。`Err` を返すと、VM は実行時エラーとして `run` から返す（host/01-execution.md の 1.5）。`Result` でない関数は、失敗しないものとして扱う。
- `CommandError::with_code(コード, [(引数名, 値), …])` は、コード（`パッケージ名.エラー名`。英語の識別子）と、名前つきの引数（`DiagArg`。文字列か整数。小数は文字列にして渡す。`.arg(名前, 値)` で 1 つずつ足せる）を持つ。文面は持たない。表示は、ホスト（CLI は FTL）の責務で、ホストは、コードごとに文面を持つ（知らないコードは、コードと引数をそのまま出す）。位置（モジュール・ラベル・行）は、VM が付ける。実行時エラーの側は、`VmErrorKind::CommandFailed { code, args }`（`command_error()` で取り出せる。VM のコードは `command_failed`）。コア自身が返すもの（パッケージの仕組みの誤り）のコードは、`core.…`（`core_command_error_codes()`）。標準パッケージのコードは、`yomi_std::error_codes()`（spec/packages/*.md の「エラーコード」）。コアの実行時エラーと同じものを返すための `CommandError::integer_overflow()`・`non_finite_float()`・`float_out_of_range()`・`division_by_zero()` もある。中断のための `CommandError::interrupted()` もある（上）。
- 台本の型は、`Result` の有無に影響されない（`Result<T, _>` を返す関数も、台本からは `T` を返すコマンドに見える）。
- 関数が `panic` しないのは、ホストの責任である。コアは、`panic` を捕まえない。

### コアのコマンドエラーコード

コア自身が `CommandFailed` で返すコード（パッケージの仕組みの誤り。ホストの実装の不具合を示す）。`core_command_error_codes()` と一致する。

| コード | 引数 | 意味 |
| --- | --- | --- |
| `core.bad_args` | `name` | コマンド `name` の引数の型が、関数と合わない |
| `core.decl_only_function` | — | 宣言ファイルから作った登録表で、コマンドを実行しようとした |
| `core.dep_is_self` | `package` | パッケージが、自分自身を `dep` で読もうとした |
| `core.dep_not_declared` | `package`, `dep` | `dep` を、`DEPENDS` に宣言していない |
| `core.dep_not_registered` | `dep` | 依存先のパッケージが、登録されていない |
| `core.package_slice_missing` | — | パッケージの状態スライスが見つからない |
| `core.persistent_type_mismatch` | `package` | パッケージの永続スライスの型が合わない |
| `core.state_type_mismatch` | `package` | パッケージの状態の型が合わない |

## 2.5 Rust の型と yomi の型の対応

| yomi | Rust |
| --- | --- |
| `int` | `i64` |
| `float` | `f64` |
| `bool` | `bool` |
| `str` | `String`（所有する UTF-8 文字列） |
| `list<T>` | `Vec<T>` |
| `map<K, V>` | `YomiMap<K, V>`（`Vec<(K, V)>` を持つ、挿入順のマップ。キーの重なりは、戻り値にするときに、あとの値で替える） |
| `Option<T>` | `Option<T>` |
| `Identity` | `Identity` |
| enum / struct | `#[derive(YomiType)]` を付けた Rust の enum / struct |
| 戻り値なし | `()` |

- Rust の enum / struct に `#[derive(YomiType)]` を付けると、yomi の enum / struct として、台本から使える。登録表に `registry.ty::<T>()`（パッケージの中では `reg.ty::<T>()`）で登録する。

  ```rust
  #[derive(YomiType)]
  #[yomi(namespace = "game")]            // 台本での名前は game.Chara
  struct Chara { name: String, hp: i64, tags: Vec<String> }

  #[derive(YomiType)]
  #[yomi(namespace = "game", name = "気分")]   // 台本での名前は game.気分
  enum Mood { Calm, Angry(i64), Mixed(String, Option<i64>) }

  registry.ty::<Chara>()?;
  registry.ty::<Mood>()?;
  ```

  - **名前**：型の名前は `名前空間.型名`。名前空間は `#[yomi(namespace = "…")]` で必ず付ける（`#[yomi(name = "名前空間.型名")]` と、名前に含めてもよい。両方は書けない）。型名は、既定では Rust の型の名前で、`#[yomi(name = "…")]` で変えられる。フィールド・バリアントの名前は、Rust と同じ。属性は、この 2 つだけ（知らない項目・フィールド・バリアントの `#[yomi]` は、`compile_error!`）。
  - **使い方**：台本は、名前空間を `import` して（`import game`）、`game.Chara { name: "案内人", hp: 3, tags: [] }`・`game.Mood.Angry(2)`・`match`・型注釈 `game.Chara` で使う。規則は、台本で宣言した型（language/03-statements.md の 3.7）と同じ。型の診断には、`game.Chara` の形の名前を出す。台本の型は、モジュール付きの名前（`main.Chara`）なので、登録した型と名前がぶつかることはない（名前空間と同じ名前のモジュールは作れない。`duplicate_module`）。
  - **使える形**：名前つきフィールドの struct（フィールドなしの `struct A {}` も可）と、unit バリアント・タプル形式のバリアントの enum。ジェネリクス・ライフタイム・`where`・名前なしのタプル struct・`struct A;`・波括弧形式のバリアント・判別値・バリアントのない enum は、`compile_error!`（日本語）。手書きで実装するときは、`YomiType`・`YomiDecl`（`decl()` が `TypeDecl`）・`IntoValue`・`FromValue` を実装する。
  - **登録の規則**：フィールド・バリアントの型は、この表の型か、**先に登録した**型でなければならない（なければ `UnknownType`）。登録の順が、再帰を作れなくする（自分自身を含むと `RecursiveType`）。`map` のキーの型は、language/03-statements.md の 3.1 の規則に従う（`InvalidMapKey`）。入れ子は 32 まで（`TypeTooDeep`）。フィールド・バリアントの名前は、識別子で、重ならない（`InvalidTypeDecl`）。同じ名前の型・コマンド・変数・別名の二重登録は、`DuplicateType`・`DuplicateCommand`。パッケージの中では、名前空間がパッケージの名前でなければならない（`InvalidName`）。失敗したとき、登録表は変わらない。
  - 登録した型は、コマンドの引数・戻り値・ホスト変数の型（2.7）・問い合わせの戻り値に使える。`let x = @cmd(…)` で、`struct`・`enum` を返すコマンドの戻り値を受けられる。
  - 登録した型は、セーブに入る（宣言の形が `types` に入る。host/04-save.md の 4.3）。宣言が変わると、その型を使う変数は初期値に戻る（4.6 の `changed_types`）。
- `usize` や `i32` など、上の表にない整数型は、受け付けない。

## 2.6 糖衣の束縛

```rust
reg.bind(Sugar::Plain,  "ui.say");      // 素の行  (text: str)
reg.bind(Sugar::Quote,  "ui.say_as");   // 引用行  (identity: Option<Identity>, text: str)
reg.bind(Sugar::Choice, "ui.choose");   // 選択
reg.ignore(Sugar::Choice);                 // 無視（検査だけ行う）
```

- 糖衣の種類は、`Plain`（素の行）・`Quote`（引用行）・`Choice`（選択）の 3 つ（language/05-sugar.md の 5.2）。
- 束縛できるのは、**停止型**のコマンドだけである。副作用型・問い合わせ型を束縛すると、エラー。
- 束縛されたコマンドの引数と戻り値の型は、糖衣の引数の形（language/05-sugar.md の 5.2）に、合っていなければならない。合わなければ、`bind` がエラーを返す。
- `ignore` は、その種類の糖衣を、コンパイルは通すが、コードを生成せず、停止点にもしない（language/05-sugar.md の 5.3）。
- 束縛も無視もされていない糖衣を、台本に書くと、コンパイルエラー。
- 同じ種類を、二度束縛するとエラー。`ignore` も同じ（束縛か無視かのどちらかを、種類ごとに一度だけ指定できる）。
- 素の行・引用行・選択は、引数の形が違うので、それぞれ別の停止型コマンドに束縛する（素の行は `(text: str)`、引用行は `(identity: Option<Identity>, text: str)`、選択は `(options: list<str>, ids: list<Option<str>>) -> int`）。

## 2.7 変数の宣言

ホストは、台本から読み書きできる**変数**を、登録表に宣言できる。

```rust
// パッケージの中（`Package::register`）
reg.var::<i64>("ui.text_speed", Kind::Persistent, 3);
reg.var::<String>("ui.player_name", Kind::Save, String::new());
reg.var::<bool>("game.debug", Kind::Save, false).read_only();
reg.var::<Vec<Chara>>("game.party", Kind::Save, vec![]).doc("パーティ");

// `Registry` に直接（パッケージに属さない）
reg.var::<i64>("game.chapter", Kind::Save, 1)?;
```

- 名前は、`パッケージ名.変数名`（パッケージの中では、前半が `NAME` でなければならない）。コマンドと同じ名前空間の規則に従い、同じ名前の二重登録はエラー（2.2）。変数名の部分は、予約語でもよい。
- 型は、`int`・`float`・`bool`・`str`・`Identity`・`list`・`map`・`Option`・登録した `struct`・`enum`（2.5。先に登録する）と、それらの組み合わせ。`()` を含む型は `RegistryError::UnsupportedVarType`。初期値は、その型の値（`NaN`・`±∞` を含む値は `InvalidVarValue`）。型が使う型の誤りは、2.5 と同じ（`UnknownType`・`InvalidMapKey`・`TypeTooDeep`）。
- 複合型の変数は、台本の変数と同じに扱う（language/03-statements.md の 3.1〜3.4）。`game.leader.hp += 1`・`game.party.push(x)`・`game.flags["a"] = 1` のように、フィールド・添字・メソッドで更新できる。`read_only` の変数は、複合型でも、台本から更新できない。
- 種類（`Kind`）は、`Save`（セーブの中の変数。`save var` と同じ）と、`Persistent`（永続データ。`persistent var` と同じ）。language/03-statements.md の 3.4。
- `read_only()` を付けた変数は、台本から代入できない（コンパイルエラー）。
- 台本からは、`import パッケージ` のあとで、`ui.text_speed` のように、名前を付けて、読み書きできる（language/04-labels-modules.md の 4.6）。パッケージに属さない変数も、名前の前半を `import` する（前半の名前が、モジュールの名前と重なるとエラー）。
- 宣言した変数は、台本で宣言した変数と同じように、セーブ（host/04-save.md の 4.3）・永続データ（4.5）に入る。ロードの互換も同じ（追加は既定値で補う、消えたものは捨てる、型が変わると救済できない。4.6）。
- パッケージの状態スライス（host/03-packages.md）とは別のものである。スライスは、問い合わせ型・副作用型のコマンドを通して読み書きする。変数は、台本から、名前で直接読み書きする。
- ホストは、`vm.get_var("名前")` で読み（`Option<Value>`。登録されていない名前は `None`）、`vm.set_var("名前", 値)` で書ける（`Result`。登録されていない名前は `UnknownVariable`、型が合わないときは `BadInput`。どちらも VM は変わらない）。複合値は、フィールド・引数の名前と型まで調べる。`NaN`・`±∞` を含む値は `NonFiniteFloat`。`read_only` の変数も、ホストからは書ける。
- ロールバック（host/01-execution.md の 1.8）では、`Kind::Save` の変数は戻り、`Kind::Persistent` の変数は戻らない。

## 2.8 宣言ファイル

- `Registry::export_decls()` は、登録表の内容を、**宣言ファイル**（JSON）として書き出す。
- ツール（LSP、`assign-ids`、検査）は、宣言ファイルを読む。ホストの Rust のコードを読み込まなくても、台本を検査できる。`Registry::from_decls(json)` は、宣言ファイルから、関数を持たない登録表を作る（検査のためだけに使える）。
- CLI：`yomi decls`（組み込みの登録表の宣言ファイルを標準出力へ）、`yomi check <台本> [--decls <ファイル>]`（台本を実行せずに検査する。`--decls` があれば、宣言ファイルから作った登録表で検査する）。

### 形式

```json
{
  "format": "yomi-decls",
  "version": 2,
  "packages": [
    { "name": "audio", "version": 1, "depends": [], "std": true }
  ],
  "commands": [
    {
      "name": "audio.bgm",
      "kind": "effect",
      "package": "audio",
      "params": [ { "name": "name", "type": "str" },
                  { "name": "volume", "type": "float", "default": 1.0 } ],
      "returns": null,
      "aliases": ["sound.bgm"],
      "doc": "BGM を再生する"
    }
  ],
  "variables": [
    { "name": "ui.text_speed", "type": "int", "kind": "persistent",
      "default": 3, "read_only": false }
  ],
  "types": [
    { "name": "game.Chara", "kind": "struct",
      "fields": [ { "name": "name", "type": "str" } ] },
    { "name": "game.Mood", "kind": "enum",
      "variants": [ { "name": "Happy", "fields": [] }, { "name": "Angry", "fields": [ { "type": "int" } ] } ] }
  ],
  "sugar": { "plain": "ui.say", "quote": "ui.say_as", "choice": "ui.choose" }
}
```

- `format` は `"yomi-decls"` で固定。`version` は、形式の版数（整数）で、今は 2。項目を足すだけの変更では、上げない。読む側は、知らない項目を無視する。読めるのは 1 と 2（1 は引数の既定値を持たない）で、ほかの版は読まない（`UnsupportedVersion`）。
- `kind`（コマンド）は、`"effect"`・`"query"`・`"stop"`（2.3）。`kind`（変数）は、`"save"`・`"persistent"`（2.7）。`kind`（型）は、`"struct"`・`"enum"`（2.5）。
- `package`（コマンド）は、コマンドを提供するパッケージ（パッケージのコマンドだけ。`import` が要る。副作用型・問い合わせ型には必ずある）。
- `type` は、台本の型の書き方の文字列（`"int"`、`"list<str>"`、`"map<str, int>"`、`"Option<Identity>"`、登録した型の名前 `"game.Chara"`）。
- `returns` は、型の文字列。戻り値なしは `null`。
- `params` の `name` は、登録のときに付けたときだけ入る（なければ省く）。`aliases` は、旧名の並び（名前の昇順。なければ空）。`doc` は、付けたときだけ入る。`doc` は、変数・型にも付けられる。
- `params` の `default` は、引数の既定値（付けたときだけ入る。値の書き方は、変数の `default` と同じ）。`default` を持つ引数は `name` を持ち、末尾に並ぶ（違うと `invalid_value`）。
- `enum` の `variants[].fields` は、引数の型の並び（`{ "type": … }`。名前はない）。
- `default`（変数）は、変数の型に合う JSON の値：`int`・`float` は数（`int` は整数の書き方だけ。`float` は整数の書き方も可）、`bool`、`str`、`Identity` は `{"id", "name"}`、`list` は配列、`map` は `[キー, 値]` の組の配列（キーは重ならない）、`Option` は `null`（`None`）か `{"some": 値}`、`struct` はフィールド名をキーにしたオブジェクト、`enum` は `{"variant": 名前, "args": [値, …]}`。
- `sugar` の値は、束縛した停止型コマンドの名前。`reg.ignore` したものは `null`。束縛も無視もないものは、省く。
- 配列の並びは、名前の昇順にする（`packages`・`commands`・`variables`・`types`）。出力が、同じ登録表なら、同じになる（差分が見やすい）。書き出しは、2 文字の字下げで整形し、オブジェクトのキーは名前の順に並べる（上の例は、項目の意味を示すもので、並びと改行の位置は実際の出力と違う）。

### 読み込み（`from_decls`）

- 作った登録表は、台本のコンパイル（検査・LSP・`assign-ids`）にだけ使える。`Registry::is_decl_only()` が真で、コマンドを実行する関数を持たない。`Vm::new` は `VmErrorKind::DeclOnlyRegistry`、`Vm::load` は `LoadErrorKind::DeclOnlyRegistry` を返す。
- 書き出した宣言から作った登録表で、同じ台本は、同じ診断（コード・位置・引数）になる。`export_decls` → `from_decls` → `export_decls` は、同じ文字列になる。
- 型は、使う型を先に登録する順に並べ替えて登録する（互いに使い合う型は `RecursiveType`）。名前の重なり・型の誤りなどの登録のエラーは、`DeclErrorKind::Registry`。
- 壊れた入力でも、パニックしない。誤り（`DeclError`）は、種類（`DeclErrorKind::code()`：`syntax`・`too_large`・`too_deep`・`not_decls`・`unsupported_version`・`missing_field`・`wrong_type`・`invalid_type`・`invalid_value`・`registry`）と、場所（`commands[3].params[0].type` の形）を持つ。入力の大きさは 16 MiB まで、JSON の入れ子は 128 まで、型の名前の入れ子は 34 までで、超えると `too_large`・`too_deep`。
- 同じオブジェクトに同じキーが重なれば、後に書いたものを使う。`f64` で表せない大きさの数は、`syntax` になる。
- `packages` の項目の `std`（標準パッケージか。`Package::STD`）は、標準パッケージのときだけ `true` で書き出し、なければ `false` として読む。`packages` の項目の `std` が真偽値でなければ `wrong_type`。
- `variables` の必須項目は `name`・`type`・`kind`・`default`。配列の項目（`packages`・`commands`・`variables`・`types`・`sugar`）は、なければ空として読む。
