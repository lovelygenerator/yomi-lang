# yomi

[![CI](https://github.com/lovelygenerator/yomi-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/lovelygenerator/yomi-lang/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/yomi-core.svg)](https://crates.io/crates/yomi-core)
[![docs.rs](https://img.shields.io/docsrs/yomi-core)](https://docs.rs/yomi-core)
[![MSRV](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](#usage)
[![License](https://img.shields.io/crates/l/yomi-core.svg)](#license)

English | [日本語](README.ja.md)

> 詠めば、動く。 (*Recite it, and it runs.*)

yomi (詠み, "reciting") is a small, statically typed scripting language that you embed in a Rust application. A running script can stop in the middle, have its whole state saved as bytes, and resume later, even from a save made before the script was edited.

## Features

- **Stop, save, resume**: a script stops at stop commands that the host registers, and continues when the host returns a value. At a stop point, the call stack, variables and random state are saved together as one byte string, and an in-memory copy (rollback) can be taken as well. When a script is edited, stable IDs at the end of lines realign the saved position.
- **Static types**: local type inference with static types. Type errors and undefined names are found before the script runs, and the same diagnostics appear in the editor.
- **Deterministic**: the random state is part of the save, and the core never reads the clock. The same script and the same input always give the same result (replays, reproducible tests).
- **Sandboxed**: a script can only do what the host registers as commands. Steps per run, value size, total memory and call depth are all limited.
- **Easy to use from Rust**: register Rust functions and closures as commands as they are. Argument and return types come from the signature. Your own `struct` and `enum` can be passed with `#[derive(YomiType)]`, and failures are returned as `Result`.
- **Safe Rust only**: every crate has `#![forbid(unsafe_code)]`. There is no GC (values are reference counted) and no C toolchain is needed.
- **Thread-friendly**: `Program`, `Registry` and `Vm` are `Send + Sync`. Share the compiled script and the registry between threads, and run a VM on a worker thread or inside a game engine.
- **Internationalization**: identifiers may use Japanese and other scripts (UAX #31), strings are counted in grapheme clusters, and the text shown to players can be swapped per language, keyed by stable IDs.

## Use cases

- Dialogue and scripts: visual novels, game conversations, quests and tutorials
- Scripts written by users: mods, rule settings, automation
- Scripts written by AI agents: check them first, then run them with limits
- Long-running procedures that wait for human input along the way

## Example

Script (`main.yomi`):

```
save var total: int = 0

label start {
  let n = @ask("How many?")
  total += n
  end
}
```

Host (Rust):

```rust
use yomi_core::{Input, Registry, Value, Vm, VmConfig, Yield, compile};

let mut reg = Registry::new();
reg.stop::<(String,), i64>("ask")?; // a command that stops the script and asks the host for an integer
let program = compile(SOURCE, "main", &reg).map_err(|d| format!("{d:?}"))?;

let mut vm = Vm::new(&program, &reg, VmConfig::default())?;
let Yield::Stop { args, .. } = vm.run()? else { unreachable!() }; // stops at `@ask`
let bytes = vm.save()?; // save the stop point

// Later (even in another process), resume
let (mut vm, _) = Vm::load(&program, &reg, VmConfig::default(), &bytes)?;
vm.resume(Input::Return(Value::Int(42)))?;
assert_eq!(vm.run()?, Yield::End);
```

## Sugar for dialogue

yomi has syntax sugar for writing conversations. The host binds it to stop commands.

```
identity shopkeeper = "The shopkeeper"
save var favor: int = 0

label start {
    The shopkeeper leans out from the back of the shop.
    shopkeeper「Welcome! What are you looking for today?」 #a3f9k2
    choice {
        「Ask about the request」 #quest => { favor += 1; jump quest }
        「Come back later」       #leave => jump village
    }
}
```

- Plain lines (narration), quote lines (`name「text」`) and choices (`choice { … }`) each become a call to a stop command of the host.
- The `#id` at the end of a line is a stable ID. It is used to find the saved position and to swap in translated text (`yomi assign-ids` can add the IDs for you).

## Status

Under development. While the version is 0.x, the grammar, the API and the save format may change.

- `crates/yomi-core`: the core of the language (parsing, type checking, VM, save)
- `crates/yomi-std`: the standard packages (`flow` `math` `text` `json` `audio` `image` `screen` `input` `event`)
- `crates/yomi-cli`: the `yomi` command (`check`, `run`, `repl`, `assign-ids` and more). It also has a Language Server for editors (`yomi lsp`) and an MCP server for AI agents (`yomi mcp`).
- `crates/yomi-derive`: the derive macro for command argument and return types
- `crates/yomi-bevy`: a Bevy 0.20 plugin (running scripts, messaging, `.yomi` assets and hot reload). Because Bevy is a heavy dependency, it lives in a separate workspace and is not published to crates.io. Run `cd crates/yomi-bevy && cargo test`. The windowed examples are `cargo run --example dialogue_ui --features window` (a small dialogue) and `cargo run --example mini_game_ui --features window` (play `examples/mini-game`).
- `editors/vscode`: a VS Code extension (syntax highlighting and a connection to `yomi lsp`)
- `guide/`: a guide (an introduction for developers who embed yomi in Rust; it follows a purchase-approval flow through scripts, host, save and resume, event waits and tests). Written in Japanese.
- `spec/`: the specification (language, host integration, packages). Written in Japanese.
- `examples/`: sample scripts

## Usage

Rust 1.88 or later (edition 2024) is required.

```toml
[dependencies]
yomi-core = "0.1"
yomi-std = "0.1"   # when you use the standard packages
```

```bash
cargo install yomi-cli   # the `yomi` command
```

## Build

```bash
cargo build --release
cargo test
./target/release/yomi --help
```

## From editors and AI agents

- `yomi lsp`: diagnostics, completion, go to definition, references, hover and rename over LSP. For VS Code, use the extension in [editors/vscode](editors/vscode).
- `yomi mcp`: checking scripts, listing reserved words and diagnostic codes, reading the specification and running scripts in a sandbox, over MCP. Put `{ "command": "yomi", "args": ["mcp"] }` in the client's configuration.
- Both can be left out with build features (`lsp` and `mcp`, enabled by default).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
