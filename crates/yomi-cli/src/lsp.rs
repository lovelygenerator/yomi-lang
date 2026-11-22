//! `yomi lsp` (a Language Server that talks over standard input and output).
//!
//! Diagnostics, completion, definition, references, hover, formatting. Scripts are analyzed as one unit: all `.yomi` under the
//! workspace root (module names follow the same rules as `yomi run`). Synchronization is full text.

mod text;
mod uri;
mod workspace;

use std::str::FromStr;

use lingokit::Localizer;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CompletionOptions, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentHighlightParams,
    DocumentSymbolParams, GotoDefinitionParams, HoverProviderCapability, OneOf,
    PublishDiagnosticsParams, ReferenceParams, RenameOptions, RenameParams, ServerCapabilities,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    WorkspaceSymbolParams,
};
use serde_json::Value;
use yomi_core::Registry;

use uri::{path_to_uri, uri_to_path};
use workspace::Workspace;

/// Handles a request and returns the result (JSON). On failure, a message.
fn handle_request(ws: &mut Workspace, method: &str, params: Value) -> Result<Value, String> {
    use lsp_types::{CompletionParams, HoverParams};
    let bad = |e: serde_json::Error| e.to_string();
    let path_of = |uri: &Uri| uri_to_path(uri).ok_or_else(|| "file: 以外の URI です".to_string());
    match method {
        "textDocument/completion" => {
            let p: CompletionParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.completion(&path, at.position)).map_err(bad)
        }
        "textDocument/hover" => {
            let p: HoverParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position_params;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.hover(&path, at.position)).map_err(bad)
        }
        "textDocument/definition" => {
            let p: GotoDefinitionParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position_params;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.definition(&path, at.position)).map_err(bad)
        }
        "textDocument/references" => {
            let p: ReferenceParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.references(&path, at.position, p.context.include_declaration))
                .map_err(bad)
        }
        "textDocument/prepareRename" => {
            let p: TextDocumentPositionParams = serde_json::from_value(params).map_err(bad)?;
            let path = path_of(&p.text_document.uri)?;
            serde_json::to_value(ws.prepare_rename(&path, p.position)).map_err(bad)
        }
        "textDocument/rename" => {
            let p: RenameParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.rename(&path, at.position, &p.new_name)?).map_err(bad)
        }
        "textDocument/documentHighlight" => {
            let p: DocumentHighlightParams = serde_json::from_value(params).map_err(bad)?;
            let at = p.text_document_position_params;
            let path = path_of(&at.text_document.uri)?;
            serde_json::to_value(ws.highlights(&path, at.position)).map_err(bad)
        }
        "textDocument/documentSymbol" => {
            let p: DocumentSymbolParams = serde_json::from_value(params).map_err(bad)?;
            let path = path_of(&p.text_document.uri)?;
            serde_json::to_value(ws.document_symbols(&path)).map_err(bad)
        }
        "textDocument/formatting" => {
            let p: lsp_types::DocumentFormattingParams =
                serde_json::from_value(params).map_err(bad)?;
            let path = path_of(&p.text_document.uri)?;
            serde_json::to_value(ws.formatting(&path)).map_err(bad)
        }
        "workspace/symbol" => {
            let p: WorkspaceSymbolParams = serde_json::from_value(params).map_err(bad)?;
            serde_json::to_value(ws.workspace_symbols(&p.query)).map_err(bad)
        }
        _ => Err(format!("対応していないリクエスト: {method}")),
    }
}

/// Handles a notification. `true` if diagnostics should be sent again.
fn handle_notification(ws: &mut Workspace, method: &str, params: Value) -> bool {
    match method {
        "textDocument/didOpen" => {
            let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(params) else {
                return false;
            };
            let Some(path) = uri_to_path(&p.text_document.uri) else {
                return false;
            };
            ws.set_document(path, p.text_document.text);
            true
        }
        "textDocument/didChange" => {
            let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(params) else {
                return false;
            };
            let Some(path) = uri_to_path(&p.text_document.uri) else {
                return false;
            };
            // Synchronization is full text, so the last change is the whole content
            match p.content_changes.into_iter().last() {
                Some(change) => {
                    ws.set_document(path, change.text);
                    true
                }
                None => false,
            }
        }
        "textDocument/didClose" => {
            let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(params) else {
                return false;
            };
            if let Some(path) = uri_to_path(&p.text_document.uri) {
                ws.close_document(&path);
            }
            true
        }
        "textDocument/didSave" => {
            let _ = serde_json::from_value::<DidSaveTextDocumentParams>(params);
            ws.invalidate();
            true
        }
        _ => false,
    }
}

/// Sends the diagnostics of all files to the client.
fn publish(connection: &Connection, ws: &mut Workspace) {
    for (path, diagnostics) in ws.diagnostics() {
        let Some(uri) = path_to_uri(&path) else {
            continue;
        };
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        let note = Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            serde_json::to_value(params).expect("書き出せる"),
        );
        let _ = connection.sender.send(Message::Notification(note));
    }
}

fn capabilities() -> Value {
    let caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), "@".into()]),
            ..CompletionOptions::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        ..ServerCapabilities::default()
    };
    serde_json::to_value(caps).expect("書き出せる")
}

/// The workspace root, from the client's initialization parameters.
fn root_of(init: &Value) -> Option<std::path::PathBuf> {
    let from = |v: &Value| {
        v.as_str()
            .and_then(|s| Uri::from_str(s).ok())
            .and_then(|u| uri_to_path(&u))
    };
    init["workspaceFolders"]
        .get(0)
        .and_then(|f| from(&f["uri"]))
        .or_else(|| from(&init["rootUri"]))
}

/// `yomi lsp`. Runs until the client asks to exit or the connection closes.
pub fn run(loc: Localizer, registry: Registry) -> i32 {
    let (connection, io_threads) = Connection::stdio();
    let init = match connection.initialize(capabilities()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut ws = Workspace::new(root_of(&init), registry, loc);
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req).unwrap_or(true) {
                    break;
                }
                let Request { id, method, params } = req;
                let resp = match handle_request(&mut ws, &method, params) {
                    Ok(v) => Response::new_ok(id, v),
                    Err(e) => Response::new_err(id, ErrorCode::InvalidRequest as i32, e),
                };
                let _ = connection.sender.send(Message::Response(resp));
            }
            Message::Notification(note) => {
                if handle_notification(&mut ws, &note.method, note.params) {
                    publish(&connection, &mut ws);
                }
            }
            Message::Response(_) => {}
        }
    }
    drop(connection);
    let _ = io_threads.join();
    0
}

#[cfg(test)]
mod tests;
