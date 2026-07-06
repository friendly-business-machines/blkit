# In-Memory

> The built-in broker that ships in core — Go channels and maps, no external
> broker required. For tests, local development, and single-binary deployments.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The in-memory broker is the built-in backend that ships in core — Go channels and
mutex-guarded maps, no external server required. It implements the full
[MessageBroker](overview.md) surface within a single process.

```go
var broker = bl.NewInMemoryMessageBroker()
```

Each call to `NewInMemoryMessageBroker` creates its **own independent broker**
(matching `NewInMemoryStateStore`); producers and workers that must talk to each
other share the same broker value. Release resources with an explicit `Close()` —
it stops the sweeper and timer goroutines and closes all subscriber channels.

## What it's good for

- **Unit tests** that exercise the broker interface without standing up a server.
- **Single-binary deployments** where the producer code, the worker, and the
  broker all live in one process.
- **Local development.**

It is **not** suitable for multi-process deployments — there is no inter-process
communication, and nothing survives a restart. For that, use one of the
[external backends](overview.md#the-backends).

## How it works

The job queue is a pending-job slice per process key, guarded by a mutex;
delivering a job moves it to an in-flight map with a redelivery timer, and a
lifecycle report settles it. The registry is a map of worker to registrations
with per-worker deadlines, swept by a background goroutine. Per-instance events
fan out to a list of buffered channels, with the latest lifecycle event and the
terminal event kept for replay. Suspend-resume timers use `time.AfterFunc`, and
cancel of a queued job scans the pending slice under the mutex and removes it
exactly.

A [payload cipher](overview.md) is honoured even in-process — payloads still
round-trip through the wire envelope — so the encryption path is exercised by the
same code as the external backends.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
