---
name: MessageBrokerBackends
description: How blkit's pluggable message-broker backends are laid out — the MessageBroker interface and in-memory backend in core, one module per external broker — and the shared client↔worker communication semantics every backend implements
targets:
  - ../../core/message_broker.go
  - ../../core/message_broker_conformance.go
---

# Message Broker Backends

> **Status:** This spec is a work in progress. No backend is implemented yet.
> This document defines the `MessageBroker` interface, the shared semantics
> every backend must honour, and how the backends are laid out in the
> codebase. Per-backend mappings to broker-native primitives live in the
> per-broker specs in this directory.

A **message broker** is how blkit's clients and workers talk to each other.
Clients (MCP servers, web servers, CLI tools, admin UIs) and workers (the
processes that actually execute blkit process definitions) may live in
different binaries on different machines; the broker is the only channel
between them.

The broker exists **only** for client↔worker communication. It has six duties:

- **(a) Worker registration** — a place where workers register which processes
  (and which versions) they are able to execute, kept live by heartbeats and
  expired by TTL.
- **(b) Process queue** — the job queue that carries work to workers.
- **(c) Start requests** — a place where clients request the start of a
  process and supply the start input values.
- **(d) Cancel-type requests** — a place where clients send cancel and
  terminate requests.
- **(e) Input requests and responses** — a place where clients receive a
  running process's requests for further input, and respond to them.
- **(f) Process outcomes** — a place where clients receive process outcomes.

## What the broker is not

**The broker holds no process state.** The execution history and the
current/historical state of every run live in the
[state store](../state-stores/overview.spec.md) as the run's
[ProcessState](../processes/process-state.spec.md) — never in the broker.
The broker carries *messages about* a run; it does not keep a record *of* the
run. There is no broker-held per-instance status record. Workers own
state-store access exclusively; state queries (admin UIs, audit) go through
the state store directly, separately from this interface.

```
   Clients (MCP, web, CLI)                Workers
            │                                │
            ▼                                ▼
     ┌───────────────────────────┐
     │       MessageBroker       │ ── broker-native primitives:
     │  (producer + worker side) │    Redis Streams/Pub-Sub/KV;
     │                           │    NATS JetStream/KV;
     │   registration · queue ·  │    RabbitMQ exchanges/queues/streams;
     │   requests · events       │    Azure Service Bus (+ RegistryStore);
     └───────────────────────────┘    Google Pub/Sub (+ RegistryStore);
                                      AWS SQS/SNS (+ RegistryStore);
                                      in-memory channels + maps
                                                 │
                                                 │ workers (only) also use
                                                 ▼
                                          ┌────────────┐
                                          │ StateStore │  history + state
                                          └────────────┘
```

---

## One lean core, one module per backend

The backends are laid out exactly like the
[state-store backends](../state-stores/overview.spec.md#one-lean-core-one-module-per-backend),
so that **an application only pulls in the dependencies of the backend it
actually uses**.

- The **core** blkit module holds the `MessageBroker` interface, the shared
  types, and one built-in backend: the **in-memory** broker, which needs
  nothing extra.
- Every **other backend** is its **own separate module**, under
  `brokers/<name>/`, which depends on the backend's own client library.

### Layout

```
blkit/                                 the core module
  core/
    message_broker.go                  the MessageBroker interface + shared types
    message_broker_conformance.go      the shared conformance suite
    in_memory_message_broker.go        the built-in in-memory backend (no extra deps)
    ... rest of core ...

  brokers/                             each subfolder is its OWN module
    redis/                             Redis/Valkey backend + its client dependency
    nats/                              NATS backend + its client dependency
    rabbitmq/                          RabbitMQ backend + its client dependency
    azure-service-bus/                 Azure Service Bus backend + Azure SDK
    google-pubsub/                     Google Pub/Sub backend + Google SDK
    aws-sqs-sns/                       AWS SQS/SNS backend + AWS SDK
```

Dependencies only ever point **inward**: a backend module depends on core;
core never depends on a backend.

### Choosing a backend

An application picks a backend by importing it and constructing it. Using the
built-in in-memory broker — nothing extra to import:

```go
var broker = bl.NewInMemoryMessageBroker()
```

Using the Redis backend — import its module and construct it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit/core"
    redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"
)

var broker, err = redisbroker.New(redisbroker.Config{Addr: "localhost:6379"})
```

Either way, `broker` is a `bl.MessageBroker` the rest of blkit uses the same
way — application code does not change when you swap backends.

---

## Interface

`bl.MessageBroker` presents two role-specific groups of methods on a single
interface:

- **Producer-side** — used by clients to submit new process runs, respond to
  a process's request for input, send cancel-type requests, and subscribe to
  instance events and the process registry.
- **Worker-side** — used by `worker.Run` to register the worker's capability
  set, refresh its TTL, fetch jobs, report lifecycle transitions, and post
  error messages.

The same implementation satisfies both roles. Whether a given binary uses
producer-side methods, worker-side methods, or both is determined by which
entry-point functions it calls (`mcpserver.Run`, `worker.Run`, etc.) — not by
which broker type it constructs.

```go
package core

