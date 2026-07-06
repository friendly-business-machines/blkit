# Google Pub/Sub

> A cloud-managed broker on Google Cloud — a filtered jobs topic, per-subscriber
> filtered subscriptions with retention and Seek replay for instance events, and a
> Firestore RegistryStore for the registry and timers.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The Google Pub/Sub backend implements [MessageBroker](overview.md) against standard
Cloud Pub/Sub — at-least-once delivery with per-message acknowledgement,
server-side attribute filters, and seekable topic retention. It is the most
workaround-heavy of the supported backends (no message deletion, no native delayed
delivery, quota-bound subscription management), and each workaround is documented
below. Pub/Sub **Lite** is not supported — a distinct product with
partitioned-log semantics.

Pub/Sub has no KV store, so the worker registry and timer records live in a
pluggable **RegistryStore**, typically Firestore.

```go
import gpsbroker "github.com/friendly-business-machines/blkit/brokers/google-pubsub"

var broker, err = gpsbroker.New(gpsbroker.Config{
    ProjectID: "my-project",
    Registry:  firestoreRegistryStore,
})
```

## What it's good for

- **Deployments already on Google Cloud** that want a managed broker.
- Teams comfortable pairing Pub/Sub with **Firestore** for the registry
  side-store.

## How it works

- **Job queue** — one **jobs Topic** with a pull **Subscription per worker pool**;
  Pub/Sub holds a message in-flight until acked or the deadline lapses, extended
  for long jobs. A crashed worker misses the deadline and Pub/Sub redelivers.
- **Selective consumption** — the subscription carries a server-side **attribute
  filter** on the process key. Filters are immutable per subscription, so a changed
  capability set means a replacement subscription.
- **Registry — RegistryStore** — **Firestore**: one document per worker with a TTL
  field (swept for expiry), and the `onSnapshot` listener as the change feed.
- **Per-instance events** — one **events Topic** with message retention (default
  24h — the window). Each subscriber creates a filtered Subscription and **Seeks**
  it back to the retention window, which delivers messages published before it
  existed — the replay path. Subscription creation is quota-bound and takes
  seconds, so long-lived clients should reuse subscriptions.
- **Timers** — none natively: a suspend writes a **timer record** to Firestore that
  a scheduler loop claims when due and publishes as the resume.
- **Cancel of queued jobs** — **unsupported natively** (no message deletion), so
  cancel always takes the message route.

## Configuration

```go
type Config struct {
    ProjectID    string              // GCP project owning the topics/subscriptions
    Credentials  *google.Credentials // nil = Application Default Credentials
    EntityPrefix string              // default "blkit"; isolates deployments sharing a project
    Registry     bl.RegistryStore    // required; Firestore implementation ships with this module
    Endpoint     string              // development only; emulator endpoint override
    Cipher       bl.PayloadCipher    // optional end-to-end payload encryption; default nil
}
```

A per-subscription **dead-letter topic** catches messages that exhaust max
delivery attempts — the fallback for repeated worker crashes.

## Local testing

The conformance suite starts the **gcloud Pub/Sub emulator** and **Firestore
emulator** containers via testcontainers-go. `BLKIT_TEST_GPUBSUB_PROJECT` (with
ambient credentials) points it at a real project instead. Emulator gaps are tagged
and run against a real project in CI when credentials are present.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
