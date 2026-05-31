---
name: StateStore
description: Pluggable execution state store — the authoritative source of execution context, execution history, and run status for process instances, with per-transaction durability via a write-through event log
targets:
  - ../data/state_store.go
---

# StateStore

A `StateStore` is the **authoritative state store** for process execution and an **audit trail** of every step taken. Workers are stateless — they reconstruct all needed state from the StateStore each time they pick up a process. The StateStore determines **where** this state is stored.

The store records two kinds of write event as they happen:

- **`Transaction`** writes from `ExecutionContext` (variable-assignment events with `Pending` / `Committed` / `Aborted` status transitions).
- **`ExecutionStep`** writes from `ExecutionHistory` (lifecycle events — `NODE_SCHEDULED`, `NODE_STARTED`, `NODE_COMPLETED`, `NODE_FAILED`, `PROCESS_*`).

Both event kinds are self-describing: every `Transaction` carries a `CommitNumber` (1-indexed monotonic per process instance) and a `Timestamp` set when the worker first appended it; every status transition and lifecycle step carries its own timestamp. **Arrival order at the backend does not matter** — `LatestContext` and `Get` reconstruct order at replay time by sorting on `(Timestamp, CommitNumber)`. This eliminates the need for sharding, ordered queues, or per-instance write serialization.

```go
type WriteOpKind int

const (
    OpRecordTransaction WriteOpKind = iota // append a Pending transaction
    OpUpdateStatus                          // flip a node's Pending transactions to Committed/Aborted
    OpRecordStep                            // append a lifecycle step
)

type WriteOp struct {
    ProcessInstanceID string
    Kind              WriteOpKind

    Transaction  *Transaction   // populated when Kind == OpRecordTransaction
    StatusUpdate *StatusUpdate  // populated when Kind == OpUpdateStatus
    Step         *ExecutionStep // populated when Kind == OpRecordStep
}

type StatusUpdate struct {
    NodeID        string
    NewStatus     TransactionStatus // Committed or Aborted
    Timestamp     time.Time         // when the worker called Commit/Abort
    CommitNumbers []int             // identifies the Pending transactions being settled
}

type StateStore interface {
    // === Execution-state factories ===
    //
    // These return ExecutionContext / ExecutionHistory wired to this store's
    // writer channel — subsequent ctx.Record / ctx.Commit / ctx.Abort and
    // history.Record(...) calls automatically stream WriteOps to this store.
    // Use these (not raw constructors) when the resulting state will be passed
    // to Process.Evaluate().

    // Build a fresh ExecutionContext + ExecutionHistory for a new process
    // instance. Generates a ProcessInstanceId; records the input variables as
    // the initial transaction against the start node identified by opts.StartId.
    NewExecutionState(
        process *Process,
        opts NewExecutionStateOpts,
    ) (*ExecutionContext, *ExecutionHistory, error)

    // Reconstruct ExecutionContext + ExecutionHistory for an existing process
    // instance from the durable event log. Returns (nil, nil, nil) if the
    // processInstanceID is unknown.
    LoadExecutionState(
        processInstanceID string,
    ) (*ExecutionContext, *ExecutionHistory, error)

    // === Reads (un-wired; for inspection / dashboards / debugging) ===

    // Retrieve the execution history for a run.
    Get(processInstanceID string) (ExecutionHistory, error)

    // Retrieve the latest execution context for a run.
    LatestContext(processInstanceID string) (*ExecutionContext, error)

    // Connection details for out-of-process workers.
    Config() map[string]string

    // === Writes ===

    // Persist process-instance metadata (status, timestamps, evaluation_count).
    // Called at evaluation boundaries; per-event payload travels via WriteBatch.
    Save(processInstanceID string, history ExecutionHistory) error

    // Primary write method. Implementations should batch the ops into a single
    // backend round-trip where the backend supports it (transaction, pipeline,
    // write batch). Backends with no batching benefit loop internally.
    WriteBatch(ops []WriteOp) error

    // Per-op convenience wrappers — default implementations are 1-element WriteBatch calls.
    RecordTransaction(processInstanceID string, tx Transaction) error
    UpdateTransactionStatus(processInstanceID string, update StatusUpdate) error
    Record(processInstanceID string, step ExecutionStep) error

    // Durability barrier. Forces any buffered writes for the instance to land
    // before returning. Called at evaluation checkpoints and on root-scope Commit/Abort.
    Flush(processInstanceID string) error
}

type NewExecutionStateOpts struct {
    StartId string         // start node id; must match a Start(id) in the process graph
    Input   map[string]any // initial variables; defaults to empty when omitted
}
```

