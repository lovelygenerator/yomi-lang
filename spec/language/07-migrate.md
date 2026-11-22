# 7. セーブの移行（migrate）

台本を更新したあとも、古いセーブを読めるようにする仕組みを定める。変数の名前の変更、型の変更、値の意味の変更を、台本の外の宣言ではなく、**凍結したスキーマ**と、手で書く **`migrate`** で表す。ロードでの適用の順序と `LoadPolicy` は、host/04-save.md の 4.6 で定める。

## 7.1 全体像

- **スキーマの版**：プロジェクト全体で 1 つの連番（`u32`）。台本の現在の版は、`migrations` モジュール（7.4）の `migrate N` のうち最大の `N`（なければ 1）。セーブと永続データは、保存したときの版を持つ（host/04-save.md の 4.3）。
- **凍結スキーマ**（`schema/vN.yomi`。7.3）：リリースした版の、`save var`・`persistent var` と、そこから到達できる `struct`・`enum` の宣言。凍結のあとは変えない。
- **`migrations` モジュール**（`migrations.yomi`。7.4）：`min_save_version`（サポートの下限）と、版ごとの `migrate N { … }`（`rename`・`convert`・`drop`）。手で書く。
- 版 `N-1` から版 `N` へ移すとき、旧い側は凍結スキーマ `vN-1`、新しい側は凍結スキーマ `vN`（`N` が最新の未リリースの版のときは、現在の台本）として、`migrate N` を型検査する。

## 7.2 版の名前空間と書き方

- 凍結スキーマの名前は、版の名前空間 `vN` の下に置く。`v2::game.favor`（変数）、`v2::game.Item`（型）、`v2::story::ch1.seen`（モジュールのパスは `::`、最後の名前は `.`。4.1・4.6）。
- 接頭辞のない名前は、現在の台本を指す。
- `vN::` の名前は、`migrations` モジュールの中でだけ書ける。ほかのモジュールで書くとエラー（`schema_name_outside_migrations`。引数 `name`）。
- `migrate N` の中で書ける接頭辞は、旧い側の `v{N-1}::` と、新しい側の `v{N}::`（`N` が最新の未リリースの版のときは、接頭辞なし）だけである。

## 7.3 凍結スキーマ

```ebnf
schema_file   = schema_header { schema_module } ;
schema_header = "schema" integer "hash" string ;
schema_module = "module" module_path "{" { schema_item } "}" ;
schema_item   = struct_decl | enum_decl | schema_var ;
schema_var    = ( "save" | "persistent" ) "var" identifier ":" type ;
```

```
schema 2 hash "0123456789abcdef"

module game {
    struct Item { id: str, count: int }
    enum Mood { Happy, Sad(int) }
    save var bag: list<game.Item>
    save var favor: int
}
module story::ch1 {
    persistent var seen_intro: bool
}
```

- **置き場所**：コンパイルの入力の、ルートの `schema/` ディレクトリの下の `vN.yomi`（`N` は 1 以上の整数。先頭に 0 を付けない）。`schema` は、モジュールの名前（パスの最初の語）に使えない（`invalid_module_name`）。ホストは、ほかのモジュールと同じように、これらのファイルを渡す。VM は、凍結スキーマを実行しない。
- **ファイル名と版**：ヘッダの `integer` は、ファイル名の `N` と同じでなければならない。違う、ヘッダが先頭の行にない、`schema/` の下のファイル名が `vN.yomi` の形でない、のいずれも、エラー（`invalid_schema_file`。引数 `path`）。
- **内容**：`module` のブロックが、台本のモジュールを、パスのまま再現する。変数の宣言には、初期値を書かない。型の名前は、常に `モジュール名.型名`（`struct`・`enum` を含むモジュールの名前を付ける）で書く。ホストが登録した型（host/02-registry.md の 2.5）は、パッケージ名のモジュールのブロックに、`struct`・`enum` の宣言として再現する。ホストが宣言した変数は含めない。
- **並び**（`yomi schema freeze` が出力する並び）：モジュールは、パスの昇順（コードポイントの順）。各モジュールの中は、`struct`・`enum` を名前の昇順、そのあとに変数を名前の昇順。字下げは 4 つの半角スペース。行は LF で終える。
- **ハッシュ**：ヘッダの `hash` は、ヘッダの行（改行を含む）を除いた残り全体の、FNV-1a 64 ビット（host/04-save.md の 4.4 と同じ初期値・乗数。バイトは UTF-8、改行は LF にそろえてから混ぜる）を、小文字の 16 桁の 16 進数にしたもの（4.4 の「部分のあとの `0xFF`」は混ぜない）。`yomi check` が、ファイルの内容から計算した値と比べる。違えば、エラー（`schema_hash_mismatch`。引数 `version`）。
- 凍結スキーマの中の名前は、同じファイルの中だけで解決する。宣言の文法と型の規則は、台本と同じ（3.1・3.7）。
- 凍結スキーマは、`yomi schema freeze` が作る。手で直さない（直すと、ハッシュが合わない）。

## 7.4 `migrations` モジュール

