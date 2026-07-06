# Overview

> How blkit's clients and workers talk to each other — pick the backend that fits
> your deployment, from a zero-dependency in-memory broker to Redis, NATS,
> RabbitMQ, or a managed cloud service.

!!! warning "Status — subsystem in progress"
    The message-broker subsystem is still being built; no external backend is
    implemented yet. These pages describe the intended design so the section's
    shape is visible and stable. The authoritative requirements live in the
    broker specs under `specs/message-brokers/`.

blkit **clients** (MCP servers, web servers, CLI tools, admin UIs) and **workers**
(the processes that execute [process](../processes/processes.md) definitions) may
live in different binaries on different machines. The **message broker** is the
only channel between them.

blkit ships one built-in broker and a family of pluggable backends. They are
**interchangeable**: your application code does not change when you swap one for
another, because every backend implements the same `MessageBroker` interface and
is held to the same behaviour by a [shared conformance suite](#conformance).

## What every broker does

The broker exists **only** for client↔worker communication. It has six duties:

- **Worker registration** — where workers advertise which processes and versions
  they can execute, kept live by heartbeats and expired by TTL.
- **Process queue** — the job queue that carries work to workers.
- **Start requests** — where clients request the start of a process and supply
  its start input values.
- **Cancel-type requests** — where clients send cancel and terminate requests.
- **Input requests and responses** — where clients receive a running process's
  requests for further input, and respond to them.
- **Process outcomes** — where clients receive process outcomes.

## What the broker is not

**The broker holds no process state.** The execution history and the
current/historical state of every run live in the
[state store](../state-stores/overview.md) — never in the broker. The broker
carries *messages about* a run; it does not keep a record *of* the run. Workers
own state-store access exclusively; state queries (admin UIs, audit) go through
the state store directly.

## Choosing a backend

You pick a backend by constructing it and handing it to blkit. The built-in
in-memory broker needs nothing extra:

```go
var broker = bl.NewInMemoryMessageBroker()
```

Every other backend lives in its own module, so you only pull in its client if you
use it. Import it and construct it — the rest of your code is identical:

```go
import (
    bl          "github.com/friendly-business-machines/blkit/core"
    redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"
)

var broker, err = redisbroker.New(redisbroker.Config{Addr: "localhost:6379"})
```

Either way, `broker` is a `bl.MessageBroker` the rest of blkit uses the same way.

## The backends

The backends fall into three families. Which fits depends on whether you run a
single binary, host your own server, or lean on a managed cloud service.

**Built into core — not durable, single process:**

| Backend | Use it for |
|---|---|
| [In-memory](in-memory.md) | Tests, examples, and single-binary deployments. Zero dependencies; nothing survives a restart. |

**Self-hosted — a server you run:**

| Backend | Use it for |
|---|---|
| [Redis](redis.md) | The lightweight self-host default: Redis or Valkey covers every duty natively, including true removal of queued jobs. |
| [NATS](nats.md) | Cleanest selective consumption via JetStream subjects; a natural fit when NATS is already your state store. |
| [RabbitMQ](rabbitmq.md) | Classic work-queue semantics with a large enterprise install base. |

**Cloud-managed — a side-store holds the registry, locally testable via emulators:**

| Backend | Use it for |
|---|---|
| [Azure Service Bus](azure-service-bus.md) | Peek-lock delivery and native scheduled messages — the best suspend-resume timer story. |
| [Google Pub/Sub](google-pubsub.md) | Filtered subscriptions and seekable retention on Google Cloud. |
| [AWS SQS/SNS](aws-sqs-sns.md) | SQS queues for jobs and SNS filtered fan-out for events on AWS. |

The cloud-managed backends have no built-in key-value store, so the worker
registry, timers, and last-event records live in a pluggable **RegistryStore**
(DynamoDB, Firestore, or Azure Table Storage / Cosmos DB) that ships with each
module.

## Conformance

Every backend is verified against a **shared conformance suite** that lives in
core, so they all behave identically — a job queued through any of them is
delivered, acknowledged, redelivered, and fanned out the same way. The suite
covers the full behaviour: durable at-least-once delivery with redelivery when a
worker dies, selective consumption per registered key, per-instance event fan-out
with latest-event replay for late subscribers, suspend-resume timers, and
best-effort cancel of still-queued jobs.

How it runs depends on the backend: the in-memory broker runs it in-process with
no setup; NATS runs it against a JetStream server embedded in the test; and the
server-based and cloud backends run it against a real server or emulator started
with testcontainers-go, skipping when neither a container nor an external endpoint
is available.

## Reference

The `MessageBroker` interface and the built-in in-memory broker are part of the
core API [Reference](../reference/blkit.md).
