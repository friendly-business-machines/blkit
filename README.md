# blkit

Experimental compiler and single-node development server for type-safe `.bl` business processes. The compiler emits Rust; the dev server runs a compiled process graph.

```sh
cargo run -- examples/approve.bl /tmp/approve.rs
```

The generated file can be included in a Rust crate with dependencies on `blkit`, `serde` (derive), `serde_json`, and `rust_decimal` (serde-str). Compile that crate with `cargo build` or `cargo test`. The example in [`examples/approve.bl`](examples/approve.bl) is compiled and executed by the test suite.

A source file begins with `namespace name` and `version "text"`. Indent blocks by two spaces. Declare records with `type Name:` and `field: Type`, enums with `enum Name:` and one variant per line, tasks with `task name(input: Type) -> Type:`, and processes with `process name(input: Type) -> Type:`. Tasks use typed `return` statements; processes must link to a named `end` (or exceptional terminal). Supported types: `Bool`, `String`, decimal `Number`, offset-aware RFC 3339 `DateTime`, `List<T>`, and declared records/enums. Task expressions support literals (including `[1, 2.5]`), input fields, enum variants, parentheses, `not`, `and`, `or`, equality/comparisons, and `if`/`else`. A standalone table value type, null, and external functions are not supported; decision tables are supported inside decision models.

A process uses named nodes and explicit, ordered `link` declarations (see [`examples/graph.bl`](examples/graph.bl)). A task node calls a typed task. A `business_rule` node calls a typed `.bl` decision model; [`examples/pricing.bl`](examples/pricing.bl) shows a table-driven quote with an expression-based input column and no-match default. Compile it with `cargo run -- examples/pricing.bl /tmp/pricing.rs`. Decision models can also contain `literal` and `context` nodes connected by `link` dependencies and typed `knowledge` functions; tables support `UNIQUE`, `ANY`, `FIRST`, `PRIORITY`, `RULE_ORDER`, `OUTPUT_ORDER`, and `COLLECT` (including `SUM`, `MIN`, `MAX`, `COUNT`). `and_split`, `or_split`, and `xor_split` pair with matching joins; AND links carry record-field labels (`as field`), and OR/XOR links carry Boolean conditions (`when ...`) with a final `else` fallback. A normal `end` receives the process's declared output type; `error`, `cancel`, and `terminate` are named process-wide terminals without payloads. For example:

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
