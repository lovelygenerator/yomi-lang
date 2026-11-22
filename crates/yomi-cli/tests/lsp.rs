//! Checks `yomi lsp` by actually running it with LSP messages (JSON-RPC with Content-Length).

#![cfg(feature = "lsp")]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_yomi"))
            .arg("lsp")
            .env("YOMI_CLI_LANG", "en")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("yomi lsp を起動できる");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, v: Value) {
        let body = v.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "サーバーが閉じた"
            );
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                len = n.parse().unwrap();
            }
        }
        let mut buf = vec![0u8; len];
        self.stdout.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let v = self.read();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Waits for the next `publishDiagnostics` (for the given URI).
    fn diagnostics(&mut self, uri: &str) -> Value {
        loop {
            let v = self.read();
            if v["method"] == "textDocument/publishDiagnostics" && v["params"]["uri"] == uri {
                return v["params"]["diagnostics"].clone();
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `file://` URI from a path (`file:///C:/…` on Windows; the `\\?\` of `canonicalize` is removed).
fn file_uri(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_start_matches("//?/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

#[test]
fn runs_from_initialize_through_diagnostics_hover_definition_completion_to_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let root = file_uri(&dir.path().canonicalize().unwrap());
    let uri = format!("{root}/main.yomi");
    let mut c = Client::start();
    let init = c.request(
        "initialize",
        json!({
            "processId": null,
            "capabilities": {},
            "rootUri": root,
        }),
    );
    let caps = &init["result"]["capabilities"];
    assert!(caps["hoverProvider"].as_bool().unwrap(), "{init}");
    assert!(caps["definitionProvider"].as_bool().unwrap());
    assert!(caps["completionProvider"].is_object());
    c.notify("initialized", json!({}));

    let text = "label start {\n  jump nowhere\n}\n\nlabel next {\n  end\n}\n";
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "yomi", "version": 1, "text": text } }),
    );
    let diags = c.diagnostics(&uri);
    let first = &diags[0];
    assert_eq!(first["severity"], 1, "{diags}");
    assert_eq!(first["range"]["start"]["line"], 1);
    assert_eq!(first["source"], "yomi");

    // Fixing and sending again makes the errors disappear
    let fixed = "label start {\n  jump next\n}\n\nlabel next {\n  end\n}\n";
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": fixed }]
        }),
    );
    let diags = c.diagnostics(&uri);
    assert!(
        diags.as_array().unwrap().iter().all(|d| d["severity"] != 1),
        "{diags}"
    );

    let at = json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 8 } });
    let def = c.request("textDocument/definition", at.clone());
    assert_eq!(def["result"]["range"]["start"]["line"], 4, "{def}");
    let hover = c.request("textDocument/hover", at);
    let value = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(value.contains("label next"), "{value}");
    let comp = c.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 7 } }),
    );
    let labels: Vec<&str> = comp["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"next"), "{labels:?}");

    let shutdown = c.request("shutdown", Value::Null);
    assert!(shutdown["error"].is_null(), "{shutdown}");
    c.notify("exit", Value::Null);
    assert!(c.child.wait().unwrap().success());
}

#[test]
fn migrations_file_is_not_treated_as_a_module() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.yomi"), "label start {\n  end\n}\n").unwrap();
    std::fs::write(dir.path().join("migrations.yomi"), "min_save_version 1\n").unwrap();
    let root = file_uri(&dir.path().canonicalize().unwrap());
    let uri = format!("{root}/main.yomi");
    let mut c = Client::start();
    c.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": root }),
    );
    c.notify("initialized", json!({}));
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "yomi", "version": 1, "text": "label start {\n  end\n}\n" } }),
    );
    let diags = c.diagnostics(&uri);
    assert!(diags.as_array().unwrap().is_empty(), "{diags}");
}

#[test]
fn formatting_returns_one_edit_for_unformatted_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = file_uri(&dir.path().canonicalize().unwrap());
    let uri = format!("{root}/main.yomi");
    let mut c = Client::start();
    let init = c.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": root }),
    );
    assert!(
        init["result"]["capabilities"]["documentFormattingProvider"]
            .as_bool()
            .unwrap()
    );
    c.notify("initialized", json!({}));
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "yomi", "version": 1, "text": "label start {\n  end\n}\n" } }),
    );
    let r = c.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": uri }, "options": { "tabSize": 4, "insertSpaces": true } }),
    );
    let edits = r["result"].as_array().unwrap();
    assert_eq!(edits.len(), 1, "{r}");
    assert_eq!(edits[0]["newText"], "label start {\n    end\n}\n");
}
