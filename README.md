<!-- markdownlint-disable MD013 -->
# blkit

Experimental compiler and single-node development server for type-safe `.bl` business processes. The compiler emits Rust; the dev server runs a compiled process graph.

```sh
cargo run -- examples/approve.bl /tmp/approve.rs
```

The generated file can be included in a Rust crate with dependencies on `blkit`, `serde` (derive), `serde_json`, and `rust_decimal` (serde-str), and `chrono` (serde) when temporal types or constructors are used. Compile that crate with `cargo build` or `cargo test`. The example in [`examples/approve.bl`](examples/approve.bl) is compiled and executed by the test suite.

The `blkit` CLI accepts `blkit SOURCE.bl OUTPUT.rs` for one file, `blkit transpile [PROJECT_DIR]` to generate a Rust Cargo project, and `blkit update [PROJECT_DIR]` to refresh its lockfile. Project commands default to the current directory. `blkit build` is no longer supported: compile the generated project yourself with Cargo. Run `blkit --help` or `blkit transpile --help` for usage, `blkit --version` for the CLI version, or `blkit --completions bash` to print shell completion (also supports `elvish`, `fish`, `powershell`, and `zsh`). From this checkout, replace `blkit` with `cargo run --` in these examples. Project `transpile` and `update` show a temporary spinner on an interactive stderr terminal; errors include the file or project path on stderr. Redirected stderr, `NO_COLOR`, and `CLICOLOR=0` disable styling and animation, so scripts receive plain diagnostics. Completion scripts go to stdout without progress output.

## Build a blkit project

Put a `blkit.toml` at the root of your project; you do not need to author a Rust project or list of `.bl` files. For example:

```toml
[project]
name = "orders"
blkit = "0.1.0"
build_target = "crate"
```

Create `types.bl`:

```bl
namespace orders
version "1"
type Order:
  amount: Number
task echo(input: Order) -> Order:
  return input
```

Create `process.bl`:

```bl
namespace orders
version "1"
process route(input: Order) -> Order:
  node start = start
  node value = task echo(input)
  node done = end
  link start -> value
  link value -> done(value)
```

Run `blkit transpile` from the directory containing `blkit.toml` (or `blkit transpile /path/to/project`; from a blkit checkout, `cargo run -- transpile /path/to/project`). This generates `.blkit/Cargo.toml` and Rust source, **not a compiled library**. Run `cargo build --manifest-path .blkit/Cargo.toml` yourself, locally or on a build machine with the generated project and its dependencies. The generated library exposes `orders::named_graph_definitions()` and can be used by another Rust crate with `orders = { path = "/path/to/project/.blkit" }` in its `Cargo.toml`. Transpilation discovers `.bl` files recursively, excluding hidden directories, `.blkit/`, and `target/`; files with the same namespace and version share types, tasks, and decisions, while different namespace/version pairs remain isolated. Every discovered source must validate.

Custom async tasks live in Cargo extension crates listed under `[dependencies]` in `blkit.toml`. For example, add `payments = { version = "0.1.0", path = "payments" }` to `[dependencies]`; `payments/Cargo.toml` declares a `payments` package at version `0.1.0` with `serde_json = "1"`. Add `payments/blkit-tasks.toml`:

```toml
[[tasks]]
name = "charge"
function = "charge"
input = "Order"
output = "Receipt"
```

In `payments/src/lib.rs`, export:

```rust
pub async fn charge(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"id": "ok"}))
}
```

In `orders.bl` (alongside the project manifest), declare the types and process:

```bl
namespace orders
version "1"
type Order:
  amount: Number
type Receipt:
  id: String
process charge_order(input: Order) -> Receipt:
  node start = start
  node paid = task payments.charge(input)
  node done = end
  link start -> paid
  link paid -> done(paid)
```

Run `blkit transpile`, then build the generated project with Cargo. A Rust consumer can start this process with `Engine::new(Registry::new_named(orders::named_graph_definitions())?, Store::open("orders.db").await?, 2)?` and `engine.start("orders", "1", "charge_order", serde_json::json!({"amount": "5"})).await?`; the completed result is `{"id":"ok"}`. Use `Engine` or `DistributedWorker` to execute async graphs: the synchronous `GraphDefinition::run` helper rejects async task nodes instead of blocking. Missing providers fail transpilation; incompatible Rust callables fail the user's Cargo build; invalid input/output JSON or a provider `Err` fails execution (subject to the process retry policy). The descriptor's types resolve in the process namespace/version. Task effects are **at least once**: retries or worker loss can call the task again after an effect but before its result commits. Make external effects idempotent (for example, use a stable idempotency key); cancellation or timeout drops an in-flight async future but cannot undo effects already made.

