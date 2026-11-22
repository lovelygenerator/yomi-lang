//! `yomi dap` as an actual command: framed requests in, framed messages out.

use super::*;
use serde_json::{Value, json};
use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

const SCRIPT: &str = "fn twice(n: int) -> int {
    let m = n * 2
    return m
}

label start {
    let x = 1
    let y = twice(x)
    あいさつ
    let z = x + y
    z
}
";

/// A running `yomi dap`. A real client waits for events before sending the next request, so this does too.
struct Dap {
    child: std::process::Child,
    messages: mpsc::Receiver<Value>,
    seen: Vec<Value>,
    seq: i64,
    path: String,
}

impl Dap {
    fn start(test: &str) -> Dap {
        let dir = scratch(test);
        let script = dir.join("a.yomi");
        std::fs::write(&script, SCRIPT).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
            .args(["--lang", "en", "dap"])
            .env_remove("YOMI_CLI_LANG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_string();
                    let length: usize = head
                        .trim()
                        .trim_start_matches("Content-Length:")
                        .trim()
                        .parse()
                        .unwrap();
                    if buf.len() < i + 4 + length {
                        break;
                    }
                    let message = serde_json::from_slice(&buf[i + 4..i + 4 + length]).unwrap();
                    buf.drain(..i + 4 + length);
                    if tx.send(message).is_err() {
                        return;
                    }
                }
            }
        });
        Dap {
            child,
            messages: rx,
            seen: Vec::new(),
            seq: 0,
            path: script.to_str().unwrap().to_string(),
        }
    }

    fn send(&mut self, command: &str, args: Value) -> i64 {
        self.seq += 1;
        // The path goes into JSON text, so backslashes (Windows) and quotes must be escaped
        let escaped = serde_json::to_string(&self.path).unwrap();
        let args = args
            .to_string()
            .replace("$PATH", &escaped[1..escaped.len() - 1]);
        let body = format!(
            r#"{{"seq":{},"type":"request","command":"{command}","arguments":{args}}}"#,
            self.seq
        );
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin
            .write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        self.seq
    }

    /// Reads messages until `found` accepts one (and returns it). Fails after 10 seconds.
    fn wait(&mut self, found: impl Fn(&Value) -> bool) -> Value {
        loop {
            let m = self
                .messages
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| {
                    panic!(
                        "no message in time; seen: {:#?}",
                        self.seen.iter().rev().take(6).collect::<Vec<_>>()
                    )
                });
            self.seen.push(m.clone());
            if found(&m) {
                return m;
            }
        }
    }

    fn request(&mut self, command: &str, args: Value) -> Value {
        let seq = self.send(command, args);
        self.wait(|m| m["type"] == "response" && m["request_seq"] == seq)
    }

    fn event(&mut self, name: &str) -> Value {
        self.wait(|m| m["type"] == "event" && m["event"] == name)
    }

    fn output(&self) -> String {
        self.seen
            .iter()
            .filter(|m| m["event"] == "output")
            .map(|m| m["body"]["output"].as_str().unwrap().to_string())
            .collect()
    }

    fn finish(mut self) -> i32 {
        self.request("disconnect", json!({}));
        self.child.stdin.take();
        self.child.wait().unwrap().code().unwrap()
    }

    /// Launches and runs to the first stop.
    fn launch(&mut self, args: Value) {
        self.request("initialize", json!({}));
        self.request("launch", args);
    }
}

