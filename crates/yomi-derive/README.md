# yomi-derive

English | [日本語](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-derive/README.ja.md)

`#[derive(YomiType)]` for the yomi language. It turns a Rust `struct` or `enum` into a type that can be an argument or a return value of a script command.

Do not use it directly. Use `yomi_core::YomiType`, which `yomi-core` re-exports (the generated code refers to `::yomi_core`).

```rust
use yomi_core::YomiType;

#[derive(YomiType)]
#[yomi(namespace = "game")]
struct Chara { name: String, hp: i64 }
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
