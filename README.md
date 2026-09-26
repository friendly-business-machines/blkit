# blkit

Experimental compiler and single-node development server for type-safe `.bl` business processes. The compiler emits Rust; the dev server runs a compiled process graph.

```sh
cargo run -- examples/approve.bl /tmp/approve.rs
```

The generated file can be included in a Rust crate with `rust_decimal = "1.39"` in its `Cargo.toml`. Compile that crate with `cargo build` or `cargo test`. The example in [`examples/approve.bl`](examples/approve.bl) is compiled and executed by the test suite.

A source file begins with `namespace name` and `version "text"`. Indent blocks by two spaces. Declare records with `type Name:` and `field: Type`, enums with `enum Name:` and one variant per line, and processes with `process name(input: Type) -> Type:`. Every process must return on all paths. Supported types: `Bool`, `String`, decimal `Number`, `List<T>`, and declared records/enums. Supported process expressions: literals (including `[1, 2.5]`), input fields, enum variants, parentheses, `not`, `and`, `or`, equality/comparisons, and `if`/`else`. Tables, null, and external functions are not supported.

Graph syntax (see [`examples/graph.bl`](examples/graph.bl)) declares `.bl` tasks with the same typed signature and body as a process. Inside a graph process, `run result = task(value)` creates a named task node and passes a typed input. A gateway uses `and:`, `or:`, or `xor:` followed by indented branches and a matching `join name` at the gateway's indentation. `and:` uses `branch name:` arms and `join name: RecordType`; `or:` and `xor:` use `when BoolExpression:` arms plus an `else:` fallback and `join name`. Gateway bodies can nest; `return name` supplies the process output. For example:

```bl
process decide(input: Order) -> Decision:
  run amount = total(input)
  xor:
    when amount > 1000:
      run high = review(input)
    else:
      run low = approve(input)
  join result
  return result
```

Existing single-body processes remain valid. Graphs are acyclic; external Rust/Python tasks, loops, and distributed execution are not supported yet.

## Run the development server

The build compiles [`examples/graph.bl`](examples/graph.bl) into the binary automatically; it never parses `.bl` per request. Start the server with:

```sh
cargo run --bin blkit-dev -- /tmp/blkit-dev.db 32 127.0.0.1:3000
```

In another terminal:

```sh
curl -sS -H 'content-type: application/json' -d '{"total":"1200"}' \
  http://127.0.0.1:3000/processes/orders/1.0/decide/instances
# Copy the returned id, then inspect or cancel that instance:
curl -sS http://127.0.0.1:3000/instances/INSTANCE_ID
curl -sS -X POST http://127.0.0.1:3000/instances/INSTANCE_ID/cancel
```

`total` is a decimal string, so `Number` retains decimal precision. Start returns `202` and an ID; status transitions through `pending`/`running` to `completed` (with `result`), `failed`, or `cancelled`. A completed `decide` instance started with `1200` returns `"review"`. Unknown identities return `404`, malformed/invalid input `400`, and cancelling a completed or failed instance `409`.

The first argument is the local in-process Turso database path (default `blkit.db`); the second is the shared maximum number of in-flight tasks (default `32`, must be positive). The optional third address defaults to loopback `127.0.0.1:3000`; binding elsewhere explicitly exposes an **unauthenticated development API**. Restarting with the same database keeps past results and marks nonterminal instances failed/interrupted without replaying them. Cancellation stops further graph advancement and signals all in-flight tasks, but cannot preempt a synchronous `.bl` function already running or undo external side effects; late results are ignored.
