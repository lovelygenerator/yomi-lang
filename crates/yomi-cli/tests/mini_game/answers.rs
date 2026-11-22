//! Makes input with empty lines from answers

use super::*;

/// The kind of stop point (decided from the last line of output).
#[derive(Debug, PartialEq)]
pub(super) enum Kind {
    /// Body or wait. Proceeds with an empty line.
    Next,
    /// Asking for an option number.
    Choice,
    /// Asking for the value of `input.*`.
    Input,
}

pub(super) fn kind_of(lines: &[String]) -> Kind {
    let last = lines
        .iter()
        .rev()
        .find(|l| !l.is_empty())
        .map(String::as_str);
    match last {
        Some(l) if l.starts_with("[input]") => Kind::Input,
        Some(l)
            if l.split_once(": ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) =>
        {
            Kind::Choice
        }
        _ => Kind::Next,
    }
}

/// A `yomi run` kept running. Each time a line is sent, reads the output up to the response to the sync signal (`:sync`).
pub(super) struct Driver {
    pub(super) stdin: ChildStdin,
    pub(super) out: BufReader<std::io::PipeReader>,
    pub(super) child: std::process::Child,
}

impl Driver {
    pub(super) fn start(dir: &Path, args: &[&str]) -> Driver {
        let (reader, writer) = std::io::pipe().unwrap();
        let mut cmd = yomi_command(dir, args);
        // Send standard output and standard error into the same pipe to keep the order they appear in
        cmd.stdin(Stdio::piped())
            .stdout(writer.try_clone().unwrap())
            .stderr(writer);
        let mut child = cmd.spawn().unwrap();
        drop(cmd);
        let stdin = child.stdin.take().unwrap();
        Driver {
            stdin,
            out: BufReader::new(reader),
            child,
        }
    }

    /// Sends `line` (or nothing) and reads up to the response to `:sync`. Returns the output lines and whether the script
    /// ended.
    pub(super) fn send(&mut self, line: Option<&str>) -> (Vec<String>, bool) {
        let mut text = String::new();
        if let Some(l) = line {
            text.push_str(l);
            text.push('\n');
        }
        text.push_str(":sync\n");
        // Writes after the end may fail (the reader can tell it ended)
        let _ = self.stdin.write_all(text.as_bytes());
        let _ = self.stdin.flush();
        let mut lines = Vec::new();
        loop {
            let mut buf = String::new();
            if self.out.read_line(&mut buf).unwrap() == 0 {
                return (lines, true);
            }
            let buf = buf.trim_end_matches(['\n', '\r']).to_string();
            if buf.contains(":sync") {
                return (lines, false);
            }
            assert!(
                !buf.contains("入力してください") && !buf.contains("整数ではありません"),
                "答えが停止点に合っていない: {buf}"
            );
            lines.push(buf);
        }
    }
}

/// Makes the whole standard input from answers (choice numbers, strings, y/n), filling in empty lines for body stop points.
pub(super) fn expand(dir: &Path, args: &[&str], answers: &[String]) -> String {
    let mut d = Driver::start(dir, args);
    let mut sent = String::new();
    let (mut lines, mut ended) = d.send(None);
    for answer in answers {
        while !ended && kind_of(&lines) == Kind::Next {
            sent.push('\n');
            (lines, ended) = d.send(Some(""));
        }
        assert!(!ended, "答え {answer:?} の前に台本が終わった");
        assert_ne!(kind_of(&lines), Kind::Next);
        sent.push_str(answer);
        sent.push('\n');
        (lines, ended) = d.send(Some(answer));
    }
    // Advance the bodies after the answers to the end
    while !ended {
        assert_eq!(
            kind_of(&lines),
            Kind::Next,
            "答えが足りない（最後の停止点: {:?}）",
            lines.last()
        );
        sent.push('\n');
        (lines, ended) = d.send(Some(""));
    }
    let _ = d.child.wait();
    sent
}

/// Reads `<name>.answers` (lines starting with `#` are comments; empty lines are answers of "just Enter").
pub(super) fn read_answers(name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(playthrough_file(name, "answers")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// If there is a play to do first, play it with the same persistent data file.
pub(super) fn play_before(dir: &Path, play: &Play, persistent: &str) {
    if let Some(before) = play.after {
        let before = PLAYS.iter().find(|p| p.name == before).unwrap();
        let o = run_play_in(dir, before, persistent, None);
        assert_eq!(o.code, 0, "{}: {}", before.name, o.stderr);
    }
}

/// Returns `<name>.in` after checking it against the input made from `<name>.answers` (rewrites it with `BLESS=1`).
pub(super) fn input_of(play: &Play) -> String {
    let dir = scratch(&format!("expand-{}", play.name));
    play_before(&dir, play, "p.bin");
    // Persistent data is written even during the expansion, so use a copy of the file after the earlier play
    if dir.join("p.bin").exists() {
        std::fs::copy(dir.join("p.bin"), dir.join("x.bin")).unwrap();
    }
    let main = main_yomi();
    let args = run_args(&main, "x.bin", None);
    let expanded = expand(&dir, &args, &read_answers(play.name));
    let path = playthrough_file(play.name, "in");
    if bless() {
        std::fs::write(&path, &expanded).unwrap();
    } else {
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} がない（BLESS=1 で作る）", path.display()));
        assert_eq!(
            actual, expanded,
            "{}.in が、{}.answers から作った入力と違う（BLESS=1 で書き直す）",
            play.name, play.name
        );
    }
    expanded
}

/// Runs one play in a working directory (`after` is not run).
pub(super) fn run_play_in(
    dir: &Path,
    play: &Play,
    persistent: &str,
    texts: Option<&str>,
) -> Output {
    let input = std::fs::read_to_string(playthrough_file(play.name, "in")).unwrap();
    let main = main_yomi();
    yomi(dir, &run_args(&main, persistent, texts), &input)
}

/// Plays `after` first, then runs one play.
pub(super) fn run_play(play: &Play, texts: Option<&str>) -> Output {
    let dir = scratch(play.name);
    play_before(&dir, play, "p.bin");
    run_play_in(&dir, play, "p.bin", texts)
}

/// Compares with the expected file (rewrites it with `BLESS=1`).
pub(super) fn check_golden(path: &Path, actual: &str) {
    if bless() {
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "{} がない（BLESS=1 で作る）。実際の出力:\n{actual}",
            path.display()
        )
    });
    assert_eq!(
        expected,
        actual,
        "{} と違う（意図した変更なら、出力を目で確かめて BLESS=1）",
        path.display()
    );
}
