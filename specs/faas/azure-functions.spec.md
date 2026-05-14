---
name: AzureFunctionHandler
description: Azure Functions handler factory — wraps one or more *Process instances in an http.HandlerFunc compatible with Azure's Go custom-handler model
targets:
  - ../faas/azurefunc.go
---

# AzureFunctionHandler

Azure Functions does not ship a first-class Go runtime; Go functions are deployed via the **custom-handler** model, in which the platform forwards every trigger to an HTTP server the user runs inside the function host. `AzureFunctionHandler` returns an `http.HandlerFunc` suitable for serving from that custom-handler HTTP listener.

The factory is kept distinct from `CloudFunctionHandler` despite the matching return type, because Azure's custom-handler envelope differs from a plain HTTP request when the function is bound to a non-HTTP trigger (Service Bus, Storage Queue, Event Grid). Keeping the symbol separate gives blkit a place to add Azure-specific envelope handling without overloading the GCP spec.

For shared options, defaults, routing, persistence, and re-enqueue behaviour, see [overview.spec.md](overview.spec.md).

```go
func AzureFunctionHandler(opts FaasHandlerOpts) http.HandlerFunc
```

The returned handler:

1. Reads the request body. For HTTP-triggered functions, the body is the raw user request body. For non-HTTP triggers (Service Bus, Storage Queue, etc.), the body is Azure's custom-handler envelope: a JSON object whose `Data` field maps each input binding name to its payload. The handler unwraps the envelope when present (see "Custom-Handler Envelope" below).
2. Calls `opts.Route(ctx, body)` (or the default extractor) to resolve which `*Process` to evaluate.
3. Calls `opts.Input(ctx, body)` (or the default JSON unmarshaller) to build the `Input` map.
4. Calls `process.Evaluate(EvaluateOpts{StartId: opts.StartID, Input: input})`.
5. If `opts.StateStore` is set, calls `Save()` with the resulting history.
6. If `opts.Gateway` is set, signals outcome on the broker: `MarkCompleted` / `MarkFailed` on terminal status, or `ReenqueueSuspended` on suspension.
7. Calls `opts.Response(ctx, result)` (or the default which returns a `map[string]any` snapshot of `result.Context`) and writes the value to the response.

The `context.Context` passed into all callbacks is `r.Context()`.

---

## Custom-Handler Envelope

When an Azure Function uses a non-HTTP trigger, the function host posts the trigger payload to the user's HTTP server using a fixed envelope:

```json
{
  "Data": {
    "<binding-name>": <payload>
  },
  "Metadata": { ... }
}
```

The handler detects the envelope by checking for top-level `Data` and `Metadata` keys. When detected:

- The body forwarded to `Route` and `Input` is the value of `Data["<binding-name>"]` if exactly one binding is present, or the full `Data` object if multiple bindings are present. The user can override this by supplying their own `Input` and `Route` that consume the full envelope.
- The handler writes its response in the envelope shape Azure expects, with the user-visible body in `Outputs["<output-binding-name>"]`. The default output binding name is `"res"`; this can be overridden via the (Azure-specific) options field below if needed.

For HTTP-triggered functions, the body is **not** wrapped in an envelope — the handler forwards the raw body as-is, behaving identically to `CloudFunctionHandler`.

---

## Default Response Format

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

When the request was wrapped in a custom-handler envelope, the response body is wrapped in the matching output envelope before writing.

`Content-Type: application/json` is set on every response.

---

## Example — HTTP Trigger

```go
// main.go
package main

import (
    "net/http"
    "os"

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

func main() {
    http.HandleFunc("/api/handle-process", faas.AzureFunctionHandler(faas.FaasHandlerOpts{
        StateStore: stateStore,
        Gateway:    gw,
    }))

    port := os.Getenv("FUNCTIONS_CUSTOMHANDLER_PORT")
    if port == "" {
        port = "8080"
    }
    http.ListenAndServe(":"+port, nil)
}
```

The path `/api/handle-process` is configured in the function's `function.json` to bind to an HTTP trigger.

---

## Example — Service Bus Trigger

For a Service Bus binding, `function.json` declares an input binding named `message` (the binding name is user-chosen). The custom-handler envelope received by the HTTP server then has the form `{"Data": {"message": <payload>}, "Metadata": {...}}`. The handler unwraps `Data["message"]` automatically.

```go
http.HandleFunc("/api/process-message", faas.AzureFunctionHandler(faas.FaasHandlerOpts{
    StateStore: stateStore,
    Gateway:    gw,
}))
```

The handler routes, evaluates, and persists exactly as it does for an HTTP trigger — only the unwrap step differs.

---

## Edge Cases

- The function host enforces a per-invocation timeout (default 5 minutes for the Consumption plan, configurable up to 60 minutes on Premium / Dedicated). Long-running processes should suspend and rely on broker continuations for resumption.
- The custom-handler envelope is a stable Azure convention but `Data` field names depend on the user's `function.json`. The handler infers a single binding's name automatically; multiple bindings require the user to consume the full envelope via a custom `Input`.
- HTTP-triggered functions are detected by absence of the `Data` / `Metadata` envelope; the handler falls back to raw body parsing.
- Cold starts: construct `*Process`, `StateStore`, and `MessageGateway` in package-level `var` blocks or `init()`. The factory itself is also reusable.
- Edge cases around `Route`, `Input`, persistence, and re-enqueue are documented in [overview.spec.md](overview.spec.md) "Edge Cases".
