# 6. 型検査と診断

台本は、実行の前に、構文と型を検査する（コンパイル）。この章は、診断（エラー・警告）の形と、型検査の範囲と、近い綴りの判定を定める。個々の規則は、各章に書いてある。ここでは、それらをまとめる。

## 6.1 診断の形

1 つの診断は、次の項目を持つ。

| 項目 | 内容 |
| --- | --- |
| 位置 | モジュール名、行、桁（1 始まり。桁は Unicode スカラ値の数。見た目の幅への変換は、表示する側が行う） |
| 重大度 | エラー または 警告（6.2） |
| コード | 変わらない英語の名前（例：`unknown_name`）。ツールが機械的に扱うために使う |
| 引数 | コードごとに決まる、名前つきの値の並び（例：型の不一致なら、期待した型と見つかった型。近い綴りなら、候補の名前）。文字列・整数・文字列の並び。行が途中で終わるときは、`found` に印を入れず、専用のコード `unexpected_eol` にする |

- コアは、診断の文面（メッセージ・ヒント）を持たない。文面は、診断を表示する側が、コードと引数から作る。

- 診断は、位置の順に、まとめて返す。1 つのエラーのあとも、連鎖しないように読み進め、ほかの診断を返す。

## 6.2 重大度

- **エラー**：台本を実行できない。1 つでもあれば、コンパイルは失敗する。
- **警告**：実行はできるが、書き間違いの可能性がある。ホストは、返された警告を見て、エラーとして扱うかを自分で決める。コアの API は、成功したコンパイルの結果とともに、警告を返す（`compile_with_warnings`）。ホストが外から開始するラベルを知らせる設定（`CompileOptions`）つきの版もある（`compile_modules_with_options`。4.5）。エラーがあって失敗したときは、診断に、警告も含む（位置の順）。
- 警告の一覧は、6.5 の表の「警告」のコードである（組み込み関数と同じ名前の変数 3.8、素の行の typo 警告 2.5、どこからも `call`・`jump` されず開始ラベルでもなく `jump` / `return` / `end` で終わらないラベル 4.5、存在しない位置・ラベルを渡した `seen` / `visited` のリテラル 5.7、到達しない `match` のアーム 3.6、旧名のコマンド host/02 2.2）。

## 6.3 型検査

- 型は静的に検査する（3.1）。検査の対象は、式、代入、条件式、関数・ラベルの引数と戻り値、コマンドの引数、`${式}`、糖衣の展開後の呼び出し（5.2）である。
- 型が合わないときは、エラー。診断の引数に、期待した型と、見つかった型を持つ。
- 暗黙の型変換はしない（`int` と `float`、条件式の `bool`。3.1、3.3）。
- 登録されていないコマンドの名前、型が合わない引数、数が合わない引数は、エラー（2.2）。
- 束縛されていない糖衣は、エラー（5.3）。束縛されたコマンドの型が、糖衣の引数の形に合わないときも、エラー（5.2）。
- `once` を付けた選択肢に、安定 ID がないときは、エラー（5.5）。
- `read_only` の変数への代入は、エラー（4.6）。
- `require_text_ids` が有効なとき、安定 ID のない素の行・引用行・選択肢の文と、訳せる引数を文字列リテラルで書いた `@` のコマンドの行は、エラー `missing_text_id`（host/05-texts.md の 5.5）。
- ラベルに戻り値の型があるとき、`return` の式の型が合わなければ、エラー（4.5）。
- 複合値（`list`・`Option`・`struct`・`enum`。3.1、3.7）：要素の型がそろわないリテラル・フィールドの型が合わない値・型の合わないパターンは `type_mismatch`。空の `[]`・`None` の型が決まらないときは `cannot_infer_type`。無いフィールド・バリアント・メソッドは、`unknown_field`・`unknown_variant`・`unknown_method`。`struct` のリテラルのフィールドの過不足・繰り返しは `missing_field`・`duplicate_field`。型の再帰は `recursive_type`。
- `match` は、網羅しているかを検査する（`non_exhaustive_match`）。到達しないアームは、警告（`unreachable_pattern`。3.6）。

## 6.4 未宣言の名前と、近い綴り

- 宣言されていない名前（変数、ラベル、コマンド、identity、型）は、使う場所に応じて、エラーまたは素の行として扱う。
  - 式・`jump`・`call`・`@` の中の未宣言の名前は、エラー。
  - 行頭の未宣言の名前の後ろに `「` が続く行は、素の行（2.5）。
