# NATS

> A durable, shareable state store backed by NATS JetStream — a natural fit when
> NATS is already your message broker.

The NATS backend keeps each run's state in **JetStream**, the part of NATS that
stores data durably, using a JetStream **key-value bucket**. It is **durable**
(state survives a restart when JetStream is backed by disk) and **shareable** —
workers on different machines can all reach the same NATS servers and work on the
same runs.

Its stand-out benefit is **infrastructure reuse**. If you already run NATS as
blkit's [message broker](../message-brokers/nats.md), this backend stores process
state in the same system, so one dependency covers both the queue and the state —
no separate database to provision, secure, and operate.

It lives in its own module, so the client it needs is only pulled in by
applications that use it:

```go
import (
    bl        "github.com/friendly-business-machines/blkit"
    natsstore "github.com/friendly-business-machines/blkit/stores/nats"
)

// The URL points at the server(s) and the Bucket names the JetStream KV bucket;
// the same two values handed to workers in other processes or on other machines
// let them all work on the same runs.
store := natsstore.New(natsstore.Config{
    URL:    "nats://localhost:4222",
    Bucket: "blkit-state",
})
defer store.Close()
```

`New` dials NATS and creates (or binds to) the bucket; following the blkit
constructor convention it panics on an invalid config or a failed connect. The
backend is built on [nats.go](https://github.com/nats-io/nats.go) using its
current `jetstream` API — not the legacy `nats.JetStreamContext`. Every write waits
for the JetStream publish acknowledgement, so a write is quorum-accepted, and
already durable, by the time the call returns.

## What it's good for

- **Setups that already run NATS** as the message broker — store state in the same
  system instead of adding a separate database.
- **Durable runs shared across many workers**, on one machine or many.
- **Single-binary or edge deployments** — NATS can run embedded inside your Go
  process, so state has no separate service to operate at all (see below).

## Running the server

The backend needs a JetStream-enabled NATS server reachable at `Config.URL`.
JetStream must be turned on: a bare `nats-server` with no JetStream cannot host a
key-value bucket. In rough order of operational weight:

- **Embedded in-process** — `github.com/nats-io/nats-server` is an ordinary Go
  library, so you can start a JetStream server inside your own binary and point the
  store at it. This is the lightest option: no separate service, no container,
  nothing to deploy alongside the app. The conformance suite runs exactly this way,
  embedding a real `nats-server` in the test process. It suits single-binary tools,
  edge nodes, and tests.
- **Local companion for development** — run `nats-server -js` from a package
  manager, or a throwaway container
  (`docker run -p 4222:4222 nats:latest -js`), and point `URL` at
  `nats://localhost:4222`. The `-js` flag enables JetStream.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a
  `nats` container (or a small cluster for HA) next to your workers with JetStream
  enabled and **file-based storage**, and point `URL` at it. Use a replicated
  bucket if state must survive a node loss.
- **Managed service** — the backend speaks plain NATS, so it works unchanged
  against **Synadia Cloud** and the **NGS** global NATS service. Point `URL` at the
  managed endpoint and carry credentials in the URL.

If NATS is already your message broker, the simplest choice is to reuse that same
server for state: one JetStream deployment, two buckets' worth of duty.

## Data model

Everything blkit persists about a run lives under its own branch of the key-value
bucket. Keys use a `.`-separated hierarchy rooted at the run id, so a single
`ListKeys` filtered by prefix gathers exactly one run's data:

| Key | Holds |
|---|---|
| `{runID}.meta` | The run's metadata record (`RunMetadata`), written by `Save`. |
| `{runID}.v.{ts}.{n}` | One value a task wrote: `{task_id, execution_id, field, value, status, ts}`. |
| `{runID}.h.{ts}.{n}` | One execution-history entry: `{kind, node_id, execution_id, payload, ts}`. |

Records are encoded as JSON, sharing the same shape the embedded backends use.
`{ts}` is the event's timestamp as **fixed-width, zero-padded decimal Unix
nanoseconds** (20 digits), and `{n}` is a small per-open counter (12 digits) whose
only job is to keep two same-nanosecond keys distinct. The key's timestamp makes
keys unique and readable, but it is **not** the authoritative order — see
[ordering](#ordering-under-parallelism) below.

**Example keys**, for a run of an `order-approval` process where `check-inventory`
succeeded on its first attempt and `approve-order` failed once (`exec_b1`) before
committing on retry (`exec_b2`) — shown as they read once both tasks have settled,
with `{ts}.{n}` abbreviated for readability (the real keys zero-pad `{ts}` to 20
digits and `{n}` to 12):

```
run_8f2c1a90.meta                         →  {"process_id":"order-approval","process_version":"v1","status":"completed", …}

run_8f2c1a90.v.1783501920340000000.1      →  {"task_id":"check-inventory","execution_id":"exec_a1","field":"in_stock","value":true,"status":"committed", …}
run_8f2c1a90.v.1783501920610000000.2      →  {"task_id":"approve-order","execution_id":"exec_b1","field":"approved","value":true,"status":"aborted", …}
run_8f2c1a90.v.1783501920780000000.3      →  {"task_id":"approve-order","execution_id":"exec_b2","field":"approved","value":true,"status":"committed", …}

run_8f2c1a90.h.1783501920120000000.4      →  {"kind":"task_started","node_id":"check-inventory","execution_id":"exec_a1", …}
run_8f2c1a90.h.1783501920410000000.5      →  {"kind":"task_completed","node_id":"check-inventory","execution_id":"exec_a1", …}
run_8f2c1a90.h.1783501920640000000.6      →  {"kind":"task_failed","node_id":"approve-order","execution_id":"exec_b1","payload":{"error":"validation timeout"}, …}
run_8f2c1a90.h.1783501920820000000.7      →  {"kind":"task_completed","node_id":"approve-order","execution_id":"exec_b2", …}
```

The `run_8f2c1a90.v.…2` key is `approve-order`'s failed attempt. Unlike the SQL
backends, its aborted status is not a new entry — it is a **second revision of the
same key** (see below): the key was first `Put` with `status: pending` when
`exec_b1` wrote `approved`, then `Put` again on that identical key with
`status: aborted` once the attempt failed. The retry (`exec_b2`) is a genuinely new
`ValueWrite`, so it gets its own new key (`…3`). A current-state read folds these
down to `in_stock: true` and `approved: true`, skipping the aborted key; a
full-history read returns all three `v.` entries plus the four `h.` entries.

### Values: pending, then settled in place

blkit never overwrites a field. Each write a task makes is its own KV entry, and the
current value of a field is *derived* from these entries rather than stored in place:

- **A write** (`ValueWrite`) `Put`s a record with `status: pending` on a **new**
  `{runID}.v.…` key. While the task runs, its outputs sit as pending entries and are
  invisible to the current-state read.
- **Settling** (`StatusFlip`) lists the run's `{runID}.v.…` keys and, for each of the
  finishing task's pending records, `Put`s the updated record back on the **same
  key** — a status flip is simply a new revision of that entry. Completing a task
  flips its records to `committed`; a failure flips them to `aborted`. Aborted
  records are kept, not deleted, so a failed attempt stays visible in the history
  next to the committed write that superseded it.

Because KV has no cross-key transaction, the flip is applied one key at a time
rather than atomically across all of a task's records. This is safe under blkit's
write contract: a still-pending record is invisible to readers, so a half-applied
flip never exposes a partially committed task, and a re-delivered flip is idempotent
(re-writing an already-committed record changes nothing). The bucket is created with
**history depth 1**, so the superseded pending revision is discarded and the bucket
stays compact — the audit trail lives in the records themselves, including the
aborted ones.

### Two reads over the entries

- **Current state** — read the run's `v.` entries and fold the latest `committed`
  record per `(task_id, field)`. The newest committed write wins; pending and
  aborted records are skipped.
- **Full history** — read the `v.` and `h.` entries together and return every one,
  each carrying its status, sorted into replay order.

### Ordering under parallelism

Every KV entry carries a JetStream **revision (stream sequence)**, assigned by the
server in the order writes were accepted. Reads sort on `(Timestamp, sequence)`:
the record's own timestamp first, with the stream sequence as the tiebreak for
equal timestamps. Parallel tasks within a run each append their own keys
concurrently; the order those calls happen to reach the server in does not matter,
because the stored timestamp and sequence — not client insertion order — define
replay order. This is what keeps the history stable and reproducible across
backends.

## Configuration

```go
type Config struct {
    URL    string // NATS server URL(s); required
    Bucket string // JetStream key-value bucket name; required
}
```

- **`URL`** — how to reach the NATS server or cluster, e.g.
  `nats://localhost:4222` (comma-separate several servers for a cluster).
  Credentials and TLS are carried in the URL in the usual NATS forms — a
  `user:pass@` prefix, a token, or a `tls://` scheme — so a managed endpoint needs
  only its connection string here.
- **`Bucket`** — the name of the JetStream key-value bucket state is stored in.
  Give independent blkit deployments different bucket names to keep their runs
  separate within one NATS system.

Because NATS is shared, the same `URL` and `Bucket` handed to workers in other
processes or on other machines let them all work on the same runs. The bucket is
created on first use if it does not already exist.

## Consistency

Reads and writes go through JetStream's key-value semantics. A `Put` returns only
once the write has been acknowledged by the JetStream server (quorum-accepted on a
replicated bucket), so a committed value is visible to the next read — a worker
always sees its own prior writes, and those of any other worker whose task finished
first. Durability follows the same rule as the other server-based backends: state
survives a restart when JetStream is configured to store data **on disk** (its
normal durable setup); the guarantee comes from how the server is provisioned, not
from this backend.

## What to keep in mind

- **Durability is JetStream's job.** Point the store at a JetStream server with
  file-based (not memory) storage — and a replicated bucket if you need to survive
  a node loss — if state must outlast a restart.
- **History grows without bound** — nothing is deleted by design, and aborted
  records are retained for audit. For long-lived deployments, plan a retention or
  cleanup policy against the bucket.
- **One bucket per deployment** when several share a NATS system, or two
  deployments will list each other's keys.

## Concurrency

Different runs use different key branches, so many runs — and many workers — can
share one NATS system without interfering. Parallel tasks within a single run each
write their own keys, so their writes never overwrite one another, and the order is
resolved from the stored timestamp and stream sequence at read time.

## Reference

The backend's API is in the [NATS reference](../reference/stores-nats.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
