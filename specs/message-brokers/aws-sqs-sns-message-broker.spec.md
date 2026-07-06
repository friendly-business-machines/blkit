---
name: AWSSQSSNSMessageBroker
description: AWS SQS/SNS message-broker backend — SQS queues per process key for jobs, an SNS topic with filter policies fanning out to per-subscriber queues for instance events, and a RegistryStore (DynamoDB) for the registry, timers, and last-event records. Its own module under brokers/aws-sqs-sns.
targets:
  - ../../brokers/aws-sqs-sns/broker.go
---

# AWS SQS/SNS Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The AWS backend implements [MessageBroker](overview.spec.md) against **SQS**
(the job queue) and **SNS** (instance-event fan-out). SQS's visibility
timeout gives natural in-flight semantics and `DelaySeconds` covers short
timers natively; SNS filter policies route instance events to per-subscriber
queues. Neither service retains messages for late subscribers or offers a KV
store, so the registry, long timers, and last-event records live in a
pluggable [RegistryStore](overview.spec.md#the-registrystore) backed by
DynamoDB.

```go
import awsbroker "github.com/friendly-business-machines/blkit/brokers/aws-sqs-sns"

broker, err := awsbroker.New(awsbroker.Config{
    Region:   "eu-west-2",
    Registry: dynamoRegistryStore,
})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one **SQS queue per `ProcessKey`**
   (`<prefix>-jobs-<ns>-<proc>-<ver>`). Delivery starts the **visibility
   timeout** (the in-flight slot); a lease goroutine calls
   `ChangeMessageVisibility` for long jobs. Terminal lifecycle reports and
   `ReportSuspended` issue `DeleteMessage`. A crashed worker's message
   becomes visible again and is redelivered; a redrive policy moves
   repeatedly-failed messages to a DLQ (see notes). FIFO queues with
   `MessageGroupId = instanceID` are the opt-in per-instance-ordering
   variant.
2. **Selective consumption** — workers long-poll only the queues for their
   registered keys.
3. **Registry — RegistryStore** — **DynamoDB** implements the core
   `RegistryStore` interface: one item per worker with a **TTL attribute**
   for heartbeat expiry, and **DynamoDB Streams** as the `Watch` change
   feed (TTL deletions surface there as `RegistryUpdateHeartbeatLost`).
4. **Per-instance events / fan-out / replay** — one **SNS topic**
   (`<prefix>-inst-events`); each subscriber gets an auto-created SQS queue
   subscribed with a **filter policy** on the `instanceID` message
   attribute, deleted on unsubscribe. **SNS retains nothing** — a late
   subscriber would see no prior events — so every lifecycle publish also
   upserts a **last-event record** in the RegistryStore; on subscribe, the
   backend delivers the latest lifecycle / terminal event from that record
   first, then follows the queue live. Last-event records expire via
   DynamoDB TTL (default 24h — the retention window).
5. **Delayed delivery** — SQS `DelaySeconds` natively, but capped at **15
   minutes**; longer suspends write a **timer record to the RegistryStore**
   and a broker-owned scheduler loop claims due timers atomically and sends
   the `JobResume`.
6. **Cancel of queued jobs** — **unsupported natively** (`DeleteMessage`
   requires a receipt handle, which only a receiver holds). `Cancel` always
   takes the `JobCancel` route.
7. **TLS** — always on through the AWS SDK (HTTPS). LocalStack is reached
   via an endpoint override — development only.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       Region      string             // e.g. "eu-west-2"
       Credentials aws.CredentialsProvider // nil = default AWS credential chain
       QueuePrefix string             // default "blkit"; isolates deployments sharing an account
       Registry    bl.RegistryStore   // required; DynamoDB implementation ships with this module
       Endpoint    string             // development only; LocalStack endpoint override
       Cipher      bl.PayloadCipher   // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite starts a **LocalStack**
   container (SQS + SNS + DynamoDB in one image, free tier) via
   testcontainers-go. `BLKIT_TEST_AWS_ENDPOINT` (with ambient credentials)
   points it at a real account or a LocalStack you run yourself. LocalStack
   gaps are tagged; those conformance areas run against a real account in CI
   when credentials are present.

## Notes

- **Dead-lettering**: each jobs queue has a redrive policy to a DLQ after
  `maxReceiveCount` (default 10) — the fallback for repeated worker crashes.
  `ReportFailed` is the normal failure path.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format)
  (base64-encoded where the transport requires text bodies); SNS/SQS message
  attributes carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