- 未宣言の名前が、宣言済みの名前に**近い綴り**のとき、診断の引数に、候補の名前を持つ（表示する側が「もしかして：〇〇」にする）。素の行になる場合（2.5）は、さらに警告（typo 警告）を出す。
- **近い綴り**の判定は、次のとおりとする。
  1. 名前を NFC に正規化して比べる（1.5）。候補の名前も、NFC の形で返す。
  2. 比べるのは、Unicode のスカラ値の列の、**レーベンシュタイン距離**（挿入・削除・置換を、それぞれ 1 と数える）。
  3. 次の距離以内のとき、近いとする。
     - 2 つの名前のうち、短い方が 4 文字以下：距離 1
     - 短い方が 5 文字以上：距離 2
  4. 候補が複数あるときは、距離の小さい順に、最大 3 つまで示す。同じ距離のときは、名前の順（コードポイントの順）。
- 同じ種類の名前だけを比べる。種類は、式の中の名前（変数）・呼び出しの名前（関数）・ラベル・コマンド・identity。
- 距離が 0 の名前（同じ名前）は、候補にしない。名前が 64 文字を超えるときは、候補を探さない。
- 近い名前がないときは、候補を持たない（引数 `suggestions` を付けない）。
- 候補は、診断の引数 `suggestions`（文字列の並び）で渡す。付くコードは、`unknown_name`・`unknown_function`・`unknown_label`・`unknown_command`・`unknown_module`・`identity_typo`。

## 6.5 診断のコードの一覧

コードは、各章の規則に対応する。「引数」は、診断の公開仕様である。`suggestions`（近い名前の候補。6.4）は、候補があるときだけ付く。「重大度」が「警告」のコード以外は、エラー。「章」は、そのコードを定める規則のある章（`host/NN` は host の部）。この表は、コアの `DiagCode::ALL`・`as_str`・`arg_names`・`severity` と完全に一致する（6.12）。

