# AWS SQS/SNS

> A cloud-managed broker on AWS — SQS queues per process key for jobs, an SNS
> topic with filter policies for instance events, and a DynamoDB RegistryStore
> for the registry, timers, and last-event records.

The AWS backend implements [MessageBroker](overview.md) against **SQS** (the job
queue) and **SNS** (instance-event fan-out). SQS's visibility timeout gives
natural in-flight semantics and `DelaySeconds` covers short timers natively; SNS
filter policies route instance events to per-subscriber queues. It is a good fit
for a deployment already on AWS that wants a fully managed broker.

Neither SQS nor SNS retains messages for a late subscriber or offers a key-value
store, so a third service fills the gaps: the worker registry, long timers, and
the last-event records that let a late subscriber catch up all live in a
**RegistryStore** backed by **DynamoDB**. The RegistryStore is a required
side-store you construct and pass in — the broker never opens it for you, so it
appears explicitly in every construction example.

```go
import (
    "log"

    awsbroker "github.com/friendly-business-machines/blkit/brokers/aws-sqs-sns"
)

// The DynamoDB RegistryStore holds everything SQS and SNS cannot: the worker
// registry, timers past SQS's 15-minute delay ceiling, and the last-event
// records that seed late subscribers. Create it first, then hand it to the
// broker. NewDynamoRegistryStore creates its table if absent and starts the
// expiry sweeper.
registry, err := awsbroker.NewDynamoRegistryStore(awsbroker.DynamoRegistryStoreConfig{
    Region: "eu-west-2",
})
if err != nil {
    log.Fatalf("connect dynamodb registry: %v", err)
}
defer registry.Close()

// New dials SQS and SNS, creates the instance-event topic, and starts the
// timer scheduler; the returned *Broker is a bl.MessageBroker the rest of
// blkit uses the same way as any other backend. The broker does not close the
// registry — the caller owns it.
broker, err := awsbroker.New(awsbroker.Config{
    Region:   "eu-west-2",
    Registry: registry,
})
if err != nil {
    log.Fatalf("connect aws broker: %v", err)
}
defer broker.Close()
```

With `Credentials` left nil both clients use the default AWS credential chain
(environment, shared config, instance/task role), so on properly configured AWS
infrastructure no secrets appear in code.

## What it's good for

- **Deployments already on AWS** that want a fully managed broker with no server
  to run.
- Teams comfortable pairing SQS/SNS with **DynamoDB** for the registry
  side-store.
- Workloads where **native short delays** (SQS `DelaySeconds`) cover most timers
  and only occasional long suspends fall back to the store scheduler.

## Provisioning

There is no server to run: SQS, SNS, and DynamoDB are managed AWS services, and
the broker creates the entities it needs on demand — a jobs queue per process
key, one FIFO instance-event topic, per-subscriber queues, and the registry
table. What you provide is an account, a region, and credentials with
permission to create and use those entities (`sqs:*`, `sns:*`, and
`dynamodb:*` scoped to the `QueuePrefix`/`TableName` you choose, or narrower).

For local development and the [conformance suite](#local-testing) the same code
runs against **LocalStack** — a single container image emulating SQS, SNS, and
DynamoDB — reached through the development-only `Endpoint` override on both the
broker and the RegistryStore. Point `Endpoint` at the LocalStack URL and
everything else stays the same.

## How it works

Each broker duty maps onto AWS primitives as follows. Every entity name carries
the configured `QueuePrefix` (default `blkit`).

- **Job queue, ack, and redelivery** — one **standard SQS queue per process
  key** (`<prefix>-jobs-<slug>-<hash>`, created idempotently; process keys
  contain characters SQS forbids, so the name pairs a sanitized slug with a hash
  of the full key). Delivery starts the queue's **visibility timeout**
  (= `InFlightTimeout` rounded up to whole seconds — the in-flight slot); a lease
  goroutine calls `ChangeMessageVisibility` on a half-timeout cadence to keep a
  long job hidden. A terminal lifecycle report and `ReportSuspended` issue
  `DeleteMessage`. A worker that dies simply stops extending the visibility, it
  lapses, and SQS redelivers — the at-least-once guarantee. Messages that fail to
  decode (poison messages) are deleted rather than redelivered forever.
- **Selective consumption** — a worker long-polls only the queues for the process
  keys it registered.
- **Registry** — the DynamoRegistryStore keeps one **item per worker** on a
  single DynamoDB table (partitioned by `worker#` / `timer#` / `inst#` key
  prefixes) holding the envelope-encoded registrations and a **deadline
  attribute**. `Touch` and `Delete` are conditional writes, so an absent or
  expired worker yields `ErrUnknownWorker`. Expiry is **client-side**: reads
  treat a past-deadline item as gone and a background **sweeper** deletes it;
  DynamoDB's own TTL is enabled only as belt-and-braces cleanup (it lags by
  minutes to hours). `Watch` is a **~200 ms polling diff loop** — not DynamoDB
  Streams — that emits `HeartbeatLost` for a worker that vanished with a lapsed
  deadline and `Removed` for one deleted while still live.