For a distributed worker, set `build_target = "worker"` in `blkit.toml`, run `blkit transpile` and `cargo build --manifest-path .blkit/Cargo.toml`, then start `.blkit/target/debug/orders-worker "$POSTGRES_URL" 32 5000` (`orders` is the project name; optional arguments are maximum concurrent tasks and lease milliseconds). If `CARGO_TARGET_DIR` is set, use `$CARGO_TARGET_DIR/debug/orders-worker` instead. `POSTGRES_URL` is a PostgreSQL connection URL, for example `postgres://postgres:postgres@127.0.0.1:5432/orders`; the database must be reachable and writable. The worker registers only the compiled namespace/version/process identities and runs matching queued work. It **does not expose HTTP**: deploy a separate distributed API using the same generated library, `DistributedControl::new(PostgresStore::connect(url).await?, orders::named_graph_definitions())`, and `server::router_distributed`. The repository's `blkit-api` binary embeds `examples/graph.bl`, not your project definitions, so it is not that API as-is.

For one local server (no PostgreSQL or separate API), set `build_target = "server"` and transpile the example `orders.bl` above, then build it with Cargo:

```sh
blkit transpile
cargo build --manifest-path .blkit/Cargo.toml
.blkit/target/debug/orders-server ./orders.db 32 127.0.0.1:3000
# In another shell:
curl -sS -X POST -H 'content-type: application/json' -d '{"amount":"5"}' http://127.0.0.1:3000/processes/orders/1/charge_order/instances
# Use the returned id:
curl -sS http://127.0.0.1:3000/instances/ID
```

The completed result is `{"id":"ok"}`. Server arguments are optional: local database file (default `blkit.db`), concurrent task limit (default `32`), and bind address (default `127.0.0.1:3000`). If `CARGO_TARGET_DIR` is set, the binary is at `$CARGO_TARGET_DIR/debug/orders-server`. The local server persists checkpoints and recovers interrupted work on restart; unlike a distributed worker, it uses a local database and hosts REST in the same process. The HTTP router has no authentication; keep the default loopback bind unless a trusted proxy supplies access control and TLS.

### Generated worker/server operational logs

Generated worker and server binaries log to **stdout at INFO level** by default. Set `BLKIT_LOG_LEVEL` to `trace`, `debug`, `info`, `warn`, or `error`. Set `BLKIT_LOG_OUTPUTS` to a nonempty comma-separated selection of `stdout`, `file`, and `otlp` (for example `stdout,file,otlp`). `file` requires `BLKIT_LOG_FILE` (an appendable path); `otlp` requires `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` (an HTTP(S) OTLP **logs** endpoint, such as `http://127.0.0.1:4318/v1/logs`). Invalid levels, duplicate/unknown outputs, and missing or invalid selected destinations fail startup with a diagnostic on stderr, before storage or work starts. Unselected destinations need no configuration. For example, after building the corresponding target:

```sh
BLKIT_LOG_OUTPUTS=stdout,file BLKIT_LOG_FILE=./orders.log .blkit/target/debug/orders-server ./orders.db 32 127.0.0.1:3000
BLKIT_LOG_OUTPUTS=stdout,file,otlp BLKIT_LOG_FILE=./worker.log OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://127.0.0.1:4318/v1/logs .blkit/target/debug/orders-worker "$POSTGRES_URL" 32 5000
```

A selected output receives operational events independently of the others. OTLP export is best effort: collector outages and abrupt termination can lose records without stopping instance execution. Files append across restarts without rotation; operators own rotation and retention. Blkit logs do not contain process input or result payloads, but cannot sanitize logs from third-party task crates. **Persisted instance status, errors, and checkpoints** remain the source of truth for execution outcomes; operational logs are not an event history.

Commit `blkit.toml`, your `.bl` files, `.blkit/.gitignore`, and `.blkit/Cargo.lock` once Cargo creates the lockfile; generated Cargo manifest, source, and build artifacts are ignored. Referenced custom task crates require Cargo to be installed even when transpiling locally and building elsewhere, because blkit uses Cargo metadata to find their task descriptors. Built-in-only projects can be transpiled without Cargo installed. If you change dependency requirements, run `blkit update` (or `blkit update /path/to/project`) to refresh the lockfile before `blkit transpile` and a later Cargo build. The project blkit version must exactly match the CLI version; `.bl` `version` identifies a business process, not the CLI. Generated manifests currently reference the CLI checkout's local blkit source when available; builds on another machine must have that source (or adjust the manifest to use a published blkit crate), along with any local path dependencies. Compiling and linking extension Rust callables is the user's Cargo build's responsibility.

