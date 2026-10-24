<!-- markdownlint-disable MD013 -->
# blkit

Experimental compiler and single-node development server for type-safe `.bl` business processes. The compiler emits Rust; the dev server runs a compiled process graph.

```sh
cargo run -p blkit-cli -- examples/minimal.bl /tmp/minimal.rs
```

The generated file can be included in a Rust crate with an exact-version `blkit-core` dependency (matching the CLI), `serde` (derive), `serde_json`, and `rust_decimal` (serde-str), and `chrono` (serde) when temporal types or constructors are used. Compile that crate with `cargo build` or `cargo test`. [`examples/minimal.bl`](examples/minimal.bl) is compiled and executed by the test suite.

## Minimal braced process

`minimal.bl` declares a `start_event` with an `amount: Number` output, a `decision_task` with a typed `literal_expression`, an `end_event` with a `result: Number` input, and a `process` that references those peer declarations. Headers, ports, expressions, `flow` and `bind` statements end in semicolons; declarations use braces. `flow` controls execution order (`start -> calculate -> done`); `bind` routes the value of one typed output port into an input port, independently of execution order.

The generated Rust exposes `named_graph_definitions()`. For `example`, pass JSON `{"amount":"7"}` to `decode_input` and `checkpoint`, then `run` with the same input and checkpoint: the completed result is the JSON string `"7"` (a one-port normal end returns its port value, not an object). Start inputs are **always** JSON objects keyed by output port, even with one port; `"7"` alone is invalid. A normal end with multiple input ports returns a JSON object keyed by port.

The `blkit` CLI accepts `blkit SOURCE.bl OUTPUT.rs` for one file, `blkit transpile [PROJECT_DIR]` to generate a Rust Cargo project, and `blkit update [PROJECT_DIR]` to refresh its lockfile. Project commands default to the current directory. `blkit build` is no longer supported: compile the generated project yourself with Cargo. Run `blkit --help` or `blkit transpile --help` for usage, `blkit --version` for the CLI version, or `blkit --completions bash` to print shell completion (also supports `elvish`, `fish`, `powershell`, and `zsh`). From this checkout, replace `blkit` with `cargo run -p blkit-cli --` in these examples. Build only the distributable CLI with `cargo build -p blkit-cli --release --bin blkit` (or `cargo build --release` using the workspace default); this does not compile the development graph or binaries. Project `transpile` and `update` show a temporary spinner on an interactive stderr terminal; errors include the file or project path on stderr. Redirected stderr, `NO_COLOR`, and `CLICOLOR=0` disable styling and animation, so scripts receive plain diagnostics. Completion scripts go to stdout without progress output.

### Compiler diagnostics

Invalid `.bl` sources report an explanation on stderr, with the source path, an excerpt and a one-based line/column label when the error can be located. Plain redirected or `NO_COLOR` stderr uses readable `snippet line` and `label at line` text without terminal styling; errors without a reliable location have no invented label. Compilation exits nonzero, writes nothing to stdout and does not create the Rust output.

For a direct compile, put this in `input.bl`:

```bl
namespace orders; version "1";
# café
start_event start { output amount: Missing; }
```

`blkit input.bl output.rs` fails with `input.bl: unknown type: Missing`; stderr excerpts line 3 and labels `Missing` at **line 3, columns 36 to 42**. No `output.rs` is written.

For a project with the `blkit.toml` shown below under **Build a blkit project**, put these files in its directory:

`a.bl`:

```bl
namespace orders; version "1"; start_event start { output amount: Number; } end_event done { input result: Number; }
```

`z.bl`:

```bl
namespace orders; version "1";
# café
process route { flow start -> done; bind start.missing -> done.result; }
```

`blkit transpile /path/to/project` fails with `/path/to/project/z.bl: unknown binding output: start.missing`; stderr excerpts line 3 of `z.bl` and labels `start.missing` at **line 3, columns 42 to 54**, not in `a.bl`. No `.blkit/src/lib.rs` is written.

## Braced decisions and inferred dependencies

[`examples/pricing.bl`](examples/pricing.bl) compiles with `cargo run -p blkit-cli -- examples/pricing.bl /tmp/pricing.rs`. Its `decision_task price` declares typed input/output ports and a `decision_table tier` with `policy FIRST;`, typed input/output columns, ordered `rule ... -> ...;` statements, and `default 2;`. The `context quoted` node contains a typed `entry offer: Number = tier.result;` and `result offer;`. The reference to `tier.result` establishes the dependency automatically—there is no decision `link`. The task's `output result: Number = quoted;` uses sole-output shorthand for `quoted.result`; when qualifying a node output, use `node.port`. Braced `knowledge` definitions use `input`, `output`, and `expression` statements and may be called from decision expressions and contexts. All statements end with semicolons.

The `quote` process accepts `{"amount":"120"}` and returns `"5"`; `{"amount":"10"}` returns `"2"`. The `Number` JSON values are strings. A table with multiple output columns declares a record-valued result port (or `List<Record>` for multi-result policies); its column names are not separate decision-node ports.

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
namespace orders;
version "1";
Order = {amount: Number};
start_event start { output input: Order; }
end_event done { input result: Order; }
```

Create `process.bl`:

```bl
namespace orders;
version "1";
decision_task echo {
  input input: Order;
  output result: Order = identity;
  literal_expression identity { output result: Order; expression input; }
}
process route {
  flow start -> echo;
  flow echo -> done;
  bind start.input -> echo.input;
  bind echo.result -> done.result;
}
```

Run `blkit transpile` from the directory containing `blkit.toml` (or `blkit transpile /path/to/project`; from a blkit checkout, `cargo run -p blkit-cli -- transpile /path/to/project`). This generates `.blkit/Cargo.toml` and Rust source, **not a compiled library**. Run `cargo build --manifest-path .blkit/Cargo.toml` yourself, locally or on a build machine with the generated project and its dependencies. The generated library exposes `orders::named_graph_definitions()` and can be used by another Rust crate with `orders = { path = "/path/to/project/.blkit" }` in its `Cargo.toml`. Transpilation discovers `.bl` files recursively, excluding hidden directories, `.blkit/`, and `target/`; files with the same namespace and version share types, decision tasks, events, graph peers, and processes, while different namespace/version pairs remain isolated. Every discovered source must validate. Decision-only transpilation requires no Cargo installation.

Run `blkit transpile`, then build the generated project with Cargo. A Rust consumer can start `route` with `Engine::new(Registry::new(orders::named_graph_definitions())?, LocalStore::open("orders.db").await?, 2)?` and `engine.start("orders", "1", "route", serde_json::json!({"input":{"amount":"5"}})).await?`; the result is `{"amount":"5"}`. Legacy generic `task` declarations and crate-backed `node paid = task payments.charge(input)` calls are **not supported** by the new source grammar. Configured Cargo dependencies can still be updated explicitly, but new sources cannot call them until a kind-specific task form is implemented. Remove or redesign these calls before migrating; transpilation rejects them instead of silently resolving an extension.

Choose one build target in `[project]` (all binaries use the `name` field; `orders` is used here):

| `build_target` | Extra setting | Cargo build | Resulting binary |
| --- | --- | --- | --- |
| `crate` | none | `cargo build --manifest-path .blkit/Cargo.toml` | library only; no worker or server dependencies |
| `api-only` | none; `.bl` sources optional | same | `orders-api` (PostgreSQL HTTP; no linked graphs) |
| `worker-only` or legacy `worker` | none | same | `orders-worker` (PostgreSQL worker; no HTTP) |
| `api-worker` | `persistence = "local"` | same | `orders-api-worker` (local durable queue + in-process poller) |
| `api-worker` | `persistence = "remote"` | same | `orders-api-worker` (PostgreSQL API and worker together) |
| `api-worker-split` | none | `cargo build --manifest-path .blkit/Cargo.toml` for worker; `cargo build --manifest-path .blkit/api/Cargo.toml` for API | `orders-worker` and `orders-api`, independently buildable |
| legacy `server` | none | `cargo build --manifest-path .blkit/Cargo.toml` | `orders-server` (equivalent to local `api-worker`) |

All targets except `api-only` require at least one `.bl` file; every discovered file is validated. The split target retains the generated root library for Rust consumers but its API package neither links that library nor enables `worker`. Use `blkit transpile` before the build; it never invokes Cargo itself. Generated manifests pin `blkit-core` to the CLI version; when generated from a checkout, the manifest also contains a local path to `crates/blkit-core`. On another machine provide that source or a compatible published release. Direct-source consumers should replace `blkit` imports with `blkit_core` and pin the same version.

For remote deployment, configure the shared cloud attachment store on every process first (see **Attachment storage and backups** below). Then start `orders-worker "$POSTGRES_URL" 32 5000` (maximum concurrent tasks and lease milliseconds are optional), then start `orders-api "$POSTGRES_URL" 127.0.0.1:3000` or the combined `orders-api-worker "$POSTGRES_URL" 32 5000 127.0.0.1:3000`. `POSTGRES_URL` is a writable PostgreSQL URL such as `postgres://postgres:postgres@127.0.0.1:5432/orders`. Without `CARGO_TARGET_DIR`, binaries built with the generated manifest live under `.blkit/target/debug/`; otherwise use `$CARGO_TARGET_DIR/debug/`. Worker registration advertises exact namespace/version/process capabilities and immutable retry/deadline policies. The graph-free API accepts syntactically valid JSON only when a matching worker is live, recently heartbeating, and not draining (otherwise HTTP 503); malformed JSON receives HTTP 400. Typed input validation and initial checkpointing occur on worker claim, **after** HTTP 202, and an invalid typed input is persisted as a terminal failure. A request admitted just before a worker exits stays pending until a capable worker returns, cancellation, or deadline expiration. Status and cancellation use `/instances/ID` and `/instances/ID/cancel`. Bind to loopback unless a trusted proxy supplies authentication and TLS.

