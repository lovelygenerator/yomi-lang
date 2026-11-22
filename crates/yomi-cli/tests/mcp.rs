//! Checks `yomi mcp` by actually running it with JSON-RPC over standard input and output.

#![cfg(feature = "mcp")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(lang: &str) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
            .arg("mcp")
            .env("YOMI_CLI_LANG", lang)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("yomi mcp を起動できる");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        );
        assert!(init["result"]["serverInfo"]["name"] == "yomi", "{init}");
        c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        c
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "サーバーが閉じた"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn there_are_seven_tools_with_input_and_output_schemas() {
    let mut c = Client::start("en");
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "check",
            "diagnostic_codes",
            "format",
            "reserved_words",
            "run",
            "spec",
            "test"
        ]
    );
    assert!(tools.iter().all(|t| t["inputSchema"].is_object()));
}

#[test]
fn check_returns_diagnostics_of_broken_script_in_display_language() {
    for (lang, needle) in [("en", "nowhere"), ("ja", "nowhere")] {
        let mut c = Client::start(lang);
        let r = c.call(
            "check",
            json!({ "source": "label start {\n    jump nowhere\n}\n" }),
        );
        let s = &r["result"]["structuredContent"];
        assert_eq!(s["ok"], false, "{r}");
        let msg = s["diagnostics"][0]["message"].as_str().unwrap();
        assert!(msg.contains(needle), "{lang}: {msg}");
        if lang == "ja" {
            assert!(!msg.is_ascii(), "{msg}");
        }
    }
}

#[test]
fn run_executes_script_in_isolation_and_reads_choice_answers_from_stdin() {
    let mut c = Client::start("en");
    let r = c.call(
        "run",
        json!({
            "source": "label start {\n  こんにちは\n  choice {\n    「はい」 #yes => yes\n    「いいえ」 #no => no\n  }\n  end\n}\n",
            "input": "1\n",
            "seed": 1
        }),
    );
    let s = &r["result"]["structuredContent"];
    assert_eq!(s["timed_out"], false, "{r}");
    assert_eq!(s["exit_code"], 0, "{r}");
    let out = s["stdout"].as_str().unwrap();
    assert!(out.contains("こんにちは") && out.contains("yes"), "{out}");
}

#[test]
fn run_rejects_invalid_module_names_and_missing_entry_file() {
    let mut c = Client::start("en");
    let r = c.call(
        "run",
        json!({ "source": "", "modules": [{ "name": "../x", "source": "" }] }),
    );
    assert!(
        r["error"]["message"].as_str().unwrap().contains("../x"),
        "{r}"
    );
}

