//! Multiple modules

use super::*;

/// Makes a directory of a script made of multiple files (`(relative path, contents)`).
pub(super) fn project(test: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = scratch(&format!("project-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

pub(super) const GAME: &[(&str, &str)] = &[
    (
        "main.yomi",
        "import story::chapter2\nimport lib::util\n\nsave var score: int = 1\n\nlabel start {\n  開始\n  call chapter2.intro(3)\n  let d = call util.double(score)\n  倍は ${d}\n  chapter2.favor += 10\n  jump chapter2.scene\n}\n\nlabel back {\n  戻った ${chapter2.favor}\n}\n",
    ),
    (
        "story/chapter2.yomi",
        "import main\n\nsave var favor: int = 5\n\nlabel intro(n: int) {\n  導入 ${n} #i1\n  main.score += n\n}\n\nlabel scene {\n  場面\n  jump main.back\n}\n",
    ),
    (
        "lib/util.yomi",
        "label double(x: int) -> int {\n  return x * 2\n}\n",
    ),
];

#[test]
fn collects_modules_under_entry_dir_and_call_jump_move_between_files() {
    let dir = project("basic", GAME);
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "ja", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "開始\n導入 3\n倍は 8\n場面\n戻った 15\n（おわり）\n"
    );
    assert_eq!(o.stderr, "");
}

#[test]
fn start_can_include_module() {
    let dir = project("start", GAME);
    let main = dir.join("main.yomi");
    let o = yomi(
        &[
            "--lang",
            "ja",
            "run",
            main.to_str().unwrap(),
            "--start",
            "story::chapter2.scene",
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "場面\n戻った 5\n（おわり）\n");
    // A plain label name is a label of the entry file's module
    let o = yomi(
        &[
            "--lang",
            "ja",
            "run",
            main.to_str().unwrap(),
            "--start",
            "back",
        ],
        "",
    );
    assert_eq!(o.stdout, "戻った 5\n（おわり）\n");
}

#[test]
fn root_changes_search_directory() {
    let dir = project("root", GAME);
    // Even if the entry file is in a subdirectory, `--root` reads the whole
    let chapter = dir.join("story/chapter2.yomi");
    let o = yomi(
        &[
            "--lang",
            "ja",
            "run",
            chapter.to_str().unwrap(),
            "--root",
            dir.to_str().unwrap(),
            "--start",
            "scene",
        ],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "場面\n戻った 5\n（おわり）\n");
    // Without `--root`, only that subdirectory, so `main` is not found
    let o = yomi(
        &[
            "--lang",
            "ja",
            "run",
            chapter.to_str().unwrap(),
            "--start",
            "scene",
        ],
        "",
    );
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains(&format!(
            "{}:1:8: エラー: 存在しないモジュールです: 'main'",
            chapter.display()
        )),
        "{}",
        o.stderr
    );
}

#[test]
fn entry_file_outside_search_dir_is_error() {
    let dir = project("outside", GAME);
    let main = dir.join("main.yomi");
    let lib = dir.join("lib");
    let o = yomi(
        &[
            "--lang",
            "en",
            "run",
            main.to_str().unwrap(),
            "--root",
            lib.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("is not under the module directory"),
        "{}",
        o.stderr
    );
}

#[test]
fn same_file_name_in_different_directories_are_different_modules_importable_with_aliases() {
    let dir = project(
        "dup",
        &[
            (
                "main.yomi",
                "import {a::util as autil, b::util as butil}\nlabel start {\n  call autil.x()\n  call butil.x()\n}\n",
            ),
            ("a/util.yomi", "label x {\n  あ\n}\n"),
            ("b/util.yomi", "label x {\n  い\n}\n"),
        ],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "あ\nい\n(end)\n");
}

#[test]
fn imports_with_same_last_word_are_errors_without_alias() {
    let dir = project(
        "dup-local",
        &[
            (
                "main.yomi",
                "import {a::util, b::util}\nlabel start {\n  あ\n}\n",
            ),
            ("a/util.yomi", "label x {\n}\n"),
            ("b/util.yomi", "label x {\n}\n"),
        ],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains(
            "main.yomi:1:18: error: 'util' is already imported (use 'as' to give it another name)"
        ),
        "{}",
        o.stderr
    );
}

#[test]
fn diagnostics_use_module_file_path() {
    let dir = project(
        "diag",
        &[
            (
                "main.yomi",
                "import sub::lib\nlabel start {\n  call lib.work(1)\n}\n",
            ),
            (
                "sub/lib.yomi",
                "label work(n: int) {\n  jump nowhere\n  call work()\n}\n",
            ),
        ],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        format!(
            "{0}:2:3: error: unknown label 'nowhere'\n{0}:3:3: error: 'work' takes 1 argument(s), but 0 given\n",
            dir.join("sub").join("lib.yomi").display()
        )
    );
}

#[test]
fn warnings_also_use_module_file_path() {
    let dir = project(
        "warn",
        &[
            ("main.yomi", "label start {\n  あ\n}\n"),
            ("lib.yomi", "label orphan(n: int) {\n  い\n}\n"),
        ],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "あ\n(end)\n");
    assert_eq!(
        o.stderr,
        format!(
            "{}:1:7: warning: the label 'orphan' is never called or jumped to, is not the start label, and does not end with jump, return or end\n",
            dir.join("lib.yomi").display()
        )
    );
}

#[test]
fn start_label_given_with_start_is_not_warned() {
    let dir = project(
        "warn_start",
        &[(
            "main.yomi",
            "label start {\n  あ\n}\nlabel intro {\n  い\n}\n",
        )],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 0);
    assert!(o.stderr.contains("the label 'intro'"), "{}", o.stderr);
    assert!(!o.stderr.contains("the label 'start'"), "{}", o.stderr);
    let o = yomi(
        &[
            "--lang",
            "en",
            "run",
            main.to_str().unwrap(),
            "--start",
            "main.intro",
        ],
        "",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "い\n(end)\n");
    assert!(o.stderr.contains("the label 'start'"), "{}", o.stderr);
    assert!(!o.stderr.contains("the label 'intro'"), "{}", o.stderr);
}

#[test]
fn runtime_errors_show_file_and_label_of_module_where_they_happened() {
    let dir = project(
        "runtime",
        &[
            (
                "main.yomi",
                "import lib\nlabel start {\n  call lib.work(0)\n}\n",
            ),
            (
                "lib.yomi",
                "label work(d: int) {\n  あ\n  let _x = 1 / d\n}\n",
            ),
        ],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "ja", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "あ\n");
    assert_eq!(
        o.stderr,
        format!(
            "{}: エラー: 0 で割りました（ラベル lib.work、3 行目）\n",
            dir.join("lib.yomi").display()
        )
    );
}

#[test]
fn exceeding_call_depth_limit_stops_with_runtime_error() {
    let dir = project(
        "depth",
        &[(
            "main.yomi",
            "label start {\n  call loop(0)\n}\n\nlabel loop(n: int) {\n  call loop(n + 1)\n}\n",
        )],
    );
    let main = dir.join("main.yomi");
    let o = yomi(&["--lang", "en", "run", main.to_str().unwrap()], "");
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        format!(
            "{}: error: calls (call or fn) are nested deeper than the call depth limit (label main.loop, line 6)\n",
            main.display()
        )
    );
}

#[test]
fn saving_in_call_and_loading_in_another_run_continues_to_return_point() {
    let dir = project("saveload", GAME);
    let main = dir.join("main.yomi");
    let main = main.to_str().unwrap();
    let save = dir.join("a.sav");
    let save = save.to_str().unwrap();

    // Save inside `chapter2.intro` (the introduction) and end
    let o = yomi(
        &["--lang", "ja", "run", main],
        &format!("\n:save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, format!("開始\n導入 3\n{save} に保存しました\n"));

    // Load in a new run. `intro` finishes and it returns to the rest of `start`
    let o = yomi(
        &["--lang", "ja", "run", main],
        &format!(":load {save}\n:vars\n\n\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        format!(
            "開始\n{save} から読み込みました\n導入 3\nmain.score = 1\nstory::chapter2.favor = 5\nn = 3\n倍は 8\n場面\n戻った 15\n（おわり）\n"
        )
    );
    // After loading, `main.score += n` in `intro` runs, and `倍は` becomes 8
    assert!(o.stdout.contains("戻った 15"));
}

#[test]
fn back_returns_to_stop_points_inside_and_outside_call() {
    let dir = project(
        "back",
        &[(
            "main.yomi",
            "label start {\n  外1\n  call inner(1)\n  外2\n}\n\nlabel inner(n: int) {\n  内 ${n}\n}\n",
        )],
    );
    let main = dir.join("main.yomi");
    let o = yomi(
        &["--lang", "ja", "run", main.to_str().unwrap()],
        "\n\n:back\n:back\n\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "外1\n内 1\n外2\n内 1\n外1\n内 1\n外2\n（おわり）\n"
    );
}

#[test]
fn quote_lines_print_with_names_and_choices_list_numbers_selectable_by_number() {
    // The first time, "話す". The second time the count has grown and "帰る" appears in the list, so choose 3.
    let o = run("ja", "choices.yomi", &[], "\n\n1\n\n\n\n\n3\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "案内人「どうする？」\n「ひとりごと」\n1: 「話す」\n2: 「調べる」\n話した\n続き\n\
         案内人「どうする？」\n「ひとりごと」\n1: 「話す」\n2: 「調べる」\n3: 「帰る」\n\
         さようなら\n（おわり）\n"
    );
}

#[test]
fn choice_reprompts_on_out_of_range_or_non_number() {
    let o = run(
        "en",
        "choices.yomi",
        &[],
        "\n\n0\n9\nabc\n\n2\n\n\n\n\n1\n\n\n\n\n2\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr.matches("enter a number from 1 to 2").count(), 4);
    assert!(o.stdout.contains("調べた\n続き\n"), "{}", o.stdout);
}

#[test]
fn chosen_once_option_disappears_from_later_lists() {
    let o = run("ja", "choices.yomi", &[], "\n\n2\n\n\n\n\n1\n\n\n\n\n2\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let rounds: Vec<&str> = o.stdout.split("案内人「どうする？」\n").collect();
    assert_eq!(rounds.len(), 4, "{}", o.stdout);
    assert!(rounds[1].contains("2: 「調べる」"), "{}", rounds[1]);
    assert!(!rounds[2].contains("調べる"), "{}", rounds[2]);
    assert!(!rounds[3].contains("調べる"), "{}", rounds[3]);
}

#[test]
fn back_before_choice_restores_once_state() {
    // Doing :back right after choosing "調べる" goes back to the choice stop point, and "調べる" appears in the list again.
    let o = run(
        "ja",
        "choices.yomi",
        &[],
        "\n\n2\n:back\n2\n\n\n\n\n1\n\n\n\n\n2\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout.matches("調べた\n").count(), 2, "{}", o.stdout);
    assert!(
        o.stdout.ends_with("さようなら\n（おわり）\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn choice_picks_1_when_input_runs_out() {
    let o = run("ja", "eof_choice.yomi", &[], "");
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "1: 「はい」\n2: 「いいえ」\n返事した\n最後\n（おわり）\n"
    );
}

#[test]
fn save_and_load_return_to_choice_stop_point() {
    let dir = scratch("choose-save");
    let save = dir.join("c.sav");
    let save = save.to_str().unwrap();
    // Save at the choice stop point, load in another run, and choose number 2
    let o = run(
        "ja",
        "eof_choice.yomi",
        &[],
        &format!(":save {save}\n:quit\n"),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = run("ja", "eof_choice.yomi", &[], &format!(":load {save}\n2\n"));
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with("（おわり）\n"), "{}", o.stdout);
    assert!(!o.stdout.contains("最後"), "{}", o.stdout);
}