<!-- codes:diag -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `unexpected_token` | `found`（文字列） | エラー | 1.7 | 思いがけない字句。`found` は、語なら語、それ以外は 1 文字 |
| `unexpected_eol` | なし | エラー | 1.7 | 行が、続きの足りないところで終わっている |
| `unterminated_string` | なし | エラー | 1.7 | 文字列リテラルが閉じていない |
| `invalid_escape` | `found` | エラー | 1.7、1.10 | 使えないエスケープ |
| `invalid_number` | `text` | エラー | 1.7 | 数値として読めない（範囲外・桁区切りの位置。1.7） |
| `full_width_space` | なし | エラー | 1.3 | コードの中の全角スペース |
| `unknown_name` | `name`、`suggestions` | エラー | 3.4 | 宣言されていない変数の名前 |
| `unknown_function` | `name`、`suggestions` | エラー | 3.5、3.8 | 宣言されていない関数（`fn`・組み込み関数）の呼び出し |
| `unknown_command` | `name`、`suggestions` | エラー | 2.2 | 登録されていないコマンド |
| `unknown_label` | `name`、`suggestions` | エラー | 4.4 | 存在しないラベル |
| `unknown_type` | `name` | エラー | 3.1 | 知らない型の名前 |
| `unknown_field` | `name`、`type`、`suggestions` | エラー | 3.2、3.7 | `struct` に無いフィールド（読み出し・代入・リテラル。3.2、3.7）。`type` は `モジュール.型名` |
| `unknown_variant` | `name`、`type`、`suggestions` | エラー | 3.2、3.6 | `enum` に無いバリアント |
| `unknown_method` | `name`、`type`、`suggestions` | エラー | 3.2 | その型に無いメソッド |
| `missing_field` | `name`、`type` | エラー | 3.7 | `struct` のリテラルに無いフィールド |
| `duplicate_field` | `name` | エラー | 3.7 | `struct` のリテラルで、同じフィールドが重なっている |
| `recursive_type` | `name` | エラー | 3.1 | 自分自身を含む型の宣言 |
| `cannot_infer_type` | なし | エラー | 3.1 | 空の `[]`・`None`・ラムダ式の引数の型が決まらない |
| `statement_method_in_expression` | `name` | エラー | 3.3 | `push`・`insert`・`remove`・`clear`・`shuffle` を式の中で使った |
| `non_exhaustive_match` | `missing`（文字列の並び） | エラー | 3.6 | `match` が、すべての値を網羅していない。足りないパターンの一覧 |
| `type_mismatch` | `expected`、`found`（型の名前） | エラー | 6.3 | 型が合わない |
| `condition_not_bool` | `found` | エラー | 3.3 | 条件式が `bool` でない |
| `arg_count` | `name`、`expected`、`found`（整数） | エラー | 2.2、3.5 | 引数の数が合わない |
| `unknown_argument` | `name`、`arg` | エラー | 3.5 | 呼び出し先にない名前の引数。`name` は呼び出し先（名前つきの引数を取らない呼び出しでは空のこともある） |
| `duplicate_argument` | `name`、`arg` | エラー | 3.5 | 同じ引数を 2 度渡している |
| `missing_argument` | `name`、`arg` | エラー | 3.5 | 既定値のない引数を渡していない |
| `argument_out_of_order` | `name`、`arg` | エラー | 3.5 | 名前つきの引数が宣言の順に並んでいない |
| `positional_after_named` | `name` | エラー | 3.5 | 名前つきの引数のあとに位置で渡す引数がある |
| `required_after_default` | `name`、`arg` | エラー | 3.4 | 既定値のある引数のあとに、既定値のない引数を宣言している |
| `invalid_operation` | `op`、`type` | エラー | 3.2 | この型には使えない演算。`op` は演算子（`+` `<` `-` `+=` など） |
| `duplicate_name` | `name` | エラー | 3.4、3.7 | 名前が重なっている。あとに書いた宣言に報告する。`let` は名前の位置。同じモジュールの型の名前の重なりも |
| `duplicate_stable_id` | `id` | エラー | 1.9 | 安定 ID が、同じラベルの中で重なっている |
| `invalid_stable_id` | `id` | エラー | 1.9、2.2 | 安定 ID の形が正しくない、または、停止型でないコマンドの行に付いている |
| `unbound_sugar` | `kind` | エラー | 5.3 | 糖衣が束縛されていない |
| `invalid_command_use` | `name`、`kind`（`effect`・`query`・`stop`） | エラー | 2.2、2.2a | コマンドの種類に合わない使い方。副作用型・停止型を `@` なしで式の中で、問い合わせ型を `@` の文で呼んだ。式の途中の `@`。`let`・`=` の右辺で戻り値を受けるとき、戻り値が `()` のコマンドと問い合わせ型、複合代入の右辺 |
| `assign_to_read_only` | `name`（`パッケージ.変数`） | エラー | 4.6 | ホストが宣言した `read_only` の変数への代入 |
| `unterminated_block` | なし | エラー | 3.3 | ブロックが閉じていない |
| `unexpected_close` | なし | エラー | 3.3 | 対応する開きのない `}` |
| `else_without_if` | なし | エラー | 3.3 | `if` のない `else` |
| `statement_outside_label` | なし | エラー | 4.3 | ラベルの外の文・本文 |
| `declaration_not_allowed` | `what`（先頭の語） | エラー | 4.3 | ラベルの中に書けない宣言（`save var`・`persistent var`・`label` など） |
| `outside_loop` | `what`（`break` か `continue`） | エラー | 3.3 | ループ（`while`・`for`）の外の `break` / `continue` |
| `unsupported_element_type` | `method`、`type` | エラー | 3.5 | 並べ替え・最小・最大のキー（`sort` は要素）が `int`・`float`・`str`・`bool` でない、または `sum` の要素が `int`・`float` でない |
| `invalid_map_key` | `type` | エラー | 3.1 | `map` のキーにできない型（`float`・`list`・`map`・`Option`、それを含む型。3.1） |
| `not_constant` | なし | エラー | 4.6 | 定数でない初期値 |
| `not_yet_supported` | `what` | エラー | 3.5 | 関数として呼べない `名前(…)`（関数の型でない変数・ラベルの名前を呼ぶ、式の結果を呼ぶ `(式)(…)` など。`what` は `call` か `postfix expression`） |
| `fn_not_saveable` | `type`・`what`（`var`・`field`・`stop`・`call`・`repl`） | エラー | 3.5 | 関数の型を含む値を、セーブに入る場所に置いた（モジュールの変数・`struct` のフィールド・REPL の `let`、停止点か `call` のあとで使う関数の型のローカル変数） |
| `not_allowed_in_fn` | `what` | エラー | 3.5 | `fn` の本体に書けない文（素の行・引用行・`choice`・`jump`・`call`・`end`・副作用型か停止型のコマンド。3.5） |
| `assign_in_fn` | `name` | エラー | 3.5 | `fn` の中での、モジュール変数・永続変数・ホスト変数への書き込み |
| `too_deep` | `limit`（整数） | エラー | 3.1 | 式の入れ子・連鎖が深すぎる。型の入れ子が深すぎる |
| `block_too_deep` | `limit` | エラー | 3.3、5.5 | ブロックの入れ子が深すぎる（`label`・`if`・`else`・`while`・`choice` を、それぞれ 1 段に数える） |
| `too_many_registers` | `limit` | エラー | 3.3 | ラベル・`fn` 1 つが使うレジスタ（ローカル変数と、式の途中の値）が多すぎる（`limit` は 65536） |
| `import_not_first` | なし | エラー | 4.1 | `import` が、ほかの宣言より後ろにある |
| `unknown_module` | `name`、`suggestions` | エラー | 4.2 | 存在しないモジュールの `import`・参照 |
| `module_not_imported` | `name` | エラー | 4.2 | `import` していないモジュールの使用 |
| `duplicate_module` | `name` | エラー | 4.1、4.2 | 同じ名前のモジュールが複数ある。パッケージの名前と、モジュールの名前が重なっているときも |
| `invalid_module_name` | `name` | エラー | 4.1 | モジュールの名前（パスの各語）が識別子でない、または予約語 |
| `package_alias` | `name` | エラー | 4.2 | パッケージの `import` に `as` で別名を付けた |
| `duplicate_import` | `name` | エラー | 4.2 | `import` で使う名前が、すでに `import` した名前と重なる（`as` で別名を付けられる） |
| `jump_to_callable_label` | `name` | エラー | 4.4 | 引数・戻り値の型を持つラベルへの `jump` |
| `jump_in_returning_label` | `name` | エラー | 4.3 | 戻り値の型を持つラベルの中の `jump`。`name` は、そのラベル |
| `call_in_expression` | なし | エラー | 4.5 | 式の途中の `call` |
| `call_without_value` | `name` | エラー | 4.5 | 戻り値のないラベルの値を受け取っている |
| `missing_return` | `name` | エラー | 3.5、4.5 | 戻り値の型があるラベル・`fn` が、`return` せずに末尾へ達しうる |
| `missing_return_value` | `expected` | エラー | 3.5、4.5 | 戻り値の型があるラベル・`fn` の、値のない `return` |
| `unexpected_return_value` | なし | エラー | 3.5、4.5 | 戻り値の型がないラベル・`fn` の、値のある `return` |
| `unterminated_quote` | なし | エラー | 2.4 | identity つきの引用行・選択肢の文の `「` が、`」` で閉じていない |
| `unclosed_bracket` | `open` | エラー | 2.8 | 式の括弧が、閉じないまま、続きの行の上限（256 行）またはファイルの終わりに達した |
| `text_after_quote` | なし | エラー | 2.4 | identity つきの引用行で、`」` の後ろに、空白と安定 ID 以外の文字が続く |
| `once_without_id` | なし | エラー | 5.5 | `once` を付けた選択肢に、安定 ID がない（`yomi assign-ids` が、ID を書き足して直せる。host/05-texts.md の 5.10） |
| `deprecated_command_name` | `name`（書かれた旧名）、`new`（今の名前） | 警告 | host/02-registry.md の 2.2 | 警告。コマンドを旧名（別名）で呼んだ |
| `repl_not_allowed` | `what`（`stable_id`・`once`・`save`・`persistent`・`label`・`jump`・`end`・`return`） | エラー | host/07-repl.md の 7.7 | REPL の入力に書けないもの。`return` は `fn` の外のもの |
| `missing_text_id` | `kind`（`plain`・`quote`・`choice`・`command`） | エラー | host/05-texts.md の 5.5 | `require_text_ids` が有効なとき、安定 ID のない素の行・引用行・選択肢の文と、訳せる引数を文字列リテラルで書いた `@` のコマンドの行 |
| `empty_choice` | なし | エラー | 5.5 | 選択肢が 1 つもない `choice` |
| `ambiguous_identity` | `name`、`candidates`（`モジュール.名前` の並び） | エラー | 2.3 | 複数の `import` 先に、同じ名前の identity がある |
| `identity_typo` | `name`、`suggestions` | 警告 | 2.5 | 未宣言の名前の後ろに `「` が続く素の行で、近い綴りの identity がある |
| `unknown_position` | `position` | 警告 | 5.7 | `seen` に渡した文字列リテラルが、台本にある停止点の位置でない |
| `unknown_label_literal` | `label` | 警告 | 5.7 | `visited` に渡した文字列リテラルが、台本にあるラベルでない |
| `unreachable_pattern` | なし | 警告 | 3.6 | `match` の、前のアームですべて合ってしまう（到達しない）アーム |
| `shadowed_builtin` | `name` | 警告 | 3.8 | 組み込み関数と同じ名前の変数 |
| `unused_import` | `name` | 警告 | 4.2 | `import` したのに、使っていない（`name` は `import` に書いたパス） |
| `label_falls_through` | `name` | 警告 | 4.5 | どこからも `call`・`jump` されず、開始ラベルでもないラベルが、末尾へ達しうる |
| `unused_variable` | `name` | 警告 | 6.13 | `let` で宣言したローカル変数を、読んでいない |
| `schema_name_outside_migrations` | `name` | エラー | 7.2 | `migrations` モジュールの外で、`vN::` の名前を書いた |
| `invalid_schema_file` | `path` | エラー | 7.3 | 凍結スキーマのファイル名・ヘッダが正しくない |
| `schema_hash_mismatch` | `version` | エラー | 7.3 | 凍結スキーマの内容が、記録したハッシュと合わない |
| `duplicate_migrate` | `version` | エラー | 7.4 | 同じ版の `migrate` が複数ある |
| `invalid_migrate_version` | `version` | エラー | 7.4 | `migrate` の版が 2 未満 |
| `invalid_min_save_version` | `version` | エラー | 7.4 | `min_save_version` が正しくない（範囲外・2 回目・先頭でない） |
| `unknown_schema_name` | `name`、`version`、`suggestions` | エラー | 7.4 | 旧い側（または新しい側）のスキーマにない名前 |
| `migrate_kind_mismatch` | `name` | エラー | 7.4 | `rename` で、`save var` と `persistent var` をまたいだ |
| `migrate_type_mismatch` | `name`、`expected`、`found` | エラー | 7.4 | 書いた型が、スキーマの型と違う |
| `duplicate_migrate_target` | `name` | エラー | 7.4 | 同じ変数を、1 つの `migrate` の中で 2 回扱った |
| `migrate_impure_expr` | `what` | エラー | 7.4 | 変換の式に、使えないもの（`fn`・変数・コマンド・乱数・停止）を書いた |
| `missing_frozen_schema` | `version` | エラー | 7.4 | 必要な版の凍結スキーマがない |
| `missing_migrate` | `version` | エラー | 7.4 | 必要な版の `migrate` がない |
| `unhandled_type_change` | `name`、`version` | 警告 | 7.5 | 型が変わった変数を、`migrate` が扱っていない |
| `schema_changed_since_freeze` | `version` | 警告 | 7.5 | 凍結したあとにスキーマが変わったが、`migrate` がない |