A source file begins with `namespace name` and `version "text"`. Indent blocks by two spaces. Declare records with `type Name:` and `field: Type`, enums with `enum Name:` and one variant per line, tasks with `task name(input: Type) -> Type:`, and processes with `process name(input: Type) -> Type:`. Tasks use typed `return` statements; processes must link to a named `end` (or exceptional terminal). Supported types: `Bool`, `String`, decimal `Number`, ISO `Date` (`YYYY-MM-DD`), offset-free `Time` (`HH:MM:SS` with optional fractional seconds), offset-aware RFC 3339 `DateTime` (explicit `Z` or numeric offset), `List<T>`, and declared records/enums. JSON temporal values are strings in those formats; invalid calendar dates, times outside 00:00:00–23:59:59, leap seconds, time offsets and offset-free datetimes are rejected. Typed expression literals are `date("2026-10-02")`, `time("09:30:00.250")`, and `dateTime("2026-10-02T08:30:00Z")`; each requires a valid quoted literal (ordinary strings remain `String`). `DateTime` comparisons use instants, so different offsets representing the same instant compare equal. Task expressions support literals (including `[1, 2.5]`), input fields, enum variants, parentheses, `not`, `and`, `or`, equality/comparisons, and `if`/`else`. A standalone table value type, null, and external functions are not supported; decision tables are supported inside decision models.

Ranges use `[a..b]`, `(a..b)`, `[a..b)`, or `(a..b]` for inclusive/exclusive endpoints. For example, `input in [1..5]`, `input in (1..5]`, and `input between 1 and 5` return `Bool`; `between` includes both ends. Use `null` only for an unbounded range endpoint, such as `input in [1..null)` or `input in (null..null)`; `null` is not a general value. Endpoints must share a type (`Number`, `Date`, `DateTime`, or `Time`), for example `input in [date("2026-10-02")..null)`. Statically reversed bounds are rejected; runtime-reversed bounds are empty. Ranges can be compared using `==` and `!=` (not `=`). `before(A, B)`/`after(A, B)` compare strictly ordered ranges; `meets(A, B)`/`metBy(A, B)` compare touching finite endpoints even when open. `overlaps(A, B)` needs a common value (empty ranges never overlap), while `overlapsBefore(A, B)` means A starts first, B starts before A ends, and A ends before B; `overlapsAfter` reverses the arguments. Unbounded starts sort before finite starts and unbounded ends after finite ends. `coincides(A, B)` compares effective endpoints and inclusion. Point functions take `includes(range, value)`, `during(value, range)`, `starts(value, range)`, `startedBy(range, value)`, `finishes(value, range)`, and `finishedBy(range, value)`; start/end points must be finite and included. For example, `starts(1, [1..5])` is true and `starts(1, (1..5])` is false.

A process uses named nodes and explicit, ordered `link` declarations (see [`examples/graph.bl`](examples/graph.bl)). A task node calls a typed task. A `business_rule` node calls a typed `.bl` decision model; [`examples/pricing.bl`](examples/pricing.bl) shows a table-driven quote with an expression-based input column and no-match default. Compile it with `cargo run -- examples/pricing.bl /tmp/pricing.rs`. Decision models can also contain `literal` and `context` nodes connected by `link` dependencies and typed `knowledge` functions; tables support `UNIQUE`, `ANY`, `FIRST`, `PRIORITY`, `RULE_ORDER`, `OUTPUT_ORDER`, and `COLLECT` (including `SUM`, `MIN`, `MAX`, `COUNT`). In table rules only, an input column may use comma-separated OR tests: `rule amount matches (< 10, [20..30]) -> 1` or `rule amount matches ([2..5], (2..5]) -> 1`. Each alternative compares the column with a same-type scalar or range; temporal columns use typed bounds such as `rule day matches ([date("2026-01-01")..date("2026-01-31")], >= date("2026-12-01")) -> true`. Combine the condition with `and`/`or` as usual; `matches` lists cannot appear in task expressions or table outputs. Existing Boolean rules and hit policies still apply. `and_split`, `or_split`, and `xor_split` pair with matching joins; AND links carry record-field labels (`as field`), and OR/XOR links carry Boolean conditions (`when ...`) with a final `else` fallback. A normal `end` receives the process's declared output type; `error`, `cancel`, and `terminate` are named process-wide terminals without payloads. For example:

