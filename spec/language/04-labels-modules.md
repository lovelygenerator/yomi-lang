# 4. ラベルとモジュール

## 4.1 モジュール

- 1 つのファイル（`.yomi`）が、1 つのモジュールである（1 章）。
- モジュールの名前（パス）は、探すディレクトリからの相対パスの、拡張子（`.yomi`）を除いたもので、区切りを `::` にする。`story/ch1.yomi` は `story::ch1`、ルート直下の `main.yomi` は `main`。パスの各語は、識別子の規則（1.5）に従う（日本語も使える）。識別子にならない語、予約語（`label` `float` など）と同じ語は、エラー（`invalid_module_name`。ディレクトリの名前が識別子でなければ、その下のファイルがこのエラーになる）。名前は、NFC に直して扱う（ファイルシステムによって、ファイル名の正規化の形が違うため。1.5）。
- 同じ名前のモジュールが複数あれば、エラー（`duplicate_module`）。ディレクトリが違えば、ファイル名が同じでも、別のモジュールである（`a/util.yomi` は `a::util`、`b/util.yomi` は `b::util`）。
- セーブ・差し替え表のキー・`seen` と `visited` の文字列・開始位置（4.7）には、パス全体を使う（`story::ch1.start`）。
- モジュールの最上位には、次のものを書ける。

```ebnf
module = { top_item } ;
top_item = import_decl | identity_decl | save_decl | persistent_decl
         | enum_decl | struct_decl | fn_decl | label_decl ;
```

- `import` は、ほかの宣言より前に書く。後ろに書くとエラー（`import_not_first`）。

## 4.2 import

```ebnf
import_decl = "import" import_tree ;
import_tree = import_path [ "as" identifier ]
            | [ import_path "::" ] "{" import_tree { "," import_tree } [ "," ] "}" ;
import_path = identifier { "::" identifier } ;
```

- `import story::ch1` のように、ほかのモジュールを参照する。標準パッケージ（`yomi-std` の `audio` `flow` `image` `input` `math` `screen` `text`）は、`import std::audio` のように、名前空間 `std` を付けて `import` する。ホストが作るパッケージは、`import ui` のように、名前だけで `import` する。
- `{ }` で、複数の名前を 1 行にまとめて書ける。パスの前半を共通にでき、入れ子にでき、末尾のカンマを許す（Rust の `use` と同じ形）。1 行に収める（複数行には分けられない）。`import story::{ch1, part::{a, b}}` は、`import story::ch1`・`import story::part::a`・`import story::part::b` と同じ。`{ }` が空のとき、`{ }` の入れ子が深すぎる（16 段を超える）ときは、構文エラー。
- `import` したモジュールは、パスの最後の 1 語で使う（`import story::ch1` のあとは `jump ch1.start`、`ch1.favor`）。`as` で別名を付けると、別名で使う（`import story::ch1 as c` のあとは `c.start`）。自分のモジュールの名前も、パスの最後の 1 語である（`story::ch1` の中では `ch1.n`）。
- 別名は、`a::util` と `b::util` のように、最後の 1 語が同じモジュールを両方 `import` するときに使える。パッケージ（標準パッケージを含む）には別名を付けられない（`package_alias`）。標準パッケージは、`import std::audio` のあと、`audio.play(…)` と、名前だけで使う。
- パッケージとモジュールの名前（パス）が同じになったら、エラー（`duplicate_module`）。ホストが宣言した変数・登録した型・コマンドの旧名（別名）の名前の前半（`ui.text_speed` の `ui`、`game.Chara` の `game`）も、パッケージと同じに、`import` する名前になる。
- `import` したものは、`@audio.play("bgm", "x")`、`math.abs(x)`、`chapter2.start` のように、名前を付けて使う。
- `import` していないモジュールのラベル・変数・型は、使えない（`module_not_imported`。引数 `name` は、台本に書いた最後の 1 語）。存在しないモジュールの `import` は `unknown_module`（引数 `name` は `import` に書いたパス、近いパスがあれば `suggestions`）。
- 自分のモジュールの名前を付けた参照（`main.score`、`jump main.back`）は、`import` なしで書ける。
- `import` で使う名前（最後の 1 語か別名）が、すでに `import` した名前と重なると、エラー（`duplicate_import`。`as` で別名を付けられる）。自分のモジュールの名前と重なる（自分自身の `import` を除く）ときは、エラー（`duplicate_name`）。
- `import` したのに、台本のどこでも使っていない名前は、警告（`unused_import`。引数 `name` は `import` に書いたパス）。エラーがあるときは、使用の記録が途中で切れるので、出さない。
- `import` で使う名前と同じ名前の変数・ラベル・引数・`let` は、エラー（`duplicate_name`）。パッケージの名前と同じ名前でも、同じ（`import` の有無によらない）。
- `std` は、標準パッケージの名前空間なので、モジュールの名前（パスの最初の語）に使えない（`invalid_module_name`。`std/` ディレクトリの下のファイルも同じ）。標準パッケージを `std::` なしで `import` する（`import audio`）と、`unknown_module`（`suggestions` の先頭が `std::audio`）。
- `migrations`、`v` に数字が続く名前（`v2` など）、`schema` も、モジュールの名前（パスの最初の語）に使えない（`invalid_module_name`。7.3・7.4）。
- `import` で指定した名前が、モジュールにもパッケージにもなければ `unknown_module`（引数 `name`、近い名前があれば `suggestions`）。パッケージの名前を `import` せずに使うと `module_not_imported`。
- `import` したモジュールで宣言された identity は、名前だけで引用行に使える（2.3）。
- 循環する `import`（`ep1` が `ep2` を、`ep2` が `ep1` を `import` する）を許す。宣言（ラベル・identity・型・変数）は、すべてのモジュールを読み込んでから解決するので、読み込みの順序には依存しない。

