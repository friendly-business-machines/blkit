---
name: BlBoolean
description: blkit's boolean type — true/false with three-valued null propagation; extends BlExpr so all logical operations are deferred and chainable
targets:
  - ../../expr/boolean.go
---

# BlBoolean

`BlBoolean` is blkit's boolean type. It has two values — `true` and `false` — and participates in three-valued logic where `null` propagates through most logical operations. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

```go
type BlBoolean struct { BlExpr }

// Singleton instances — also serve as literal BlExpr leaf nodes
// TRUE BlBoolean
// FALSE BlBoolean

// Construction is via Bl.Boolean(value). See bl.spec.md.

// Logical operations — inherited from BlExpr (And, Or, Not)
// See bl-expr.spec.md for signatures. Three-valued logic is documented below.

// Comparison — deferred; evaluates to BlBoolean
func (b *BlBoolean) Equals(other BlExpr) BlExpr { ... }
func (b *BlBoolean) NotEqual(other BlExpr) BlExpr { ... }

// Eager host-language utilities — only valid on a concrete BlBoolean after .Evaluate()
// Value bool  // the underlying Go bool
func (b *BlBoolean) ToNativeBoolean() bool { ... }
func (b *BlBoolean) String() string { ... }  // "true" or "false"
```

## Deferred semantics

`BlBoolean.TRUE` and `BlBoolean.FALSE` are literal leaf nodes. Chaining is deferred:

```go
expr := Bl.BooleanVar("isEligible").And(Bl.BooleanVar("hasConsent"))
result := expr.Evaluate(map[string]BlExpr{"isEligible": BlBoolean.TRUE, "hasConsent": BlBoolean.FALSE})
// result == BlBoolean.FALSE
```

## Three-Valued Logic

When a logical operand evaluates to `BlNull`, the result follows SQL-style ternary logic:

| `a` | `b` | `a and b` | `a or b` |
|---|---|---|---|
| `true` | `true` | `true` | `true` |
| `true` | `false` | `false` | `true` |
| `true` | `null` | `null` | `true` |
| `false` | `false` | `false` | `false` |
| `false` | `null` | `false` | `null` |
| `null` | `null` | `null` | `null` |

Key observations:
- `true and null` → `null` (unknown)
- `false and null` → `false` (short-circuit: false regardless of unknown)
- `true or null` → `true` (short-circuit: true regardless of unknown)
- `false or null` → `null` (unknown)

## `not_()`

`not_()` returns a deferred node that evaluates to the logical complement: `true` → `false`, `false` → `true`. When the operand evaluates to `BlNull`, the result is `BlNull` (handled consistently across `BlBoolean` and `BlNull`).

## Singletons

`BlBoolean.TRUE` and `BlBoolean.FALSE` are singleton instances. Implementations should return these singletons from `of()` rather than allocating new objects.

## Equality

Two `BlBoolean` values are equal if they have the same `value`. Equality with `BlNull` always evaluates to `BlBoolean.FALSE` (not `BlNull`).

## Edge Cases

- blkit does not perform truthy/falsy coercion: integers, strings, and other non-boolean types are never implicitly converted to boolean. Logical operations on non-booleans evaluate to `BlNull`.
- Boolean literals `true` and `false` are case-sensitive; `True` and `TRUE` are not equivalent.
