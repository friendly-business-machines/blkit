---
name: LambdaHandler
description: AWS Lambda handler factory — wraps one or more *Process instances in a function whose signature matches lambda.Start()'s expected shape
targets:
  - ../faas/lambda.go
---

# LambdaHandler

`LambdaHandler` returns a function that AWS Lambda's `aws-lambda-go` runtime can invoke directly. The returned function accepts a raw `json.RawMessage` event so a single handler can serve any Lambda trigger source — API Gateway, SQS, EventBridge, S3, Step Functions, etc. — by parsing the event inside the user-supplied `Input` and `Route` callbacks.

For shared options, defaults, routing, persistence, and re-enqueue behaviour, see [overview.spec.md](overview.spec.md).

```go
func LambdaHandler(opts FaasHandlerOpts) func(ctx context.Context, event json.RawMessage) (any, error)
```

The returned function:

1. Calls `opts.Route(ctx, event)` (or the default extractor) to resolve which `*Process` to evaluate.
2. Calls `opts.Input(ctx, event)` (or the default JSON unmarshaller) to build the `Input` map.
3. Calls `process.Evaluate(EvaluateOpts{StartId: opts.StartID, Input: input})`.
4. If `opts.StateStore` is set, calls `Save()` with the resulting history.
5. If `opts.Gateway` is set, publishes `Event`s during/after `Evaluate` and (on suspension) a continuation command via the gateway.
6. Calls `opts.Response(ctx, result)` (or the default which returns a `map[string]any` snapshot of `result.Context`) and returns the value.

Any error from these steps is returned to `aws-lambda-go`, which propagates it to the Lambda runtime — the platform's retry / DLQ semantics then apply.

---

## Example

```go
// main.go
package main

import (
    "github.com/aws/aws-lambda-go/lambda"
    "github.com/friendly-business-machines/blkit"
    "github.com/friendly-business-machines/blkit/faas"
    "github.com/friendly-business-machines/blkit/messagebroker"

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
    gw, _      = messagebroker.NewRedisBrokerGateway(messagebroker.RedisOpts{Addr: "redis.internal:6379"})
)

func main() {
    lambda.Start(faas.LambdaHandler(faas.FaasHandlerOpts{
        StateStore: stateStore,
        Gateway:    gw,
    }))
}
```

A request body of:

```json
{
  "namespace": "example.com/area/lendingflows/v1",
  "processId": "loan-application",
  "version": "1.0",
  "applicant": { "name": "Ada", "income": 90000 }
}
```

resolves to the process registered by `example.com/area/lendingflows/v1` under `(namespace="example.com/area/lendingflows/v1", id="loan-application", version="1.0")`, runs `Evaluate()` with `Input = {"applicant": {...}}`, and returns the post-evaluation context snapshot as JSON. The default `Input` extractor passes the entire event object as the input map; the routing fields (`namespace`, `processId`, `version`) are present in `Input` too, which is harmless — `Process.Evaluate()` ignores variables it does not consume.

For events where the routing fields should be stripped before handing input to the process, supply a custom `Input`:

```go
faas.LambdaHandler(faas.FaasHandlerOpts{
    Input: func(ctx context.Context, event json.RawMessage) (map[string]any, error) {
        var raw map[string]any
        if err := json.Unmarshal(event, &raw); err != nil {
            return nil, err
        }
        delete(raw, "namespace")
        delete(raw, "processId")
        delete(raw, "version")
        return raw, nil
    },
})
```

---

## SQS Trigger Example

The default `Input` and `Route` parse the entire Lambda event as one JSON object. SQS triggers wrap one or more user payloads inside an `events.SQSEvent` envelope — each user payload is in `Records[i].Body` as a JSON string. Supply custom callbacks to handle this.

```go
// main.go
func main() {
    lambda.Start(func(ctx context.Context, event events.SQSEvent) error {
        h := faas.LambdaHandler(faas.FaasHandlerOpts{
            StateStore: stateStore,
            Gateway:    gw,
        })
        for _, rec := range event.Records {
            if _, err := h(ctx, json.RawMessage(rec.Body)); err != nil {
                return err
            }
        }
        return nil
    })
}
```

---

## Edge Cases

- The Lambda runtime may invoke the handler concurrently. The handler is safe under concurrent use; see [overview.spec.md](overview.spec.md) "Edge Cases".
- Lambda's 15-minute execution limit applies. Processes whose total execution time may exceed this should set `MaxRunTime` (see [process.spec.md](../processes/process.spec.md)) to fail fast, or be designed to suspend (e.g. via timer or message catch events) and resume via the `Gateway` continuation path.
- Lambda retries depend on the trigger source and configuration (asynchronous invocations retry up to twice by default; SQS retries via visibility timeout until the message is moved to a DLQ). The handler does not attempt its own retry — it relies on the platform.
- Cold starts: construct `*Process`, `StateStore`, and `BrokerGateway` in package-level `var` blocks or `init()` so they are reused across warm invocations. The factory itself is also reusable.
- Returning `(nil, nil)` is permitted; some Lambda trigger types (e.g. SQS, EventBridge) ignore the return value.
- Edge cases around `Route`, `Input`, persistence, and re-enqueue are documented in [overview.spec.md](overview.spec.md) "Edge Cases".