Remote PostgreSQL binaries accept `BLKIT_API_DB_POOL_MAX` (API requests **and** reconciler) and `BLKIT_WORKER_DB_POOL_MAX` (worker persistence), defaulting to **4 connections per active role**. Set each active role's limit to a positive integer; a zero or malformed value fails startup with the setting name, while a setting for an inactive role is ignored. The combined remote `orders-api-worker` uses **separate** API and worker pools; `orders-api` reads only the API setting and `orders-worker` only the worker setting, including split builds. Local-persistence binaries ignore both settings. These limits are independent of the worker's `MAX_TASKS` positional argument, which remains unchanged.

Budget PostgreSQL/proxy `max_connections` for the **sum of both role limits across all running processes**, plus **one dedicated LISTEN session per API replica that serves remote SSE**, observers, admin clients and other users. For example, two API-only replicas with `BLKIT_API_DB_POOL_MAX=4` can consume up to 10 sessions (2 × [4 request-pool connections + 1 listener]); a combined API/worker process also needs its worker pool budget. The listener is outside the bounded request pool and requires a session-persistent PostgreSQL connection (not a transaction/statement-pooled proxy). Pools open connections lazily but can reach their configured maxima under load. Each remote store checks out a connection and runs schema setup before startup succeeds, so an unavailable database or schema error fails startup. A saturated pool waits at most 1 second for checkout; creating a connection has a 2-second timeout and recycling one a 1-second timeout. Failed checkouts and database operations report errors rather than acknowledging uncommitted writes; outages or contention can still disrupt lease renewal, so size leases for expected pauses. On a connection-limited server, set the relevant limit(s) to `1` before rollout (both settings for combined mode) to keep the previous per-role session budget, then raise them only after checking capacity and workload. More connections do not guarantee faster execution. These pool settings require no database migration; input-request persistence does (see rollout below).

For one local server (no PostgreSQL or separate API), set `build_target = "server"` and transpile the `types.bl` and `process.bl` example above, then build it with Cargo:

```sh
blkit transpile
cargo build --manifest-path .blkit/Cargo.toml
.blkit/target/debug/orders-server ./orders.db 32 127.0.0.1:3000
# In another shell:
curl -sS -X POST -H 'content-type: application/json' -d '{"input":{"amount":"5"}}' http://127.0.0.1:3000/processes/orders/1/route/instances
# Use the returned id:
curl -sS http://127.0.0.1:3000/instances/ID
```

To start with files:

```sh
curl -sS -X POST \
  -F 'input={"input":{"amount":"5"}};type=application/json' \
  -F 'files=@./invoice.pdf;type=application/pdf' \
  -F 'files=@./receipt.png;type=image/png' \
  http://127.0.0.1:3000/processes/orders/1/route/instances
```

The multipart contract requires exactly one `input` part for starts or one `response` part for replies, plus one or more `files` parts with nonempty filenames and content; other parts, duplicate/missing JSON, or invalid JSON return 400. The limits on either endpoint are **10 files**, **25 MiB per file**, and **100 MiB combined file bytes** (exceeding one returns 413). JSON parts have a 2 MiB limit; the whole multipart envelope has a 103 MiB limit. Uploads stream to private storage; JSON-only calls keep their original request format.

The completed result is `{"amount":"5"}`. For the newer local `api-worker` target the same command uses `orders-api-worker` instead. Server arguments are optional: local database file (default `blkit.db`), concurrent task limit (default `32`), and bind address (default `127.0.0.1:3000`). If `CARGO_TARGET_DIR` is set, the binary is at `$CARGO_TARGET_DIR/debug/orders-server`. The local server validates process identity and typed input before accepting a request. It persists accepted work in the local database, and an in-process worker polls that durable queue to run eligible work under the configured task limit. Pending work remains claimable after restart; interrupted running work follows the checkpoint/retry recovery rules. There is no separate local worker executable or PostgreSQL queue. The HTTP router has no authentication; keep the default loopback bind unless a trusted proxy supplies access control and TLS.

### Generated worker/server operational logs

Generated worker and server binaries log to **stdout at INFO level** by default. Set `BLKIT_LOG_LEVEL` to `trace`, `debug`, `info`, `warn`, or `error`. Set `BLKIT_LOG_OUTPUTS` to a nonempty comma-separated selection of `stdout`, `file`, and `otlp` (for example `stdout,file,otlp`). `file` requires `BLKIT_LOG_FILE` (an appendable path); `otlp` requires `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` (an HTTP(S) OTLP **logs** endpoint, such as `http://127.0.0.1:4318/v1/logs`). Invalid levels, duplicate/unknown outputs, and missing or invalid selected destinations fail startup with a diagnostic on stderr, before storage or work starts. Unselected destinations need no configuration. For example, after building the corresponding target:

```sh
BLKIT_LOG_OUTPUTS=stdout,file BLKIT_LOG_FILE=./orders.log .blkit/target/debug/orders-server ./orders.db 32 127.0.0.1:3000
BLKIT_LOG_OUTPUTS=stdout,file,otlp BLKIT_LOG_FILE=./worker.log OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://127.0.0.1:4318/v1/logs .blkit/target/debug/orders-worker "$POSTGRES_URL" 32 5000
```

A selected output receives operational events independently of the others. OTLP export is best effort: collector outages and abrupt termination can lose records without stopping instance execution. Files append across restarts without rotation; operators own rotation and retention. Blkit logs do not contain process input or result payloads, but cannot sanitize logs from third-party task crates. **Persisted instance status, errors, and checkpoints** remain the source of truth for execution outcomes; operational logs are not an event history.

Commit `blkit.toml`, your `.bl` files, `.blkit/.gitignore`, and `.blkit/Cargo.lock` once Cargo creates the lockfile; generated Cargo manifest, source, and build artifacts are ignored. If you change dependency requirements, run `blkit update` (or `blkit update /path/to/project`) to refresh the lockfile before a later Cargo build. The project blkit version must exactly match the CLI version; `.bl` `version` identifies a business process, not the CLI. Generated manifests currently reference the CLI checkout's local `blkit-core` source when available; builds on another machine must have that source (or a compatible published `blkit-core` release), along with any local path dependencies.

A source file begins with `namespace name;` and `version "text";`. Named dictionaries use `Order = {total: Number, blocked: Bool};` with comma-separated typed members; keys such as `"my key"` may be quoted. Migrate old `type Order:` declarations to this syntax; old record declarations are rejected. Enums still use `enum Name:` with semicolon-terminated variants. `decision_task name { ... }` declares typed input/output ports and braced decision nodes; `process name { ... }` routes between declared peers using `flow` and `bind` statements. There are no generic `task` declarations, implicit returns, or legacy graph nodes. Supported types: `Bool`, `String`, decimal `Number`, ISO `Date` (`YYYY-MM-DD`), `Time` (`HH:MM:SS` with optional fractional seconds), `DateTime` (`YYYY-MM-DDTHH:MM:SS`), `List<T>`, `Dictionary`, `Dictionary<T>`, `DictionaryEntry<T>`, `Table<Row>` for a declared dictionary row shape, `Value`, and declared dictionaries/enums. A declared `T?` field or result permits null but still requires its field to be present. Temporal JSON inputs and outputs are strings: each form accepts a naive value, `Z` or a fixed offset such as `+05:30`, or an RFC 9557 IANA suffix such as `[Europe/Paris]` (not both). `Time` accepts `24:00:00` and normalizes it to `00:00:00`; later times and leap seconds are rejected. `DateTime` requires `T` (not a space). Invalid dates, offsets, IANA zones, DST gaps and folds fail validation; offset-bearing RFC 3339 inputs remain valid. Temporal constructors are `date("2026-10-02")`, `time("09:30:00.250")`, and `datetime("2026-10-02T08:30:00Z")` (also accept dynamic strings), `date(year, month, day)`, `time(hour, minute, second[, offset])`, `datetime(date, time)`, and `date(datetime)`/`time(datetime)`. `today()` and `now()` share a clock snapshot within each decision evaluation. Calendar/time fields (`year`, `month`, `day`, `dayOfYear`, `weekOfYear`, `isoWeekOfYear`, `isoYearWeek`, `quarter`, `yearQuarter`, `dayName`, `dayNameShort`, `monthName`, `monthNameShort`, `hour`, `minute`, `second`, `offset`, `timezone`) are available on their applicable values; `.offset` returns `DTDuration`, while `.timezone` requires an IANA zone. The old `dateTime(...)` spelling is not supported. `DateTime` comparisons use instants, so different offsets representing the same instant compare equal. Decision expressions support literals (including `[1, 2.5]`), input fields, enum variants, parentheses, `not`, `and`, `or`, and equality/comparisons. A general-purpose `null` literal and external functions are not supported; decision tables are available in `decision_table` nodes.

Duration inputs and outputs use ISO strings. `DTDuration` accepts signed `PnDTnHnMnS` (for example `PT90M`, `-P2DT3H45M10S`, or `PT0.1234567891S`); `YMDuration` accepts signed `PnYnM` (for example `P1.5Y` or `P13M`). Units are case-insensitive on input and uppercase, normalized on output (`PT90M` becomes `PT1H30M`; `P13M` becomes `P1Y1M`). Components may use finite decimal fractions. Duration arithmetic and totals use the same finite decimal `Number` precision (about 28–29 significant digits); division such as one hour by seven rounds half-even to the available precision, not to an exact rational. Inputs outside that precision, overflow, and division by zero fail; days/time durations cannot be combined with years/months durations. Both duration kinds support `abs`, `isNegative` and `round`/`roundUp`/`roundDown`/`roundHalfUp`/`roundHalfDown`/`roundHalfEven` with a positive same-kind duration step (`round` means half-up; up/down move away from/toward zero). `dtDurationBetween(from,to)` and `ymDurationBetween(from,to)` return signed elapsed time and whole months for two Dates or two DateTimes, respectively.