- **Per-instance events, fan-out, and replay** — one **FIFO SNS topic**
  (`<prefix>-inst-events.fifo`). Each subscriber gets a freshly created **FIFO
  SQS queue** subscribed to the topic with a **filter policy** on the
  `instanceID` message attribute (raw delivery), deleted when the subscription
  ends. FIFO ordering (`MessageGroupId = instanceID`) is required because a plain
  SNS→SQS fan-out is unordered and could let the terminal result overtake the
  `Completed` lifecycle event. Because **SNS retains nothing**, every lifecycle
  and terminal publish also upserts a **last-event record** in DynamoDB (latest
  lifecycle envelope, terminal envelope, a finished flag, and an atomically
  allocated `seq`); on subscribe the broker replays that record first, then
  follows the queue live, deduping by `seq`. Records expire after
  `EventRetention`.
- **Suspend-resume timers** — a short wait uses SQS **`DelaySeconds`** natively,
  but that is capped at **15 minutes**; a longer suspend writes a **timer record**
  to the RegistryStore, and a broker-owned scheduler loop (200 ms poll) claims
  due timers with a conditional delete — so concurrent brokers never fire one
  twice — and sends the `JobResume`.
- **Cancel of a queued job** — **unsupported natively**: `DeleteMessage` needs a
  receipt handle that only a receiver holds, so `Cancel` always takes the
  `JobCancel` route and always requires `AllowExternalCancel`.

## Configuration

The broker's own `Config`:

```go
type Config struct {
    Region      string                  // e.g. "eu-west-2"; required
    Credentials aws.CredentialsProvider // nil = default AWS credential chain
    QueuePrefix string                  // default "blkit"; isolates deployments sharing an account
    Registry    bl.RegistryStore        // required; must also implement InstanceEventStore (DynamoRegistryStore does)
    Endpoint    string                  // development only; LocalStack endpoint override
    Cipher      bl.PayloadCipher        // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s (3× the 30s heartbeat interval)
    InFlightTimeout time.Duration // default 150s; SQS visibility timeout, rounded up to whole seconds
    EventRetention  time.Duration // default 1h; last-event record lifetime
}
```

- **`Region`, `Credentials`** — how to reach AWS. Leave `Credentials` nil to use
  the default chain (environment, shared config, instance/task role).
- **`QueuePrefix`** — namespaces every queue, topic, and registry key this broker
  creates. Change it to run several independent blkit deployments in one account
  without collision.
- **`Registry`** — the required side-store. It must also implement the module's
  `InstanceEventStore` interface (the last-event records); `DynamoRegistryStore`
  does, so passing one satisfies both.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) that
  encrypts message payloads end to end, so AWS never sees plaintext. All brokers,
  workers, and the RegistryStore in a deployment must share the same cipher.
- **The three timing knobs** default to values derived from the 30 s worker
  heartbeat:
  - **`RegistrationTTL`** (90 s) — how long a worker's registration outlives its
    last heartbeat before the sweeper declares it lost.
  - **`InFlightTimeout`** (150 s) — how long a delivered job may go unsettled
    before SQS redelivers it. It becomes the queue's visibility timeout,
    **rounded up to whole seconds**, and must comfortably exceed your longest
    task step.
  - **`EventRetention`** (1 h) — how long after an instance finishes its
    last-event record stays replayable to late subscribers.

The RegistryStore has its own `Config`:

```go
type DynamoRegistryStoreConfig struct {
    Region      string                  // e.g. "eu-west-2"; required
    Credentials aws.CredentialsProvider // nil = default AWS credential chain
    Endpoint    string                  // development only; LocalStack endpoint override
    TableName   string                  // default "blkit-registry"; created if absent
    Cipher      bl.PayloadCipher        // optional; must match the broker's

    PollInterval  time.Duration // Watch/DueTimers polling cadence; default 200ms
    SweepInterval time.Duration // expired-registration sweeper cadence; default 1s
}
```

- **`TableName`** — the single DynamoDB table holding workers, timers, and
  last-event records; created on first use if absent. Give each deployment its
  own table (or match `QueuePrefix`) to keep them isolated.
- **`Cipher`** — must match the broker's, because registrations are stored
  envelope-encoded and the store decodes them for `Snapshot`/`Watch`.
- **`PollInterval`** (200 ms) — the cadence of the `Watch` change feed and the
  scheduler's due-timer scan. **`SweepInterval`** (1 s) — how often the sweeper
  removes items whose deadline has lapsed. Both trade responsiveness against
  DynamoDB read cost.

## Local testing

The [conformance suite](overview.md#conformance) starts a **LocalStack**
container (SQS, SNS, and DynamoDB in one free-tier image) with testcontainers-go,
exactly as the SQL state stores start their databases. Set
`BLKIT_TEST_AWS_ENDPOINT` (with ambient credentials) to point the suite at a
real account or a LocalStack you run yourself instead. A handful of behaviours
LocalStack does not fully emulate are tagged and run against a real account in
CI when credentials are present.

## What to keep in mind

- **Cancel is best-effort by design** — a job already queued cannot be pulled
  back through SQS, so cancellation is delivered as a `JobCancel` the worker
  honours, and only when the process opted in with `AllowExternalCancel`.
- **The broker creates AWS entities on demand**, so the credentials it runs under
  need permission to create and use queues, topics, and the DynamoDB table — not
  just to send and receive. Scope those permissions to your `QueuePrefix` and
  `TableName`.
- **Each subscriber creates and deletes its own FIFO queue**, so a workload with
  very many concurrent instance subscriptions churns SQS entities; long-lived
  subscribers amortise that cost.
- **One `QueuePrefix` (and table) per deployment** when sharing an account, or
  two deployments will read each other's queues and registry.

## Reference

The backend's API is in the [AWS SQS/SNS reference](../reference/brokers-aws-sqs-sns.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
