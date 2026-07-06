# RabbitMQ

> A self-hosted broker backed by RabbitMQ 3.13+ — quorum queues behind a topic
> exchange for jobs, RabbitMQ Streams for instance events, heartbeat broadcast for
> the registry.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The RabbitMQ backend implements [MessageBroker](overview.md) against RabbitMQ
3.13+. RabbitMQ's work-queue semantics — manual acks, automatic requeue of unacked
messages, per-consumer prefetch — fit the job queue naturally; the registry and
delayed delivery use documented patterns because RabbitMQ has no KV store or native
message scheduling. Chosen for its large enterprise install base.

```go
import rabbitbroker "github.com/friendly-business-machines/blkit/brokers/rabbitmq"

var broker, err = rabbitbroker.New(rabbitbroker.Config{URL: "amqp://localhost:5672"})
```

## What it's good for

- **Enterprises already standardised on RabbitMQ.**
- Deployments that value its **strong redelivery story** — a worker crash requeues
  every unacked message immediately, with no timeout to wait out.

## How it works

- **Job queue** — one **quorum queue per process key**, fed by a topic exchange.
  Workers consume with manual ack and bounded prefetch; a worker crash closes its
  channel and RabbitMQ **requeues every unacked message immediately**.
- **Selective consumption** — workers consume only the queues for their registered
  keys, declared idempotently on registration.
- **Registry — heartbeat broadcast** (no KV): each heartbeat publishes the
  worker's full registration set on a fanout exchange. Subscribers assemble the
  snapshot over one heartbeat window and mark a worker lost after ~3 missed
  intervals.
- **Per-instance events** — a **RabbitMQ Stream** (retention + offset replay),
  partitioned by instance id, so a late subscriber replays the latest lifecycle
  and terminal events before following live. Retention (default 24h) is the
  window.
- **Timers** — the **per-message-TTL + dead-letter-exchange** pattern (no plugin
  required): the resume is published to a holding queue whose dead-letter exchange
  is the jobs exchange.
- **Cancel of queued jobs** — **unsupported natively** (AMQP has no selective
  removal), so cancel always takes the message route.

## Configuration

```go
type Config struct {
    URL            string            // e.g. "amqp://user:pass@localhost:5672/"
    TLS            *tls.Config       // nil = plaintext (development); amqps when set
    ExchangePrefix string            // default "blkit"; isolates deployments sharing a server
    Cipher         bl.PayloadCipher  // optional end-to-end payload encryption; default nil
}
```

## Local testing

The conformance suite starts a throwaway RabbitMQ container (the management image,
which includes streams) with testcontainers-go. `BLKIT_TEST_RABBITMQ_URL` points
it at an already-running instance instead; the test skips only when neither is
available.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