```bl
process decide(input: Order) -> Decision:
  node start = start
  node amount = task total(input)
  node route = xor_split
  node high = task review(input)
  node low = task approve(input)
  node chosen = xor_join(route)
  node done = end
  link start -> amount
  link amount -> route
  link route -> high when amount > 1000
  link route -> low else
  link high -> chosen(high)
  link low -> chosen(low)
  link chosen -> done(chosen)
```

A `node called = subprocess child(input)` invokes a process in the same namespace and version, including one declared in another project `.bl` file. Give it one normal success link; optionally route a child's named `error`, `cancel`, or `terminate` with `link called -> handler on error`, `on cancel`, or `on terminate`. These handler links carry no child output. Unhandled child outcomes propagate with their terminal name, while parent cancellation and deadlines cannot be caught. Parent and child share one instance and task-capacity limit, with child progress checkpointed for waits, retries, and recovery. See [`examples/subprocess.bl`](examples/subprocess.bl): `cargo run -- examples/subprocess.bl /tmp/subprocess.rs` compiles it; running `parent` with Number inputs `2`, `0`, `-1`, and `11` produces `"2"`, `"100"`, `"200"`, and `"300"` respectively. Load *both* generated definitions into `Registry::new_named` (or `DistributedWorker`) to execute the parent.

Optionally declare `retry max_retries 2 retry_for "10m" retry_delay "1s" backoff exponential` before the nodes. `max_retries` counts additional attempts; both the first-failure time window and retry count must permit another attempt. Delays start at the declared minimum and double on later retries. Without a declaration, execution errors fail without retry. Named `error`, `cancel`, and `terminate` are business outcomes, not retryable execution errors. A process may declare `deadline queued "10m"` or `deadline first_claimed "10m"` before its nodes. `node hold = pause_for "1m"` and `node hold = pause_until input.closes_at` persist a wake time once; `pause_until` requires a `DateTime`. Waits release worker claims and survive restart without restarting their timer. A deadline expires even while queued, retrying, waiting, or executing; it produces `business-error` with reserved `terminal_name: "timeout"`, without retrying. In-flight synchronous work is cooperative: it may finish executing, but cannot commit after timeout.

Task nodes can repeat with `repeat_pre(repeated < 3) max_iterations 3 initial input` (zero iterations yields the typed initial value) or `repeat_post(repeated < 3) max_duration "1m"`; both bounds may be supplied, and exceeding one returns `business-error` with `terminal_name: "task-iteration-limit"`. Multi-instance nodes use `node batch = task echo each input parallel` (or `sequential`) when `input` is a `List<T>`; each item is passed to the typed task and the ordered `List<U>` result is available as `batch`. Empty lists yield empty results. [`examples/iteration.bl`](examples/iteration.bl) includes a cyclic graph with a required two-second deadline and a parallel multi-instance process. Its positive-input cycle repeats until timeout; the example is a limit demonstration, not productive work. Compile it with `cargo run -- examples/iteration.bl /tmp/blkit-iteration.rs`. External Rust/Python tasks are not supported yet.

**Breaking syntax change:** old single-body process `return` and implicit `run`/`join` sequencing are rejected. Migrate business expressions into typed tasks and explicitly link each process route to its terminal. Distributed API and worker binaries are described below.

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

`total` is a decimal string, so `Number` retains decimal precision. Start returns `202` and an ID. Status may be `pending`, `running`, `waiting`, or `retry-waiting` (with last failure, attempt count and next eligibility); terminal states are `completed` (typed `result`), `business-error` (with `terminal_name`), `failed` (execution `error`), `cancelled`, or `terminated`. A completed `decide` instance started with `1200` returns `"review"`. Status includes `queued_at_ms`, optional `first_claim_at_ms`, `wake_at` (when waiting), `deadline_origin`, `deadline_duration_ms`, and `deadline_at_ms`; times are Unix milliseconds. `first_claimed` deadlines have no `deadline_at_ms` until the first claim. Unknown identities return `404`, malformed/invalid input `400`, and cancelling a terminal instance returns `409` (repeating a completed cancellation is idempotent).

Independent graph branches run concurrently. Each completed task's typed output and routing/join state are checkpointed in Turso before successors start; a retry resumes at that checkpoint rather than rerunning committed tasks. In-flight or otherwise uncommitted tasks may run again after a failure, so external side effects are **at least once**, not exactly once; make them idempotent.