Temporal arithmetic supports `Date`/`DateTime` ± `DTDuration` or `YMDuration`, `Time` ± `DTDuration`, and Date/DateTime subtraction returning `DTDuration`. Month changes clamp invalid days; `Time` wraps at midnight; date-only days-time arithmetic discards sub-day portions. Point comparisons and range/list membership use wall time for naive values, instants for zoned values (a zoned date projects to midnight, and a zoned time uses the evaluation date in its own zone). Comparing naive and zoned points fails. Applying a fractional month or a sub-nanosecond days-time duration to `Time`/`DateTime` fails. `withOffset(time|datetime, dtDuration)` and `withTimezone(datetime, "IANA/Name")` change the zone while retaining the instant; `withoutOffset`, `withoutTimezone`, and `withoutOffsetOrTimezone` strip matching zones from dates/datetimes while retaining wall-clock fields. Re-zoning a naive value has no defined instant and fails.

Dictionary ports accept JSON objects. A named `Order = {id: Number, price: Number?};` input may be `{"id":"1","price":null}`: both fields are required, including nullable `price`; missing or extra fields, null `id`, and incorrect non-null field types are rejected. Typed `Number` fields remain decimal strings in ordinary dictionary JSON. `Dictionary<Number>` uses the same string-valued Number representation (for example `{"alice":"90","bob":"75"}`). An ad hoc `Dictionary` supports mixed JSON values (for example `{"name":"Alice","age":30}`); its numeric JSON values remain numbers, including high-precision decimals. `{}` is an empty dictionary, `{a: 2, b: a * 2}` evaluates values in order, `{a: {b: 3}}.a.b` navigates nested keys, and `input["my key"]` accepts quoted or dynamic String keys. Dictionaries compare structurally with `==` or `!=`, ignoring insertion order.

`getValue(d, "key")` and `getValue(d, ["outer", "inner"])` retrieve a key or nested path. Missing keys and non-dictionary intermediate path values fail evaluation; they do not yield null. `keys(d)` returns sorted `List<String>` by Unicode code point, and `values(d)` and `getEntries(d)` use the same key order. `getEntries(d)` exposes each entry's `.key` and `.value`. `has(d, "key")`, `size(d)`, and `isEmpty(d)` inspect a dictionary; `isEmpty` also accepts its existing String and Calendar inputs. `dictionaryPut(d, "key", value)` or `dictionaryPut(d, ["outer", "inner"], value)` returns a new dictionary; nested paths require dictionary-valued existing intermediates. `dictionaryMerge([first, second])` is a shallow merge with later keys winning; `dictionaryRemove(d, "key")` deletes a direct key. None mutate the inputs; shape-changing operations on a named dictionary return an ad hoc `Dictionary`.

For `scores: Dictionary<Number>`, `sum(values(scores))` computes a total, `every v in values(scores) satisfies v >= 50` checks every score (true for an empty list), and `for e in getEntries(scores) return e.value > 80` builds an ordered `List<Bool>` (empty for an empty list). Iteration variables are scoped to their expression and may shadow outer names; the source must be a list, and errors inside the body propagate.

### Standalone Table expressions

`Table` is a distinct value (not a list of dictionaries or a `decision_table` node); every table input/output port must name a declared dictionary row shape as `Table<Row>`, not bare `Table`. `table(names, rows…)` takes distinct String column names in order, then positional value lists of the same width. Cell expressions are evaluated, and each column infers one compatible type; nullable values retain their nullability without requiring column type declarations. `tableFromDicts(rows)` adopts a list of dictionaries only when every row has the same keys and compatible column values. Its columns use canonical dictionary key order. Both constructors reject duplicate headers, missing/extra row cells or keys, and incompatible values rather than changing the schema. Tables are immutable; constructing one does not turn a separate `List<Dictionary>` into a table.

| Expression | Result (`Table<Row>` JSON) |
| --- | --- |
| `table(["region", "rate"], ["domestic", 5.99], ["europe", 15.99])` | `{"columns":["region","rate"],"rows":[{"region":"domestic","rate":5.99},{"region":"europe","rate":15.99}]}` |
| `tableFromDicts([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])` | `{"columns":["rate","region"],"rows":[{"rate":5.99,"region":"domestic"},{"rate":15.99,"region":"europe"}]}` |
| `table(["rate"])` | `{"columns":["rate"],"rows":[]}` |
| `table([])` / `tableFromDicts([])` | `{"columns":[],"rows":[]}` |

An empty named column retains its name and position, even though its runtime cell type remains unresolved; a declared `Table<Row>` output supplies the expected row shape. For example, `Rate = {rate: Number};` and `output result: Table<Rate>; expression table(["rate"]);` produce the named empty table above. A table with zero columns uses `Empty = {};` and `Table<Empty>`. An unresolved column type needed by an expression is a transpilation error, not a runtime guess.