- ルートの `migrations.yomi`。プロジェクトに、1 つだけ置ける。ホストは、ほかのモジュールと同じように渡す。`import` も設定もなしで、使われる。
- `migrations`、および `v` に数字が続く名前（`v2` など）は、台本のモジュールの名前（パスの最初の語）に使えない（`invalid_module_name`）。
- このモジュールには、ラベル・変数・型・コマンドを書けない（`unexpected_token`）。書けるのは、次のものだけである。

```ebnf
migrations_file = [ min_decl ] { migrate_block } ;
min_decl        = "min_save_version" integer ;
migrate_block   = "migrate" integer "{" { migrate_stmt } "}" ;
migrate_stmt    = rename_stmt | convert_stmt | drop_stmt ;
rename_stmt     = "rename" var_ref "->" var_ref ;
convert_stmt    = "convert" var_ref ":" type "->" [ var_ref ":" ] type "=" lambda ;
drop_stmt       = "drop" var_ref ;
var_ref         = [ version_prefix ] [ module_path "." ] identifier ;   (* module_path は `::` 区切り *)
version_prefix  = "v" digits "::" ;
```

- `migrate`・`rename`・`convert`・`drop`・`min_save_version` は、このモジュールの中だけで意味を持つ語である。ほかのモジュールの識別子には、何の制限もない。
- 文は、行で区切る。`convert` は、ラムダ式の終わりまでが 1 文である。`=` のあとがラムダ式でなければ、構文エラーである（`unexpected_token`）。

```
min_save_version 2

migrate 3 {
    rename v2::game.favor -> game.affection
    convert v2::game.hp: int -> float = |v| to_float(v)
    convert v2::game.score: int -> game.score: int = |v| v * 10
    drop v2::game.old_flag
}
```

### 文の意味

- `rename 旧 -> 新`：変数の名前（モジュールの移動を含む）を変える。値と型は変えない。旧い変数と新しい変数の型は、同じでなければならない。種類（`save var` か `persistent var` か）も同じでなければならない。
- `convert 旧: 旧型 -> 新: 新型 = ラムダ式`：値を変換する。名前が同じ（同じモジュールの同じ名前）なら、`新:` を省略できる。名前を変えながら変換するときも、この文 1 つで書く（`rename` と併用しない）。
- `drop 旧`：変数を捨てる。
- 1 つの `migrate N` の中の文は、旧い側の値を基準に、互いに独立して扱う（書く順序で結果は変わらない）。同じ旧い変数を 2 回扱う、同じ新しい変数を 2 回作る、はエラー（`duplicate_migrate_target`。引数 `name`）。
- 変数の名前は、モジュールのパスを付けて書く（`main.hp`）。モジュールのパスのない名前、接頭辞が合わない名前（旧い側で `v{N-1}::` がない、新しい側が凍結スキーマ `vN` のときに `vN::` がない、など）は、その側のスキーマにない名前として扱う。
- 旧い名前は、`v{N-1}` の凍結スキーマになければならない。なければ、エラー（`unknown_schema_name`。引数 `name`、`version`。近い名前があれば `suggestions`）。新しい名前も、新しい側のスキーマになければならない。
- `convert` に書く型は、スキーマの型と同じでなければならない（旧型は旧い側、新型は新しい側）。違えば、エラー（`migrate_type_mismatch`。引数 `name`、`expected`、`found`）。`rename` の旧い変数と新しい変数の型が違うときも、同じ。種類が違うときは、エラー（`migrate_kind_mismatch`。引数 `name`）。
- 型は、`int`・`float`・`bool`・`str`・`Option`・`list`・`map`・`struct`・`enum`（入れ子を含む）を扱える。

### 変換の式

- `convert` の式は、**ラムダ式**で、引数を 1 つ（旧い値）取り、新しい型の値を返す。引数の型は、旧型のとおりで、書かなくてよい。
- 本体の名前は、新しい側のスキーマの型を指す。旧い型の名前を書くときは、`v{N-1}::` を付ける。
- 使えるのは、引数、リテラル、演算、組み込みのメソッド（`list`・`map` の高階メソッドを含む）、`struct`・`enum` の構築、ラムダ式である。
- 台本の `fn`・モジュール変数・ホストが宣言した変数・ホストのコマンド・乱数・停止するものは、使えない（`migrate_impure_expr`。引数 `what`）。移行の結果が、台本の後の変更や、そのときの実行の状態に左右されないようにするためである。共通の変換は、`migrate` ごとにラムダ式を書く。
- 実行時エラー（0 による割り算・整数の桁あふれなど）は、ロードの失敗または初期値への補い（host/04-save.md の 4.6）になる。

```
migrate 4 {
    convert v3::game.bag: list<v3::game.Item> -> list<game.Item> =
        |items| items.map(|it| game.Item { id: it.id, count: it.count, rarity: 0 })
}
```

### 版の規則

