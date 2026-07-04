# NATS

> A durable, shareable state store backed by NATS JetStream — a natural fit when
> NATS is already your message broker.

The NATS backend keeps each run's state in **JetStream**, the part of NATS that
stores data durably. It is **durable** and **shareable** across workers on different
machines. Its stand-out benefit is **infrastructure reuse**: if you already run NATS
as blkit's message broker, this backend stores process state in the same system,
with no extra database to run.

It lives in its own module, so its client is only pulled in by applications that
use it:

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

The backend is built on [nats.go](https://github.com/nats-io/nats.go) using its
current `jetstream` API, against a JetStream key-value bucket. Each write waits for
the JetStream publish acknowledgement, so a write is quorum-accepted — and already
durable — by the time the call returns.

## What it's good for

- **Setups that already run NATS** as the message broker — store state in the same
  system instead of adding a separate database.
- **Durable runs shared across many workers**, on one machine or many.

## How state is stored

Each run occupies its own branch of the key-value bucket: a metadata entry, one entry
per value a task writes, and one entry per execution-history entry. A value entry
starts pending and is settled in place when the task finishes; reads treat any
still-pending entry as invisible, and the current state folds the latest committed
entry per field. The full history returns every entry — including pending and aborted
ones — sorted by timestamp with the JetStream stream sequence as the tiebreak, so
arrival order at the store does not matter.

## Configuration

Construct the backend with the details for reaching the NATS servers — the URL (or
URLs), any credentials, and the name of the JetStream bucket to store state in.
Because NATS is shared, the same details can be handed to workers in other processes
or on other machines.

## Durability

State is durable when **JetStream is configured to store data on disk** (its normal
durable configuration); in that setup runs survive a restart. As with the other
server-based backends, the durability guarantee comes from how the server is
configured, not from this backend.

## Concurrency

Different runs use different keys, so many runs and workers can share the same NATS
system without interfering. Parallel tasks within a run each write their own keys,
and the order is sorted out from the timestamps at read time.

## Reference

The backend's API is in the [NATS reference](../reference/stores-nats.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
