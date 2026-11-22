# yomi-std

English | [日本語](https://github.com/lovelygenerator/yomi-lang/blob/main/crates/yomi-std/README.ja.md)

The standard packages of the yomi language: `flow`, `math`, `text`, `json`, `audio`, `image`, `screen`, `input` and `event`.

```rust
let mut reg = yomi_core::Registry::new();
yomi_std::register_all(&mut reg)?; // registers all of them at once
```

To pick packages, use `yomi_core::Registry::package`. The audio, image and screen effects keep state and cues. At a stop point, the host reads them with `Vm::package_state`, plays the sounds and draws the pictures. The specification of each package is in `spec/packages/` of the [repository](https://github.com/lovelygenerator/yomi-lang) (written in Japanese).

While the version is 0.x, the commands and the shape of the state of the packages may change.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