- **`NewExecutionState`** and **`LoadExecutionState`** are the canonical factories for execution state that will be evaluated. They produce wired Context and History objects whose `Record` / `Commit` / `Abort` calls stream to this store via the writer pool. The bare `Get` / `LatestContext` methods return un-wired snapshots suitable for read-only inspection only — passing those to `Evaluate` would silently swallow all per-event writes.
- **`WriteBatch`** is the canonical method an implementation must provide. The per-op methods (`RecordTransaction`, `UpdateTransactionStatus`, `Record`) are convenience wrappers that call `WriteBatch` with a 1-element slice.
- **`StateStore` methods are synchronous** — they may block on I/O. The non-blocking, write-through behaviour seen by `ctx.Record` / `ctx.Commit` callers is provided by the runtime's writer pool (see [../worker/worker.spec.md](../worker/worker.spec.md#writer-pool)), not by the `StateStore` itself.
- **`Flush`** is the durability barrier — callers use it when they need a guarantee that prior writes for the instance have landed.
- **`config()`** returns a dictionary of connection details needed to reconstruct a connection to this state store from another process. Used to forward connection details to worker binaries running on other hosts. `InMemoryStateStore.config()` raises `ValueError` since in-memory state cannot be shared.

### Dual Role

The StateStore serves two purposes:

- **State store** — the source of truth for execution context, completed tasks, and run status. Workers call `Get()` to load the full execution history and `LatestContext()` to load the current variable state. `Save()` persists process-instance metadata (status, timestamps, `evaluation_count`) at evaluation boundaries. Per-event payload — `Transaction`s and `ExecutionStep`s — is written via `WriteBatch` as the runtime's writer pool drains its queue.
- **Audit trail** — a chronological record of every `Transaction` and `ExecutionStep`, accessible via `Get()` and `LatestContext()`. Each event carries its own timestamp, making the durable log a complete, replayable record of execution.

---

## Replay-time ordering

Persistent backends are not required to preserve the order events arrived at the store. On `LatestContext` / `Get`, the backend:

1. Fetches all events for the process instance.
2. Sorts ascending by `(Timestamp, CommitNumber)` — `Timestamp` primary, `CommitNumber` for ties.
3. Folds the events in order: `OpRecordTransaction` appends a `Pending` transaction to the rebuilt `ExecutionContext`; `OpUpdateStatus` flips the matching transactions' status; `OpRecordStep` appends to `ExecutionHistory`.

Implementations may keep a materialized "latest" view to avoid full replay on every pickup, but the contract is "the durable event log is authoritative".

---

## Context Serialization

For `InMemoryStateStore`, `Transaction.Values` are held as `Bl` value objects directly — no serialization is needed.

For persistent state store implementations (SQLite, RocksDB, BadgerDB, PostgreSQL, AzureSQL, S3, LocalFS), each `Transaction.Values` map is stored in two forms:

- **CBOR binary** — the canonical representation used for loading the value back into the worker. CBOR's semantic tags preserve `Bl` types losslessly (e.g. tag 4 for arbitrary-precision decimal fractions). This is the format consumed by replay.
- **JSON string** — a human-readable copy for inspection and debugging. Not used for deserialization. Each `Bl` value is represented as a JSON object with a type discriminator (e.g. `{"_type": "date", "year": 2026, "month": 4, "day": 3, "offset": "+05:30", "timezone": "Asia/Kolkata"}`).