#[test]
fn it_stops_on_entry_and_reports_the_stack_and_variables() {
    let mut d = Dap::start("dap_entry");
    let init = d.request("initialize", json!({}));
    assert_eq!(init["body"]["supportsConfigurationDoneRequest"], true);
    d.event("initialized");
    assert!(
        d.request("launch", json!({ "program": "$PATH" }))["success"]
            .as_bool()
            .unwrap()
    );
    d.request("configurationDone", json!({}));
    assert_eq!(d.event("stopped")["body"]["reason"], "entry");
    let frame = &d.request("stackTrace", json!({ "threadId": 1 }))["body"]["stackFrames"][0];
    assert_eq!(frame["name"], "start");
    assert_eq!(frame["line"], 7);
    let scopes = d.request("scopes", json!({ "frameId": 0 }));
    assert_eq!(scopes["body"]["scopes"][0]["name"], "Locals");
    assert!(
        d.request("variables", json!({ "variablesReference": 1 }))["success"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(d.finish(), 0);
}

#[test]
fn breakpoints_are_verified_moved_and_hit() {
    let mut d = Dap::start("dap_break");
    d.launch(json!({ "program": "$PATH", "stopOnEntry": false }));
    let r = d.request(
        "setBreakpoints",
        json!({ "source": { "path": "$PATH" }, "breakpoints": [{ "line": 2 }, { "line": 4 }, { "line": 99 }, { "line": 3, "condition": "x" }] }),
    );
    let b = &r["body"]["breakpoints"];
    assert_eq!(b[0]["verified"], true);
    assert_eq!(b[1]["line"], 6);
    assert_eq!(b[2]["verified"], false);
    assert_eq!(b[3]["verified"], false);
    d.request("configurationDone", json!({}));
    assert_eq!(d.event("stopped")["body"]["reason"], "breakpoint");
    let frame = &d.request("stackTrace", json!({}))["body"]["stackFrames"][0];
    assert_eq!(frame["name"], "twice");
    assert_eq!(d.finish(), 0);
}

#[test]
fn steps_answers_a_stop_point_and_ends() {
    let mut d = Dap::start("dap_stop");
    d.launch(json!({ "program": "$PATH", "stopOnEntry": false }));
    d.request(
        "setBreakpoints",
        json!({ "source": { "path": "$PATH" }, "breakpoints": [{ "line": 9 }] }),
    );
    d.request("configurationDone", json!({}));
    assert_eq!(d.event("stopped")["body"]["reason"], "breakpoint");
    // Running the plain line shows it and waits for an answer at the stop point.
    d.request("next", json!({}));
    let stop = d.event("stopped");
    assert_eq!(stop["body"]["reason"], "step");
    assert!(d.output().contains("あいさつ"), "{}", d.output());
    assert_eq!(
        d.request(
            "evaluate",
            json!({ "expression": "x", "context": "watch", "frameId": 0 })
        )["body"]["result"],
        "1"
    );
    assert!(d.request("continue", json!({}))["success"] == false);
    assert!(
        d.request("evaluate", json!({ "expression": "", "context": "repl" }))["success"]
            .as_bool()
            .unwrap()
    );
    // The answer resumes with `next`, which pauses on the following line.
    assert_eq!(d.event("stopped")["body"]["reason"], "step");
    d.request("continue", json!({}));
    // The last plain line is a stop point too; answering it with `continue` as the motion runs to the end.
    assert_eq!(
        d.event("stopped")["body"]["description"],
        "waiting for an answer at a stop point"
    );
    d.request("evaluate", json!({ "expression": "", "context": "repl" }));
    d.event("terminated");
    assert_eq!(d.event("exited")["body"]["exitCode"], 0);
    assert!(d.output().contains("(end)"));
    assert_eq!(d.finish(), 0);
}

#[test]
fn unknown_requests_and_bad_launch_fail() {
    let mut d = Dap::start("dap_bad");
    d.request("initialize", json!({}));
    assert_eq!(d.request("frobnicate", json!({}))["success"], false);
    assert_eq!(d.request("launch", json!({}))["success"], false);
    d.event("terminated");
    assert_eq!(d.request("stackTrace", json!({}))["success"], false);
    assert_eq!(d.finish(), 0);
}

#[test]
fn it_exits_with_two_when_given_arguments() {
    let o = yomi(&["dap", "extra"], "");
    assert_eq!(o.code, 2);
}