type MessageBroker interface {
    // ===== Producer-side =====

    // Submit a new process run. The broker resolves the process from its
    // registry snapshot (no live worker advertises it -> ErrUnknownProcess),
    // resolves StartID (ErrUnknownStartID), validates Input against the
    // registration's InputContract (DataContractValidationError), generates
    // the ProcessInstanceID client-side, publishes a JobStart, publishes a
    // Queued lifecycle event on the instance's topic, and returns the ID.
    //
    // If the registry snapshot has not arrived yet (cold start), Submit
    // blocks until it does, bounded by ctx.
    //
    // Synchronous errors: ErrUnknownProcess, ErrUnknownStartID,
    // DataContractValidationError, broker-publish errors. Errors that
    // surface only after a worker picks the job up are delivered as an
    // InstanceEvent of kind Error on SubscribeToInstance.
    Submit(ctx context.Context, req StartRequest) (instanceID string, err error)

    // Cancel is fire-and-forget. Behavior:
    //   - If the JobStart is still findable in the queue (no worker has
    //     picked it up), remove it — best-effort, conditioned only on the
    //     job actually being in the queue. The broker itself publishes the
    //     terminal Cancelled lifecycle event. No opt-in required. Returns nil.
    //   - Otherwise publish a JobCancel for a worker to honor. Requires the
    //     process to have AllowExternalCancel (checked against the registry
    //     snapshot); otherwise returns ErrCancelNotAllowed. Returns nil on
    //     successful publish.
    //
    // Whether the instance is already finished is NOT checked here — the
    // broker holds no status record. A worker that receives a JobCancel for
    // an instance the state store shows as finished posts
    // InstanceError{Code: "ALREADY_FINISHED"}.
    //
    // Other synchronous errors: ErrUnknownProcess, broker-publish errors.
    Cancel(ctx context.Context, req CancelRequest) error

    // Terminate is fire-and-forget: publish a JobTerminate for a worker to
    // honor. Hard stop: the worker drives the instance to
    // ProcessStatusCancelled via the graph's TerminateEvent if the process
    // defines one, otherwise marks it terminated without graceful unwind.
    //
    // Requires AllowExternalTerminate (checked against the registry
    // snapshot). Already-finished instances are reported asynchronously as
    // InstanceError{Code: "ALREADY_FINISHED"}, exactly as for Cancel.
    //
    // Synchronous errors: ErrUnknownProcess, ErrTerminateNotAllowed,
    // broker-publish errors.
    Terminate(ctx context.Context, req TerminateRequest) error

    // Respond to a RequestInputTask. The process explicitly executed a
    // RequestInputTask node and is now waiting for input matching the
    // task's ResponseContract; this verb delivers that input keyed by
    // the requestID emitted on the instance's topic when the task fired.
    //
    // Synchronous errors: only broker-publish errors. Whether the
    // instance exists or is waiting on the given requestID is not
    // checked at the broker — those errors flow back as an
    // InstanceEvent of kind Error.
    RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error

    // Subscribe to events for a single process instance. Returns a
    // channel that closes when the instance enters a finished status
    // (Completed, Cancelled, or Failed) or when ctx is cancelled.
    //
    // A late subscriber (including a client that reconnects) first
    // receives the instance's latest lifecycle event — and the terminal
    // Result event, if the instance already finished within the backend's
    // retention window — so it can tell whether the instance is queued,
    // running, suspended, or finished. See "Lifecycle events".
    //
    // Synchronous errors: broker-subscribe errors only. INSTANCE_NOT_FOUND
    // flows back as the first InstanceEvent if no such instance has any
    // events visible to the broker.
    SubscribeToInstance(ctx context.Context, instanceID string) (<-chan InstanceEvent, error)

    // Subscribe to the process registry. The first message on the channel
    // is a full snapshot of currently-live registrations; subsequent
    // messages are incremental changes (added / removed / heartbeat-loss).
    // The channel closes only on ctx cancellation.
    //
    // Use this to populate and maintain a local cache so a REST or MCP
    // server can pre-check whether an incoming request targets a process
    // that some live worker is registered to execute. Submit's own
    // validation runs against the same registry data.
    SubscribeToProcessRegistry(ctx context.Context) (<-chan RegistryUpdate, error)

    // ===== Worker-side: registration lifecycle =====

    // Register this worker's capability set — which processes, and which
    // versions, it is able to execute, including each process's start-event
    // input contracts so producers can validate Submits. Each call replaces
    // any prior registration set for the same workerID. Idempotent. The TTL
    // is set internally by the implementation (typically tied to the
    // heartbeat interval × 3).
    RegisterProcesses(ctx context.Context, workerID string, regs []ProcessRegistration) error

    // Refresh the TTL on this worker's registrations. Called periodically
    // by the worker's heartbeat goroutine.
    //
    // Synchronous errors: ErrUnknownWorker, broker-publish errors.
    Heartbeat(ctx context.Context, workerID string) error

    // Remove this worker's registrations. Called on graceful shutdown so
    // the broker stops advertising processes the worker can no longer
    // service.
    //
    // Synchronous errors: ErrUnknownWorker, broker-publish errors.
    Unregister(ctx context.Context, workerID string) error

    // ===== Worker-side: job queue =====

    // Fetch jobs from the queue for the given process keys. Returns a
    // channel that yields jobs the worker should dispatch to executors.
    // Each Job carries its kind (Start / RespondToInput / Cancel /
    // Terminate / Resume) and the payload the worker needs.
    //
    // The broker keeps a job in-flight from the moment it is delivered
    // until the worker either:
    //   - calls ReportCompleted / ReportCancelled / ReportFailed for the
    //     instance (terminal outcome); or
    //   - calls ReportSuspended for the instance (the process suspended
    //     and should be picked back up later).
    //
    // If the worker dies before any of these, the broker times out the
    // in-flight slot and redelivers the job to another worker.
    //
    // Closes when ctx is cancelled.
    FetchJobs(ctx context.Context, keys []ProcessKey) (<-chan Job, error)

    // ===== Worker-side: lifecycle reports =====
    //
    // Each report publishes a Lifecycle event on the instance's topic —
    // that event stream, not any broker-held record, is how clients tell
    // where an instance currently is. Terminal reports (Completed /
    // Failed / Cancelled) and ReportSuspended also settle the in-flight
    // job. The worker persists the authoritative status to the state
    // store separately, before reporting.
    //
    // Synchronous errors for all five: broker-publish errors.

    // The first verb a worker calls after fetching a JobStart (or a
    // JobResume). Publishes Lifecycle{Phase: Running}. Does NOT settle
    // the in-flight job.
    ReportRunning(ctx context.Context, instanceID string) error

    // The process suspended (Suspend*/Pause* event or RequestInputTask).
    // Publishes Lifecycle{Phase: Suspended} and re-enqueues: the job
    // leaves in-flight and a JobResume is delivered when the wait
    // condition is satisfied (duration elapsed, datetime reached,
    // RespondToInputRequest delivered).
    ReportSuspended(ctx context.Context, instanceID string) error

    // Terminal outcomes. Each publishes the corresponding Lifecycle event
    // (plus the Result / Error event), settles the in-flight job, and
    // closes SubscribeToInstance channels after the final event delivers.
    ReportCompleted(ctx context.Context, instanceID string, result EvaluationResult) error
    ReportFailed(ctx context.Context, instanceID string, err InstanceError) error
    ReportCancelled(ctx context.Context, instanceID string) error

    // ===== Worker-side: errors =====

    // Post an error message to the instance's topic. Visible to
    // subscribers as an InstanceEvent of kind Error. Does NOT by itself
    // change the instance's lifecycle — use ReportFailed for a terminal
    // failure.
    //
    // Synchronous errors: broker-publish errors.
    PostError(ctx context.Context, instanceID string, err InstanceError) error

    // Close releases the broker's resources (connections, goroutines).
    Close() error
}
```

### Request types

```go
type StartRequest struct {
    Namespace string         // package import path of the registered process
    ProcessID string         // the registered process's Id
    Version   string         // the registered process's Version
    StartID   string         // which StartEvent to use as the entrypoint
    Input     map[string]any // input variables; validated against the registry-carried InputContract before publish

    // Optional client-side correlation key recorded in every InstanceEvent
    // emitted for this instance. Use to tie the instance back to a
    // request-id / user-id / etc. on the client side.
    CorrelationKey *string
}

