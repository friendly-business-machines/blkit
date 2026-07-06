---
name: NATSMessageBroker
description: NATS + JetStream message-broker backend — subject-filtered pull consumers for the job queue, per-subject streams for instance events, KV with watch for the registry. Its own module under brokers/nats.
targets:
  - ../../brokers/nats/broker.go
---

# NATS Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The NATS backend implements [MessageBroker](overview.spec.md) against NATS
with **JetStream** (plain core NATS lacks the durability the job queue
needs). JetStream's subject hierarchy gives the cleanest selective
consumption of any backend, and NATS KV maps directly onto the worker
registry. It is a natural fit when NATS is already your
[state store](../state-stores/nats-state-store.spec.md) — one infrastructure
piece serves both.

```go
import natsbroker "github.com/friendly-business-machines/blkit/brokers/nats"

broker, err := natsbroker.New(natsbroker.Config{URL: "nats://localhost:4222"})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one JetStream **jobs stream** capturing
   `<prefix>.jobs.>`, published to subjects
   `<prefix>.jobs.<ns>.<proc>.<ver>`. Workers use **durable pull
   consumers**. Terminal lifecycle reports and `ReportSuspended` `Ack` the
   message; a worker that dies stops extending its `AckWait` and JetStream
   redelivers automatically.
2. **Selective consumption** — pull consumers with **subject filters** on
   exactly the worker's registered keys. Native, no client-side filtering.
3. **Registry** — a NATS **KV bucket** with per-key TTL, one entry per
   worker, refreshed by `Heartbeat`. `KV.Watch()` natively delivers
   snapshot-then-updates — a direct mapping onto
   `SubscribeToProcessRegistry`; TTL-expired keys surface as
   `RegistryUpdateHeartbeatLost`.
4. **Per-instance events / fan-out / replay** — instance events publish to
   `<prefix>.inst.<id>.<eventKind>` in an events stream. Subscribers use
   ordered consumers with `DeliverLastPerSubject` to recover the latest
   lifecycle event (and terminal event) before following live — the
   latest-event replay requirement, natively. Stream `MaxAge` (default 24h)
   is the retention window. Each subscriber gets its own consumer
   (broadcast).
5. **Delayed delivery** — no native delayed publish. `ReportSuspended` for a
   duration/datetime wait `Ack`s the job and records the wake-up as a
   message on a **timers subject** consumed by a broker-owned scheduler
   consumer that uses `NakWithDelay` until the fire-time is reached, then
   publishes the `JobResume` to the instance's job subject.
6. **Cancel of queued jobs** — best-effort: look up the instance's
   `JobStart` by subject (`GetLastMsg`), and `DeleteMsg` by sequence if it
   has not been delivered. Otherwise fall through to the `JobCancel` route.
7. **TLS** — `Config.TLS *tls.Config`; nil means plaintext (development).
   NATS credentials files and NKeys are supported via `Credentials`.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       URL           string            // e.g. "nats://localhost:4222"
       Credentials   string            // optional path to a .creds file
       TLS           *tls.Config      // nil = plaintext
       SubjectPrefix string            // default "blkit"; isolates deployments sharing a server
       Cipher        bl.PayloadCipher  // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite runs against a real JetStream
   server **embedded in the test process** (`nats-server` is importable as a
   Go library) — no container needed, same as the NATS state store.
   `BLKIT_TEST_NATS_URL` points it at an external server instead.

## Notes

- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; routing data lives in the subject and NATS headers,
  cleartext.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
