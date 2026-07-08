---
name: RabbitMQMessageBroker
description: RabbitMQ message-broker backend — quorum queues behind a topic exchange for jobs, RabbitMQ Streams for instance events, heartbeat-broadcast for the registry. Its own module under brokers/rabbitmq.
targets:
  - ../../brokers/rabbitmq/broker.go
---

# RabbitMQ Message Broker

> **Status:** Implemented.

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

1. **Queue + ack + redelivery** — one **quorum queue per `ProcessKey`**
   (`<prefix>.jobs.<enc>`; each key segment hex-encoded — `/` and `.` are
   AMQP-significant — with a truncated-SHA-256 fallback for long segments),
   fed by a topic exchange (`<prefix>.jobs`). Queues are declared
   idempotently on both `FetchJobs` *and* before every job publish, so a
   job can never be lost for want of a binding. Workers consume with manual
   ack and prefetch 16. Terminal lifecycle reports and `ReportSuspended`
   issue `basic.ack` for every delivery held for the instance. `FetchJobs`
   owns a dedicated channel closed on ctx-cancel *without* acking, so a
   worker crash (or fetch shutdown) makes RabbitMQ **requeue every unacked
   message immediately** — redelivery is channel-close-driven, not
   timer-driven (`Config.InFlightTimeout` is accepted but unused as a
   timer). Undecodable payloads (foreign cipher key) are nacked without
   requeue and surfaced as an Error event with code `DECRYPT_FAILED`.
2. **Selective consumption** — workers consume only the queues for their
   registered keys.
3. **Registry — heartbeat broadcast** (no KV): `RegisterProcesses`,
   `Heartbeat`, and `Unregister` publish the worker's full envelope-encoded
   registration set (or an explicit removal) on a fanout exchange
   (`<prefix>.registry`). Every broker handle binds a private exclusive
   auto-delete queue at construction and maintains a local registry cache;
   updates carry an origin-handle header so a handle skips its own echoes
   (its verbs apply to the local cache synchronously). The handle also
   **re-broadcasts registrations it originated every `RegistrationTTL/4`**
   while the worker's verb-based deadline has not lapsed, so registrations
   survive between `Heartbeat` calls; subscribers mark a worker
   `RegistryUpdateHeartbeatLost` client-side when no broadcast arrives
   within the TTL (at worst ~2× TTL after the last verb). `Submit` blocks
   only while the cache is completely empty (cold start); once any
   registration is visible an unknown key fails fast with
   `ErrUnknownProcess`.
4. **Per-instance events / fan-out / replay** — **one shared stream queue**
   (`<prefix>.inst-events`, `x-queue-type: stream`, `x-max-age` =
   `EventRetention`, default 1h) for *all* instances, with the instance id
   (and routing key) in cleartext headers, filtered client-side. On
   subscribe, the backend publishes a unique **marker** message, consumes
   from `x-stream-offset: first`, collapses everything before its marker
   into the latest lifecycle + terminal events (or `INSTANCE_NOT_FOUND`),
   then follows live; each subscriber reads at its own offset (broadcast).
   *Trade-off*: plain topic exchanges with per-subscriber auto-delete
   queues would be simpler but retain nothing — a late subscriber would see
   no events at all — so a stream is required. Instance→key/correlation
   resolution uses a handle-local map fed from Submit, delivered jobs, and
   observed events, with a marker-bounded stream replay as fallback.
5. **Delayed delivery** — the **per-message-TTL + dead-letter-exchange**
   pattern (no plugin dependency): the `JobResume` is published with an
   expiration to a holding queue (`<prefix>.delay.q`) whose dead-letter
   exchange is the jobs exchange, preserving the original routing key.
   Head-of-line caveat: per-message TTL only dead-letters from the queue
   head, so a long delay ahead postpones shorter ones behind it. The
   delayed-message-exchange plugin is a documented alternative for
   deployments that already run it.
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
       URL            string            // e.g. "amqp://user:pass@localhost:5672/"; rewritten to amqps:// when TLS is set
       TLS            *tls.Config       // nil = plaintext
       ExchangePrefix string            // default "blkit"; isolates deployments sharing a server
       Cipher         bl.PayloadCipher  // optional end-to-end payload encryption; default nil

       RegistrationTTL time.Duration // default 90s; heartbeat-broadcast loss window
       InFlightTimeout time.Duration // accepted for interface symmetry; redelivery is channel-close-driven
       EventRetention  time.Duration // default 1h; stream x-max-age
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
