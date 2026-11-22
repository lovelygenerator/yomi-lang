// `yomi lsp` を起動して、VS Code につなぐ。
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const config = vscode.workspace.getConfiguration("yomi");
  const command = config.get("server.path") || "yomi";
  const args = ["lsp"];
  const decls = config.get("server.decls");
  if (decls) {
    args.push("--decls", decls);
  }
  client = new LanguageClient(
    "yomi",
    "yomi",
    { command, args },
    { documentSelector: [{ scheme: "file", language: "yomi" }] },
  );
  context.subscriptions.push(client);
  client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
