---
name: AzureServiceBusMessageBroker
description: Azure Service Bus message-broker backend — peek-lock queues for jobs, a filtered topic for instance events, native scheduled messages for timers, and a RegistryStore (Table Storage / Cosmos DB) for the registry. Its own module under brokers/azure-service-bus.
targets:
  - ../../brokers/azure-service-bus/broker.go
---

# Azure Service Bus Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The Azure Service Bus backend implements [MessageBroker](overview.spec.md)
against Service Bus queues and topics. Service Bus brings durable queues,
peek-lock delivery, per-session ordering, and — uniquely among the supported
backends — **native scheduled messages**, the best suspend-resume timer
story. Service Bus has no KV store, so the worker registry lives in a
pluggable [RegistryStore](overview.spec.md#the-registrystore), typically
Azure Table Storage or Cosmos DB.

```go
import azsbbroker "github.com/friendly-business-machines/blkit/brokers/azure-service-bus"

broker, err := azsbbroker.New(azsbbroker.Config{
    Namespace: "myns.servicebus.windows.net",
    Credential: cred, // azcore.TokenCredential (Managed Identity in production)
    Registry:   tableRegistryStore,
})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one Service Bus **queue per
   `ProcessKey`** (`<prefix>-jobs-<ns>-<proc>-<ver>`; queue-per-key maps
   onto Service Bus's native scaling unit, at the cost of entity count for
   many-process deployments). Delivery is **peek-lock** with a lock-renewal
   goroutine for long jobs. Terminal lifecycle reports issue `Complete()`;
   `ReportSuspended` for an input wait issues `Defer()` and records the
   sequence number so the matching `JobResume` can `ReceiveDeferredMessage`
   later. A crashed worker's lock expires and Service Bus redelivers;
   repeated expiry dead-letters at `MaxDeliveryCount` (see notes).
2. **Selective consumption** — workers receive only from their registered
   keys' queues.
3. **Registry — RegistryStore** — Azure **Table Storage** (default) or
   Cosmos DB implements the core `RegistryStore` interface: registrations
   with TTL columns, a sweeper for heartbeat expiry, and a Service Bus topic
   (`<prefix>-reg-feed`) carrying change events emitted by
   Register/Unregister/sweeper as the `Watch` feed.
4. **Per-instance events / fan-out / replay** — one Service Bus **Topic**
   (`<prefix>-inst-events`) with a per-subscriber **Subscription** created
   on demand, SQL-filtered on the `instanceID` application property, and
   `AutoDeleteOnIdle` for cleanup. `DefaultMessageTimeToLive` (default 24h)
   is the retention window; because a Subscription only receives messages
   published after it exists, the backend recovers the **latest lifecycle /
   terminal event from the RegistryStore's last-event records** on
   subscribe, then follows the Subscription live.
5. **Delayed delivery** — native `ScheduledEnqueueTime` on the `JobResume`.
   Scheduled messages can also be **cancelled by sequence number**, which
   gives a clean cancel-of-a-scheduled-resume.
6. **Cancel of queued jobs** — Service Bus cannot delete an arbitrary queued
   message, so removal is best-effort **only for scheduled (not yet
   enqueued) messages**; otherwise `Cancel` takes the `JobCancel` route.
7. **TLS** — always on (AMQPS/HTTPS through the Azure SDK). The emulator
   requires a connection string and a development-only insecure knob
   (`UseEmulator`), which never applies to a real namespace.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       Namespace        string                 // "myns.servicebus.windows.net"
       ConnectionString string                 // SAS auth; or set Credential
       Credential       azcore.TokenCredential // Azure AD / Managed Identity (recommended)
       EntityPrefix     string                 // default "blkit"; isolates deployments sharing a namespace
       Registry         bl.RegistryStore       // required; Table Storage or Cosmos DB implementation ships with this module
       UseEmulator      bool                   // development only; permits the emulator's non-TLS endpoint
       Cipher           bl.PayloadCipher       // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite starts Microsoft's **Service
   Bus emulator** container (`mcr.microsoft.com/azure-messaging/servicebus-emulator`)
   plus **Azurite** for the Table Storage RegistryStore, via
   testcontainers-go. `BLKIT_TEST_AZURESB_CONNECTION` points it at a real
   namespace instead. The emulator has feature gaps (verified during
   implementation — e.g. around scheduled-message cancellation); conformance
   areas the emulator cannot run are tagged to skip locally and run against
   a real namespace in CI when credentials are present.

## Notes

- **Sessions** (FIFO per `instanceID`) are opt-in via queue configuration:
  they keep an instance's `JobCancel`/`JobResume` ordered behind its
  `JobStart` and pinned to one consumer, at the cost of per-instance
  throughput. Default off; the at-least-once + state-store-check model does
  not require them.
- **Dead-lettering**: jobs that exhaust `MaxDeliveryCount` (default 10) land
  in the built-in DLQ as the fallback for repeated worker crashes;
  `ReportFailed` is the normal failure path and happens before DLQ ever
  applies.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; application properties carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