## 4.3 ラベル

```ebnf
label_decl = "label" identifier [ "(" [ param { "," param } ] ")" [ "->" type ] ] block ;
param      = identifier ":" type [ "=" expression ] ;   (* 既定値。3.5 *)
label_path = identifier [ "." identifier ] ;       (* ラベル または モジュール.ラベル *)
```

- ラベルは、台本の流れの単位である。ラベルの中には、引用行・素の行・コマンド・文を書ける。
- 引数と戻り値の型は、書くときは型注釈が必須（`fn` と同じ）。省略すると、引数なし・戻り値なし。引数の既定値と、`call` での名前つきの引数は、`fn` と同じ（3.5）。
- 引数または戻り値の型を持つラベルは、`call` でだけ呼べる。`jump` の対象にできない（4.4）。
- 戻り値の型を持つラベルの中では、`jump` を書けない（`jump_in_returning_label`）。`jump` 先のラベルは、戻り値を持たない呼び出し元へ戻ってしまうことがあり、戻り値の型が守れなくなるため。
- ラベルの中の `let` は、そのラベルの中だけで使えるローカル変数である。引数もローカル変数として扱う。
- ラベルの名前は、同じモジュールの中で一意でなければならない。`fn`・変数・identity・型と同じ名前は、エラー。
- モジュールの最上位の宣言（ラベル・`fn`・変数・型・identity）は、すべて、ほかのモジュールから見える（v0 では、公開を限る仕組みを持たない）。

## 4.4 遷移：`jump`

```ebnf
jump_stmt = "jump" label_path ;
```

- `jump` は、現在のラベルを離れて、対象のラベルの先頭へ移る。引数は渡さない。
- 現在のラベルのローカル変数（引数を含む）は、破棄される。
- コールスタック（`call` で積まれたもの）は変えない。`jump` した先で `return` すると、`call` した元へ戻る。
- 対象は、引数も戻り値の型も持たないラベルでなければならない。引数または戻り値の型を持つラベルへの `jump` は、エラー（`jump_to_callable_label`）。
- 別のモジュールのラベルは、`jump chapter2.start` と書く。そのモジュールを `import` していなければエラー。
- 遷移先のラベルで、前のラベルのローカル変数の名前を使うと、「未宣言」のエラーになる。診断には、「`jump` で破棄された変数かもしれない」というヒントを添える。

## 4.5 呼び出し：`call`

```ebnf
label_call      = "call" label_path "(" [ expression { "," expression } ] ")" ;
label_call_stmt = label_call ;                                 (* 戻り値は捨てる *)
```

- `call` は、対象のラベルを呼び、`return` かラベルの末尾で、呼び出しの次の位置へ戻る。
- 引数の数と型は、ラベルの宣言と合わなければエラー（`arg_count`・`type_mismatch`）。引数は、呼ばれたラベルのローカル変数になる。
- `call` は、次の 3 つの位置にだけ書ける。戻り値のないラベルの値を受け取るとエラー（`call_without_value`）。