A typed port requires the JSON object `{ "columns": ["name", ...], "rows": [{"name": value, ...}, ...] }`, even when the rows are empty. `columns` must match the declared row fields **in declaration order**, with every field present in each row; duplicate/extra/missing columns, wrong cell types, extra envelope fields, and bare arrays are rejected on input. A `Number` table cell is a JSON number (unlike a standalone `Number` port's JSON string). A `T?` field accepts null while a `T` field rejects it. Declared enum and temporal fields retain their type identity after JSON decoding. Outputs keep the same envelope, field order, and empty schema. Bare `Table` ports fail transpilation.

```bl
Order = {id: Number, price: Number?};
decision_task echo {
  input data: Table<Order>;
  output result: Table<Order> = value;
  literal_expression value { output result: Table<Order>; expression data; }
}
```

This task accepts `{"columns":["id","price"],"rows":[{"id":1,"price":null}]}` but rejects a row missing `price` or containing null `id`. A separate `List<Dictionary>` remains a list; it is never implicitly converted to a table. `table(["rate"], [5.99])` still infers the non-null cell type without a column declaration.

#### Table selection and row operations

With `Rate = {region: String, rate: Number?};`, `OnlyRate = {rate: Number?};`, `data: Table<Rate>`, and `minimum: Number`, `data[rate > var(minimum), rate]` yields a `Table<OnlyRate>`: bare `rate` reads the row, while `var(minimum)` explicitly reads the outer input. A missing column is a transpilation error, not an outer-variable lookup. `data[1, rate]` is **still a table** containing one cell; `data[1, rate].toValue()` explicitly extracts its `Number?`. `data.rate` or `data[, rate].toList()` yields `List<Number?>`; `data[1].toDict()` yields a `Dictionary`, while a multi-column `data.toList()` yields `List<Dictionary>`. Bad row indices and conversions with the wrong number of rows/columns fail evaluation.

`data.filter(rate > var(minimum))` is equivalent to `data[rate > var(minimum)]`; `filterOut` retains only false predicates. Both exclude unknown (null) predicates. `select(region, rate)` keeps the chosen column order; `slice(1:2)` keeps the first two rows; `distinct()` keeps the first instance of every complete row. `rename(rate -> price)` changes a column name without mutating `data`; simultaneous swaps are allowed, but final duplicate names and missing sources are rejected. `withColumn(with_tax = price * 1.2)` adds a nullable column after renaming (null prices produce null tax), while `withColumn(rate = coalesce(rate, 0) * 2)` replaces `rate` in place. A declared result such as `Priced = {region: String, price: Number?, with_tax: Number?};` and `Table<Priced>` checks the chained output shape before generation.

`data.sort(region, desc(rate))` sorts stably by region ascending then rate descending; `asc(rate)` is explicit ascending. `inOrder(region, ["eu", "eu", "us"])` ranks the first occurrence of each listed value; unlisted values follow in ascending order, and nulls sort last unless explicitly ranked. Incompatible temporal zones, non-comparable cells, unknown keys, and excessive sorting work return checked errors.

Table `union(left, right)` and `left.union(right)` **concatenate** rows, preserving duplicates (unlike list `union`, which deduplicates); use `.distinct()` explicitly if needed. The default mode `"error"` requires matching column sets and keeps the first table's order even when the second lists them differently. `"common"` keeps only shared columns in first-table order. `union(left, right, "all")` keeps first-table columns followed by new ones and supplies null for absent cells. For `LeftRow = {id: Number, price: Number};` and `RightRow = {id: Number, region: String};`, the `"all"` output needs `AllRow = {id: Number, price: Number?, region: String?};` and `Table<AllRow>`; non-null result fields would be rejected before generation.

`join(left, right, [region, id], "outer")` and `left.join(right, [region, id], "outer")` pair rows with equal values for both keys, including duplicate-key combinations. Columns are keys in argument order, then left non-keys, then right non-keys. Default `"inner"` retains only matches; `"left"`, `"right"`, and `"outer"` retain unmatched rows with **nullable** missing-side non-key cells. For `LeftRow = {region: String, id: Number, amount: Number};` and `RightRow = {region: String, id: Number, fee: Number};`, an outer output needs `OuterRow = {region: String, id: Number, amount: Number?, fee: Number?};` and `Table<OuterRow>`. `join(left, right, [], "cross")` multiplies every pair and needs disjoint column names. Null keys do not match; unknown keys, non-key name collisions, incompatible types/zones, and oversized joins fail explicitly.

`data.groupBy(region).agg(total = sum(rate), average = mean(rate), n = count())` groups by `region` in first-seen order. With `Rate = {region: String, rate: Number?};`, the result has `Summary = {region: String, total: Number, average: Number?, n: Number};` and type `Table<Summary>`. Inside `agg`, `rate` is the **list of rate cells in that group**; use `var(adjustment)` for an outer variable, never a bare name. `sum(rate)` ignores nulls and returns zero for all-null groups; `mean(rate)` ignores nulls and returns null for all-null groups; `count()` counts all rows, including rows with null rates. Group-key columns precede aggregate outputs. `groupBy` without `.agg(…)`, unknown keys, duplicate or colliding output names, and `count()` outside a group are errors.

### List expressions

List literals are ordered `List<T>` values: elements must all have the same type (including the same nesting depth), and `null` is not a list element. An empty `[]` needs a type from its context, such as a `List<Number>` output. Numeric indexes are one-based integers; `-1` is the last element, and zero, fractional, or out-of-bounds indexes fail. A Boolean bracket expression filters in input order; only inside that filter does leading `.item` mean the current element. `mod` is numeric floor remainder (like `modulo`) and fails on a zero divisor.

| Expression | Result |
| --- | --- |
| `[1,2,3,4][1]` / `[1,2,3,4][-2]` | `1` / `3` |
| `[1,2,3,4][.item > 2]` | `[3,4]` |
| `[1,2,3,4][.item mod 2 == 0]` | `[2,4]` |
| `[1,2][odd(.item)]` | `[1]` |
| `[1,2] == [1,2]` / `[1,2] != [2,1]` | `true` / `true` |
| `2 in [1,2,3]` | `true` |
| `for x in [1,2], y in [3,4] return x * y` | `[3,4,6,8]` |
| `some x in [1,2], y in [3,4] satisfies x + y > 5` | `true` |
| `every x in [1,2], y in [3,4] satisfies x + y >= 4` | `true` |
| `4:2` | `[4,3,2]` |

`for` and `every` also accept comma-separated bindings in Cartesian order; `some` succeeds if any binding combination satisfies its Boolean body (and is false for an empty source). `start:end` produces an inclusive `List<Number>` in either direction, unlike the existing `[start..end]` range.

For `Order = {name: String, item: Number};`, `orders.name` on a `List<Order>` projects `List<String>` in order (for orders named `"A"` and `"B"`, the result is `["A","B"]`); `order.item` on an `Order` remains a scalar field. Plain `item` is an ordinary decision node, context entry, input, or iteration binding, not the filter element: with `item: Number` equal to `2`, `[4,5][item]` returns `5`. Leading `.item` outside a bracket filter is invalid. List `==` and `!=` compare elements in order; scalar `in` tests membership, not a list ordering comparison.

List inspection and transformations do not change their input lists. The following examples use a `List<Number>` input for `isEmpty(input)` (an empty literal needs a type from context). Unlike string `indexOf`, list `indexOf` returns **all** matching one-based positions, or `[]` if absent. `sublist` accepts a negative-from-end start, defaults to the remainder, and truncates a requested length at the end.

| Expression | Result |
| --- | --- |
| `count([1,2,3])` / `isEmpty(input)` with an empty input | `3` / `true` |
| `listContains([1,2,3], 2)` / `indexOf([1,2,3,2], 2)` | `true` / `[2,4]` |
| `sublist([1,2,3], -2)` | `[2,3]` |
| `append([1], 2, 3)` / `prepend([2,3], 1)` | `[1,2,3]` / `[1,2,3]` |
| `concatenate([1,2], [3])` / `union([2,1], [1,3])` | `[1,2,3]` / `[2,1,3]` |
| `reverse([1,2,3])` / `flatten([[[1,2]],[[3,4]]])` | `[3,2,1]` / `[1,2,3,4]` |

Positional edits use one-based positions; a negative position counts from the end, so `-1` refers to the **last existing element** for all four operations (not the slot before the list). A list supplied as the third insertion argument is spread at that position; an outer list wraps a nested list to insert it as one element. Predicates use `function(i)` with `i` scoped to each element, return `Bool`, and edit **every** match. `function` is reserved and cannot name a knowledge model.

| Expression | Result |
| --- | --- |
| `insertBefore([1,3], 1, 2)` / `insertBefore([10,20,30], -1, 5)` | `[2,1,3]` / `[10,20,5,30]` |
| `insertAfter([10,20,30], -1, 5)` / `insertAfter([1,5], 1, [2,3,4])` | `[10,20,30,5]` / `[1,2,3,4,5]` |
| `insertBefore([[1],[4]], 2, [[2,3]])` | `[[1],[2,3],[4]]` (one nested element) |
| `remove([1,2,3], 2)` / `remove([10,20,30], -1)` | `[1,3]` / `[10,20]` |
| `listReplace([1,2,3], 2, 9)` / `listReplace([10,20,30], -1, 5)` | `[1,9,3]` / `[10,20,5]` |
| `remove([1,2,3,2], function(i) i == 2)` | `[1,3]` |
| `listReplace([2,4,7], function(i) i < 5, 5)` | `[5,5,7]` |
| `distinct([2,1,2,3,1])` / `duplicateValues([2,1,1,2,2])` | `[2,1,3]` / `[2,1]` |
| `intersection([3,2,3,1], [2,3])` | `[3,2]` |

`union` retains first occurrences across lists in argument order; `distinct` retains first occurrences, `duplicateValues` returns repeated values once in first-occurrence order, and `intersection` returns shared values once in the **first list's order**. Equality is structural even for nested lists and dictionaries; all list arguments must have compatible element types. A mismatched element in `append([1], "2")`, a String position in `sublist([1,2], "1")`, or a non-Boolean predicate in `remove(input, function(i) i + 1)` (with a `List<Number>` input) fails validation. At runtime, `remove(input, position)` and `sublist(input, position)` with input `[1,2,3]` fail for position `0`, `4`, `-4`, or `1.5`: positions must be integral and within the list. A `sublist` length must be a nonnegative integer. Errors in evaluated predicates propagate instead of silently skipping elements.

Sorting, joining, and sequences also produce new values without changing their input lists. These expressions are typed decision-task examples; `[]` gets its element type from the declared output or function argument. `sort` accepts `List<Number>`, `List<String>`, or a list of one comparable `Date`, `Time`, `DateTime`, `DTDuration`, or `YMDuration` kind. It sorts ascending by default or with `"asc"`, and descending with `"desc"`; a priority list puts its first-listed values first, then unlisted values in ascending order. Sorting is stable for values that compare equal, uses case-sensitive Unicode code-point order for strings, and reports errors for incompatible temporal values. **At most 1024 entries across source and priority lists combined** are accepted, including when the source is empty.

| Expression | Type | Result |
| --- | --- | --- |
| `sort([3,1,2])` | `List<Number>` | `[1,2,3]` |
| `sort([3,1,2], "desc")` | `List<Number>` | `[3,2,1]` |
| `sort(["m","s","xl"], ["s","m","l"])` | `List<String>` | `["s","m","xl"]` |
| `stringJoin(["a","b"])` | `String` | `"ab"` |
| `stringJoin(["a","b"], "-", "[", "]")` | `String` | `"[a-b]"` |
| `stringJoin([], "-", "[", "]")` | `String` | `"[]"` |
| `zipStringJoin([["a","b"],["1","2"]], "-", "[", "]")` | `List<String>` | `["[a-1]","[b-2]"]` |
| `zipStringJoin([["a","b"],["1","2"],["x","y"]], ["-", ":"], ["<","(","{"], [">",")","}"])` | `List<String>` | `["<a>-(1):{x}","<b>-(2):{y}"]` |
| `zipStringJoin([], [], [], [])` | `List<String>` | `[]` |
| `seq(5,10)` | `List<Number>` | `[5,6,7,8,9,10]` |
| `seq(10,5)` | `List<Number>` | `[10,9,8,7,6,5]` |
| `seq(0,1,0.25)` | `List<Number>` | `[0,0.25,0.5,0.75,1]` |
| `seq(5,10,-1)` | `List<Number>` | `[5,6,7,8,9,10]` |
| `seq(0,0.95,0.1)` | `List<Number>` | `[0,0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9]` |
| `10:5` | `List<Number>` | `[10,9,8,7,6,5]` |
| `all([true,true,false])` | `Bool` | `false` |
| `any([false,false,true])` | `Bool` | `true` |
| `all([])` | `Bool` | `true` |
| `any([])` | `Bool` | `false` |

`stringJoin` separates elements of a `List<String>` (default separator `""`); optional prefix and suffix wrap the completed string once, even for an empty list. `zipStringJoin` joins corresponding elements of equally long `List<String>` columns, returning one string per position. Its delimiter may be one `String` for every gap or a `List<String>` of exactly **N−1** delimiters for N columns; prefix and suffix may each be one `String` for every result or a `List<String>` of exactly N wrappers, one per column contribution. Omitted formatting uses empty strings. With zero columns it returns `[]`: scalar formatting is allowed, but list-valued delimiter and wrapper arguments must all be empty. Unequal column lengths, wrong types, or invalid formatter lengths fail rather than truncate or coerce.

`seq(start,end[,step])` and `start:end` return inclusive decimal `List<Number>` values. The default step magnitude is 1; even an explicitly negative step follows the endpoints' direction. Fractional steps use decimal precision and stop before an unreached endpoint. Zero or non-progressing steps, overflow, and sequences exceeding one million elements report checked execution errors. `all` and `any` require `List<Bool>`; on an empty list they return `true` and `false`, respectively, and non-Boolean elements fail validation.

A `Calendar` port accepts JSON `{"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[{"value":"2025-04-18","name":"Good Friday"},{"value":{"start":"2025-04-18","end":"2025-04-20","includeStart":true,"includeEnd":false}}]}`. Bounds and values are ISO `Date` or `DateTime` strings (including fixed-offset or IANA suffixes); range endpoints must have the same point type. `name` is optional and range inclusivity defaults to `true`. Entries are returned in chronological order, with point entries before ranges at the same start. Calendar deserialization rejects invalid values, mixed naive/zoned kinds, reversed ranges/bounds, or entries outside validity bounds. Calendars are supplied as inputs; there is no calendar expression literal. Queries include `count`, `isEmpty`, `entries`, `names`, `find`, `contains`, `entriesFor`, `overlaps`, `entriesIn`, `validFrom`, `validTo`, `validRange`, `entryValue`, `entryName`, `next`, and `prev`. `entryName` fails on unnamed entries and `next`/`prev` require positive `n`. `calendarDrop(c, target[, rangeMatch="equality"])`, `calendarKeep(...)`, and `calendarMerge([a, b][, dedupeBy="value"][, tiebreak="first"])` return new calendars. `pattern("^Easter")` matches entry names only in drop/keep targets; rangeMatch accepts `equality`, `entryWithin`, `entryEncloses`, or `overlap`. Options use named call arguments (`=`), not dictionary literals or equality comparisons (`==`).

Date and DateTime values support `isWeekday`, `isWeekend`, `isPublicHoliday`, `isBusinessDay`, month/weekday boundaries, strict `next*`/`prev*` navigation, `addBusinessDays`/`subtractBusinessDays`, and inclusive order-independent `weekdaysBetween`/`businessDaysBetween`. Business days exclude weekends and optionally calendar entries; a zero add/subtract count preserves the input. DateTime navigation preserves time and zone. When iterating outside a supplied calendar's validity range, holidays are ignored by default; set the trailing `strictCalendarRange` argument to `true` (with a calendar) to report `bl.CalendarRangeError` instead. Day-of-week arguments are integers 1–7 (Monday–Sunday). `daysBetween(a,b[,includeTime])` returns signed days; `monthsBetween(a,b[,basis[,includeTime]])` and `yearsBetween(...)` default to calendar whole periods plus a fraction of the following period. Bases `actual/365` and `actual/360` divide actual elapsed days by a fixed year length, `actual/actual` (ISDA) splits days at calendar-year boundaries and uses each year's 365/366-day length, and `30/360` (US/NASD) or `30E/360` (European) use 30-day months; months are twelve times the annual ratio. `includeTime` applies only to DateTime and includes elapsed sub-day fractions. `financialYear(value,basis)` and `financialYearQuarter(value,basis)` accept a start month 1–12 or `AU` (July 1), `UK` (April 6), `US` (October 1), and `IN`/`JP`/`CA`/`NZ` (April 1), and label years by their ending year.

Number expressions support decimal and scientific literals (`0.1`, `1.5e3`), unary `-`, `+`, `-`, `*`, `/`, and `**`. Powers bind right-to-left; powers bind tighter than unary `-`, then multiplication/division, then addition/subtraction, then comparisons and `==` (never `=`). For example, `-2 ** 3 ** 2` is `-512` and `0.1 + 0.2 == 0.3` is true. Arithmetic uses decimal `Number` (up to 28–29 significant digits), rounding results beyond its precision; division by zero, invalid powers, and unrepresentable results return execution errors. `string(Number)` removes trailing fractional zeros (for example, `string(1500.50)` is `"1500.5"`).

Number helpers include `round(n, scale)` (alias `roundHalfUp`), `roundUp`, `roundDown`, `roundHalfDown`, `roundHalfEven`, and `floor(n[, scale])`/`ceiling(n[, scale])`. Scales are integral decimal places from -28 through 28; negative places round to tens, hundreds, etc. Up/down round away from/toward zero; half-up/half-down break exact ties away from/toward zero; half-even breaks ties to the even neighbor. `floor` and `ceiling` round toward negative and positive infinity, respectively. Invalid scales return execution errors. `abs`, `modulo`, `sqrt`, `exp`, `ln`, `log(n[, base])`, `clamp(n, min, max)`, `odd`, `even`, `isPositive`, `isNegative`, and `isZero` take `Number` arguments; `modulo` has the nonzero divisor's sign, `log` defaults to base 10, and `odd`/`even` require integral values. Invalid domains, bounds, divisors, and overflows return execution errors.

Aggregates each take one homogeneous list. `sum` and `mean` accept `List<Number>` or `List<Number?>`; both ignore null values, including in projected table columns. Numeric `sum` returns `Number`, while numeric `mean` returns `Number?` (so it can be null on an empty or all-null list). Duration `sum`/`mean` and `median` accept their existing homogeneous types; even-sized `median` averages the middle pair. `min` and `max` return an element of a comparable `List<Number>`, `List<String>`, `List<Date>`, `List<Time>`, `List<DateTime>`, `List<DTDuration>`, or `List<YMDuration>`, using the type's comparison rules (including code-point order for strings and checked temporal compatibility). `product` and `stddev` accept only `List<Number>` and return `Number`; `stddev` uses the **sample** denominator N−1 and needs at least two values. Mixed types or duration kinds fail validation, and incompatible temporal values or unrepresentable arithmetic report execution errors.

**Breaking change:** `mode(List<T>)` now returns `List<T>`, not a scalar `Number`. It contains **all** highest-frequency values, including a one-element list for a unique winner. Numeric ties are sorted ascending; types without natural ordering retain tied values in first-occurrence order. Migrate a decision task that previously declared `output result: Number` for `mode(values)` to `List<Number>` on both the task and literal expression outputs:

```bl
decision_task modes {
  input values: List<Number>;
  output result: List<Number> = calc;
  literal_expression calc {
    output result: List<Number>;
    expression mode(values);
  }
}
```

If a scalar is required, explicitly select a one-based element (for example `mode([6,1,6,1])[1]`); this selects the smallest tied Number, but an empty input still fails. `mode` rejects more than **1024** input elements with a checked execution error.

| Expression | Type | Result |
| --- | --- | --- |
| `mode([6,1,6,1])` | `List<Number>` | `[1,6]` |
| `mode([3,2,3])` | `List<Number>` | `[3]` |
| `mode([6,1,6,1])[1]` | `Number` | `1` |
| `sum([dtDuration("PT1H"),dtDuration("PT2H")])` | `DTDuration` | `dtDuration("PT3H")` |
| `sum([ymDuration("P1Y"),ymDuration("P6M")])` | `YMDuration` | `ymDuration("P1Y6M")` |
| `mean([dtDuration("PT1H"),dtDuration("PT2H")])` | `DTDuration` | `dtDuration("PT1H30M")` |
| `median([ymDuration("P1Y"),ymDuration("P2Y")])` | `YMDuration` | `ymDuration("P1Y6M")` |
| `min(["banana","Apple"])` | `String` | `"Apple"` |
| `max([date("2025-01-01"),date("2025-02-01")])` | `Date` | `date("2025-02-01")` |
| `product([2,3,4])` | `Number` | `24` |
| `stddev([1,2,3])` | `Number` | `1` |
| `sum([])` | `Number` | `0` |
| `mean([])` | `Number?` | `null` |
| `coalesce(mean([]), 0)` | `Number` | `0` |
| `product([])` | `Number` | `1` |
| `sum([])` | `DTDuration` | `dtDuration("PT0S")` |

The `[]` examples obtain their list type from context. Numeric empty/all-null `sum` returns 0; empty/all-null `mean` returns null; empty `product` returns 1; empty duration `sum` returns its kind's zero duration. Empty `min`, `max`, `median`, and `mode` fail evaluation, and `stddev` fails for fewer than two elements. A nullable numeric column `t.price` containing 20 and null yields `sum(t.price) = 20`, `mean(t.price) = 20`, and `coalesce(mean(t.price), 0) = 20`; if every cell is null, the last expression yields 0. `coalesce(value, fallback)` requires a nullable value and a matching non-null fallback, which is evaluated only when needed. Nullable arithmetic/comparisons propagate null, `not unknown` remains unknown, `false and unknown` is false, and `true or unknown` is true. Table filters retain only true predicates, never false or unknown. Source-authored `null` and `[null]` are still invalid.

`number(text)` parses an ungrouped signed decimal `String` with `.` as the decimal separator. `number(text, groupingSeparator, decimalSeparator)` uses distinct single-character ASCII punctuation separators, for example `number("1.500,50", ".", ",")` is `1500.5`; each group after the first must contain exactly three digits. Malformed text, invalid grouping/separators, and unrepresentable values fail validation when constant and return execution errors when dynamic. No locale is guessed.

String expressions support `"foo" + "bar"`, `"order-" + string(123)`, case-sensitive `==` and `!=` (not `=`), and `"active" in ["active", "pending"]`. `+` binds more tightly than comparisons; `in` also retains range membership. `string(from)` converts scalar `String`, `Number`, `Bool`, `Date`, `Time`, or `DateTime`; `stringJoin(List<String>, separator)` joins with a literal separator.

String functions: `stringLength(s)`, `substring(s, start[, length])`, `substringBefore(s, match)`, `substringAfter(s, match)`, `upperCase(s)`, `lowerCase(s)`, `trim(s)`, `trimLeading(s)`, `trimTrailing(s)`, `contains(s, match)`, `startsWith(s, match)`, `endsWith(s, match)`, `matches(s, pattern[, flags])`, `replace(s, pattern, repl[, flags])`, `split(s, delimiter)` or `split(s, delimiters: List<String>)`, `extract(s, pattern[, flags])`, `isBlank(s)`, `isEmpty(s)`, `indexOf(s, match)`, `charAt(s, position)`, `reverse(s)`, `padLeading(s, length[, padChar])`, `padTrailing(s, length[, padChar])`, and `repeat(s, times)`. Positions count visible Unicode characters, start at **1**, and negative positions count from the end (`-1` is last); `indexOf` returns `0` when absent. `substring` lengths, padding lengths, and repeat counts must be nonnegative integers; invalid positions and counts produce execution errors. Literal search and `split` do not interpret regular expressions. Regex `matches` searches anywhere; `replace` replaces every match and supports `$1` capture substitution; `extract` returns `List<List<String>>`, grouped by match, with participating capture groups (or full matches when none are declared). Regex flags: `i` (case insensitive), `m` (multiline), `s` (dot matches newline). `.bl` double-quoted strings do **not** interpret escapes, so use a single backslash in a regex such as `"order-(\d+)"`.

Generated Rust decision tasks return `Result<T, String>`; handle fallible string operations with `?` or `Err`. Processes report expression errors as execution failures rather than panicking.

Ranges use `[a..b]`, `(a..b)`, `[a..b)`, or `(a..b]` for inclusive/exclusive endpoints. For example, `input in [1..5]`, `input in (1..5]`, and `input between 1 and 5` return `Bool`; `between` includes both ends. Use `null` only for an unbounded range endpoint, such as `input in [1..null)` or `input in (null..null)`; `null` is not a general value. Endpoints must share a type (`Number`, `Date`, `DateTime`, or `Time`), for example `input in [date("2026-10-02")..null)`. Statically reversed bounds are rejected; runtime-reversed bounds are empty. Ranges can be compared using `==` and `!=` (not `=`). `before(A, B)`/`after(A, B)` compare strictly ordered ranges; `meets(A, B)`/`metBy(A, B)` compare touching finite endpoints even when open. `overlaps(A, B)` needs a common value (empty ranges never overlap), while `overlapsBefore(A, B)` means A starts first, B starts before A ends, and A ends before B; `overlapsAfter` reverses the arguments. Unbounded starts sort before finite starts and unbounded ends after finite ends. `coincides(A, B)` compares effective endpoints and inclusion. Point functions take `includes(range, value)`, `during(value, range)`, `starts(value, range)`, `startedBy(range, value)`, `finishes(value, range)`, and `finishedBy(range, value)`; start/end points must be finite and included. For example, `starts(1, [1..5])` is true and `starts(1, (1..5])` is false. For `Number` points, `before(0, [1..5])`, `after(6, [1..5])`, and `before([1..5], 6)` compare against the finite near endpoint; `meets(1, (1..5])` and `metBy([1..5], 1)` compare endpoint equality even when open. Their argument orders can be reversed. If the endpoint needed for the comparison is unbounded, the result is false; no point overload exists for `overlaps` or `coincides`.

A `process` refers to peer `start_event`, `decision_task`, gateway, and `end_event` declarations using ordered `flow` edges. Separate typed `bind` edges connect output ports to input ports. [`tools/dev-harness/examples/graph.bl`](tools/dev-harness/examples/graph.bl) runs `decide`, `parallel`, and `offers`: XOR selects one route, AND combines labelled branches into a record, and OR collects selected branches in declaration order. A start is always a JSON object (`{"total":"1200"}`); the resulting `decide` value is `"review"`, `parallel` returns `{"left":"1200","right":"1200"}`, and `offers` returns `["1200","1200"]`. For example:

```bl
start_event start { output total: Number; }
end_event done { input result: Number; }
decision_task echo {
  input total: Number;
  output result: Number = value;
  literal_expression value { output result: Number; expression total; }
}
process pass_through {
  flow start -> echo;
  flow echo -> done;
  bind start.total -> echo.total;
  bind echo.result -> done.result;
}
```

[`examples/input_request.bl`](examples/input_request.bl) shows a client response port in a process. Declare `input_request review { prompt "Approve this order?"; output approved: Bool; }` as a named task peer: `prompt` must be a nonempty literal string, and at least one named, typed `output` is required. Use `flow start -> review;` and `flow review -> done;` to place the request in the route; `bind review.approved -> done.approved;` declares a typed connection from the request's `Bool` output to the end event's `Bool` input. Validate and compile the `.bl` source (without running the process) with `cargo run -p blkit-cli -- examples/input_request.bl /tmp/blkit-input-request.rs` (or `blkit examples/input_request.bl /tmp/blkit-input-request.rs` with the installed CLI).

### Answering an input request over HTTP

To run [`examples/input_request.bl`](examples/input_request.bl) on a local server from this checkout (in a separate shell after building):

```sh
mkdir -p /tmp/blkit-approval
cp examples/input_request.bl /tmp/blkit-approval/
cat > /tmp/blkit-approval/blkit.toml <<'TOML'
[project]
name = "approvaldemo"
blkit = "0.1.0"
build_target = "api-worker"
persistence = "local"
TOML
cargo run -p blkit-cli -- transpile /tmp/blkit-approval
cargo build --manifest-path /tmp/blkit-approval/.blkit/Cargo.toml
# Use $CARGO_TARGET_DIR/debug/approvaldemo-api-worker if CARGO_TARGET_DIR is set.
/tmp/blkit-approval/.blkit/target/debug/approvaldemo-api-worker /tmp/blkit-approval/orders.db 32 127.0.0.1:3000
```

Use the returned instance ID to inspect the outstanding request, then its request ID to reply (replace `INSTANCE_ID` and `REQUEST_ID` with the actual IDs):

```sh
curl -sS -H 'content-type: application/json' -d '{}' \
  http://127.0.0.1:3000/processes/demo/1.0/approval/instances
# HTTP 202: {"id":"INSTANCE_ID","status":"pending"}
curl -sS http://127.0.0.1:3000/instances/INSTANCE_ID
# When waiting, relevant fields: {"id":"INSTANCE_ID","status":"waiting-for-input","requests":[{"id":"REQUEST_ID","prompt":"Approve this order?","schema":{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}}]}
curl -sS -X POST -H 'content-type: application/json' -d '{"approved":true}' \
  http://127.0.0.1:3000/instances/INSTANCE_ID/requests/REQUEST_ID/response
# HTTP 202: {"id":"INSTANCE_ID","request_id":"REQUEST_ID","status":"accepted"}
curl -sS http://127.0.0.1:3000/instances/INSTANCE_ID
# Once completed, relevant fields: {"status":"completed","requests":[],"result":true}
```

Multipart clients can submit the JSON response and files in one request:

```sh
curl -sS -X POST \
  -F 'response={"approved":true};type=application/json' \
  -F 'files=@./approval.pdf;type=application/pdf' \
  http://127.0.0.1:3000/instances/INSTANCE_ID/requests/REQUEST_ID/response
```

The GET examples show **selected fields**; actual responses also contain instance identity, input, attempt and timing metadata. `requests` contains only outstanding requests, with stable IDs, prompts and portable `.bl` response types. An answered request disappears from `requests`; its raw accepted JSON is **not** returned by GET. `result` is the process's ordinary completed output (this example binds `approved` into the end event's `Bool` input), not a reply-history endpoint. POST must send a JSON **object with exactly the declared fields** and compatible values. Malformed JSON, missing/extra fields or wrong types return HTTP 400 (`{"error":"..."}`) without accepting the reply; unknown instance/request returns 404, and a duplicate or late answer after cancellation/terminal state returns 409. Only a successful committed answer returns 202; a remote reply can remain durable until a matching worker resumes the instance.

