---
name: FAAS Handlers Overview
description: Shared design, options, and behaviour for blkit FAAS handler factories — thin wrappers over Process.Evaluate() targeting AWS Lambda, GCP Cloud Functions, and Azure Functions
targets:
  - ../faas/handler.go
---

# FAAS Handlers Overview

The `blkit.faas` namespace provides per-vendor handler-factory functions that adapt the global blkit process registry into a function whose signature matches the vendor SDK's expected handler shape. The user instantiates the factory in their `main()` and hands the result to the vendor SDK. Processes are made available to the handler by importing the packages that define them — `NewProcess()` registers each process automatically (see [process.spec.md "Registry"](../processes/process.spec.md)).

The FAAS layer is a thin wrapper around `Process.Evaluate()` (see [process.spec.md "Execution"](../processes/process.spec.md#execution)). It does **not** involve a long-running worker or a writer pool — each invocation is a single, synchronous evaluation. Each FAAS invocation:

1. Resolves the inbound event to a `*Process` via `Route` + `blkit.LookupProcess(namespace, id, version)`.
2. Extracts initial `Input` variables from the event via `Input`.
3. Builds the initial state via `store.NewExecutionState(process, NewExecutionStateOpts{StartId: opts.StartID, Input: input})`, calls `gw.MarkRunning(...)` if a `Gateway` is configured, then calls `process.Evaluate(EvaluateOpts{Context: ctx, History: hist})`.
4. Persists the resulting `ExecutionHistory` and `ExecutionContext` to the configured `StateStore`, if any.
5. Signals outcome on the configured `MessageGateway`, if any: `MarkCompleted` / `MarkFailed` on terminal status, or `ReenqueueSuspended` if the process suspended (so a future `JobResume` can resume it).
6. Formats and returns a response in the shape the vendor SDK expects.

Per-vendor specs:

- [aws-lambda.spec.md](aws-lambda.spec.md) — `LambdaHandler` for AWS Lambda
- [gcp-cloud-functions.spec.md](gcp-cloud-functions.spec.md) — `CloudFunctionHandler` for GCP Cloud Functions / Cloud Run functions
- [azure-functions.spec.md](azure-functions.spec.md) — `AzureFunctionHandler` for Azure Functions

---

## Shared Options

All vendor factories accept the same `FaasHandlerOpts` struct. Vendor-specific specs reference this section rather than re-stating it.

```go
type FaasHandlerOpts struct {
    Route      func(ctx context.Context, event json.RawMessage) (namespace, id, version string, error) // optional; default reads top-level "namespace", "processId", and "version" fields from the event JSON
    StartID    string                                                                                  // default "start"
    Input      func(ctx context.Context, event json.RawMessage) (map[string]any, error)                // optional; default unmarshals event JSON into map[string]any
    Response   func(ctx context.Context, result *EvaluationResult) (any, error)                        // optional; default returns a map[string]any snapshot of result.Context (each task's Output keyed by task id)
    StateStore StateStore                                                                              // optional; if set, history and context are persisted after Evaluate()
    Gateway    messagegateway.MessageGateway                                                             // optional; if set, the handler calls MarkRunning, posts errors via PostError, and signals outcome via MarkCompleted / MarkFailed / ReenqueueSuspended
}
```

Processes the handler can serve are not configured here — the handler resolves every routed `(namespace, id, version)` against the global registry maintained by `NewProcess()`. To make a process callable through the handler, import the package that defines it.

In typical usage, `main` doesn't reference the process package's exported symbols directly — the FAAS handler invokes the registered processes through the registry. Go therefore requires a **blank import** (`import _ "..."`) so the compiler doesn't reject the unused name. Each blank-imported process package runs its package-level `var` initializers and `init()` functions, which is when `NewProcess()` runs and registers each process. The registration is the load-bearing effect of the import — without it the handler can't resolve any request — so "blank import" describes the *syntax*, not the importance.

---

## Routing

The handler resolves which `*Process` to evaluate by combining `Route` with a registry lookup:

1. **`Route` is always invoked** to extract `(namespace, id, version)` from the event. If `Route` is `nil`, a default extractor is used: it parses the event JSON and reads the top-level `"namespace"`, `"processId"`, and `"version"` fields.
2. **Lookup against the global registry** — the handler calls `blkit.LookupProcess(namespace, id, version)` (see [process.spec.md "Registry"](../processes/process.spec.md)). The registry is populated by `NewProcess()` when each process package is imported.
3. **No match** — if `LookupProcess` returns `false`, the handler returns an `UnknownProcessError`.
4. **`Route` error** — if `Route` itself returns an error, the handler returns that error to the vendor SDK without invoking `Evaluate()`.

The default extractor is intentionally minimal — users with non-standard event shapes are expected to supply their own `Route`.

---

## Input Extraction

The `Input` callback converts the inbound event into the `map[string]any` accepted by `EvaluateOpts.Input`.

- **Default** — `json.Unmarshal(event, &input)` where `input` is a `map[string]any`. If the event is not a JSON object, the default returns an error.
- **Custom** — supply an `Input` function for events that need decoding (e.g. base64-wrapped queue payloads, vendor envelope formats, or selective field extraction).

`Input` is called after `Route`, so `Route` cannot consume any of the input variables — both run against the same raw event.

The extracted input is validated against the resolved `StartEvent`'s `InputContract` during `store.NewExecutionState(...)`, before any token is placed. Every `StartEvent` carries an `InputContract` by construction (see [../processes/event-nodes.spec.md](../processes/event-nodes.spec.md)), so this validation is unconditional. A failure produces a `DataContractValidationError` and the handler returns it to the vendor SDK without persisting or enqueuing — the submission is rejected synchronously. See [../data/data-contract.spec.md](../data/data-contract.spec.md).

---

## Evaluation

The handler calls:

```go
result, err := process.Evaluate(EvaluateOpts{
    StartId: opts.StartID,         // defaults to "start" when zero-valued
    Input:   input,
})
```

If `Evaluate()` itself returns an error (distinct from `result.Status == Failed`), the handler returns that error to the vendor SDK without persisting or enqueuing.

The `ProcessInstanceID` generated by `Evaluate()` is read from `result.History.ProcessInstanceID` for the persistence and re-enqueue paths.

---

## Persistence

If `StateStore` is set, the handler persists the resulting state after `Evaluate()` returns, regardless of `result.Status`:

```go
err := opts.StateStore.Save(result.History.ProcessInstanceID, result.History)
```

`Save()` writes the full `ExecutionHistory`, which includes the latest `ExecutionContext` snapshot per the StateStore contract (see [state-store.spec.md](../data/state-store.spec.md)).

If `StateStore.Save()` returns an error, the handler returns that error to the vendor SDK so the platform's retry mechanism can fire. Silent persistence loss is worse than a visible failure.

If `StateStore` is `nil`, persistence is skipped — the user can persist inside `Response` if they prefer.

---

## Event Emission

If `Gateway` is set, the handler emits events to subscribers during evaluation via the gateway's per-event verbs:

- `MarkRunning(instanceID)` once, before calling `Evaluate`.
- `PostError(instanceID, err)` for any non-terminal task errors that the retry policy will handle (the gateway delivers these as `InstanceEventError` to subscribers; status does not change).
- Node-completion and input-request events are emitted by `Evaluate`'s internal publish hooks (per-impl wiring; the FAAS layer itself does not call out node-completion verbs directly).

External subscribers (MCP servers, web servers, admin UIs) observe progress via `gw.SubscribeToInstance(...)`.

If `Gateway` is `nil`, no events are emitted. The handler runs to a terminal status invisibly to the broker.

---

## Outcome on Suspension

If `Gateway` is set **and** the process suspended (did not reach a terminal `ProcessStatusCompleted` or `ProcessStatusFailed` — see [process.spec.md "Suspension"](../processes/process.spec.md#suspension)), the handler calls `gw.ReenqueueSuspended(ctx, instanceID)`. This places a `JobResume` on the broker queue for the process key, to be picked up later — by a worker pool, by another FAAS invocation triggered via a broker-event-bridge (EventBridge / Pub/Sub / NATS triggers), or by any other consumer of the broker's job stream.

The exact wire shape for the `JobResume` (subject naming, payload encoding, delivery substrate) is documented in each implementation's spec — see [../messagegateway/redis.spec.md](../messagegateway/redis.spec.md), [nats.spec.md](../messagegateway/nats.spec.md), [azure-service-bus.spec.md](../messagegateway/azure-service-bus.spec.md), [google-pubsub.spec.md](../messagegateway/google-pubsub.spec.md), [in-memory.spec.md](../messagegateway/in-memory.spec.md).

After a successful `ReenqueueSuspended`, the handler returns success to the vendor SDK. The current invocation has done its job.

If `ReenqueueSuspended` fails, the handler returns that error to the vendor SDK.

If `Gateway` is `nil` and the process suspended, the handler still persists (if `StateStore` is set) and returns the `Response` for the suspended state, but no re-enqueue happens. The user is responsible for resuming the process by some other mechanism.

## Outcome on Completion / Failure

If `Gateway` is set and `result.Status == Completed`, the handler calls `gw.MarkCompleted(ctx, instanceID, *result.EvaluationResult)`. If `result.Status == Failed`, the handler calls `gw.MarkFailed(ctx, instanceID, InstanceError{Code: "PROCESS_FAILED", Message: ...})`. These verbs publish the terminal event to subscribers and mark the instance finished in the broker's status record (so a later `Cancel` / `Terminate` will see the correct `ErrAlready*`).

---

## Response Formatting

The `Response` callback shapes the value returned to the vendor SDK.

- **Default** — returns a `map[string]any` snapshot of the post-evaluation `ExecutionContext`. Each task's `Output` map is keyed by its task id, producing a `map[string]any` of the form `{taskId1: {...output...}, taskId2: {...}}`. Vendor specs document how each vendor's default handler then serializes this — typically as JSON.
- **Custom** — supply a `Response` function to return a different shape (e.g. only specific output fields, a vendor-specific response envelope, or status codes derived from `result.Status`).

`Response` runs after persistence and re-enqueue. If either of those failed, `Response` is not called.

---

## Failure Mapping

| Outcome | Returned to vendor SDK |
|---|---|
| `result.Status == Completed` | success, with `Response` value |
| `result.Status == Suspended` and `Gateway` set, `ReenqueueSuspended` succeeds | success, with `Response` value |
| `result.Status == Suspended` and `Gateway` not set | success, with `Response` value (no re-enqueue scheduled) |
| `result.Status == Failed` | error to vendor SDK |
| `Evaluate()` returned a Go error | that error to vendor SDK |
| `StateStore.Save()` returned an error | that error to vendor SDK |
| `Gateway.MarkRunning` / `MarkCompleted` / `MarkFailed` returned an error | that error to vendor SDK |
| `Gateway.ReenqueueSuspended` returned an error | that error to vendor SDK |
| `Route` returned an error | that error to vendor SDK |
| `Input` returned an error | that error to vendor SDK |
| `Route` resolved a non-registered process | `UnknownProcessError` to vendor SDK |
| Submission inputs failed the resolved `StartEvent`'s `InputContract` | `DataContractValidationError` to vendor SDK; nothing persisted or enqueued |

The vendor SDK (and the FAAS platform) decides retry semantics based on whether a Go `error` is returned.

---

## Cold Start and Reuse

`*Process` instances, the `StateStore` connection, and the `MessageGateway` connection should all be constructed in the FAAS init / global scope so they are reused across warm invocations. The handler factory itself can also be constructed once in `init()` or `main()` and re-used; it captures the options by value and is safe to call concurrently.

---

## Edge Cases

- The global registry is empty when the handler is invoked — the handler returns `UnknownProcessError` for every request, since no `(namespace, id, version)` can match. To make processes available, import the package that defines them so `NewProcess()` runs at init.
- `Route` is `nil` — the default extractor is used. If the event JSON has no `namespace`, `processId`, or `version` field, the handler returns `ValueError`.
- Duplicate `(Namespace, Id, Version)` registration is impossible at runtime — `NewProcess()` panics at init time on collision (see [process.spec.md "Registry"](../processes/process.spec.md)).
- `StartID` is empty — defaults to `"start"`.
- `Input` returns `nil` — treated as an empty map, equivalent to `Input` being unset.
- The event payload is empty — `Input` is called with an empty `json.RawMessage`. The default extractor returns an error for empty input.
- `Evaluate()` returns a non-nil error and a partially-populated `result` — the handler returns the error without persisting or enqueuing. State for the partial run is lost (no rollback, since FAAS handlers persist nothing during evaluation).
- The process completes in the same invocation — `result.Status == Completed`. No continuation is published even if `Gateway` is set.
- The process fails — `result.Status == Failed`. State is persisted (if `StateStore` is set) so the failure is recorded; no continuation is enqueued.
- Concurrent invocations of the handler — safe. The handler does not mutate `FaasHandlerOpts`, the `*Process` instances are stateless per [process.spec.md "Statefulness"](../processes/process.spec.md), and the `StateStore` / `MessageGateway` interfaces are required to be safe for concurrent use.
- A FAAS-only deployment (no worker pool) does not register processes with the broker. As a result, those processes do not appear in registry snapshots delivered by `gw.SubscribeToProcessRegistry(...)`, and producers using that path (e.g. an MCP server) won't see them. To expose FAAS-deployed processes to producers, run a worker pool on the same broker — the worker registers and FAAS handles the actual evaluation; the two roles can coexist on the same `(Namespace, ProcessID, Version)`.
