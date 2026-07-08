---
name: NATSMessageBroker
description: NATS + JetStream message-broker backend — subject-filtered pull consumers for the job queue, per-subject streams for instance events, KV with watch for the registry. Its own module under brokers/nats.
targets:
  - ../../brokers/nats/broker.go
---

# NATS Message Broker

> **Status:** Implemented.

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
   `<prefix>.jobs.<ns>.<proc>.<ver>.<instanceID>` (key parts are
   subject-encoded — namespaces contain `/` and `.`). The stream uses
   **Limits retention with explicit `DeleteMsg` on settle** — not
   WorkQueue (which forbids overlapping consumer filters) and not Interest
   (which drops jobs published before any worker fetches). Workers use
   **durable pull consumers** (durable name derived from the sorted filter
   set, so same-key fetchers compete on one consumer) with
   `AckWait = InFlightTimeout`. Terminal lifecycle reports and
   `ReportSuspended` `Ack` **and delete** the message; a worker that dies
   stops extending its `AckWait` and JetStream redelivers automatically.
2. **Selective consumption** — pull consumers with **subject filters** on
   exactly the worker's registered keys. Native, no client-side filtering.
3. **Registry** — a NATS **KV bucket**, one envelope-encoded entry per
   worker carrying the stamped registrations, a deadline, and a generation
   counter (so heartbeat refreshes don't spam watchers). Expiry is
   **sweeper-driven, not KV TTL** — age-based expiry produces no reliable
   watch events — and the sweeper tombstones with the old registrations and
   a reason before deleting, so watchers can distinguish `Removed`
   (Unregister) from `HeartbeatLost` (sweeper). `KV.Watch()` delivers
   snapshot-then-updates onto `SubscribeToProcessRegistry`. `Submit` reads
   the bucket directly (immediately consistent), so there is no cold-start
   snapshot wait.
4. **Per-instance events / fan-out / replay** — instance events publish to
   per-kind subjects `<prefix>.inst.<id>.lifecycle` / `.terminal` /
   `.inputreq` / `.node` / `.err` in an events stream. Late subscribers
   replay via `GetLastMsgForSubject` (latest lifecycle + terminal only),
   then follow live from the next stream sequence with an ordered consumer
   — no gap, no duplicate. Stream `MaxAge` (default 1h, the
   `EventRetention` knob) is the retention window. Each subscriber gets its
   own consumer (broadcast). A small `<prefix>-instmeta` KV bucket holds
   per-instance routing key + correlation key + finish time, for
   `RespondToInputRequest` routing, correlation-key mirroring, and
   retention sweeping.
5. **Delayed delivery** — no native delayed publish. `ReportSuspended` with
   a `resumeAt` currently schedules the `JobResume` with an **in-process
   timer**; a broker restart before the fire-time loses the pending resume.
   Durable timers (the NakWithDelay timers-subject pattern) are pending.
6. **Cancel of queued jobs** — best-effort: the instance id is the job
   subject's last token, so look up the `JobStart` with
   `GetLastMsgForSubject`, kind-check it, verify it is undelivered (local
   in-flight map + consumer delivered floors), and `DeleteMsg` by sequence;
   the broker then publishes the terminal Cancelled event itself.
   Otherwise fall through to the `JobCancel` route.
7. **TLS** — `Config.TLS *tls.Config`; nil means plaintext (development).
   NATS credentials files and NKeys are supported via `Credentials`.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       URL           string            // e.g. "nats://localhost:4222"
       Credentials   string            // optional path to a .creds file
       TLS           *tls.Config       // nil = plaintext
       SubjectPrefix string            // default "blkit"; isolates deployments sharing a server
       Cipher        bl.PayloadCipher  // optional end-to-end payload encryption; default nil

       RegistrationTTL time.Duration   // default 90s (3× heartbeat interval)
       InFlightTimeout time.Duration   // default 150s; maps to consumer AckWait
       EventRetention  time.Duration   // default 1h; maps to events-stream MaxAge
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
