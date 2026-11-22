//! Checks that the copies placed in each crate for the crates.io packages (`spec/` and `guide/` of `yomi-cli` and each crate's
//! `LICENSE-*`) are the same as the originals at the workspace root. To fix them, run `tools/sync-crate-files.sh`.
//!
//! If the originals at the root are missing (such as when only the package is extracted), it finishes without checking
//! anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The files in a directory (relative path → contents).
fn files_in(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap().to_path_buf();
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

const FIX: &str = "tools/sync-crate-files.sh を実行する";

#[test]
fn cli_spec_is_copy_of_root_spec() {
    assert_dir_is_copy("spec");
}

#[test]
fn cli_guide_is_copy_of_root_guide() {
    assert_dir_is_copy("guide");
}

fn assert_dir_is_copy(name: &str) {
    let root = workspace_root().join(name);
    if !root.is_dir() {
        return;
    }
    let original = files_in(&root);
    let copy = files_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join(name));
    let keys = |m: &BTreeMap<PathBuf, Vec<u8>>| m.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        keys(&copy),
        keys(&original),
        "ファイルの一覧が違う（{FIX}）"
    );
    for (path, text) in &original {
        assert!(copy[path] == *text, "{} が違う（{FIX}）", path.display());
    }
}

#[test]
fn each_crates_license_is_copy_of_root() {
    let root = workspace_root();
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        let Ok(original) = fs::read(root.join(name)) else {
            return;
        };
        for entry in fs::read_dir(root.join("crates")).unwrap() {
            let krate = entry.unwrap().path();
            let copy = fs::read(krate.join(name)).unwrap_or_default();
            assert!(
                copy == original,
                "{} の {name} が違う（{FIX}）",
                krate.display()
            );
        }
    }
}
