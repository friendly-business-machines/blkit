# Azure Service Bus

> A cloud-managed broker on Azure — peek-lock queues for jobs, a filtered topic for
> instance events, native scheduled messages for timers, and a RegistryStore in
> Table Storage or Cosmos DB.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The Azure Service Bus backend implements [MessageBroker](overview.md) against
Service Bus queues and topics. Service Bus brings durable queues, peek-lock
delivery, per-session ordering, and — uniquely among the supported backends —
**native scheduled messages**, the best suspend-resume timer story. Service Bus
has no KV store, so the worker registry lives in a pluggable **RegistryStore**,
typically Azure Table Storage or Cosmos DB.

```go
import azsbbroker "github.com/friendly-business-machines/blkit/brokers/azure-service-bus"

var broker, err = azsbbroker.New(azsbbroker.Config{
    Namespace:  "myns.servicebus.windows.net",
    Credential: cred, // azcore.TokenCredential (Managed Identity in production)
    Registry:   tableRegistryStore,
})
```

## What it's good for

- **Deployments already on Azure** that want a fully managed broker.
- Workflows with **suspend-resume timers**, which map onto native scheduled
  messages more cleanly than on any other backend.

## How it works

- **Job queue** — one **queue per process key**; delivery is **peek-lock** with a
  lock-renewal goroutine for long jobs. Terminal reports `Complete()`; an input
  wait `Defer()`s the message and records its sequence number so the resume can
  fetch it later. A crashed worker's lock expires and Service Bus redelivers.
- **Selective consumption** — workers receive only from their registered keys'
  queues.
- **Registry — RegistryStore** — Azure **Table Storage** (default) or Cosmos DB:
  registrations with TTL columns, a sweeper for expiry, and a Service Bus topic
  carrying change events as the feed.
- **Per-instance events** — one **Topic** with a per-subscriber **Subscription**
  created on demand, SQL-filtered on the instance id and auto-deleted on idle.
  Because a Subscription only sees messages published after it exists, the latest
  lifecycle and terminal events are recovered from the RegistryStore's last-event
  records on subscribe. `DefaultMessageTimeToLive` (default 24h) is the window.
- **Timers** — native `ScheduledEnqueueTime` on the resume, cancellable by
  sequence number.
- **Cancel of queued jobs** — best-effort **only for scheduled (not yet enqueued)
  messages**; otherwise cancel takes the message route.

## Configuration

```go
type Config struct {
    Namespace        string                 // "myns.servicebus.windows.net"
    ConnectionString string                 // SAS auth; or set Credential
    Credential       azcore.TokenCredential // Azure AD / Managed Identity (recommended)
    EntityPrefix     string                 // default "blkit"; isolates deployments sharing a namespace
    Registry         bl.RegistryStore       // required; Table Storage or Cosmos DB ships with this module
    UseEmulator      bool                   // development only; permits the emulator's non-TLS endpoint
    Cipher           bl.PayloadCipher       // optional end-to-end payload encryption; default nil
}
```

Jobs that exhaust `MaxDeliveryCount` land in the built-in **dead-letter queue** —
the fallback for repeated worker crashes.

## Local testing

The conformance suite starts Microsoft's **Service Bus emulator** container plus
**Azurite** for the Table Storage RegistryStore, via testcontainers-go.
`BLKIT_TEST_AZURESB_CONNECTION` points it at a real namespace instead. Emulator
feature gaps are tagged and run against a real namespace in CI when credentials
are present.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
