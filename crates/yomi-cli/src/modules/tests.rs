use super::*;

/// A working directory for each test (made empty).
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yomi-modules-test-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn collects_recursively_under_entry_dir_with_module_names_from_relative_paths() {
    let dir = scratch("recursive");
    write(&dir.join("main.yomi"), "m");
    write(&dir.join("chapter/ep1.yomi"), "e1");
    write(&dir.join("chapter/deep/ep2.yomi"), "e2");
    write(&dir.join("readme.txt"), "x");
    write(&dir.join("chapter/notes.md"), "x");
    let (files, entry) = collect(&dir.join("main.yomi"), None).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.module.as_str()).collect();
    assert_eq!(files[entry].module, "main");
    assert_eq!(files[entry].source, "m");
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["chapter::deep::ep2", "chapter::ep1", "main"]);
}

#[test]
fn root_searches_under_given_directory() {
    let dir = scratch("root");
    write(&dir.join("game/main.yomi"), "m");
    write(&dir.join("lib/util.yomi"), "u");
    // By default, only the directory of the entry file
    let (files, _) = collect(&dir.join("game/main.yomi"), None).unwrap();
    assert_eq!(files.len(), 1);
    // Widen it with `--root`
    let (files, entry) = collect(&dir.join("game/main.yomi"), Some(&dir)).unwrap();
    let mut names: Vec<_> = files.iter().map(|f| f.module.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["game::main", "lib::util"]);
    assert_eq!(files[entry].module, "game::main");
}

#[test]
fn same_file_name_in_different_directories_are_different_modules() {
    let dir = scratch("dup");
    write(&dir.join("main.yomi"), "m");
    write(&dir.join("a/util.yomi"), "1");
    write(&dir.join("b/util.yomi"), "2");
    let (files, _) = collect(&dir.join("main.yomi"), None).unwrap();
    let mut names: Vec<_> = files.iter().map(|f| f.module.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["a::util", "b::util", "main"]);
}

#[test]
fn module_names_come_from_relative_paths_from_search_directory() {
    let root = Path::new("/r/game");
    assert_eq!(module_name(Path::new("/r/game/main.yomi"), root), "main");
    assert_eq!(
        module_name(Path::new("/r/game/story/ch1.yomi"), root),
        "story::ch1"
    );
    assert_eq!(module_name(Path::new("./x/y.yomi"), Path::new(".")), "x::y");
}

#[test]
fn entry_file_outside_search_directory_errors() {
    let dir = scratch("outside");
    write(&dir.join("game/main.yomi"), "m");
    write(&dir.join("lib/util.yomi"), "u");
    let errors = collect(&dir.join("game/main.yomi"), Some(&dir.join("lib"))).unwrap_err();
    assert!(matches!(errors[0], CollectError::EntryOutsideRoot { .. }));
}

#[test]
fn unreadable_entry_file_returns_reason() {
    let errors = collect(Path::new("/nonexistent/none.yomi"), None).unwrap_err();
    assert!(
        matches!(&errors[0], CollectError::Read { path, .. } if path == "/nonexistent/none.yomi")
    );
}

#[test]
fn entry_file_is_shown_with_given_path() {
    let dir = scratch("shown");
    write(&dir.join("main.yomi"), "m");
    write(&dir.join("util.yomi"), "u");
    let entry = dir.join("main.yomi");
    let (files, i) = collect(&entry, None).unwrap();
    assert_eq!(files[i].path, entry.display().to_string());
}

#[test]
fn directories_and_files_starting_with_dot_are_not_collected() {
    let dir = scratch("hidden");
    write(&dir.join("main.yomi"), "m");
    write(&dir.join(".git/hook.yomi"), "x");
    write(&dir.join(".cache/deep/y.yomi"), "x");
    write(&dir.join(".swap.yomi"), "x");
    write(&dir.join("story/.tmp.yomi"), "x");
    write(&dir.join("story/ch1.yomi"), "c");
    let (files, _) = collect(&dir.join("main.yomi"), None).unwrap();
    let mut names: Vec<_> = files.iter().map(|f| f.module.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["main", "story::ch1"]);
}

#[test]
fn entry_file_is_collected_even_under_hidden_files_or_dirs() {
    let dir = scratch("hidden-entry");
    write(&dir.join(".work/main.yomi"), "m");
    write(&dir.join(".work/lib.yomi"), "l");
    write(&dir.join(".other/x.yomi"), "x");
    // Collect under the hidden directory that contains the entry file. Do not collect other hidden directories
    let (files, entry) = collect(&dir.join(".work/main.yomi"), Some(&dir)).unwrap();
    assert_eq!(files[entry].source, "m");
    let mut names: Vec<_> = files.iter().map(|f| f.module.as_str()).collect();
    names.sort();
    assert_eq!(names, vec![".work::lib", ".work::main"]);
}
