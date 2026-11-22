//! Startup and exit codes (section 7.1 of `spec/host/07-repl.md`)

use super::*;

#[test]
fn script_compile_errors_exit_1_without_starting_repl() {
    let path = fixture("typo.yomi");
    let o = yomi(&["--lang", "en", "repl", path.to_str().unwrap()], "1\n");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(o.stderr.contains("error:"), "{}", o.stderr);
}

#[test]
fn unreadable_script_file_exits_1() {
    let o = yomi(&["--lang", "en", "repl", "/nonexistent/x.yomi"], "1\n");
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(o.stderr.contains("cannot read"), "{}", o.stderr);
}

#[test]
fn argument_errors_exit_2() {
    let o = yomi(&["--lang", "en", "repl", "--nonexistent"], "");
    assert_eq!(o.code, 2);
    let o = yomi(&["--lang", "en", "repl", "--seed", "x"], "");
    assert_eq!(o.code, 2);
}

#[test]
fn repl_can_start_with_script_without_start_label() {
    let dir = scratch("no_start");
    let file = dir.join("nostart.yomi");
    std::fs::write(&file, "save var n: int = 4\nlabel other {\n  あ\n}\n").unwrap();
    let o = yomi(
        &["--lang", "en", "repl", file.to_str().unwrap()],
        "n\ncall other()\n\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "4\nあ\n");
}

#[test]
fn same_seed_gives_same_random_sequence() {
    let a = repl_empty("random(1, 1000000)\nrandom(1, 1000000)\n");
    let b = repl_empty("random(1, 1000000)\nrandom(1, 1000000)\n");
    assert_eq!(a.code, 0);
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn persistent_is_read_only_and_does_not_rewrite_file() {
    let dir = scratch("persistent");
    let persist = dir.join("persist.bin");
    // Play one round and make persistent data
    let script = fixture("persist.yomi");
    let o = yomi(
        &[
            "--lang",
            "en",
            "run",
            script.to_str().unwrap(),
            "--persistent",
            persist.to_str().unwrap(),
        ],
        "\n\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let before = std::fs::read(&persist).unwrap();
    let o = yomi(
        &[
            "--lang",
            "en",
            "repl",
            script.to_str().unwrap(),
            "--persistent",
            persist.to_str().unwrap(),
        ],
        "plays\nbest\nplays = 99\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "1\n10\n");
    assert_eq!(std::fs::read(&persist).unwrap(), before);
}

#[test]
fn missing_persistent_file_starts_new_and_broken_one_exits_1() {
    let dir = scratch("persistent_bad");
    let script = fixture("persist.yomi");
    let missing = dir.join("missing.bin");
    let o = yomi(
        &[
            "--lang",
            "en",
            "repl",
            script.to_str().unwrap(),
            "--persistent",
            missing.to_str().unwrap(),
        ],
        "plays\n",
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "0\n");
    assert!(!missing.exists());
    let bad = dir.join("bad.bin");
    std::fs::write(&bad, [1u8, 2, 3]).unwrap();
    let o = yomi(
        &[
            "--lang",
            "en",
            "repl",
            script.to_str().unwrap(),
            "--persistent",
            bad.to_str().unwrap(),
        ],
        "plays\n",
    );
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(
        o.stderr.contains("cannot read the persistent data"),
        "{}",
        o.stderr
    );
}

#[test]
fn audio_and_image_cues_come_before_stop_points_and_end_of_input() {
    let o = repl_empty("import std::audio\n@audio.se(\"door\")\n\\あ\n\n");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "[audio] se door\nあ\n");
}