- `migrate N` の `N` は、2 以上の整数。重なると、エラー（`duplicate_migrate`。引数 `version`）。1 以下は、エラー（`invalid_migrate_version`。引数 `version`）。書く順序は問わない。
- `min_save_version N`：サポートする最も古い版。モジュールの先頭（コメントを除く）に、1 回だけ書ける。書かなければ 1。`N` は、1 以上、台本の現在の版以下でなければならない（`invalid_min_save_version`。引数 `version`）。保存した版が `N` より古いセーブは、ロードできない（host/04-save.md の 4.6）。
- 版 `K`（`min_save_version` ≦ `K` ≦ 台本の現在の版）の凍結スキーマ `schema/vK.yomi` が欠けるときは、最新の版を除いて、エラー（`missing_frozen_schema`。引数 `version`）。最新の版（台本の現在の版）の凍結スキーマは、リリースのときに作るので、なくてよい。
- `migrate K`（`min_save_version` < `K` ≦ 台本の現在の版）が欠けるときは、エラー（`missing_migrate`。引数 `version`）。変更のない版も、空のブロック（`migrate 5 {}`）を書く。台本の現在の版より大きい凍結スキーマ `vK.yomi` があるときも、同じ。
- `min_save_version` より古い版の凍結スキーマと `migrate` は、削除してよい。

## 7.5 検査と警告

- 次の場合に、警告を出す。
  - 版 `N-1` から版 `N` の間で、型が変わった変数があるのに、`migrate N` が、その変数を `convert`・`rename`・`drop` していない（`unhandled_type_change`。引数 `name`、`version`）。`Strict` では、この変数を含むセーブを読めない。
  - 最新の凍結スキーマと、現在の台本のスキーマに差があるのに、それより新しい `migrate` がない（`schema_changed_since_freeze`。引数 `version`）。診断の位置は、最新の凍結スキーマのファイルの 1 行目である。
- 追加された変数・消えた変数は、`migrate` がなくても、警告にならない（追加は初期値になり、消えたものは捨てる。host/04-save.md の 4.6）。

## 7.5a ホスト（Rust）への渡し方

`migrations.yomi` と凍結スキーマは、台本のモジュールとは別に、コンパイルの設定（`CompileOptions`）で渡す。`compile_modules` などの、モジュールの組の引数は、変えない。

- `CompileOptions::migrations(source: &str)`：`migrations.yomi` の内容。1 回だけ渡せる（なければ、`migrate` のないプロジェクト）。
- `CompileOptions::frozen_schema_file(path: &str, source: &str)`：凍結スキーマの内容。`path` は、探すディレクトリからの相対パスで、`schema/v2.yomi` の形（区切りは `/`）。何回でも渡せる。ファイル名の `vN` から版 `N` を読み、ヘッダの版と合わなければ、`invalid_schema_file`（7.3）。
- 診断の位置のモジュール名は、`migrations` と、凍結スキーマの `path`（拡張子を除く。`schema/v2`）。
- `Program::schema_version() -> u32`、`Program::min_save_version() -> u32`：台本の現在の版と、サポートの下限（host/04-save.md の 4.3・4.6）。
- `yomi` の CLI は、探すディレクトリの `migrations.yomi` と `schema/` の下の `.yomi` を、これらの設定に詰める（language/04-labels-modules.md の 4.1）。

## 7.6 診断のコード

| コード | 引数 | 重大度 | 意味 |
| --- | --- | --- | --- |
| `schema_name_outside_migrations` | `name` | エラー | `migrations` モジュールの外で、`vN::` の名前を書いた |
| `invalid_schema_file` | `path` | エラー | 凍結スキーマのファイル名・ヘッダが正しくない |
| `schema_hash_mismatch` | `version` | エラー | 凍結スキーマの内容が、記録したハッシュと合わない |
| `duplicate_migrate` | `version` | エラー | 同じ版の `migrate` が複数ある |
| `invalid_migrate_version` | `version` | エラー | `migrate` の版が 2 未満 |
| `invalid_min_save_version` | `version` | エラー | `min_save_version` が正しくない（範囲外・2 回目・先頭でない） |
| `unknown_schema_name` | `name`、`version` | エラー | 旧い側（または新しい側）のスキーマにない名前 |
| `migrate_kind_mismatch` | `name` | エラー | `rename` で、`save var` と `persistent var` をまたいだ |
| `migrate_type_mismatch` | `name`、`expected`、`found` | エラー | 書いた型が、スキーマの型と違う（`rename` では、旧い変数と新しい変数の型が違う） |
| `duplicate_migrate_target` | `name` | エラー | 同じ変数を、1 つの `migrate` の中で 2 回扱った |
| `migrate_impure_expr` | `what` | エラー | 変換の式に、使えないもの（`fn`・変数・コマンド・乱数・停止）を書いた |
| `missing_frozen_schema` | `version` | エラー | 必要な版の凍結スキーマがない |
| `missing_migrate` | `version` | エラー | 必要な版の `migrate` がない |
| `unhandled_type_change` | `name`、`version` | 警告 | 型が変わった変数を、`migrate` が扱っていない |
| `schema_changed_since_freeze` | `version` | 警告 | 凍結したあとにスキーマが変わったが、`migrate` がない |

- 変換の式の型が合わないときは、型の不一致の診断（6.3）である（引数は、期待した型と見つかった型）。
- 凍結スキーマの文法の誤りは、`unexpected_token` などの、一般の構文の診断である。
