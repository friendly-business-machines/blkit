# AWS SQS/SNS

> A cloud-managed broker on AWS — SQS queues per process key for jobs, an SNS topic
> with filter policies for instance events, and a DynamoDB RegistryStore for the
> registry, timers, and last-event records.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The AWS backend implements [MessageBroker](overview.md) against **SQS** (the job
queue) and **SNS** (instance-event fan-out). SQS's visibility timeout gives
natural in-flight semantics and `DelaySeconds` covers short timers; SNS filter
policies route instance events to per-subscriber queues. Neither service retains
messages for late subscribers or offers a KV store, so the registry, long timers,
and last-event records live in a pluggable **RegistryStore** backed by DynamoDB.

```go
import awsbroker "github.com/friendly-business-machines/blkit/brokers/aws-sqs-sns"

var broker, err = awsbroker.New(awsbroker.Config{
    Region:   "eu-west-2",
    Registry: dynamoRegistryStore,
})
```

## What it's good for

- **Deployments already on AWS** that want a fully managed broker.
- Teams comfortable pairing SQS/SNS with **DynamoDB** for the registry side-store.

## How it works

- **Job queue** — one **SQS queue per process key**; delivery starts the
  **visibility timeout** (the in-flight slot), extended for long jobs. Terminal
  reports issue `DeleteMessage`; a crashed worker's message becomes visible again
  and is redelivered.
- **Selective consumption** — workers long-poll only the queues for their
  registered keys.
- **Registry — RegistryStore** — **DynamoDB**: one item per worker with a **TTL
  attribute** for heartbeat expiry, and **DynamoDB Streams** as the change feed.
- **Per-instance events** — one **SNS topic**; each subscriber gets an SQS queue
  subscribed with a **filter policy** on the instance id. SNS retains nothing, so
  every lifecycle publish also upserts a **last-event record** in DynamoDB (TTL
  default 24h — the retention window) that seeds a late subscriber before it
  follows live.
- **Timers** — `DelaySeconds` natively (capped at 15 minutes); longer suspends
  write a **timer record** to the RegistryStore that a scheduler loop claims when
  due.
- **Cancel of queued jobs** — **unsupported natively** (deletion needs a receipt
  handle), so cancel always takes the message route.

## Configuration

```go
type Config struct {
    Region      string                  // e.g. "eu-west-2"
    Credentials aws.CredentialsProvider // nil = default AWS credential chain
    QueuePrefix string                  // default "blkit"; isolates deployments sharing an account
    Registry    bl.RegistryStore        // required; DynamoDB implementation ships with this module
    Endpoint    string                  // development only; LocalStack endpoint override
    Cipher      bl.PayloadCipher        // optional end-to-end payload encryption; default nil
}
```

Each jobs queue has a redrive policy to a **dead-letter queue** after a
configurable receive count — the fallback for repeated worker crashes.

## Local testing

The conformance suite starts a **LocalStack** container (SQS + SNS + DynamoDB in
one image) via testcontainers-go. `BLKIT_TEST_AWS_ENDPOINT` (with ambient
credentials) points it at a real account or a LocalStack you run yourself.
LocalStack gaps are tagged and run against a real account in CI when credentials
are present.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