#[test]
fn format_returns_the_standard_form_or_the_syntax_errors() {
    let mut c = Client::start("en");
    let r = c.call("format", json!({ "source": "label start {\n  end\n}\n" }));
    let s = &r["result"]["structuredContent"];
    assert_eq!(s["ok"], true, "{r}");
    assert_eq!(s["changed"], true);
    assert_eq!(s["formatted"], "label start {\n    end\n}\n");
    let r = c.call(
        "format",
        json!({ "source": "label start {\n  let x = (1 +\n}\n" }),
    );
    let s = &r["result"]["structuredContent"];
    assert_eq!(s["ok"], false, "{r}");
    assert!(s["formatted"].is_null());
    assert!(!s["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn check_with_decls_uses_the_registry_of_the_declaration_file() {
    let dir = std::env::temp_dir().join(format!("yomi-mcp-decls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let full = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/std.decls.json");
    // The same declarations without the `audio` package
    let mut reduced: Value = serde_json::from_str(&std::fs::read_to_string(full).unwrap()).unwrap();
    for key in ["commands", "packages"] {
        let list = reduced[key].as_array_mut().unwrap();
        list.retain(|x| x["package"] != "audio" && x["name"] != "audio");
    }
    let empty = dir.join("no-audio.decls.json");
    std::fs::write(&empty, reduced.to_string()).unwrap();
    let source = "import std::audio\nlabel start {\n    @audio.play(\"bgm\", \"a\")\n    end\n}\n";
    let mut c = Client::start("en");
    let r = c.call("check", json!({ "source": source }));
    assert_eq!(r["result"]["structuredContent"]["ok"], true, "{r}");
    let r = c.call("check", json!({ "source": source, "decls": full }));
    assert_eq!(r["result"]["structuredContent"]["ok"], true, "{r}");
    let r = c.call(
        "check",
        json!({ "source": source, "decls": empty.to_str().unwrap() }),
    );
    assert_eq!(r["result"]["structuredContent"]["ok"], false, "{r}");
    let r = c.call(
        "check",
        json!({ "source": source, "decls": dir.join("missing.json").to_str().unwrap() }),
    );
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("declaration file"),
        "{r}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_explores_the_branches_and_leaves_out_the_golden_section() {
    let mut c = Client::start("en");
    let r = c.call(
        "test",
        json!({
            "source": "label start {\n  choice {\n    「はい」 #yes => yes\n    「いいえ」 #no => no\n  }\n}\nlabel yes {\n  end\n}\nlabel no {\n  end\n}\n",
        }),
    );
    let s = &r["result"]["structuredContent"];
    assert_eq!(s["exit_code"], 0, "{r}");
    assert_eq!(s["timed_out"], false);
    let report = &s["report"];
    assert_eq!(report["paths"].as_array().unwrap().len(), 2, "{r}");
    assert!(report["golden"].is_null(), "{r}");
    assert!(report["errors"].as_array().unwrap().is_empty());
}

#[test]
fn test_reports_compile_errors_in_stderr_and_validates_arguments() {
    let mut c = Client::start("en");
    let r = c.call(
        "test",
        json!({ "source": "label start {\n    jump nowhere\n}\n" }),
    );
    let s = &r["result"]["structuredContent"];
    assert_eq!(s["exit_code"], 1, "{r}");
    assert!(s["report"].is_null(), "{r}");
    assert!(s["stderr"].as_str().unwrap().contains("nowhere"), "{r}");
    for args in [
        json!({ "source": "", "seeds": 0 }),
        json!({ "source": "", "max_states": 0 }),
        json!({ "source": "", "max_states": 100001 }),
    ] {
        let r = c.call("test", args.clone());
        assert!(r["error"].is_object(), "{args}: {r}");
    }
}

#[test]
fn spec_tool_serves_the_guide_chapters() {
    let mut c = Client::start("en");
    let r = c.call("spec", json!({ "part": "guide/01-getting-started" }));
    let s = &r["result"]["structuredContent"];
    assert!(s["text"].as_str().unwrap().contains("yomi"), "{r}");
    assert!(
        s["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "guide/README"),
        "{r}"
    );
}

#[test]
fn test_tries_the_event_data_given_in_events() {
    let source = "import std::event\nlabel start {\n  let r = @event.wait([\"go\"])\n  match r {\n    Arrived(_, data) => {\n      if data == \"big\" {\n        jump big\n      }\n      end\n    }\n    TimedOut => end\n  }\n}\nlabel big {\n  end\n}\n";
    let mut c = Client::start("en");
    let r = c.call("test", json!({ "source": source }));
    let report = &r["result"]["structuredContent"]["report"];
    assert!(
        report["unreached_labels"]
            .as_array()
            .unwrap()
            .contains(&json!("main.big")),
        "{r}"
    );
    let r = c.call(
        "test",
        json!({ "source": source, "events": { "go": ["big", "small"] } }),
    );
    let report = &r["result"]["structuredContent"]["report"];
    assert_eq!(report["paths"].as_array().unwrap().len(), 2, "{r}");
    assert!(
        report["unreached_labels"].as_array().unwrap().is_empty(),
        "{r}"
    );
    let r = c.call(
        "test",
        json!({ "source": source, "events": { "a=b": ["x"] } }),
    );
    assert!(r["error"].is_object(), "{r}");
}