The CBOR encoding maps `Bl` types to CBOR as follows:

| Bl type | CBOR representation |
|---|---|
| `BlNumber` | Decimal fraction (tag 4) |
| `BlString` | Text string |
| `BlBoolean` | Boolean |
| `BlNull` | Null |
| `BlDate` | Tagged map (blkit tag) with year, month, day, offset, timezone |
| `BlTime` | Tagged map (blkit tag) with hour, minute, second, offset, timezone |
| `BlDateTime` | Tagged map (blkit tag) with date and time components |
| `BlDuration` | Tagged map (blkit tag) with years, months, days, hours, minutes, seconds |
| `BlList` | Array of `Bl` values |
| `BlDictionary` | Map of string keys to `Bl` values |
| `BlRange` | Tagged map (blkit tag) with start, end, and inclusive flags |

`Bl` types with blkit-specific attributes (e.g. `BlDate` with offset and timezone) use blkit-defined semantic tags from CBOR's private-use range, encoding the full set of attributes as a CBOR map. This ensures lossless round-tripping of all `Bl` values regardless of custom attributes.

---

## Performance

`LatestContext()` is on the critical path — workers call it on every process evaluation task. Implementations should optimize this method for fast retrieval, either by maintaining a materialized "latest" projection that is updated as `WriteBatch` is applied, or by indexing the event log so replay is bounded.

`WriteBatch` should map to the backend's most efficient batched-write primitive (single transaction, pipeline, write batch). Implementations that loop the per-op methods internally without batching forfeit much of the writer pool's throughput benefit on bursty workloads.

---

## InMemoryStateStore (default)

The default state store holds events in memory. Fast and zero-dependency, but lost when the worker process exits. When in-process or transient durability is acceptable, instantiate `NewInMemoryStateStore()` and pass it to `worker.Run`. `config()` raises `ValueError` — in-memory state cannot be shared with external processes. `WriteBatch` loops over the ops and applies them to in-memory data structures; `Flush` is a no-op.

```go
type InMemoryStateStore struct{}

func NewInMemoryStateStore() *InMemoryStateStore
```

---

## SQLiteStateStore

Persists events to an embedded SQLite database. Provides a durable audit trail without external dependencies. Events are queryable using standard SQLite tooling. `Transaction.Values` are stored as CBOR binary with a parallel JSON string column. `WriteBatch` wraps the batch in a single `BEGIN IMMEDIATE; … COMMIT;` block. `Flush` is implicit in the commit (SQLite syncs per the journal mode).

`config()` returns `{"type": "sqlite", "path": "<path>"}`.

```go
type SQLiteStateStore struct{}

func NewSQLiteStateStore(path string) *SQLiteStateStore
```

---

## RocksDBStateStore

Persists events to an embedded RocksDB database. Higher write throughput than `SQLiteStateStore` under heavy workloads, at the cost of not being queryable with SQL. `Transaction.Values` are stored as CBOR binary with a parallel JSON value. `WriteBatch` issues a single RocksDB `WriteBatch` (atomic, batched fsync). Requires cgo.

`config()` returns `{"type": "rocksdb", "path": "<path>"}`.

```go
type RocksDBStateStore struct{}

func NewRocksDBStateStore(path string) *RocksDBStateStore
```

---

## BadgerDBStateStore

Persists events to an embedded BadgerDB database. Pure-Go alternative to `RocksDBStateStore` — no cgo dependency, comparable write throughput. Events are stored under per-instance key prefixes. `Transaction.Values` use CBOR binary with a parallel JSON value. `WriteBatch` runs the batch inside a single `db.Update(...)` write transaction. `Flush` triggers `db.Sync()` for the configured value-log.

`config()` returns `{"type": "badger", "path": "<path>"}`.