For local SSE, open a stream while the instance is active, or reconnect after any disconnection:

```sh
curl -N -H 'Accept: text/event-stream' http://127.0.0.1:3000/instances/INSTANCE_ID/events
# First event: event: snapshot, data: the current GET instance JSON (including outstanding requests)
# Later, on a change:
# event: changed
# data: {"id":"INSTANCE_ID"}
```

Each connection gets a fresh persisted `snapshot` before live `changed` hints. On `changed`, fetch `GET /instances/INSTANCE_ID` for the authoritative state; hints carry only the instance ID, not a reply. SSE sends keepalives; slow clients can be disconnected. Reconnect for a new snapshot after interruption: there is no `Last-Event-ID` replay or guaranteed history. GET works independently of SSE. Local and remote PostgreSQL SSE both provide snapshots and live hints; remote streams resynchronize from persisted state after listener reconnection or an expired registration. GET and response POST work in both local and remote modes (for a remote deployment, use a request-capable worker with the same process identity; see rollout below).

The server provides **no native authentication or TLS** and binds to loopback (`127.0.0.1:3000`) by default. Remote clients must use operator-managed authenticated TLS ingress with **per-instance authorization** on GET, SSE and response POST (and admission/cancellation where enabled); never expose the listener or database directly. Browser `EventSource` cannot set arbitrary `Authorization` headers: use an authenticated ingress cookie/session for browser SSE and authorize the same instance on the corresponding GET/POST; non-browser HTTP/SSE clients can send ingress authorization headers. Do not place credentials or reply values in an SSE URL.

