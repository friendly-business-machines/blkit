---
name: NatsStateStore
description: A durable, shareable state-store backend that keeps each run's ProcessState in NATS JetStream — its own module; a natural fit when NATS is already the message broker
targets:
  - ../../stores/nats/store.go
---

# NatsStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The NATS backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in **NATS** — specifically in
**JetStream**, the part of NATS that stores data durably. It is **durable** and
**shareable** (workers on different machines can all reach the same NATS servers).

Its stand-out benefit is **infrastructure reuse**: if you already run NATS as the
message broker for blkit (see
[messagegateway/overview.spec.md](../messagegateway/overview.spec.md)), this backend
lets you store process state in the same system, with no extra database to run.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/nats`,
so the client it needs is only pulled in by applications that actually use it.

```go
import (
    bl        "github.com/friendly-business-machines/blkit"
    natsstore "github.com/friendly-business-machines/blkit/stores/nats"
)

var store = natsstore.New(natsstore.Config{
    URL:    "nats://localhost:4222",
    Bucket: "blkit-state",
})
```

---

## Implementation

The backend is built on **`github.com/nats-io/nats.go`** using its **`jetstream`**
package (the current API — not the legacy `nats.JetStreamContext`), against a
**JetStream key-value bucket** named in the config. Records are encoded as JSON.

## Key layout

KV keys use `.`-separated hierarchy, one branch per run:

```
{runID}.meta                → run metadata record (written via Save)
{runID}.v.{ts}.{n}          → {task_id, execution_id, field, value, status}
{runID}.h.{ts}.{n}          → {kind, node_id, execution_id, payload}
```

`{ts}` is the event timestamp as fixed-width decimal Unix nanoseconds; `{n}` is a
small per-open counter that only exists to keep same-nanosecond keys distinct. The
authoritative **arrival tiebreak is the entry's JetStream stream sequence**, which
every KV entry carries and which replay uses to sort `(Timestamp, arrival order)`.

How the write ops map onto it:

- **`ValueWrite`** → `Put` a record with `status: pending` on a new `{runID}.v.` key.
- **`StatusFlip`** → list the run's `{runID}.v.>` keys, and for each of the task's
  pending records, `Put` the updated record back on the **same key** — a status flip
  is simply a new revision of that entry. There is no cross-key transaction in KV,
  so the flip applies per key; this is acceptable under the
  [write contract](./overview.spec.md#the-write-contract) because reads treat any
  still-pending record as invisible, and a re-delivered flip is idempotent.
- **`HistoryEntry`** → `Put` a record on a new `{runID}.h.` key.
- A `WriteBatch` has no KV batching primitive, so ops are applied one by one — each
  `Put` waits for the JetStream **publish acknowledgement**, meaning a write is
  quorum-accepted when the call returns. Because every accepted write is already
  durable, **`Flush` is a no-op**.
- **Current version** — read the run's `v.` entries and fold the latest committed
  record per `(task_id, field)`. **Full history** — read `v.` and `h.` entries and
  sort by `(Timestamp, stream sequence)`; pending and aborted records are included
  with their status.
- The bucket is created with **history depth 1** — old revisions of a flipped entry
  are not needed (the audit story lives in the records themselves, including aborted
  ones), so the bucket stays compact.

---

## Configuration

The backend is constructed with the details for reaching the NATS servers — the
server URL (or URLs), any credentials, and the name of the JetStream bucket to store
state in. Because NATS is shared, these same details can be handed to workers in
other processes or on other machines so they can all work on the same runs.

---

## Durability

State is durable when **JetStream is set up to store data on disk** (its normal
durable configuration). In that setup runs survive a restart. As with the other
server-based backends, the durability guarantee comes from how the server is
configured, not from this backend.

---

## What it is good for

- **Setups that already run NATS** as the message broker — store state in the same
  system instead of adding a separate database.
- **Durable runs shared across many workers**, on one machine or many.

---

## Concurrency

- **Different runs** use different keys, so many runs — and many workers — can share
  the same NATS system without interfering.
- **Parallel tasks within one run** each write their own keys, so their writes do not
  overwrite each other. The order is sorted out from the timestamps at read time.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a **real
NATS server with JetStream enabled, embedded in the test process** (`nats-server`
is importable as a Go library) — the genuine engine, with no external system or
container needed. Each subtest gets its own server, store directory, and bucket.

`[@test] ../../stores/nats/store_test.go`