type CancelRequest struct {
    Namespace string  // (Namespace, ProcessID, Version) of the target instance —
    ProcessID string  // needed so the broker can check the process's
    Version   string  // AllowExternalCancel flag in the registry snapshot

    InstanceID string
    Reason     *string
}

type TerminateRequest struct {
    Namespace string  // (Namespace, ProcessID, Version) of the target instance —
    ProcessID string  // needed so the broker can check the process's
    Version   string  // AllowExternalTerminate flag in the registry snapshot

    InstanceID string
    Reason     *string
}
```

### Registry types

```go
type ProcessRegistration struct {
    Namespace   string
    ProcessID   string
    Version     string
    Name        *string
    Description *string

    // Boundary surface — what producers need to construct and validate a
    // Submit. The contracts are runtime-derived from the process
    // definition and travel through the broker in the wire format (see
    // "Wire format"), so producers never import the process packages.
    StartEvents []StartEventInfo
    EndEvents   []EndEventInfo

    // Operation hints — what the MCP / web UI can offer for this process
    AllowExternalCancel    bool
    AllowExternalTerminate bool

    // Pre-rendered markdown for tools like the MCP describe_process built-in
    Markdown string

    // For observability
    WorkerID      string    // set by the worker on RegisterProcesses
    RegisteredAt  time.Time // set by the broker on first RegisterProcesses
    LastHeartbeat time.Time // set by the broker on each Heartbeat / Register
}

type StartEventInfo struct {
    Id            string
    Name          string
    InputContract *InputContract // see ../data/data-contract.spec.md
}

type EndEventInfo struct {
    Id       string
    Name     string
    Contract *OutputContract // optional
}

type ProcessKey struct {
    Namespace string
    ProcessID string
    Version   string
}

type RegistryUpdate struct {
    Kind         RegistryUpdateKind
    Registration *ProcessRegistration // present for all kinds
}

type RegistryUpdateKind int

const (
    // First batch delivered on SubscribeToProcessRegistry — one
    // RegistryUpdateSnapshot per currently-live registration, then a
    // RegistryUpdateSnapshotComplete sentinel with nil Registration.
    RegistryUpdateSnapshot RegistryUpdateKind = iota
    RegistryUpdateSnapshotComplete
    RegistryUpdateAdded         // a worker freshly registered this process
    RegistryUpdateRemoved       // a worker called Unregister
    RegistryUpdateHeartbeatLost // TTL expired without a Heartbeat
)
```

### Job types (worker-side)

`Job` is a tagged union of broker-published work items the worker dispatches to executors.

```go
type Job struct {
    Kind JobKind
    Key  ProcessKey // routing key

    // Kind-specific payload. Exactly one is set per Job.
    Start            *StartJob
    RespondToInput   *RespondToInputJob
    Cancel           *CancelJob
    Terminate        *TerminateJob
    Resume           *ResumeJob // re-evaluation after a Suspend* / Pause* / RequestInputTask
}

type JobKind int

const (
    JobStart JobKind = iota
    JobRespondToInput
    JobCancel
    JobTerminate
    JobResume
)

type StartJob struct {
    InstanceID     string  // broker-generated UUID
    StartID        string
    Input          map[string]any
    CorrelationKey *string
}

type RespondToInputJob struct {
    InstanceID string
    RequestID  string
    Payload    map[string]any
}

type CancelJob struct {
    InstanceID string
    Reason     *string
}

type TerminateJob struct {
    InstanceID string
    Reason     *string
}

type ResumeJob struct {
    InstanceID string
}
```

### Event types

`InstanceEvent` is what `SubscribeToInstance` delivers. It is a tagged union covering lifecycle transitions, input requests, node completions, errors, and final results.

```go
type InstanceEvent struct {
    InstanceID     string
    CorrelationKey *string             // mirrored from StartRequest if set
    Kind           InstanceEventKind
    OccurredAt     time.Time

    // Kind-specific payloads; only one is set per InstanceEvent.
    Lifecycle       *LifecycleChange
    InputRequest    *InputRequest     // RequestInputTask fired; carries the requestID
    NodeCompleted   *NodeCompleted
    Error           *InstanceError
    Result          *EvaluationResult // emitted as the channel closes
}

type InstanceEventKind int

const (
    InstanceEventLifecycle InstanceEventKind = iota
    InstanceEventInputRequest
    InstanceEventNodeCompleted
    InstanceEventError
    InstanceEventResult
)

// A lifecycle transition. Phase uses ProcessStatus (see
// ../processes/process.spec.md): Pending (queued), Running, Suspended,
// Completed, Cancelled, Failed.
type LifecycleChange struct {
    Phase ProcessStatus
}