```go
type BadgerDBStateStore struct{}

func NewBadgerDBStateStore(path string) *BadgerDBStateStore
```

---

## PostgresStateStore

Persists events to PostgreSQL. Suitable for distributed deployments where multiple worker instances need to contribute to a shared event log, with the added benefit of full SQL queryability. Each event is one row in a per-instance partition; `Transaction.Values` are stored in `bytea` (CBOR) and `jsonb` columns side-by-side. `WriteBatch` issues a single multi-row `INSERT ... VALUES (...), (...), ...` inside a transaction.

`config()` returns `{"type": "postgres", "url": "<url>", "schema": "<schema>"}`.

```go
type PostgresStateStore struct{}

func NewPostgresStateStore(
    url string,    // PostgreSQL connection URL
    schema string, // PostgreSQL schema namespace
) *PostgresStateStore
```

---

## AzureSQLStateStore

Persists events to Azure SQL Database (or any Microsoft SQL Server). Suitable for distributed deployments running on Azure where the surrounding infrastructure already standardizes on SQL Server, with the same SQL queryability benefits as `PostgresStateStore`. Each event is one row in a per-instance partition; `Transaction.Values` are stored in `varbinary(max)` (CBOR) and `nvarchar(max)` JSON columns side-by-side. `WriteBatch` issues a single multi-row `INSERT ... VALUES (...), (...), ...` inside a transaction; SQL Server caps a single `VALUES` clause at 1000 rows, so larger batches are split into multiple statements within the same transaction.

`config()` returns `{"type": "azuresql", "url": "<url>", "schema": "<schema>"}`. Authentication follows the connection string (SQL auth, Azure AD via the `Authentication=` parameter, or Managed Identity).

```go
type AzureSQLStateStore struct{}

func NewAzureSQLStateStore(
    url string,    // Azure SQL / SQL Server connection URL
    schema string, // SQL schema namespace
) *AzureSQLStateStore
```

---

## S3StateStore

Persists events to an S3-compatible object store. Compatible with AWS S3, MinIO, Cloudflare R2, GCS via S3 gateway, and any other server speaking the S3 API. Suitable for audit / archival use cases, long-running human workflows, and environments where cheap, durable, geo-redundant storage matters more than per-event latency.

Layout: one object per `WriteBatch` at `{prefix}/{processInstanceID}/{firstTimestamp}-{batchUUID}.cbor`. The object body is the CBOR-encoded slice of `WriteOp`s; a sibling `.json` object carries the human-readable representation. `LatestContext` / `Get` list the prefix `{prefix}/{processInstanceID}/`, fetch the matching objects in parallel, and replay-sort.

