# Generated-project dependency baseline (before split)

From commit `7a73d98` (includes `0f62def` temporal work), with `examples/minimal.bl` and `[project] blkit = "0.1.0"`. Commands: `blkit transpile DIR` then `CARGO_NET_OFFLINE=true cargo tree --offline -e normal --manifest-path DIR/.blkit/Cargo.toml` for each `build_target`.

| Target | Tree lines | Unwanted normal dependencies observed |
| --- | ---: | --- |
| crate | 703 | `blkit`, `clap`, `indicatif`, `axum`, `turso`, `tokio-postgres`, `opentelemetry` |
| worker | 703 | `blkit`, `clap`, `indicatif`, `axum`, `turso`, `tokio-postgres`, `opentelemetry` |
| server | 703 | `blkit`, `clap`, `indicatif`, `axum`, `turso`, `tokio-postgres`, `opentelemetry` |

The full baseline trees and generated fixtures are at `/tmp/blkit-dependency-baseline.09UUHk/` for local comparison.

## After split (normal Cargo edges only)

Generated-project manifests were inspected with `cargo tree --offline -e normal --manifest-path ... -p orders` (split API with `-p orders-api`); captured trees are at `/tmp/blkit-role-trees.pPb0c8/`.

| Target | Tree lines | HTTP | Local store | PostgreSQL | OTLP |
| --- | ---: | --- | --- | --- | --- |
| crate | 45 | no | no | no | no |
| api-only | 232 | yes | no | yes | no |
| worker-only | 375 | no | no | yes | yes (operational logs) |
| api-worker-split worker | 375 | no | no | yes | yes (operational logs) |
| api-worker-split API | 232 | yes | no | yes | no |
| api-worker local | 597 | yes | yes | no | yes (operational logs) |
| api-worker remote | 422 | yes | no | yes | yes (operational logs) |

No generated role tree includes `blkit-transpiler`, `clap`, or `indicatif`. The split API's normal tree excludes the root generated library and all worker features; its Cargo build was tested independently. `CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/workspaces/blkit/target cargo test --locked --test cli --test generated` passed: 15 CLI and 58 generated tests. The generated tests took about 637 seconds. The example-graph build emitted two known generated-Rust warnings (parentheses and unused `Pair`).
