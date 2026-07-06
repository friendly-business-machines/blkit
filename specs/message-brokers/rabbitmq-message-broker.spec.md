---
name: RabbitMQMessageBroker
description: RabbitMQ message-broker backend — quorum queues behind a topic exchange for jobs, RabbitMQ Streams for instance events, heartbeat-broadcast for the registry. Its own module under brokers/rabbitmq.
targets:
  - ../../brokers/rabbitmq/broker.go
---

# RabbitMQ Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The RabbitMQ backend implements [MessageBroker](overview.spec.md) against
RabbitMQ 3.13+. RabbitMQ's work-queue semantics (manual acks, automatic
requeue of unacked messages, per-consumer prefetch) fit the job queue
naturally; the registry and delayed delivery use documented patterns because
RabbitMQ has no KV store or native message scheduling. Chosen for its large
enterprise install base.

```go
import rabbitbroker "github.com/friendly-business-machines/blkit/brokers/rabbitmq"

broker, err := rabbitbroker.New(rabbitbroker.Config{URL: "amqp://localhost:5672"})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one **quorum queue per `ProcessKey`**, fed
   by a topic exchange (`<prefix>.jobs`) with routing keys
   `<ns>.<proc>.<ver>`. Workers consume with manual ack and a bounded
   prefetch. Terminal lifecycle reports and `ReportSuspended` issue
   `basic.ack`. A worker crash closes its channel and RabbitMQ **requeues
   every unacked message immediately** — the strongest redelivery story of
   any backend (no timeout wait).
2. **Selective consumption** — workers consume only the queues for their
   registered keys; queues are declared idempotently on registration.
3. **Registry — heartbeat broadcast** (no KV): each `Heartbeat` (and
   `RegisterProcesses`) publishes the worker's full envelope-encoded
   registration set on a fanout exchange (`<prefix>.registry`).
   `SubscribeToProcessRegistry` binds a private queue to it, assembles the
   snapshot over one heartbeat window (emitting the snapshot sentinel when
   the window closes), and marks a worker `RegistryUpdateHeartbeatLost`
   client-side after ~3 missed intervals. `Unregister` broadcasts an
   explicit removal. The broker's internal registry cache (used by `Submit`)
   is fed the same way — which is why Submit's cold-start block (one
   heartbeat window at worst) matters here.
4. **Per-instance events / fan-out / replay** — instance events go to a
   **RabbitMQ Stream** (streams are core since 3.9; retention + offset
   replay), partitioned by instance id in the message. On subscribe, the
   backend replays from the retention window to recover the latest lifecycle
   and terminal events, then follows live; each subscriber reads at its own
   offset (broadcast). Stream retention (default 24h) is the window.
   *Trade-off*: plain topic exchanges with per-subscriber auto-delete queues
   would be simpler but retain nothing — a late subscriber would see no
   events at all — so streams are required.
5. **Delayed delivery** — the **per-message-TTL + dead-letter-exchange**
   pattern (no plugin dependency): the `JobResume` is published with an
   expiration to a holding queue whose dead-letter exchange is the jobs
   exchange; when the TTL fires the message dead-letters into the instance's
   job queue. The delayed-message-exchange plugin is a documented
   alternative for deployments that already run it.
6. **Cancel of queued jobs** — **unsupported natively**: AMQP has no
   selective removal from a queue. `Cancel` always takes the `JobCancel`
   route (the opt-in check therefore applies to every cancel of an
   undelivered job too — the overview's step-2 removal is skipped, as its
   spec allows).
7. **TLS** — `Config.TLS *tls.Config`; nil means plaintext (development).
   With TLS set, the connection uses `amqps`.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       URL            string            // e.g. "amqp://user:pass@localhost:5672/"
       TLS            *tls.Config       // nil = plaintext
       ExchangePrefix string            // default "blkit"; isolates deployments sharing a server
       Cipher         bl.PayloadCipher  // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite starts a throwaway RabbitMQ
   container with testcontainers-go (the management image, which includes
   streams). `BLKIT_TEST_RABBITMQ_URL` points it at an already-running
   instance instead; the test skips only when neither is available.

## Notes

- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; routing keys and AMQP headers carry the cleartext routing
  metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