The first argument is the local in-process Turso database path (default `blkit.db`); the second is the shared maximum number of in-flight tasks (default `32`, must be positive). The optional third address defaults to loopback `127.0.0.1:3000`; binding elsewhere explicitly exposes an **unauthenticated development API**. Restarting with the same database keeps past results and checkpoints. A checkpointed pending instance starts, retry-waiting work resumes when eligible, and interrupted running work counts as an execution failure: it resumes from the last committed checkpoint after a permitted delay, or becomes failed if retry limits prohibit it. Accepted cancellation stays cancelled. Turso adds the checkpoint/retry columns to existing local databases on open; old active records without a checkpoint are marked interrupted/failed rather than guessed or replayed, while old terminal records remain intact. Cancellation stops further graph advancement and signals all in-flight tasks, but cannot preempt a synchronous `.bl` function already running or undo external side effects; late results are ignored.

## PostgreSQL integration tests

Run isolated PostgreSQL tests with the Docker-compatible Podman socket mounted in this devcontainer:

```sh
BLKIT_TESTCONTAINERS_HOST=host.containers.internal cargo test --test postgres
```

The devcontainer sets `BLKIT_TESTCONTAINERS_HOST` for new sessions; pass it explicitly in an already-running container. Outside Podman-in-a-container, omit it to use Testcontainers' discovered host. The Rust Testcontainers PostgreSQL module launches a fresh `postgres:17.6-alpine` database for each test, waits for readiness, and removes it afterward. No system PostgreSQL or manually managed database is needed.

To run the two-process failover example (kills the owner after B commits and proves C resumes without rerunning B):

```sh
BLKIT_TESTCONTAINERS_HOST=host.containers.internal cargo test --test postgres second_worker_process_resumes_c_after_owner_killed_without_replaying_b -- --nocapture
```

PostgreSQL keeps each instance as a durable queue row: `pending` or eligible `retry-waiting` rows remain present when claimed, with exact process identity, checkpoint, attempt metadata, owner, lease deadline and incrementing claim generation. Claims use `FOR UPDATE SKIP LOCKED`; committed checkpoint/terminal updates and lease renewals require the current unexpired owner and generation. Expired owners are reconciled by the API and worker loops using the policy persisted with each instance; a capable worker is required to claim the retry. A worker takes a positive lease duration in milliseconds (default `5000`) and renews it while executing; configure it longer than expected transient database pauses. Claims are fenced by owner, unexpired lease and generation.

## Running distributed API and workers

`build.rs` compiles [`examples/graph.bl`](examples/graph.bl) ahead of time to Rust, and `src/bin/blkit-api.rs` and `src/bin/blkit-worker.rs` both include its `named_graph_definitions()` at build time. To deploy another graph, replace the `build.rs` source with your `.bl` file and rebuild **both** binaries. No worker compiles `.bl` at runtime. Use a new `version` when changing graph semantics; same-identity graph compatibility is not checked. Keep old-version workers until their queued and claimed work finishes.

For a local demonstration **outside the devcontainer**, start PostgreSQL with a loopback-only port and a non-example password, then in separate terminals run:

```sh
# Terminal 1: PostgreSQL; remove the container when finished.
docker run --rm --name blkit-postgres -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
  -p 127.0.0.1:55432:5432 postgres:17.6-alpine

# Other terminals, with DATABASE_URL set to the matching private PostgreSQL address:
cargo build --bin blkit-api --bin blkit-worker
cargo run --bin blkit-api -- "$DATABASE_URL"                  # default 127.0.0.1:3000
cargo run --bin blkit-worker -- "$DATABASE_URL" 32 5000      # start two terminals
```

Set `DATABASE_URL=postgres://postgres:<password>@127.0.0.1:55432/postgres` on the same host; use URL-encoding for special password characters. In this devcontainer, the host Podman loopback is **not** reachable from inside the container: use a privately reachable PostgreSQL endpoint instead, or run the Testcontainers multi-worker command above. Start an instance with `curl -sS -H 'content-type: application/json' -d '{"total":"12"}' http://127.0.0.1:3000/processes/orders/1.0/parallel/instances`, then poll `/instances/ID`; its `result` is `{"left":"12","right":"12"}`. Queue rows remain inspectable even with no capable worker.

To drain a worker, read its ID from its `blkit-worker <id> ready` log line and execute `UPDATE workers SET draining=TRUE WHERE id='<id>';` in PostgreSQL. It stops claiming, renews existing leases until each attempt finishes or releases for retry, then unregisters and exits. Keep another worker with the **same exact namespace/version/process** for old-version backlog; new-version workers do not take it.

Both REST binaries bind to loopback by default. The API has **no built-in authentication or TLS**: for remote clients, put it behind an operator-managed authenticated TLS ingress; do not expose the listener or PostgreSQL port directly. Checkpoints prevent replay of committed tasks, but interrupted or uncommitted tasks and their external side effects are **at least once**. Make side-effecting tasks idempotent; lease fencing prevents stale database writes, not external effects.
