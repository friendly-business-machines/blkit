# NATS

> A self-hosted broker backed by NATS with JetStream — subject-filtered pull
> consumers for the job queue, per-subject streams for events, KV with watch for
> the registry. A natural fit when NATS is already your state store.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The NATS backend implements [MessageBroker](overview.md) against NATS with
**JetStream** (plain core NATS lacks the durability the job queue needs).
JetStream's subject hierarchy gives the cleanest selective consumption of any
backend, and NATS KV maps directly onto the worker registry. It is a natural fit
when NATS is already your [state store](../state-stores/nats.md) — one
infrastructure piece serves both.

```go
import natsbroker "github.com/friendly-business-machines/blkit/brokers/nats"

var broker, err = natsbroker.New(natsbroker.Config{URL: "nats://localhost:4222"})
```

## What it's good for

- **Setups that already run NATS** — as the state store, the broker, or both.
- Deployments that want **native, server-side selective consumption** without any
  client-side filtering.

## How it works

- **Job queue** — one JetStream **jobs stream**; workers use **durable pull
  consumers**. Terminal reports `Ack` the message, and a worker that dies stops
  extending its `AckWait` so JetStream redelivers automatically.
- **Selective consumption** — pull consumers with **subject filters** on exactly
  the worker's registered keys. Native, no client-side filtering.
- **Registry** — a NATS **KV bucket** with per-key TTL, refreshed by heartbeats.
  `KV.Watch()` delivers snapshot-then-updates natively, and TTL-expired keys
  surface as heartbeat loss.
- **Per-instance events** — instance events publish to per-instance subjects in an
  events stream; subscribers use ordered consumers with `DeliverLastPerSubject` to
  recover the latest lifecycle and terminal events before following live. Stream
  `MaxAge` (default 24h) is the retention window.
- **Timers** — no native delayed publish, so a broker-owned scheduler consumer
  uses `NakWithDelay` until the fire-time, then publishes the resume.
- **Cancel of queued jobs** — best-effort: look up the job by subject and delete
  it by sequence if it has not been delivered.

## Configuration

```go
type Config struct {
    URL           string            // e.g. "nats://localhost:4222"
    Credentials   string            // optional path to a .creds file
    TLS           *tls.Config       // nil = plaintext (development)
    SubjectPrefix string            // default "blkit"; isolates deployments sharing a server
    Cipher        bl.PayloadCipher  // optional end-to-end payload encryption; default nil
}
```

## Local testing

The conformance suite runs against a real JetStream server **embedded in the test
process** (`nats-server` is importable as a Go library) — no container needed,
same as the NATS state store. `BLKIT_TEST_NATS_URL` points it at an external
server instead.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
