//! yomi run with `:reload` and `--watch`

use super::*;
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::time::Duration;

const V1: &str = "label start {\n    一行目 #a\n    二行目 #b\n    三行目 #c\n}\n";
const V2: &str = "label start {\n    一行目 #a\n    直した二行目 #b\n    三行目 #c\n}\n";

/// A running `yomi run` whose lines of output can be waited for.
struct Live {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
    stderr_path: PathBuf,
}

impl Live {
    fn start(test: &str, extra: &[&str], source: &str) -> (Live, PathBuf) {
        let dir = scratch(test);
        let script = dir.join("a.yomi");
        std::fs::write(&script, source).unwrap();
        let stderr_path = dir.join("stderr.txt");
        let mut args = vec!["--lang", "en", "run", script.to_str().unwrap()];
        args.extend_from_slice(extra);
        let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
            .args(&args)
            .env_remove("YOMI_CLI_LANG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr_path).unwrap())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        (
            Live {
                child,
                lines: rx,
                stderr_path,
            },
            script,
        )
    }

    fn send(&mut self, text: &str) {
        self.try_send(text).unwrap();
    }

    fn try_send(&mut self, text: &str) -> std::io::Result<()> {
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(text.as_bytes())?;
        stdin.flush()
    }

    fn line(&self) -> String {
        match self.lines.recv_timeout(Duration::from_secs(20)) {
            Ok(l) => l,
            Err(_) => panic!("no output line; stderr: {}", self.stderr()),
        }
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }

    fn finish(mut self) -> i32 {
        // A script that ran to its end has already made the process exit, so the pipe may be closed
        let _ = self.try_send(":quit\n");
        self.child.wait().unwrap().code().unwrap()
    }
}

#[test]
fn reload_command_swaps_in_the_edited_script_at_the_same_stop_point() {
    let (mut live, script) = Live::start("reload_cmd", &[], V1);
    assert_eq!(live.line(), "一行目");
    live.send("\n");
    assert_eq!(live.line(), "二行目");
    std::fs::write(&script, V2).unwrap();
    live.send(":reload\n");
    assert_eq!(live.line(), "直した二行目");
    live.send("\n");
    assert_eq!(live.line(), "三行目");
    let stderr = live.stderr();
    assert!(stderr.contains("[reload]"), "{stderr}");
    assert_eq!(live.finish(), 0);
}

#[test]
fn reload_command_keeps_running_the_old_script_when_the_new_one_does_not_compile() {
    let (mut live, script) = Live::start("reload_broken", &[], V1);
    assert_eq!(live.line(), "一行目");
    std::fs::write(&script, "label start {\n    @nothing_here(\n}\n").unwrap();
    live.send(":reload\n");
    live.send("\n");
    assert_eq!(live.line(), "二行目");
    let stderr = live.stderr();
    assert!(
        stderr.contains("still running the previous scripts"),
        "{stderr}"
    );
    assert_eq!(live.finish(), 0);
}

#[test]
fn reload_command_falls_back_to_the_nearest_stop_point_when_ids_are_gone() {
    let (mut live, script) = Live::start("reload_gone", &[], V1);
    assert_eq!(live.line(), "一行目");
    live.send("\n");
    assert_eq!(live.line(), "二行目");
    std::fs::write(
        &script,
        "label start {\n    別の行 #x\n}\nlabel other {\n    もう一つ\n}\n",
    )
    .unwrap();
    live.send(":reload\n");
    live.send("\n");
    assert_eq!(live.line(), "別の行");
    assert_eq!(live.finish(), 0);
}

#[test]
fn watch_reloads_by_itself_while_waiting_for_input() {
    let (mut live, script) = Live::start("watch_auto_reload", &["--watch"], V1);
    assert_eq!(live.line(), "一行目");
    live.send("\n");
    assert_eq!(live.line(), "二行目");
    // The watcher may not be ready the instant the first line appears, so write until it is noticed
    let mut shown = None;
    for i in 0..40 {
        std::fs::write(&script, format!("{V2}// edit {i}\n")).unwrap();
        if let Ok(l) = live.lines.recv_timeout(Duration::from_millis(500)) {
            shown = Some(l);
            break;
        }
    }
    assert_eq!(shown.as_deref(), Some("直した二行目"), "{}", live.stderr());
    assert!(live.stderr().contains("[reload]"));
    assert_eq!(live.finish(), 0);
}

#[test]
fn watch_cannot_be_combined_with_auto() {
    let p = scratch("watch_auto");
    let script = p.join("a.yomi");
    std::fs::write(&script, V1).unwrap();
    let o = yomi(&["run", script.to_str().unwrap(), "--watch", "--auto"], "");
    assert_eq!(o.code, 2, "{}", o.stderr);
}