A `subprocess called { process child; input input: Number; output result: Number; }` peer invokes a same-namespace/version child process. `flow called -> done;` routes a normal completion, while `flow called -> recover on error;`, `on cancel`, and `on terminate` handle child terminal outcomes. Child terminal values are not bound to handlers. Unhandled outcomes propagate with their terminal name; parent cancellation and deadlines cannot be caught. Parent and child share one instance and task-capacity limit. [`examples/subprocess.bl`](examples/subprocess.bl) compiles with `cargo run -p blkit-cli -- examples/subprocess.bl /tmp/subprocess.rs`; running `parent` with `{"input":"2"}`, `{"input":"0"}`, `{"input":"-1"}`, and `{"input":"11"}` returns `"2"`, `"100"`, `"200"`, and `"300"`. Register both generated processes to run the parent.

Optionally declare `retry max_retries 2 retry_for "10m" retry_delay "1s" backoff exponential;` and `deadline queued "10m";` (or `first_claimed`) inside a process before flows. `max_retries` counts additional attempts; retry windows and delays are enforced by the runtime. Named `error_event`, `cancel_event`, and `terminate_event` are business outcomes, not retryable execution errors. Peers `pause_for hold { duration "1m"; }` and `pause_until hold { input at: DateTime; }` persist their wake time; bind a `DateTime` with `bind start.closes_at -> hold.at;`. Waits release worker claims and survive restart. Deadlines produce `business-error` with reserved `terminal_name: "timeout"` without retrying.

A process can use `repeat_pre echo while echo.result < 3 initial 0 max_iterations 3;` (zero iterations yields the typed initial value) or `repeat_post echo while echo.result < 3 max_duration "1m";`; one or both positive bounds are required. Exceeding a bound produces `business-error` with `terminal_name: "task-iteration-limit"`. For a single-input `decision_task`, `multi_instance echo each start.values parallel;` (or `sequential`) takes a `List<T>` and produces an ordered `List<U>`; an empty list yields an empty result. [`examples/iteration.bl`](examples/iteration.bl) demonstrates a cyclic route with a required two-second deadline and a parallel multi-instance process. Its positive-input cycle repeats until timeout; it is a limit demonstration, not productive work. Compile it with `cargo run -p blkit-cli -- examples/iteration.bl /tmp/blkit-iteration.rs`.

