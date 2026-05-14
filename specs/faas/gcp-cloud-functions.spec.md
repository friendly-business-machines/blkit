---
name: CloudFunctionHandler
description: GCP Cloud Functions / Cloud Run functions handler factory — wraps one or more *Process instances in an http.HandlerFunc
targets:
  - ../faas/gcpfunc.go
---

# CloudFunctionHandler

`CloudFunctionHandler` returns an `http.HandlerFunc`. The Go runtimes for both GCP Cloud Functions (gen 2) and Cloud Run functions are built on `net/http`, so the same factory targets both. The user registers the handler with the functions framework or `http.Handle` and the platform routes requests to it.

For shared options, defaults, routing, persistence, and re-enqueue behaviour, see [overview.spec.md](overview.spec.md).

```go
func CloudFunctionHandler(opts FaasHandlerOpts) http.HandlerFunc
```

The returned handler:

1. Reads the request body as `json.RawMessage`.
2. Calls `opts.Route(ctx, body)` (or the default extractor) to resolve which `*Process` to evaluate.
3. Calls `opts.Input(ctx, body)` (or the default JSON unmarshaller) to build the `Input` map.
4. Calls `process.Evaluate(EvaluateOpts{StartId: opts.StartID, Input: input})`.
5. If `opts.StateStore` is set, calls `Save()` with the resulting history.
6. If `opts.Gateway` is set, signals outcome on the broker: `MarkCompleted` / `MarkFailed` on terminal status, or `ReenqueueSuspended` on suspension.
7. Calls `opts.Response(ctx, result)` (or the default which returns a `map[string]any` snapshot of `result.Context`) and writes the value to the response.

The `context.Context` passed into all callbacks is `r.Context()`.

---

## Default Response Format

Unlike `LambdaHandler` (which returns a Go value to the SDK), the GCP handler must write the response itself. The default `Response` writes:

| Outcome | HTTP status | Body |
|---|---|---|
| `result.Status == Completed` | `200 OK` | JSON-encoded context snapshot |
| `result.Status == Suspended` and `Gateway` set, `ReenqueueSuspended` succeeds | `202 Accepted` | JSON `{"processInstanceId": "..."}` |
| `result.Status == Suspended` and `Gateway` not set | `200 OK` | JSON-encoded context snapshot |
| `result.Status == Failed` | `500 Internal Server Error` | JSON `{"error": "..."}` |
| `Evaluate()` returned a Go error | `500 Internal Server Error` | JSON `{"error": "..."}` |
| `StateStore.Save()` returned an error | `500 Internal Server Error` | JSON `{"error": "..."}` |
| Any `Gateway.Mark*` / `ReenqueueSuspended` returned an error | `500 Internal Server Error` | JSON `{"error": "..."}` |
| `Route` returned `UnknownProcessError` | `404 Not Found` | JSON `{"error": "..."}` |
| `Route` or `Input` returned any other error | `400 Bad Request` | JSON `{"error": "..."}` |

A custom `Response` overrides the body and status entirely — implementations that want the default status mapping but a different body should call the default first and then post-process.

`Content-Type: application/json` is set on every response.

---

## Example

```go
// function.go
package lendinghandler

import (
    "github.com/GoogleCloudPlatform/functions-framework-go/functions"
    "github.com/friendly-business-machines/blkit"
    "github.com/friendly-business-machines/blkit/faas"
    "github.com/friendly-business-machines/blkit/messagegateway"

    // Blank-imported with "_" because nothing in this file references the
    // package directly — a regular import would fail to compile with Go's
    // "imported and not used" error. The import is still doing real work:
    // when the program starts, Go runs every imported package's var
    // initializers and init() functions, exactly as it does for any other
    // package. Each NewProcess(...) call inside this package executes during
    // that normal initialization and writes itself into the global registry.
    _ "example.com/area/lendingflows/v1"
)

var (
    stateStore = blkit.NewPostgresStateStore("postgresql://blkit:secret@db.internal:5432/blkit", "loan_app")
    gw, _      = messagegateway.NewRedisMessageGateway(messagegateway.RedisOpts{Addr: "redis.internal:6379"})
)

func init() {
    functions.HTTP("HandleProcess", faas.CloudFunctionHandler(faas.FaasHandlerOpts{
        StateStore: stateStore,
        Gateway:    gw,
    }))
}
```

A `POST` request body of:

```json
{
  "namespace": "example.com/area/lendingflows/v1",
  "processId": "loan-application",
  "version": "1.0",
  "applicant": { "name": "Ada", "income": 90000 }
}
```

resolves to the process registered by `example.com/area/lendingflows/v1` under `(namespace="example.com/area/lendingflows/v1", id="loan-application", version="1.0")`, evaluates it locally, and returns `200 OK` with the post-evaluation context snapshot as JSON.

---

## Pub/Sub Trigger

GCP Cloud Functions can also be triggered by Pub/Sub messages, in which case the function signature is event-shaped rather than HTTP-shaped. For Pub/Sub triggers, construct the handler the same way and invoke it manually inside an event-shaped function:

```go
// function.go
package lendinghandler

import (
    "context"

    "github.com/cloudevents/sdk-go/v2/event"
    "github.com/GoogleCloudPlatform/functions-framework-go/functions"
    "github.com/friendly-business-machines/blkit"
    "github.com/friendly-business-machines/blkit/faas"
)

func init() {
    h := faas.CloudFunctionHandler(faas.FaasHandlerOpts{ /* ... */ })
    functions.CloudEvent("HandleProcessEvent", func(ctx context.Context, e event.Event) error {
        // Wrap the event body in an http.Request-shaped invocation.
        // See blkit/faas examples for the helper pattern.
        return invokeHandlerWithBody(ctx, h, e.Data())
    })
}
```

The HTTP-handler shape remains the canonical FAAS interface for GCP; the Pub/Sub path is a thin adapter the user writes themselves.

---

## Edge Cases

- Cloud Functions and Cloud Run functions impose request-size and execution-time limits that vary by tier. Long-running processes should suspend and re-enqueue rather than exceed the limit.
- The handler is safe under concurrent use; the platform may dispatch multiple requests to the same instance.
- A request body that is not valid JSON results in `400 Bad Request` (the default `Input` returns an error).
- A `GET` request results in `400 Bad Request` (the default `Input` requires a body).
- Cold starts: construct `*Process`, `StateStore`, and `MessageGateway` in package-level `var` blocks or `init()`. The factory itself is also reusable.
- Edge cases around `Route`, `Input`, persistence, and re-enqueue are documented in [overview.spec.md](overview.spec.md) "Edge Cases".