type InputRequest struct {
    NodeID     string
    RequestID  string         // pass to RespondToInputRequest

    // Request body. Set by RequestInputTask to describe what is being
    // asked of the responder (form schema, prompt text, current case state).
    Payload    map[string]any
}

type NodeCompleted struct {
    NodeID   string
    NodeKind string // "Task", "DecisionTask", "EndEvent", etc.
    Outputs  map[string]any
}

type InstanceError struct {
    Code    string // see "InstanceError codes" below
    Message string
}
```

---

## Lifecycle events

The broker holds no per-instance status record, but a client can still always
tell whether its request is queued, being worked on right now, suspended back
in the queue, or finished — because **every lifecycle transition is an event
on the instance's topic**:

| Event | Published by | When |
|---|---|---|
| `Lifecycle{Pending}` | the broker | `Submit` publishes the `JobStart` ("queued") |
| `Lifecycle{Running}` | the worker (`ReportRunning`) | a worker picks the job up, or resumes a suspended instance |
| `Lifecycle{Suspended}` | the worker (`ReportSuspended`) | the process suspends (Suspend*/Pause* event, RequestInputTask) |
| `Lifecycle{Completed}` + `Result` | the worker (`ReportCompleted`) | the process reached an EndEvent |
| `Lifecycle{Cancelled}` | the worker (`ReportCancelled`), or the broker itself when `Cancel` removed a still-queued job | clean abort |
| `Lifecycle{Failed}` + `Error` | the worker (`ReportFailed`) | terminal failure |

The instance's **current phase is the latest lifecycle event**. Every backend
must support **latest-event replay**: a subscriber that arrives (or
reconnects) after events have already fired receives, first, the latest
lifecycle event — and the terminal `Result`/`Error` event if the instance
already finished — provided the instance is within the backend's retention
window. Each per-broker spec documents the retention mechanism and window.

The event stream is a *view* for clients, with bounded retention. The
authoritative, permanent record of what happened is the run's
[ProcessState](../processes/process-state.spec.md) in the
[state store](../state-stores/overview.spec.md), written by the worker.

---

## Wire format

Every message a backend publishes — jobs, lifecycle events, registry entries —
is a **CBOR-encoded envelope**:

```go
// Envelope is the versioned wire wrapper around every broker message.
type Envelope struct {
    V              uint8   `cbor:"v"`   // envelope version; currently 1
    Kind           string  `cbor:"k"`   // e.g. "job.start", "inst.lifecycle", "registry.reg"
    InstanceID     string  `cbor:"i,omitempty"`
    CorrelationKey *string `cbor:"c,omitempty"`
    KeyID          *string `cbor:"kid,omitempty"` // set when Payload is E2E-encrypted
    Nonce          []byte  `cbor:"n,omitempty"`   // set when Payload is E2E-encrypted
    Payload        []byte  `cbor:"p"`             // CBOR-encoded body (ciphertext when encrypted)
}
```

- **CBOR, self-describing.** The `Payload` is the CBOR encoding of the
  kind-specific body (`StartJob`, `LifecycleChange`, `ProcessRegistration`,
  …). CBOR preserves what JSON cannot — timestamps, binary, big numbers —
  and matches the encoding the state stores already use for Bl values. The
  canonical CBOR mapping for Bl values lives in
  [data-contract.spec.md](../data/data-contract.spec.md#cbor-encoding).
- **No schema artifacts.** Payloads are self-describing and validated at the
  edges against runtime-derived
  [DataContracts](../data/data-contract.spec.md) carried in the registry.
  There is no build step, no code generation, and no broker-native schema
  registry (Confluent-style registries are explicitly not used) — an
  application that uses blkit never generates wire artifacts.
- **Routing metadata stays cleartext.** The fields a broker routes and
  filters on — process key, instance id, message kind — are duplicated into
  the backend's native message metadata (Redis stream fields, NATS subjects
  and headers, AMQP routing keys and headers, Service Bus application
  properties, Pub/Sub attributes, SNS message attributes). They are never
  encrypted; only `Payload` is.
- **Versioned.** `V` is bumped on incompatible envelope changes; backends
  reject envelopes with a version they do not understand.

---

## Transport security and payload encryption

Two independent layers protect payloads in motion:

### TLS — first-class on every backend

Transport encryption to the broker is uniform, not an afterthought:

- **Self-hosted backends** (Redis/Valkey, NATS, RabbitMQ) expose
  `TLS *tls.Config` in their `Config`. Nil means plaintext (development);
  setting it enables TLS with full control over roots, client certificates,
  and server-name verification.
- **Cloud backends** (Azure Service Bus, Google Pub/Sub, AWS SQS/SNS) are
  TLS-always through their SDKs. Their `Config` exposes endpoint overrides
  (and, where required by the emulator, an insecure knob) **only** so the
  local-test emulators can be reached; production traffic is always TLS.

### Optional end-to-end payload encryption

TLS protects the hop to the broker; it does not hide payloads from the broker
itself. Deployments that must keep the broker blind (multi-tenant brokers,
cloud-managed brokers under strict data policies) can plug in a payload
cipher:

```go
// PayloadCipher encrypts envelope payloads end-to-end: producers and
// workers hold the cipher; the broker only ever sees ciphertext.
type PayloadCipher interface {
    // KeyID identifies the key in use; it rides in the envelope so the
    // receiving side can select the right key (and keys can rotate).
    KeyID() string
    Encrypt(plaintext []byte) (ciphertext, nonce []byte, err error)
    Decrypt(keyID string, nonce, ciphertext []byte) ([]byte, error)
}
```

- Core provides an AES-256-GCM implementation:
  `bl.NewAESGCMPayloadCipher(keyID string, key []byte)`. Keys are distributed
  out-of-band by the application.
- Every backend `Config` has a `Cipher PayloadCipher` field. **Default nil —
  no end-to-end encryption**; TLS alone is the normal posture.
- When set, `Envelope.KeyID` and `Envelope.Nonce` are populated and
  `Envelope.Payload` is ciphertext. Routing metadata stays cleartext (see
  "Wire format") — the broker can route what it cannot read.
- A receiver that gets an envelope with a `KeyID` it cannot resolve posts /
  returns a decryption error; it never processes the payload.

---

## The RegistryStore

Redis, NATS, and the in-memory backend keep the worker registry in the
broker's own KV facilities (TTL'd keys, KV buckets, maps). RabbitMQ has no
KV and uses a broadcast pattern instead (see its spec). The cloud brokers —
Azure Service Bus, Google Pub/Sub, AWS SQS/SNS — have no KV store at all, so
their modules accept a pluggable **RegistryStore**, defined in core:

```go
// RegistryStore is the side-store used by backends whose broker has no
// native KV facility. It holds ONLY broker-coordination data — never
// process state (that's the StateStore's job):
//   - the worker registry: live ProcessRegistrations with TTL expiry and
//     a change feed (added / removed / heartbeat-lost)
//   - timer records: durable "deliver a JobResume at time T" entries, for
//     backends without native delayed delivery
//   - last-event records: the latest lifecycle event per instance, ONLY
//     for backends whose event fabric cannot deliver messages published
//     before a subscriber attached (Azure Service Bus subscriptions,
//     AWS SNS)
type RegistryStore interface {
    PutRegistrations(ctx context.Context, workerID string, regs []ProcessRegistration, ttl time.Duration) error
    Touch(ctx context.Context, workerID string, ttl time.Duration) error // heartbeat
    Delete(ctx context.Context, workerID string) error                   // unregister
    Snapshot(ctx context.Context) ([]ProcessRegistration, error)
    Watch(ctx context.Context) (<-chan RegistryUpdate, error)

    PutTimer(ctx context.Context, instanceID string, fireAt time.Time) error
    DueTimers(ctx context.Context, now time.Time) ([]string, error) // instanceIDs; claimed atomically
    DeleteTimer(ctx context.Context, instanceID string) error

    Close() error
}
```

Each cloud-broker spec names its natural RegistryStore implementation (Azure
Table Storage / Cosmos DB; Firestore; DynamoDB) and its local-test story. The
per-broker module ships that implementation; the interface stays in core so
implementations are swappable.

---

## Operation flows

### `Submit`

1. Resolve `(Namespace, ProcessID, Version)` from the broker's **registry
   snapshot** (the same data `SubscribeToProcessRegistry` delivers; the
   broker maintains an internal cache of it). No live worker advertises the
   key → `ErrUnknownProcess`. If the initial snapshot has not arrived yet
   (cold start), block until it does, bounded by `ctx`.
2. Find the `StartEventInfo` matching `StartID`. Missing → `ErrUnknownStartID`.
3. Validate `Input` against the registration's `InputContract` (see
   [../data/data-contract.spec.md](../data/data-contract.spec.md)). Failure →
   `DataContractValidationError`.
4. Generate a `ProcessInstanceID` (UUIDv7 or similar).
5. Publish a `JobStart` carrying `(ProcessInstanceID, StartRequest)`, and a
   `Lifecycle{Pending}` event on the instance's topic. Broker error →
   returned synchronously.
6. Return `ProcessInstanceID`.

Because validation runs against registry-carried contracts, **clients never
import the process-definition packages** — only workers do. A client binary
needs nothing but core, a broker backend, and the process's string key.

The worker that fetches the `JobStart` creates the run's persisted
[ProcessState](../processes/process-state.spec.md) in the state store using
the broker-supplied `ProcessInstanceID`, calls `ReportRunning`, begins
evaluation, and publishes further events via the worker-side verbs.

If the broker delivers the same `JobStart` twice (at-least-once semantics),
the worker treats the second arrival as a no-op — the `ProcessInstanceID`
already has persisted state.

### `RespondToInputRequest`

1. Publish a `JobRespondToInput` carrying `(instanceID, requestID, payload)`.
   Broker error → returned synchronously.
2. Return.

The worker that fetches the resulting job loads the run's state from the
state store, confirms it is waiting on the given `requestID`, validates the
payload against the `RequestInputTask`'s `ResponseContract`, and resumes
evaluation. If state is missing or the requestID doesn't match, the worker
calls `PostError` with `Code: "INSTANCE_NOT_FOUND"` or `"NOT_WAITING"`.

### `Cancel`

1. Resolve the process from the registry snapshot. Missing →
   `ErrUnknownProcess`.
2. **Best-effort queue removal**: if the instance's `JobStart` is still
   findable in the queue (no worker has picked it up), atomically remove it,
   publish `Lifecycle{Cancelled}` on the instance's topic, and return nil.
   No opt-in check — pre-execution cancellation is always allowed. This is
   conditioned only on the job actually being in the queue; backends that
   cannot remove queued messages skip straight to step 3 (their specs say
   so).
3. Otherwise check `AllowExternalCancel` on the registration. Not opted in →
   `ErrCancelNotAllowed`. Opted in → publish a `JobCancel` and return nil.

Cancel is **fire-and-forget** past this point. A worker handling a
`JobCancel` checks the run in the state store: already finished → it posts
`InstanceError{Code: "ALREADY_FINISHED"}` and settles the job; otherwise it
appends a synthetic `CancelEvent` to the run's history, drives the instance
to `ProcessStatusCancelled`, and calls `ReportCancelled`.

### `Terminate`

1. Resolve the process from the registry snapshot. Missing →
   `ErrUnknownProcess`.
2. Check `AllowExternalTerminate`. Not opted in → `ErrTerminateNotAllowed`.
3. Publish a `JobTerminate`. Broker error → returned synchronously.
4. Return. Fire-and-forget; already-finished instances surface as
   `InstanceError{Code: "ALREADY_FINISHED"}`.

A worker handling a `JobTerminate` drives the instance to terminal status
(via `TerminateEvent` if the process defines one) and calls `ReportCancelled`
(terminate is a stronger form of cancel from the status perspective;
processes that need to distinguish do so via the synthetic event in the run's
history, not via status).

### `SubscribeToProcessRegistry`

The first messages on the returned channel are a snapshot — one
`RegistryUpdateSnapshot` per currently-live registration — followed by a
single `RegistryUpdateSnapshotComplete` sentinel. After the sentinel, the
channel delivers incremental updates as workers register, unregister, or have
their TTL expire.

Consumers maintain a local map keyed by `(Namespace, ProcessID, Version)`. A
REST or MCP server uses this map to pre-check whether an incoming request
targets a process that some live worker is currently registered to execute —
using the registration's contracts, without importing the process packages.

### `RegisterProcesses` / `Heartbeat` / `Unregister`

The worker calls `RegisterProcesses` once on startup with one
`ProcessRegistration` per process in `bl.AllProcesses()` — including the
runtime-derived input/output contracts. The broker stores them in its
registry with a TTL.

`Heartbeat` is called by a worker-owned heartbeat goroutine on a configurable
interval (default 30s). It refreshes the TTL on every registration this
`workerID` published. If the worker stops heartbeating (crash, network
partition), entries expire and a `RegistryUpdateHeartbeatLost` is delivered
to registry subscribers.

`Unregister` is called on graceful shutdown to remove the entries immediately
rather than waiting for TTL expiry.

The worker also calls `RegisterProcesses` again after every successful change
to its capability set (rare in practice — it would require dynamic process
loading, which blkit does not currently support).

### `FetchJobs` and the report verbs

`FetchJobs(ctx, keys)` returns a channel of `Job`s targeted at processes in
`keys`. The implementation handles the broker-specific selective-consumption
mechanism (Redis: per-key streams with consumer groups; NATS: JetStream
subject filters; RabbitMQ: routing-key bindings; the cloud brokers: filters
or per-key queues — see each spec).

For each `Job` received, the worker dispatches it to an executor goroutine.
The broker holds the job in-flight until the worker calls one of:

- `ReportCompleted(instanceID, result)` — the process reached an `EndEvent`.
  Settles the job.
- `ReportCancelled(instanceID)` — the process reached a `CancelEvent`
  (whether triggered internally or by an external `JobCancel` /
  `JobTerminate`). Settles the job.
- `ReportFailed(instanceID, err)` — terminal failure. Settles the job.
- `ReportSuspended(instanceID)` — the process suspended. The job leaves
  in-flight; a new `JobResume` is delivered when the wait condition is
  satisfied.

If the worker dies before any of these, the broker times out the in-flight
slot (per-backend configurable; default 5× heartbeat interval) and redelivers
to another worker. There is no explicit Ack/Nack — the report verbs and the
timeout cover the same cases.

`ReportRunning` is called first after fetching a `JobStart` or `JobResume`;
it publishes the lifecycle event but does **not** settle the job.

### `PostError`

Workers call `PostError` to surface a non-terminal error to subscribers: a
transient task failure that the retry policy will handle, an invalid response
to a `RespondToInputRequest` (`NOT_WAITING`), a cancel for an instance that
already finished (`ALREADY_FINISHED`), etc. The lifecycle does not change. If
the error is terminal, the worker calls `ReportFailed` instead — which also
publishes an Error event.

---

## `SubscribeToInstance` and event delivery

Push-only. Implementations open a long-lived subscription on the broker and
forward events to the returned channel.

```go
ch, err := broker.SubscribeToInstance(ctx, instanceID)
for evt := range ch {
    switch evt.Kind {
    case bl.InstanceEventLifecycle:
        // Pending / Running / Suspended / terminal — where the instance is now
    case bl.InstanceEventInputRequest:
        // surface to UI; eventually call broker.RespondToInputRequest(...)
    case bl.InstanceEventResult:
        // channel closes after this
    case bl.InstanceEventError:
        log.Printf("error: code=%s message=%s", evt.Error.Code, evt.Error.Message)
    }
}
```

The channel closes when:

- The context is cancelled, **or**
- The instance has reached `ProcessStatusCompleted` / `Cancelled` / `Failed`
  and the corresponding final event has been delivered.

### Late subscribers

A subscriber that attaches after events have fired first receives the
instance's latest lifecycle event (and the terminal `Result`/`Error` if the
instance finished), provided the instance is within the backend's retention
window — see [Lifecycle events](#lifecycle-events). Outside the retention
window, the subscription delivers `INSTANCE_NOT_FOUND`; the authoritative
record is always available from the state store.

### Backpressure

If a subscriber's channel buffer fills, events are **dropped** rather than
blocking the broker reader. When the buffer recovers, a synthetic
`InstanceEvent{Kind: Error, Error.Code: "BACKPRESSURE_DROP"}` is delivered.
Per-backend specs may override this default.

### Fan-out

Multiple subscribers to the same instance: each gets the full event stream by
default (broadcast). Per-backend specs may add a queue-group mode where
events are load-balanced across subscribers.

---

## `ProcessOpts` opt-ins

`Cancel` (when a worker already holds the instance) and `Terminate` require
the process author to opt in. See
[../processes/process.spec.md](../processes/process.spec.md):

```go
type ProcessOpts struct {
    // ... existing fields ...

    AllowExternalCancel    bool // default false; required for MessageBroker.Cancel once a worker holds the instance
    AllowExternalTerminate bool // default false; required for MessageBroker.Terminate
}
```

Queue-side cancellation (`Cancel` of a job no worker has picked up) does NOT
require opt-in — the process author can't reasonably forbid cancelling a run
that never started. The flags travel in the `ProcessRegistration`, so the
broker checks them against the registry snapshot.

---

## Error model

### Synchronous errors

| Error | Returned from | When |
|---|---|---|
| `ErrUnknownProcess` | `Submit`, `Cancel`, `Terminate` | `(Namespace, ProcessID, Version)` not in the registry snapshot |
| `ErrUnknownStartID` | `Submit` | `StartID` does not match any `StartEvent` in the registration |
| `DataContractValidationError` | `Submit` | `Input` fails the registration's `InputContract` |
| `ErrCancelNotAllowed` | `Cancel` (when the job is no longer in the queue) | Process has `AllowExternalCancel: false` |
| `ErrTerminateNotAllowed` | `Terminate` | Process has `AllowExternalTerminate: false` |
| `ErrUnknownWorker` | `Heartbeat`, `Unregister` | `workerID` not currently registered |
| broker-publish errors | All publish ops | Connection refused, auth failure, etc. |

There are **no** `ErrAlreadyCompleted` / `ErrAlreadyCancelled` /
`ErrAlreadyFailed` synchronous errors — detecting them would require a
broker-held status record, and the broker holds none. Already-finished
instances surface asynchronously as `ALREADY_FINISHED`.

### Asynchronous errors (via `InstanceEvent{Kind: Error}` on `SubscribeToInstance`)

Posted by workers via `PostError` (non-terminal) or `ReportFailed` (terminal).

| `InstanceError.Code` | Source |
|---|---|
| `INSTANCE_NOT_FOUND` | Worker handling a job for an unknown `instanceID`; or a subscription to an instance outside retention |
| `NOT_WAITING` | `RespondToInputRequest` for an instance not waiting on the given `requestID` |
| `ALREADY_FINISHED` | `JobCancel` / `JobTerminate` for an instance the state store shows as already Completed / Cancelled / Failed |
| `ALREADY_INTERRUPTING` | `Cancel` / `Terminate` job arrives while a prior interrupt is still being processed |
| `TASK_FAILED` | A non-terminal task error during evaluation (retry policy applies) |
| `PROCESS_FAILED` | The instance reached `ProcessStatusFailed` — paired with `ReportFailed` |
| `BACKPRESSURE_DROP` | Subscriber's buffer overflowed; events were dropped (synthesized by the broker) |
| decryption errors | An envelope's `KeyID` could not be resolved or its payload failed to decrypt |

---

## Supported backends

The backends fall into three families.

**Built into core — not durable, single process:**

- **In-memory** — [in-memory-message-broker.spec.md](./in-memory-message-broker.spec.md)
  — Go channels and maps, no extra dependencies. For tests, local development,
  and single-binary deployments.

**Self-hosted — a server you run:**

- **Redis / Valkey** — [redis-message-broker.spec.md](./redis-message-broker.spec.md)
  — Streams, Pub/Sub, and TTL'd keys cover every duty natively, including true
  removal of queued jobs. The lightweight self-host default.
- **NATS** — [nats-message-broker.spec.md](./nats-message-broker.spec.md)
  — JetStream subject filtering gives the cleanest selective consumption; KV
  with watch maps directly onto the registry. A natural fit when NATS is
  already your state store.
- **RabbitMQ** — [rabbitmq-message-broker.spec.md](./rabbitmq-message-broker.spec.md)
  — classic work-queue semantics with a huge enterprise install base; the
  registry and timers use documented patterns rather than native KV.

**Cloud-managed — with a RegistryStore side-store, locally testable via emulators:**

- **Azure Service Bus** — [azure-service-bus-message-broker.spec.md](./azure-service-bus-message-broker.spec.md)
  — peek-lock delivery and native scheduled messages (the best timer story);
  registry and last-event records in Azure Table Storage / Cosmos DB.
- **Google Pub/Sub** — [google-pubsub-message-broker.spec.md](./google-pubsub-message-broker.spec.md)
  — filtered subscriptions and seekable retention; registry and timers in
  Firestore. The most workaround-heavy fit; its spec documents each one.
- **AWS SQS/SNS** — [aws-sqs-sns-message-broker.spec.md](./aws-sqs-sns-message-broker.spec.md)
  — SQS queues for jobs, SNS filtered fan-out for events; registry, timers,
  and last-event records in DynamoDB.

**Excluded:** Kafka, RedPanda, AutoMQ, and Pulsar are deliberately not
supported — the partitioned-log consumption model (consumer-group offsets
rather than per-message acknowledgement, no selective removal, no delayed
delivery, per-instance topics as an anti-pattern) is a poor fit for a job
queue with per-instance fan-out.

### Desired properties — admitting a future backend

A broker qualifies for a blkit backend if it can provide:

1. **A durable job queue** with per-message acknowledgement, at-least-once
   delivery to competing consumers, and redelivery when a consumer dies
   mid-job.
2. **Selective consumption** — workers receive only jobs for the
   `(Namespace, ProcessID, Version)` keys they registered.
3. **Per-instance event fan-out** — many subscribers per instance, each
   receiving the full stream — with **latest-event replay** for late
   subscribers within a retention window.
4. **Delayed / scheduled delivery** for suspend-resume timers — natively, or
   via a documented timer pattern (RegistryStore timer records + scheduler).
5. **Best-effort removal of still-queued jobs** — natively, or a documented
   statement that Cancel always takes the message route.
6. **A worker registry** — native TTL'd KV with a change feed, or a workable
   pattern (heartbeat broadcast), or a `RegistryStore` side-store.
7. **TLS** transport encryption.
8. **A maintained Go client.**
9. **Local testability** — in-process, embedded, or a container/emulator
   startable with testcontainers-go, so `go test` needs only a Docker daemon.

Every per-broker spec answers the same nine points in this order, so the
trade-offs stay comparable across backends.

---

## Testing

Every backend is verified against a **shared conformance suite**, so they all
behave identically. The suite lives in core
(`core/message_broker_conformance.go`) and each backend module runs it
against its own broker. It checks the shared semantics above:

- **Registration roundtrip** — register, snapshot, heartbeat, unregister;
  TTL expiry delivers `RegistryUpdateHeartbeatLost`.
- **Registry-based Submit** — validation against registry-carried contracts;
  `ErrUnknownProcess` / `ErrUnknownStartID` / `DataContractValidationError`;
  cold-start blocking bounded by ctx.
- **Job delivery** — selective consumption by process key; at-least-once
  delivery; duplicate `JobStart` tolerated; in-flight timeout redelivers to
  another consumer.
- **Lifecycle events** — the Pending → Running → (Suspended → Running)* →
  terminal event sequence arrives in order; late subscribers get
  latest-event replay; channels close after the final event.
- **Cancel semantics** — still-queued jobs are removed where the backend
  supports it (and the broker publishes the terminal Cancelled event);
  otherwise a `JobCancel` is delivered; opt-in flags are enforced from the
  registry.
- **Suspend / resume** — `ReportSuspended` releases the job; a `JobResume`
  arrives when the wait condition is satisfied (input response, timer).
- **Envelope integrity** — payloads round-trip through CBOR; with a
  `PayloadCipher` configured, payloads are unreadable on the wire and
  decrypt correctly at the receiver; unknown `KeyID` is rejected.
- **Backpressure** — a slow subscriber gets `BACKPRESSURE_DROP`, not a
  wedged broker reader.

How the suite is run depends on the backend:

- **In-memory** runs the suite in-process — no setup, part of the normal
  test run.
- **NATS** runs the suite against a real JetStream server **embedded in the
  test process** (`nats-server` is importable as a Go library) — no
  container needed, same as the NATS state store.
- **Redis/Valkey** and **RabbitMQ** spin up a throwaway container with
  [testcontainers-go](https://golang.testcontainers.org/), exactly as the
  SQL state stores do.
- **Azure Service Bus** runs against Microsoft's Service Bus **emulator**
  container, plus **Azurite** for the Table Storage RegistryStore.
- **Google Pub/Sub** runs against the gcloud **Pub/Sub emulator** container,
  plus the **Firestore emulator** for the RegistryStore.
- **AWS SQS/SNS** runs against **LocalStack** (SQS + SNS + DynamoDB in one
  container).

Setting `BLKIT_TEST_<NAME>_URL` (per-backend variants documented in each
spec) points the suite at an already-running instance instead. A test skips
only when neither an endpoint nor a reachable Docker daemon is available.
Emulators do not implement every cloud feature; each cloud spec lists which
conformance areas run against the emulator and which need a real endpoint.

---

## End-to-end example

A web server submits a loan-application process, surfaces a
`RequestInputTask` for human approval, and delivers the response back. The
worker pool runs separately — in a different binary that imports the process
packages. **This client binary does not**: it knows the process only by its
string key, courtesy of registry-based validation.

```go
package main