`WriteBatch` issues one PUT per call. The PUT latency (~50–200 ms typical) sets the floor on per-event durability lag — tune `MaxBatchWait` (see [../worker/worker.spec.md](../worker/worker.spec.md#configuration-knobs)) higher for throughput, lower for latency. `Flush` is a no-op once the in-flight PUTs return — S3 is durable on PUT acknowledgement.

`config()` returns `{"type": "s3", "endpoint": "<endpoint>", "region": "<region>", "bucket": "<bucket>", "prefix": "<prefix>"}`. Credentials follow the standard AWS SDK chain (env vars, IAM, profile).

```go
type S3StateStore struct{}

func NewS3StateStore(
    endpoint string, // empty for AWS S3; set for MinIO/R2/etc.
    region   string,
    bucket   string,
    prefix   string,
) *S3StateStore
```

---

## LocalFSStateStore

Persists events as plain files on the local filesystem, mirroring the `S3StateStore` layout: one CBOR file per `WriteBatch` at `{root}/{processInstanceID}/{firstTimestamp}-{batchUUID}.cbor`, with a sibling `.json` for inspection. `WriteBatch` writes one pair of files per call; `Flush` calls `fsync(2)` on each pending file and the containing directory to guarantee durability.

Useful when:

- You want events as files on disk for forensic inspection, simple `rsync`-based backup/migration, or pipeline integration with standard text tooling.
- The deployment forbids embedded DBs (RocksDB / BadgerDB / SQLite).
- You want a development backend that's trivially debuggable without DB tooling.

For most production single-machine deployments, `BadgerDBStateStore` or `SQLiteStateStore` will perform better — `LocalFSStateStore`'s file-per-batch model trades throughput for inspectability.

`config()` returns `{"type": "localfs", "root": "<absolute-path>"}`. Note that `LocalFSStateStore.config()` is only meaningful if every consuming process can mount the same absolute path — for cross-machine sharing, use `S3StateStore` instead. Network filesystems (NFS / SMB / EFS) are explicitly unsupported as the durable layer due to unreliable locking and `fsync` propagation.

```go
type LocalFSStateStore struct{}

func NewLocalFSStateStore(root string) *LocalFSStateStore
```

---

## Custom State Stores

The `StateStore` interface allows custom implementations — for example, streaming to an observability platform or appending to a log file. Implementations need only provide `WriteBatch` (the per-op wrappers can default to 1-element `WriteBatch` calls), the read methods, and `Flush`.

---

## Edge Cases

- The state store implementation is independent of the `MessageGateway` choice. Any combination is valid (e.g. `PostgresStateStore` with `RedisMessageGateway`).
- `SQLiteStateStore`, `RocksDBStateStore`, `BadgerDBStateStore`, and `LocalFSStateStore` create their database / directory at the specified path if it does not exist. If the path is not writable, store creation fails with an I/O error.
- `PostgresStateStore` and `AzureSQLStateStore` require a reachable database server. Behaviour on unreachability is governed by the worker's `WritePolicy` (see [../worker/worker.spec.md](../worker/worker.spec.md#write-policy)).
- `PostgresStateStore` and `AzureSQLStateStore` create the schema and tables if they do not exist. If the user lacks `CREATE` privileges, store creation fails with a permissions error.
- `S3StateStore` requires the bucket to exist and credentials with `PutObject` / `ListObjectsV2` / `GetObject` permissions. Bucket auto-creation is not attempted.
- `LocalFSStateStore` is single-machine only — `config()` is meaningful only when every consuming process can mount the same absolute path. Network filesystems are explicitly unsupported.
- `Get()` for an unknown `process_instance_id` returns `None` (not an error).
- `LatestContext()` for a run with no recorded events returns `None`.
- `Save()` persists the `ExecutionHistory` object's metadata (`process_instance_status`, `evaluation_count`, timestamps). For persistent backends, this is an upsert keyed by `process_instance_id`. The per-event payload (`Transaction`s, `ExecutionStep`s) is written via `WriteBatch`, not `Save`.
- `WriteBatch` is atomic per call where the backend supports atomicity (SQLite transaction, RocksDB write batch, BadgerDB `Update`, Postgres transaction, Azure SQL transaction). For S3 and LocalFS, the unit of atomicity is a single object/file — partial-batch failures are visible only as missing objects/files, which replay tolerates (the missing events simply do not contribute to projected state).
- `Flush` is a per-instance barrier — it returns once every `WriteBatch` enqueued before the call has been confirmed durable by the backend.
- `RecordTransaction` / `UpdateTransactionStatus` / `Record` are convenience wrappers; default implementations call `WriteBatch` with a 1-element slice. Backends do not need to override them unless they have a faster single-op path.
- Replay tolerates an `OpUpdateStatus` event whose `Timestamp` is later than its target `OpRecordTransaction` events regardless of arrival order at the backend — the sort step puts them in order before applying.
- Replay tolerates duplicate events (same `(processInstanceID, NodeID, ExecutionID, CommitNumber)` for transactions; same `(processInstanceID, ExecutionID, Step.Type, Timestamp)` for steps) by deduplicating during the fold step. This makes idempotent retries on the writer pool safe.