**Breaking syntax change:** legacy `task`, `decision`, `node =`, and `link` declarations are rejected. Use named `decision_task` peers, braced processes, typed bindings, and explicit flows. Distributed API and worker binaries are described below.

## Run the development server

The development harness builds [`tools/dev-harness/examples/graph.bl`](tools/dev-harness/examples/graph.bl) into the binary automatically; it never parses `.bl` per request. Start the server with:

```sh
cargo run -p blkit-dev-harness --bin blkit-dev -- /tmp/blkit-dev.db 32 127.0.0.1:3000
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

The first argument is the local in-process Turso database path (default `blkit.db`); the second is the shared maximum number of in-flight tasks (default `32`, must be positive). The optional third address defaults to loopback `127.0.0.1:3000`; binding elsewhere explicitly exposes an **unauthenticated development API**. Restarting with the same database keeps past results and checkpoints. The in-process worker polls the same durable instance rows that HTTP admission writes; `202` means the instance was persisted, not that execution has started. Local typed-invalid input is rejected immediately without queuing. A checkpointed pending instance is claimed by the poller, retry-waiting work resumes when eligible, and interrupted running work counts as an execution failure: it resumes from the last committed checkpoint after a permitted delay, or becomes failed if retry limits prohibit it. Cancelling pending work before claim prevents its task from running; accepted cancellation stays cancelled. Turso adds the checkpoint/retry columns to existing local databases on open; old active records without a checkpoint are marked interrupted/failed rather than guessed or replayed, while old terminal records remain intact. Cancellation stops further graph advancement and signals all in-flight tasks, but cannot preempt a synchronous `.bl` function already running or undo external side effects; late results are ignored.

## Development containers

In VS Code, run **Dev Containers: Reopen in Container** and select the configuration for your host: the existing **blkit (Ubuntu 26.04 LTS)** option is for Fedora Silverblue with rootless Podman; **blkit (Windows + Docker Desktop)** is for Windows VS Code with a Windows checkout and Docker Desktop's WSL 2 backend. Both use the same Dockerfile. The Windows option forwards Docker Desktop's socket without making the host socket world-writable; do not use it from a VS Code Remote–WSL window.

If the checkout is on a Windows-backed mount (such as WSL's `/mnt/c` or a devcontainer's 9p-mounted `/workspaces`), keep Cargo build output on a Linux filesystem. On this setup, the full suite passed with:

```sh
CARGO_TARGET_DIR=/tmp/blkit-generated-test-target CARGO_BUILD_JOBS=8 cargo test --all --workspace -- --test-threads=4
```

Project integration tests use distinct generated binary names, so test threads can run in parallel. Moving the target directory is not specific to WSL and is unnecessary when Cargo already builds on a fast Linux filesystem. `/tmp` may be cleared between sessions, so use a persistent Linux filesystem if you need the build cache to survive restarts.

## PostgreSQL integration tests

Run isolated PostgreSQL tests with the Docker-compatible container-engine socket mounted in the selected devcontainer:

```sh
cargo test --test postgres
```

The Podman devcontainer sets `BLKIT_TESTCONTAINERS_HOST=host.containers.internal`; the Docker Desktop option sets it to `host.docker.internal`. If using a container started before selecting the new configuration, rebuild/reopen it to get the right value. Outside a devcontainer, omit it to use Testcontainers' discovered host. The Rust Testcontainers PostgreSQL module launches a fresh `postgres:17.6-alpine` database for each test, waits for readiness, and removes it afterward. No system PostgreSQL or manually managed database is needed.

To run the two-process failover example (kills the owner after B commits and proves C resumes without rerunning B):

```sh
cargo test --test postgres second_worker_process_resumes_c_after_owner_killed_without_replaying_b -- --nocapture
```

PostgreSQL keeps each instance as a durable queue row: `pending` or eligible `retry-waiting` rows remain present when claimed, with exact process identity, checkpoint, attempt metadata, owner, lease deadline and incrementing claim generation. Claims use `FOR UPDATE SKIP LOCKED`; committed checkpoint/terminal updates and lease renewals require the current unexpired owner and generation. Expired owners are reconciled by the API and worker loops using the policy persisted with each instance; a capable worker is required to claim the retry. A worker takes a positive lease duration in milliseconds (default `5000`) and renews it while executing; configure it longer than expected transient database pauses. Claims are fenced by owner, unexpired lease and generation.

## Running distributed API and workers

`tools/dev-harness/build.rs` compiles [`tools/dev-harness/examples/graph.bl`](tools/dev-harness/examples/graph.bl) ahead of time to Rust; the API uses `DistributedControl` without linked graphs, while `blkit-worker` links `named_graph_definitions()`. To deploy another graph, replace the `tools/dev-harness/build.rs` source with your `.bl` file and rebuild the worker. No worker compiles `.bl` at runtime. Use a new `.bl` `version` when changing graph semantics; same-identity graph compatibility is not checked. Keep old-version workers until their queued and claimed work finishes.

For an input-request rollout on an existing database with the `instances` table, apply these idempotent PostgreSQL additions **before** starting updated binaries (set `DATABASE_URL` to the same writable PostgreSQL URL used below):

```sh
psql -X -v ON_ERROR_STOP=1 "$DATABASE_URL" <<'SQL'
ALTER TABLE instances ADD COLUMN IF NOT EXISTS input_resume BOOLEAN NOT NULL DEFAULT FALSE;
CREATE TABLE IF NOT EXISTS input_requests (
    instance_id TEXT NOT NULL REFERENCES instances(id), request_id TEXT NOT NULL,
    prompt TEXT NOT NULL, schema TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'outstanding' CHECK (status IN ('outstanding','answered','consumed','retired')),
    response TEXT, PRIMARY KEY (instance_id, request_id)
);
CREATE INDEX IF NOT EXISTS input_requests_answered ON input_requests(instance_id) WHERE status='answered';
CREATE TABLE IF NOT EXISTS instance_attachments (
    instance_id TEXT NOT NULL REFERENCES instances(id),
    request_id TEXT,
    ordinal BIGINT NOT NULL CHECK (ordinal >= 0),
    object_key TEXT NOT NULL,
    filename TEXT NOT NULL,
    media_type TEXT,
    byte_length BIGINT NOT NULL CHECK (byte_length >= 0),
    FOREIGN KEY (instance_id, request_id) REFERENCES input_requests(instance_id, request_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_start ON instance_attachments(instance_id, ordinal) WHERE request_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_reply ON instance_attachments(instance_id, request_id, ordinal) WHERE request_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS instance_interests (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    channel TEXT NOT NULL CHECK (octet_length(channel) BETWEEN 1 AND 63),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (instance_id, channel)
);
CREATE OR REPLACE FUNCTION notify_instance_interests() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE changed_id TEXT;
BEGIN
    IF TG_TABLE_NAME = 'instances' THEN
        IF NEW.status NOT IN ('completed', 'business-error', 'cancelled', 'terminated', 'failed')
            OR NEW.status = OLD.status THEN
            RETURN NEW;
        END IF;
        changed_id := NEW.id;
    ELSE
        IF TG_OP = 'INSERT' THEN
            IF NEW.status <> 'outstanding' THEN RETURN NEW; END IF;
        ELSIF OLD.status <> 'outstanding' OR NEW.status NOT IN ('answered', 'retired') THEN
            RETURN NEW;
        END IF;
        changed_id := NEW.instance_id;
    END IF;
    PERFORM pg_notify(interest.channel, changed_id)
        FROM instance_interests interest
        WHERE interest.instance_id = changed_id AND interest.expires_at > clock_timestamp();
    RETURN NEW;
END $$;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='input_requests'::regclass AND tgname='input_request_interests_notify') THEN
        CREATE TRIGGER input_request_interests_notify
            AFTER INSERT OR UPDATE OF status ON input_requests
            FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='instances'::regclass AND tgname='instance_terminal_interests_notify') THEN
        CREATE TRIGGER instance_terminal_interests_notify
            AFTER UPDATE OF status ON instances
            FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
    END IF;