- 複数のモジュールを同時にコンパイルしたとき（`compile_modules`）、診断は、モジュール名を持つ。並びは、渡したモジュールの順、そのなかで（行、桁）の順。
- 失敗した宣言（`let`・`save var`・ラベルの見出し、型が不正な `save var`）の名前は、あとで使っても、`unknown_name`・`unknown_label` にしない（1 つの誤りから、連鎖して診断を出さないため。6.1）。

## 6.6 実行時エラーのコード

実行時エラー（`VmErrorKind`）は、`Vm` の操作（`run`・`resume`・`save` など）が `VmError` で返す。種類のコード（`code()`）と引数は、次のとおり。`VmError` は、種類のほかに、わかるときだけモジュール・ラベル・行を持つ。重大度は、すべてエラー。文面は持たない。

<!-- codes:vm -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `step_limit_exceeded` | なし | エラー | host/01 1.9 | 命令数の上限を超えた |
| `interrupted` | なし | エラー | host/01 1.9a | 中断のトークンで、中断が求められた |
| `call_depth_exceeded` | なし | エラー | host/01 1.9 | 呼び出しの深さ（`call` の入れ子・再帰）が上限を超えた |
| `value_too_large` | なし | エラー | host/01 1.9 | 値（文字列・`list`・`struct` など）が大きさの上限を超えた |
| `memory_limit_exceeded` | なし | エラー | host/01 1.9 | 変数が保持する値の大きさの合計が上限を超えた |
| `index_out_of_range` | `index`、`len` | エラー | language/03 3.2 | `list` の添字が範囲の外。`index` は渡された添字、`len` は `list` の長さ |
| `key_not_found` | `key` | エラー | language/03 3.2、3.3 | `map` にないキーを、添字で読んだか更新した。`key` は渡されたキー |
| `integer_overflow` | なし | エラー | language/03 3.2 | 整数がオーバーフローした |
| `division_by_zero` | なし | エラー | language/03 3.2 | 0 で割った（`int` でも `float` でも） |
| `non_finite_float` | なし | エラー | language/03 3.2 | `float` の演算の結果が `NaN` か `±∞` になった |
| `float_out_of_range` | なし | エラー | language/03 3.8 | `to_int` の結果が `int` の範囲に収まらない |
| `not_stopped` | なし | エラー | host/01 1.4 | 停止していないときに `resume`・`save` を呼んだ |
| `already_stopped` | なし | エラー | host/01 1.4 | 停止中に `run` を呼んだ |
| `ended` | なし | エラー | host/01 1.4 | 終わったあとに `run` を呼んだ |
| `no_choice_available` | なし | エラー | language/05 5.5 | `choice` の選択肢が、すべて一覧に含まれない |
| `choice_out_of_range` | `index`、`count` | エラー | language/05 5.5 | `resume` に渡した選択の番号が、渡した一覧の範囲の外。`index` は渡された番号、`count` は一覧の長さ |
| `poisoned` | なし | エラー | host/01 1.4 | 実行時エラーのあとで、続けようとした |
| `persistent_data` | `reason` | エラー | host/04 4.5 | 永続データを読めなかった（`Vm::new`）。`reason` は、ロードエラーの文面 |
| `invalid_random_range` | `min`、`max` | エラー | language/03 3.8 | `random(最小, 最大)` の最小が最大より大きい。`min`・`max` は渡された値 |
| `snapshot_mismatch` | なし | エラー | host/04 4.7 | 別のプログラムで作ったスナップショットを `restore` に渡した |
| `bad_input` | `expected`、`found` | エラー | host/01 1.3 | `resume` の入力の型が合わない。`expected` は期待した型、`found` は渡された型 |
| `start_label_not_found` | `name` | エラー | host/01 1.1 | 開始のラベルがない。`name` はそのラベル |
| `command_missing` | `name` | エラー | host/02 2.1 | プログラムが使うコマンド・変数が、登録表にない。`name` はその名前 |
| `command_failed` | `code` | エラー | host/02 2.4 | コマンドの関数が `Err` を返した。`code` は、パッケージのエラーのコード（`パッケージ名.エラー名`。コアのものは `core.…`）。このほか、コードごとに決まる名前つきの引数を持つ（`VmErrorKind::command_error()`。標準パッケージのものは、spec/packages/*.md の「エラーコード」） |
| `unknown_variable` | `name` | エラー | host/02 2.7 | `get_var`・`set_var` に渡した名前の変数が、登録されていない。`name` はその名前 |
| `slice_serialize` | `name` | エラー | host/03 3.2 | パッケージの状態スライスを直列化できなかった。`name` はパッケージの名前 |
| `decl_only_registry` | なし | エラー | host/02 2.8 | 宣言ファイルから作った登録表（コマンドを実行する関数を持たない）で、VM を作ろうとした |

## 6.7 ロードエラーのコード

ロードエラー（`LoadErrorKind`）は、`Vm::load`・`Vm::load_persistent` が `LoadError` で返す（host/04-save.md の 4.8）。

<!-- codes:load -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `bad_magic` | なし | エラー | host/04 4.2 | マジックが違う |
| `unsupported_version` | `version` | エラー | host/04 4.2 | 知らない形式の版数。`version` は読んだ版数 |
| `checksum_mismatch` | なし | エラー | host/04 4.2 | チェックサムが合わない |
| `corrupt` | なし | エラー | host/04 4.2 | 本体が壊れている |
| `label_not_found` | `name` | エラー | host/04 4.4 | 保存したラベルが、台本にない。`name` はそのラベル |
| `module_mismatch` | `name` | エラー | host/04 4.4 | 保存したモジュールの名前が、台本のどのモジュールにもない。`name` はそのモジュール |
| `command_missing` | `name` | エラー | host/04 4.6 | プログラムが使うコマンドが、登録表にない。`name` はそのコマンド |
| `stop_args_failed` | なし | エラー | host/04 4.5 | 停止点の引数を、作り直せなかった |
| `package_slice` | `name` | エラー | host/04 4.6 | パッケージのスライスを復元できなかった（壊れている・`migrate` が `Err`）。`name` はパッケージの名前 |
| `decl_only_registry` | なし | エラー | host/02 2.8 | 宣言ファイルから作った登録表では、ロードできない |
| `save_from_newer_version` | `saved`・`current` | エラー | host/04 4.6 | セーブのスキーマの版（`saved`）が、台本の版（`current`）より新しい |
| `save_version_unsupported` | `saved`・`min` | エラー | host/04 4.6 | セーブのスキーマの版（`saved`）が、台本の `min_save_version`（`min`）より古い |
| `schema_mismatch` | `vars` | エラー | host/04 4.6 | `Strict` で、保存した型と台本の型が違う変数がある。`vars` は、すべての変数の「名前: 保存した型 -> 台本の型」 |
| `migrate_failed` | `vars` | エラー | host/04 4.6 | `Strict` で、`migrate` の変換が実行時エラーになった。`vars` は、失敗した変数の名前 |

## 6.8 差し替え表のコード

差し替え表（host/05-texts.md）の誤り（`TextErrorKind`）と、台本との突き合わせの結果（`TextIssueKind`）。誤りは、`TextError`（コード・引数・キー・行）で返す。突き合わせの結果は、`TextIssue`（重大度・キー・種類）で返す。

<!-- codes:text-error -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `invalid_escape` | `found`、`key` | エラー | host/05 5.3 | 訳文の、使えないエスケープ。`found` は `\` と次の文字、`key` は訳文のキー |
| `invalid_placeholder` | `found`、`key` | エラー | host/05 5.3 | 訳文の `${n}` の形でない `${…}`（`n` は 1 以上の整数）か、閉じていない `${`。`found` は見つけた綴り |
| `missing_tab` | なし | エラー | host/05 5.7 | 表のファイルの行に、キーと訳文を分ける TAB がない |
| `empty_key` | なし | エラー | host/05 5.7 | 表のファイルの行の、キーが空 |
| `duplicate_key` | `key` | エラー | host/05 5.7 | 表のファイルで、同じキーが 2 度ある。`key` はそのキー |
| `unknown_directive` | `name` | エラー | host/05 5.7 | 表のファイルの、知らない `@` の指示（`@lang` 以外）。`name` は `@` を除く名前 |

<!-- codes:text-issue -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `missing` | `key` | 警告 | host/05 5.4 | 台本の、安定 ID つきの本文にあって、表にないキー（足りない訳） |
| `unused` | `key` | 警告 | host/05 5.4 | 表にあって、台本にないキー（余った訳） |
| `bad_number` | `key`、`number`、`count` | エラー | host/05 5.4 | 訳文の `${n}` の `n` が、原文の `${式}` の個数を超える。`number` は超えている最大の `n`、`count` は原文の個数 |

## 6.9 宣言ファイルのコード

宣言ファイル（host/02-registry.md の 2.8）を読めなかったときの誤り（`DeclErrorKind`）。`DeclError` は、種類のほかに、誤りのある場所（JSON のパス）を持つ。

<!-- codes:decl-error -->
| コード | 引数 | 重大度 | 章 | 意味 |
| --- | --- | --- | --- | --- |
| `syntax` | `line`、`col` | エラー | host/02 2.8 | JSON として読めない。`line`・`col` は位置（1 始まり） |
| `too_large` | なし | エラー | host/02 2.8 | 入力が大きすぎる（16 MiB を超える） |
| `too_deep` | なし | エラー | host/02 2.8 | JSON か型の名前の入れ子が深すぎる（128 を超える） |
| `not_decls` | なし | エラー | host/02 2.8 | 宣言ファイルではない（全体がオブジェクトでない、`format` が `"yomi-decls"` でない） |
| `unsupported_version` | `version` | エラー | host/02 2.8 | 読めない版数の宣言ファイル。`version` は書かれた版数（読めるのは 1 だけ） |
| `missing_field` | なし | エラー | host/02 2.8 | 必要な項目がない |
| `wrong_type` | なし | エラー | host/02 2.8 | 項目の JSON の型が合わない |
| `invalid_type` | `found` | エラー | host/02 2.8 | 型の名前が正しくない（登録されていない型・読めない書き方）。`found` は書かれた文字列 |
| `invalid_value` | なし | エラー | host/02 2.8 | 値が正しくない（知らない `kind`・型に合わない `default` など） |
| `registry` | `reason` | エラー | host/02 2.8 | 登録表として正しくない（名前の重なり・型の宣言の誤りなど）。`reason` は、登録エラーの説明（6.10） |

## 6.10 登録エラー

登録表への登録（`Registry`・`PackageReg` のメソッド）の誤りは、`RegistryError` で返す。診断のコードを持たず、メッセージは英語の `Display` のままである（ホストのプログラマに向けた誤りなので、`yomi-cli` の FTL の対象にしない）。宣言ファイルから読むときは、`decl-error` の `registry` に包んで返す（6.9）。

| 種類（`RegistryError`） | 意味 |
| --- | --- |
| `DuplicateCommand` | 同じ名前のコマンド（または変数・型・別名）を、二度登録した |
| `UnknownCommand` | 登録されていないコマンドを束縛した（`bind`）か、別名の指す先にした |
| `SugarMismatch` | 束縛するコマンドの型が、糖衣の引数の形に合わない |
| `AlreadyBound` | 同じ種類の糖衣を、二度束縛した |
| `InvalidName` | 名前が正しくない（識別子でない・名前空間やパッケージ名などが予約語・パッケージ名の前半が `Package::NAME` と違う） |
| `DuplicatePackage` | 同じ名前のパッケージを、二度登録した |
| `MissingDependency` | `DEPENDS` に宣言した相手が、登録済みでも、同じ一括の中にもない |
| `DependencyCycle` | パッケージの依存が輪になっている（自分自身への依存を含む） |
| `QueryWithoutValue` | 問い合わせ型の戻り値が `()` |
| `UnsupportedVarType` | ホスト変数の型が使えない（`()` を含む） |
| `InvalidVarValue` | ホスト変数の初期値が、`NaN`・`±∞` か、変数の型に合わない |
| `DuplicateType` | 同じ名前の型を、二度登録した |
| `UnknownType` | 型・コマンド・変数が使う型が、まだ登録されていない（型は、使われるより先に登録する） |
| `RecursiveType` | 型が、自分自身を含む |
| `InvalidMapKey` | `map` のキーにできない型を使っている |
| `TypeTooDeep` | 型の入れ子が深すぎる |
| `InvalidTypeDecl` | 型の宣言が正しくない（フィールド・バリアントの名前が識別子でない・重なっている、バリアントが 1 つもない、`()` のフィールド） |
| `NoCommand` | 別名・説明・引数の名前を付ける先のコマンドがない（直前にコマンドを登録していない） |
| `InvalidParams` | 引数の名前の個数が、コマンドの引数の個数と違う |

## 6.11 `yomi codes`

`yomi codes [--json] [--packages]` は、6.5〜6.9 の組（`diag`・`vm`・`load`・`text-error`・`text-issue`・`decl-error`）のコードを、すべて表示する。

- 既定：1 コードを 1 行に、TAB 区切りで、組・コード・重大度・引数の名前（`,` 区切り）・今の言語の 1 行の説明（引数は `<名前>` の形で示す）。
- `--json`：言語によらない機械可読の形。`{ "format": "yomi-codes", "version": 1, "codes": [ { "group", "code", "severity"（`error` か `warning`）, "args": [名前…] } … ] }`。
- `--packages`：コマンドのエラーコード（標準パッケージの `error_codes()` と、コアの `core_command_error_codes()`。host/02 の 2.4）を、言語のコードの後ろに足す。既定の表示では、組の列が `package`・`core`、重大度が `error`。`--json` では、全項目が `kind`（`language`・`package`・`core`）を持ち、`language` の項目は従来の `group` も持つ。`package`・`core` の項目は `group` を持たず、`code` が `パッケージ名.エラー名` の形になる。`--packages` なしの出力は、この項目の追加の影響を受けない。
- 文面（説明）は、`yomi-cli` の FTL（en・ja）にあり、キーは、組の接頭辞（`diag-`・`vm-`・`load-`・`text-error-`・`text-issue-`・`decl-error-`）にコードを続けた形。コマンドの実行時エラー（`command_failed`）の文面は、パッケージのコードごとに `std-error-<コード。. は ->` のキーで引く（host/02 の 2.4）。コアは、文面を持たない。

## 6.12 一覧と実装の一致

- 6.5〜6.9 の表（`<!-- codes:… -->` の行の次の表）は、コアの `all_codes()`（`DiagCode::ALL` と、各種類の `code()`）と、組ごとに、コードの集合・引数の名前の集合・重大度が一致する。コアのテスト（`tests/spec_codes.rs`）が、表を読んで確かめる。表にあってコードにないもの、コードにあって表にないもの、の両方を失敗にする。
- コードを足す・消す・改めるときは、実装（`DiagCode` の一覧・`as_str`・`arg_names`・`severity`、または各種類の `code()` と `yomi-core` の一覧）、`yomi-cli` の FTL（en・ja）、この表を、同時に直す。FTL に、全コードのキーがそろっていることは、`yomi-cli` のテストが確かめる。

## 6.13 そのほかの診断

コード `unused_variable`（引数 `name`。警告）の表の行は、6.5 の表にある。

### 未使用の変数の警告

- 対象は、`let` で宣言した変数（コマンドの戻り値を受ける `let` を含む）。代入だけして、読まない変数も、対象である。
- 名前が `_` で始まる変数は、対象外である。ラベルと `fn` の引数、`for` の変数、`save var`・`persistent var` も、対象外である。
- 読みとして数えるもの：式の中の変数の参照、関数の型の変数の呼び出し、ラムダ式の捕捉、複合代入（`x += 1`）、変数の一部への代入（`x[i] = v`・`x.f = v`）、変数に対するメソッドの文（`xs.push(v)`）。数えないのは、`x = 値` だけの代入である。
- 台本のラベルと `fn` の本体で検査する。REPL の入力の中の `let` は、対象外である。
- エラーがあるときは、使用の記録が途中で切れるので、出さない（未使用の `import` の警告と同じく、全体にエラーがないときだけ出す）。