```
let r = call chapter2.intro(favor)
r = call chapter2.intro(favor)
call chapter2.intro(favor)            // 戻り値を捨てる
```

- `call` は、式の途中には書けない（例：`1 + call f()` はエラー）。呼び出しの途中で停止点（引用行など）での停止が起きるため、セーブできる位置を単純にするための制限。エラー（`call_in_expression`）。
- `call` で積まれたフレーム（呼び出し元の位置とローカル変数）は、セーブに含まれる。
- 引数を持たないラベルも、`call` で呼べる。

### return

```ebnf
return_stmt = "return" [ expression ] ;      (* 3.3 *)
```

- `return` は、`fn` と、ラベルの中で使える。`fn` の中の `return` は、`fn` の呼び出し元へ戻り、`fn` の戻り値の型について、ラベルと同じ検査をする（language/03-statements.md の 3.5）。
- ラベルに戻り値の型があれば、`return` は式を取らなければならず（`missing_return_value`）、型が合わなければエラー（`type_mismatch`）。戻り値の型がなければ、式を取らない（`unexpected_return_value`）。
- 呼び出し元がない（コールスタックが空）ときの `return`（値なし）は、実行時は `end` と同じに扱う。戻り値の型を持つラベルは `jump` の対象にできないので（4.4）、値つきの `return` が、呼び出し元なしで実行されることはない。

### end と、ラベルの末尾

```ebnf
end_stmt = "end" ;
```

- `end` は、台本全体を終わらせる文である。コールスタックに積まれたものがあっても、そこへは戻らず、VM は `Yield::End` を返す。
- ラベルの末尾（最後の文）まで進んだとき：
  - `call` で呼ばれていれば、呼び出し元へ戻る（`return` と同じ）。戻り値の型があるラベルは、末尾に達する前に必ず `return` しなければならない（そうでなければ、コンパイルエラー `missing_return`）。末尾に達しうるかは、`return`・`jump`・`end`・`break`・`continue` を終わりとみなして、文の流れから決める。`else` のある `if` は、どの枝も末尾に達しなければ達しない。`while true` は、`break` がなければ達しない。
  - 呼び出し元がない（コールスタックが空）なら、実行時は `end` と同じに扱う。
- コンパイル時、次のすべてに当てはまるラベルは、警告 `label_falls_through`（引数 `name`）にする（書き忘れの発見のため）。
  - 戻り値の型を持たない（持つ場合は `missing_return` が出る）。
  - どのラベルからも `call`・`jump` されていない（ほかのモジュールからのものも数える）。
  - 開始ラベルでない（下記）。
  - 末尾に達しうる（`jump`・`return`・`end` で終わらない）。
- 開始ラベルは、ホストが、コンパイルの設定（`CompileOptions::entry_labels`）に、`モジュール.ラベル` で渡す（モジュールが 1 つだけなら、ラベルの名前だけでもよい）。外から開始するラベルは、どこからも呼ばれないので、渡さないと、警告になる。渡す値は、`VmConfig::start`（4.7）と同じにする。
- `entry_labels` が空（指定なし）のときは、名前が `start` のラベルを、開始ラベルとして対象から外す（`VmConfig::start` の既定が `start` なので）。1 つでも渡したときは、渡したものだけを外す（`start` も、渡さなければ対象になる）。

## 4.6 変数の参照

- `save var` と `persistent var` は、宣言したモジュールの中では、名前だけで使える。
- ほかのモジュールの変数は、`chapter2.favor` のように、名前を付けて使える。代入もできる（`chapter2.favor += 10`）。
- ホストが宣言した変数は、`import パッケージ` のあとで、`パッケージ名.変数名` と書いて、読み書きできる（host/02-registry.md の 2.7）。`read_only` の変数への代入は、エラー（`assign_to_read_only`。引数 `name`）。
- 初期値は、リテラルと、リテラルだけの式に限る（モジュールの読み込み順に依存しないようにするため）。

## 4.7 開始位置

- 台本の開始ラベルは、ホストが指定する（`VmConfig::start`。host/01-execution.md の 1.9）。`モジュール.ラベル` と書く。モジュールが 1 つだけのときは、ラベルの名前だけでもよい。
- 安定 ID の名前空間は、「モジュール + ラベル + ID」。