END $$;
SQL
```

These statements match `PostgresStore::connect_with_pool_size`; startup also runs schema setup, but that is not a substitute for sequencing a mixed-version rollout. Local Turso creates the same `instance_attachments` table and partial unique indexes when opening its database; reopen is safe. Each row contains an instance ID, optional request ID, nonnegative ordinal, opaque object key, display filename, optional media type and nonnegative byte length — **never file bytes**. A reply's `(instance_id, request_id)` must match an existing input request; start files use a NULL request ID. Ordinals are unique within each start or reply scope, including NULL start scope, and listing uses ordinal order. Consuming a response leaves its attachment rows available. Store attachment objects privately under the application-owned prefix `blkit/attachments/`, not under client filenames.

Trusted Rust worker code can construct `blkit_core::attachment_reader::AttachmentReader::local(LocalStore, AttachmentStore)` or `::remote(PostgresStore, AttachmentStore)` with the same durable database and object location as the API. Call `list(instance_id, None)` for ordered start-file metadata or `list(instance_id, Some(request_id))` for reply metadata (including consumed replies); `read(instance_id, request_id, ordinal)` returns `Ok(None)` only when that attachment record is absent, and an error if the referenced object cannot be read. Metadata returned by `list` excludes object keys and file bytes. Remote workers must use the same configured cloud store (`AttachmentStore::remote_from_env(AttachmentRole::Worker)`); the local filesystem store is for single-node deployments only. This Rust interface does not add an HTTP download route or a `.bl` file type.

### Attachment storage and backups

Provision storage **before** starting a generated binary, including for JSON-only clients. A local `server` or `api-worker` (`persistence = "local"`) creates a writable `attachments/` directory beside its selected database file (default `blkit.db`); for `./orders.db` this is `./attachments/`. To use another persistent filesystem, set `BLKIT_ATTACHMENT_ROOT=/persistent/orders-attachments` on that local process. Keep both the local database file and attachment root on persistent storage; startup fails if the root cannot be written. No cloud settings are needed locally.

Every remote `api-only`, `worker-only` (or `worker`), `api-worker` with `persistence = "remote"`, and both processes of `api-worker-split` **require** `BLKIT_ATTACHMENT_BACKEND=s3|gcs|azure` plus `BLKIT_ATTACHMENT_BUCKET` for S3/GCS or `BLKIT_ATTACHMENT_CONTAINER` for Azure. Remote filesystem (`fs`) and a JSON-only fallback are not supported. Use the **same private bucket/container and `blkit/attachments/` prefix** on all API and worker replicas. The API role (including combined) needs write/read/delete permissions for upload and startup probes; for S3, also grant `s3:AbortMultipartUpload` on attachment objects so failed multipart uploads can be aborted (the startup probe does not check this permission). Worker roles (including combined) need list/read. Missing/invalid settings or failed store probes prevent startup, even if no files are sent. Provision the location and credentials for the role before launching it; do not place secrets in `blkit.toml` or client requests.

For example, supply one of these provider-native credential sets alongside the mandatory blkit variables on **each** remote process (use secret management rather than committing credentials):

| Backend | Required blkit location | Example provider environment for `object_store` |
| --- | --- | --- |
| S3 | `BLKIT_ATTACHMENT_BACKEND=s3`, `BLKIT_ATTACHMENT_BUCKET=orders-private` | `AWS_DEFAULT_REGION=us-east-1`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN` for temporary keys); alternatively an IAM role via the AWS credential chain. `AWS_ENDPOINT_URL_S3` can select a custom S3 endpoint. |
| GCS | `BLKIT_ATTACHMENT_BACKEND=gcs`, `BLKIT_ATTACHMENT_BUCKET=orders-private` | `GOOGLE_APPLICATION_CREDENTIALS=/secure/orders-gcs.json` for application default credentials (or `GOOGLE_SERVICE_ACCOUNT=/secure/orders-gcs.json`); workload identity also works where available. |
| Azure Blob | `BLKIT_ATTACHMENT_BACKEND=azure`, `BLKIT_ATTACHMENT_CONTAINER=orders-private` | `AZURE_STORAGE_ACCOUNT_NAME=ordersaccount`, `AZURE_STORAGE_ACCOUNT_KEY` (or `AZURE_STORAGE_CLIENT_ID`, `AZURE_STORAGE_CLIENT_SECRET`, `AZURE_STORAGE_TENANT_ID` for a service principal). |

The generated binaries use provider `object_store` builders' `from_env()` with the blkit bucket/container name; there is no `BLKIT_ATTACHMENT_CREDENTIALS` setting. For S3, for example, after exporting `BLKIT_ATTACHMENT_BACKEND=s3`, `BLKIT_ATTACHMENT_BUCKET=orders-private`, `AWS_DEFAULT_REGION=us-east-1` and valid AWS credentials in **both** shells, run `orders-api "$POSTGRES_URL" 127.0.0.1:3000` and `orders-worker "$POSTGRES_URL" 32 5000`; for split builds, build the API from `.blkit/api/Cargo.toml` and the worker from `.blkit/Cargo.toml` (both binaries are under `.blkit/target/debug/` by default). A combined remote `orders-api-worker "$POSTGRES_URL" 32 5000 127.0.0.1:3000` needs both permission sets. The same configuration applies to the dev-harness `blkit-api` and `blkit-worker` binaries.

Back up the database **and** the corresponding private objects together with compatible retention windows, including terminal and cancelled instances; retain a consistent database/object snapshot for restore. Restoring metadata without its objects breaks worker reads; restoring objects without rows creates orphans. Restrict access (no public URLs) and track storage capacity. There is no automatic orphan deletion or per-instance retention policy: lifecycle rules must **not** delete referenced files merely because an instance is terminal or cancelled.

An API crash during upload or between upload and database commit can leave unreachable objects. For a manual orphan audit, first stop new admissions/uploads on **all** API replicas (or otherwise establish a quiescent, consistent database/object snapshot); wait for in-flight requests and uncertain commits to settle. Export all `instance_attachments.object_key` values from the authoritative database, list keys under `blkit/attachments/` in the corresponding private object location, and identify only keys absent from the export. Allow a conservative grace period for uploads, listing delays and backup/restore lag; immediately before deleting a candidate, check the authoritative database again for any association and skip deletion on a failed/inconclusive check. Never delete any key referenced by a committed row; retain uncertain candidates for another audit. Object-store lifecycle expiry independent of database retention is unsafe. JSON-only clients do not need to change their request format.

Deploy updated API replicas first, then request-capable workers and compiled processes with a **new** namespace/version/process identity (normally a new `.bl` `version`), so old workers cannot claim request-bearing instances. Retain old workers for old-version backlog. Existing version-2 checkpoints without request fields remain readable; unsupported older checkpoint formats are rejected, not silently replayed. To roll back, stop admitting new request-using process versions, and keep capable workers and APIs running until outstanding requests drain or are cancelled. Do not drop the request schema or switch active request checkpoints to binaries that cannot read them.

Worker checkpoints and request records commit together in PostgreSQL: each outstanding request has a stable instance/request ID, prompt, and portable response schema with resolved named dictionary shapes. The persisted port types let a graph-free API validate responses through the HTTP reply route; the store validates accepted replies, and the worker checks again before consumption. An accepted reply is conditional and durable even while no matching worker is available. An idle instance reports `waiting-for-input` and releases its claim without a timer or retry; a matching worker resumes it after an answer, subject to the original deadline. GET, reply POST and SSE are available in both modes.

Remote SSE starts a dedicated LISTEN session on first use per API replica, commits LISTEN before registering leased instance interests and reading the initial snapshot, and keeps the listener out of the request pool. PostgreSQL delivers NOTIFY only on commit; the notification is a hint, not a durable event or a replacement for GET. Request creation/answering and watched terminal changes notify interested replicas, not every checkpoint. Even selective NOTIFY adds commit work and can serialize concurrent notification-bearing commits; a full PostgreSQL notification queue can cause those transactions to fail at commit (not succeed without a hint). A listener stuck in a long transaction can prevent queue cleanup. Do not assume zero queue usage in a quiet test means unlimited headroom under load; measure commit latency and queue usage at expected request rates and replica counts before increasing them.

When diagnosing missing or delayed remote hints, check API logs for `SSE listener disconnected; resynchronizing`, `SSE listener notification buffer overflow; reconnecting`, `SSE interest cleanup failed`, or an SSE startup error (`SSE listener unavailable`); verify PostgreSQL connectivity/session capacity and that the listener is idle rather than `idle in transaction`. Inspect the database (with a role allowed to view session details):

```sql
SELECT pg_notification_queue_usage() AS queue_fraction;
SELECT application_name, pid, state, xact_start
FROM pg_stat_activity WHERE application_name LIKE 'blkit_%';
SELECT channel, count(*) AS watched_instances, min(expires_at) AS earliest_expiry
FROM instance_interests WHERE expires_at > clock_timestamp() GROUP BY channel;
```

`pg_notification_queue_usage()` is a fraction, not a message count; repeat it during load and investigate sustained growth, long listener transactions and commit errors. A lost listener reconnects, re-registers interests and sends fresh persisted snapshots to attached clients; an expired live interest is renewed and resynchronized. During an outage hints can be missed and a new SSE subscription can fail until the listener returns. Clients should reconnect for a fresh snapshot or GET the current instance after interruption; do not rely on replay. If the database is unavailable, restore connectivity/session capacity first, then confirm listener reconnection and current GET state. Do not drop request rows or reset a notification queue as a recovery shortcut.

For a local demonstration **outside the devcontainer**, start PostgreSQL with a loopback-only port and a non-example password, then in separate terminals run:

```sh
# Terminal 1: PostgreSQL; remove the container when finished.
docker run --rm --name blkit-postgres -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
  -p 127.0.0.1:55432:5432 postgres:17.6-alpine

# Other terminals, with DATABASE_URL set to the matching private PostgreSQL address
# and the same cloud backend, bucket/container and provider credentials in each shell:
export BLKIT_ATTACHMENT_BACKEND=s3 BLKIT_ATTACHMENT_BUCKET=orders-private AWS_DEFAULT_REGION=us-east-1
# Supply valid AWS credentials or an IAM role with the API/worker permissions above.
cargo build -p blkit-dev-harness --bin blkit-api --bin blkit-worker
cargo run -p blkit-dev-harness --bin blkit-api -- "$DATABASE_URL"                  # default 127.0.0.1:3000
cargo run -p blkit-dev-harness --bin blkit-worker -- "$DATABASE_URL" 32 5000      # start two terminals
```

Set `DATABASE_URL=postgres://postgres:<password>@127.0.0.1:55432/postgres` on the same host; use URL-encoding for special password characters. In this devcontainer, the host Podman loopback is **not** reachable from inside the container: use a privately reachable PostgreSQL endpoint instead, or run the Testcontainers multi-worker command above. Start an instance with `curl -sS -H 'content-type: application/json' -d '{"total":"12"}' http://127.0.0.1:3000/processes/orders/1.0/parallel/instances`, then poll `/instances/ID`; its `result` is `{"left":"12","right":"12"}`. Queue rows remain inspectable even with no capable worker.

To drain a worker, read its ID from its `blkit-worker <id> ready` log line and execute `UPDATE workers SET draining=TRUE WHERE id='<id>';` in PostgreSQL. It stops claiming, renews existing leases until each attempt finishes or releases for retry, then unregisters and exits. Keep another worker with the **same exact namespace/version/process** for old-version backlog; new-version workers do not take it.

Both REST binaries bind to loopback by default. The API has **no built-in authentication or TLS**: for remote clients, put it behind an operator-managed authenticated TLS ingress; do not expose the listener or PostgreSQL port directly. Checkpoints prevent replay of committed tasks, but interrupted or uncommitted tasks and their external side effects are **at least once**. Make side-effecting tasks idempotent; lease fencing prevents stale database writes, not external effects.
