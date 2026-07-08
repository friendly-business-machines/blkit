---
name: AWSSQSSNSMessageBroker
description: AWS SQS/SNS message-broker backend — SQS queues per process key for jobs, an SNS topic with filter policies fanning out to per-subscriber queues for instance events, and a RegistryStore (DynamoDB) for the registry, timers, and last-event records. Its own module under brokers/aws-sqs-sns.
targets:
  - ../../brokers/aws-sqs-sns/broker.go
---

# AWS SQS/SNS Message Broker

> **Status:** Implemented.

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

1. **Queue + ack + redelivery** — one standard **SQS queue per
   `ProcessKey`** (`<prefix>-jobs-<slug>-<sha256/8>`, created idempotently;
   key parts are slugged + hashed since queue names are limited to 80
   safe characters). Delivery starts the **visibility timeout** (=
   `InFlightTimeout` rounded up to whole seconds — the in-flight slot); a
   lease goroutine calls `ChangeMessageVisibility` every half-timeout.
   Terminal lifecycle reports and `ReportSuspended` issue `DeleteMessage`.
   A crashed worker's message becomes visible again and is redelivered. A
   DLQ redrive policy and an opt-in FIFO jobs-queue variant
   (`MessageGroupId = instanceID` per-instance ordering) are documented
   but not yet implemented.
2. **Selective consumption** — workers long-poll only the queues for their
   registered keys.
3. **Registry — RegistryStore** — `DynamoRegistryStore` implements the core
   `RegistryStore` interface on one DynamoDB table (`worker#` / `timer#` /
   `inst#` key prefixes): one item per worker with envelope-encoded
   registrations and a **deadline attribute**; `Touch`/`Delete` use
   conditional writes so absent or expired workers yield `ErrUnknownWorker`.
   Expiry is **client-side** (deadline + sweeper — DynamoDB's native TTL
   deletion lags by minutes to hours and is set only as belt-and-braces
   cleanup), and `Watch` is a **~200ms polling diff loop** rather than
   DynamoDB Streams (disappeared-with-lapsed-deadline → `HeartbeatLost`;
   deleted-before-deadline → `Removed`). The store takes its own `Cipher`
   config (matching the broker's) since registrations are envelope-encoded
   at the store. `Submit` resolves from a direct consistent-read
   `Snapshot()` per call, so there is no cold-start snapshot wait.
4. **Per-instance events / fan-out / replay** — one **FIFO SNS topic**
   (`<prefix>-inst-events.fifo`); each subscriber gets an auto-created
   **FIFO** SQS queue subscribed with a **filter policy** on the
   `instanceID` message attribute (raw delivery), deleted on unsubscribe.
   FIFO (`MessageGroupId = instanceID`,
   `MessageDeduplicationId = instanceID-seq`) is required because standard
   SNS→SQS delivery is unordered and could let the terminal `Result`
   overtake the `Completed` lifecycle event. Events carry `seq`/`final`
   attributes (seq from an atomic DynamoDB counter) for replay dedupe and
   close-once. **SNS retains nothing** — a late subscriber would see no
   prior events — so every lifecycle publish also upserts a **last-event
   record** (process key, correlation key, latest lifecycle + terminal
   envelopes, finished flag) via the module's exported `InstanceEventStore`
   interface, which `DynamoRegistryStore` implements; on subscribe, the
   backend delivers the record's events first, then follows the queue live,
   deduping by seq. Records expire after `EventRetention` (default 1h).
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
       Registry    bl.RegistryStore   // required; must also implement this module's InstanceEventStore (DynamoRegistryStore does)
       Endpoint    string             // development only; LocalStack endpoint override
       Cipher      bl.PayloadCipher   // optional end-to-end payload encryption; default nil

       RegistrationTTL time.Duration // default 90s
       InFlightTimeout time.Duration // default 150s; SQS visibility timeout, whole seconds
       EventRetention  time.Duration // default 1h; last-event record lifetime
   }
   ```

9. **Local testing** — the conformance suite starts a **LocalStack**
   container (SQS + SNS + DynamoDB in one image, free tier) via
   testcontainers-go. `BLKIT_TEST_AWS_ENDPOINT` (with ambient credentials)
   points it at a real account or a LocalStack you run yourself. LocalStack
   gaps are tagged; those conformance areas run against a real account in CI
   when credentials are present.

## Notes

- **Dead-lettering**: a redrive policy to a DLQ after `maxReceiveCount`
  (default 10) is the intended fallback for repeated worker crashes;
  `ReportFailed` is the normal failure path. Not yet implemented.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format)
  (base64-encoded where the transport requires text bodies); SNS/SQS message
  attributes carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