import (
    "context"
    "log"

    bl          "github.com/friendly-business-machines/blkit/core"
    redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"
)

func main() {
    broker, err := redisbroker.New(redisbroker.Config{
        Addr: "localhost:6379",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer broker.Close()

    ctx := context.Background()

    instanceID, err := broker.Submit(ctx, bl.StartRequest{
        Namespace: "example.com/processes/lending",
        ProcessID: "loan-application",
        Version:   "1.0",
        StartID:   "start",
        Input: map[string]any{
            "applicant":   applicantPayload,
            "loan_amount": 250000,
        },
    })
    if err != nil {
        log.Fatalf("submit: %v", err)
    }

    events, err := broker.SubscribeToInstance(ctx, instanceID)
    if err != nil {
        log.Fatalf("subscribe: %v", err)
    }

    for evt := range events {
        switch evt.Kind {
        case bl.InstanceEventLifecycle:
            log.Printf("instance is now %v", evt.Lifecycle.Phase)

        case bl.InstanceEventInputRequest:
            response := promptUserForApproval(ctx, evt.InputRequest)
            if err := broker.RespondToInputRequest(ctx, instanceID, evt.InputRequest.RequestID, response); err != nil {
                log.Printf("respond: %v", err)
            }

        case bl.InstanceEventResult:
            log.Printf("done: status=%v outputs=%v", evt.Result.Status, evt.Result.Context)

        case bl.InstanceEventError:
            log.Printf("error: code=%s message=%s", evt.Error.Code, evt.Error.Message)
        }
    }
}
```

---

## Edge cases

- **Only workers import process packages.** Clients validate against
  registry-carried contracts; workers use the in-process registry
  (`bl.AllProcesses()`) for registration and capability-set filtering on
  `FetchJobs`.
- **A `Cancel` racing with a graph-driven terminal event**: whichever the
  worker observes first wins. If the cancel-job arrives after the instance
  finished, the worker posts `InstanceError{Code: "ALREADY_FINISHED"}`.
- **`Terminate` after `Cancel` already requested but not yet processed**:
  implementations may accept (terminate overrides; "harder" wins) or reject
  with `ALREADY_INTERRUPTING`. The overview recommends accepting; per-backend
  specs document their choice.
- **`SubscribeToInstance` on an instance that already finished**: within the
  backend's retention window the subscriber gets the latest lifecycle event
  and the terminal `Result`/`Error` (latest-event replay); outside it,
  `INSTANCE_NOT_FOUND`. The state store always has the authoritative answer.
- **A worker that crashes silently** leaves stale registrations until TTL
  expiry — typically `heartbeat_interval × 3`. `SubscribeToProcessRegistry`
  delivers `RegistryUpdateHeartbeatLost` when this happens. Any job it held
  in-flight is redelivered after the in-flight timeout.
- **Multiple workers register the same `(Namespace, ProcessID, Version)`**:
  the registry holds one `ProcessRegistration` per worker.
  `SubscribeToProcessRegistry` consumers typically collapse to a
  per-`ProcessKey` view; per-backend specs document the on-wire shape.
- **Two workers register the same key with different contracts** (a rolling
  deploy mid-upgrade): registrations are per-worker, so both shapes are
  visible; `Submit` validates against any one live registration for the key.
  Version your processes — that is what `Version` is for.
- **`FetchJobs` with an empty `keys` slice**: returns a channel that yields
  nothing. Idle wait until ctx cancels.
- **A `JobStart` delivered twice**: the second arrival is a no-op
  (broker-generated `instanceID` is the idempotency key; the worker checks
  the state store).
- **A worker calls `ReportSuspended` but the wait condition is never
  satisfied** (no `RespondToInputRequest` ever arrives): the instance stays
  suspended forever. Per-process timeouts on `RequestInputTask` are the
  recommended guard.
- **Registry snapshot staleness on Submit**: a worker may crash between the
  snapshot check and job delivery. The job waits in the queue until a capable
  worker appears — clients observe the instance stuck at `Pending` and may
  `Cancel` it (queue removal needs no opt-in).
