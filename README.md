# blkit

Experimental compiler for type-safe `.bl` business decisions. It emits Rust source; it does not run a server or execute workflows.

```sh
cargo run -- examples/approve.bl /tmp/approve.rs
```

The generated file can be included in a Rust crate with `rust_decimal = "1.39"` in its `Cargo.toml`. Compile that crate with `cargo build` or `cargo test`. The example in [`examples/approve.bl`](examples/approve.bl) is compiled and executed by the test suite.

A source file begins with `namespace name` and `version "text"`. Indent blocks by two spaces. Declare records with `type Name:` and `field: Type`, enums with `enum Name:` and one variant per line, and processes with `process name(input: Type) -> Type:`. Every process must return on all paths. Supported types: `Bool`, `String`, decimal `Number`, `List<T>`, and declared records/enums. Supported process expressions: literals (including `[1, 2.5]`), input fields, enum variants, parentheses, `not`, `and`, `or`, equality/comparisons, and `if`/`else`. Tables, null, external functions, and runtime services are not part of this MVP.
