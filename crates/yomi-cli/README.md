# yomi-cli

English | [日本語](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-cli/README.ja.md)

The `yomi` command of the yomi language: a small, statically typed scripting language that you embed in a Rust application, whose scripts can stop, be saved and resume.

```bash
cargo install yomi-cli
yomi check main.yomi     # check without running
yomi run main.yomi       # run, stopping at each stop point
yomi repl                # try expressions, statements and declarations
```

- `yomi lsp`: a Language Server for editors (diagnostics, completion, go to definition, references, hover, rename)
- `yomi mcp`: an MCP server for AI agents (checking, listing reserved words and diagnostic codes, reading the specification, running in a sandbox). Put `{ "command": "yomi", "args": ["mcp"] }` in the client's configuration.
- There are also `assign-ids` (add stable IDs), `decls` (declaration files), `codes` (list diagnostic codes) and `texts` (text replacement tables). See `yomi --help` for the full list.
- `lsp` and `mcp` are features (enabled by default) and can be left out: `cargo install yomi-cli --no-default-features`.
- Messages are shown in English or Japanese (`--lang`, or the environment variables `YOMI_CLI_LANG`, `LANG` and so on).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
