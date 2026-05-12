---
name: BlContext
description: blkit's context type (modelled on FEEL) — an ordered key-value map; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/context.go
---

# BlContext

`BlContext` is blkit's context type, modelled on FEEL's context: an ordered map from string keys to blkit values. In FEEL-style notation it looks like `{ name: "Alice", age: 30 }`. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

`BlContext` is distinct from `ExecutionContext` (the mutable process variable store). `BlContext` is a pure value type used within expressions and decision tables.

```go
type BlContext struct { BlExpr }

// Construction is via Bl.Context(entries map[string]BlExpr) — see bl.spec.md.
// Bl.Context(nil) yields the empty context.

// Properties — deferred
// Size BlNumber

func (c *BlContext) IsEmpty() BlExpr { ... }   // evaluates to BlBoolean

// Access — deferred
func (c *BlContext) Get(key string) BlExpr { ... }         // evaluates to BlNull if key not present
func (c *BlContext) Has(key string) BlExpr { ... }          // evaluates to BlBoolean
func (c *BlContext) Keys() BlList { ... }                   // BlList of BlString keys (insertion order)
func (c *BlContext) Values() BlList { ... }                 // BlList of values (insertion order)
func (c *BlContext) GetEntries() BlList { ... }             // BlList of BlContext {key, value} pairs

// Immutable modification — deferred; evaluate to BlContext
func (c *BlContext) Put(key string, value BlExpr) BlContext { ... }   // add or overwrite a key
func (c *BlContext) PutAll(other BlExpr) BlContext { ... }            // merge; other's keys overwrite
func (c *BlContext) Remove(key string) BlContext { ... }              // context without that key
func (c *BlContext) Merge(others ...BlExpr) BlContext { ... }         // merge all; later contexts overwrite

// Equality — deferred; evaluates to BlBoolean (order-insensitive)
func (c *BlContext) Equals(other BlExpr) BlExpr { ... }
func (c *BlContext) NotEqual(other BlExpr) BlExpr { ... }

// Eager host-language utilities — only valid on a concrete BlContext after .Evaluate()
func (c *BlContext) ToRecord() map[string]BlValue { ... }
func (c *BlContext) String() string { ... }  // FEEL-style notation: '{ name: "Alice", age: 30 }'
```

## Deferred semantics

```go
ctx := Bl.Context(map[string]BlExpr{"applicant": Bl.Context(map[string]BlExpr{"age": Bl.Number(30)})})
expr := ctx.Get("applicant").Put("score", Bl.NumberVar("computedScore"))
result := expr.Evaluate(map[string]BlExpr{"computedScore": Bl.Number(720)})
// result is a BlContext: {"applicant": {"age": 30}, "score": 720}
```

## Key Ordering

`BlContext` preserves insertion order. `keys()` and `values()` evaluate in insertion order. Equality is order-insensitive.

## Key Type

Keys are always non-empty strings. Keys are case-sensitive.

## Merging

`merge(*others)` creates a new context containing all keys from `self` and all `others`. When the same key appears in multiple contexts, the **last** context's value wins (right-to-left precedence, modelled on FEEL's `context merge` built-in).

## Expression Scope

When blkit's evaluator evaluates an expression, the evaluation scope is modelled as a `BlContext`. Variable names in scope are keys; path expressions navigate nested `BlContext` values.

## Edge Cases

- `get()` on a missing key evaluates to `BlNull`.
- `put()` with an empty string key produces a `BlTypeError` at evaluation time.
- `remove()` on a non-existent key evaluates to the original context unchanged.
- `put_all()` / `merge()` with an empty context is a no-op.
- Keys with special characters require quoted key syntax in FEEL-style notation: `context["my key"]`.
