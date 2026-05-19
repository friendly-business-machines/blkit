---
name: BlNull
description: blkit's null singleton — represents absence or unknown; extends BlExpr so logical operations are deferred and chainable
targets:
  - ../../expr/null.go
---

# BlNull

`BlNull` is blkit's null type — a singleton value representing the absence of a value or an unknown. It is the result of missing context keys, out-of-range list access, division by zero, and other operations that produce no meaningful result. It extends `BlExpr`, so `BlNull.INSTANCE` is itself a valid leaf node in any expression tree and logical operations return deferred `BlExpr` nodes.

```go
type BlNull struct { BlExpr }

// Singleton — the only instance of BlNull
// INSTANCE BlNull

// Type check — eager; always returns true
func (n *BlNull) IsNull() bool { ... }

// Logical operations — inherited from BlExpr (And, Or, Not)
// See bl-expr.spec.md for signatures. Three-valued logic applies: Not(null) → null.

// Equality — deferred; always evaluates to BlBoolean.FALSE (null ≠ null)
func (n *BlNull) Equals(other BlExpr) BlExpr { ... }
func (n *BlNull) NotEqual(other BlExpr) BlExpr { ... }

// Eager host-language utility
func (n *BlNull) String() string { ... }   // "null"
```

## Singleton

`BlNull.INSTANCE` is the only value of this type. All operations that produce null must return this singleton. Implementations must not create multiple `BlNull` instances. `BlNull.INSTANCE.evaluate()` returns `BlNull.INSTANCE` (identity, as with all literal leaf nodes).

## `is_null()`

`is_null()` is an **eager** host-language utility (returns native `bool`), not a deferred expression. Use it in host code to check whether a concrete evaluation result is null:

```go
result := expr.Evaluate(context)
if result.IsNull() {
    // handle null
}
```

To check for null **within an expression**, use `instance_of("Null")` (inherited from `BlExpr` and available on every typed variable factory). The choice of typed factory is irrelevant for this check — `InstanceOf` is universal:

```go
Bl.NumberVar("x").InstanceOf("Null")  // any typed factory works; the check is universal
```

## Null Propagation

`BlNull` propagates through arithmetic, string concatenation, path expressions, and most other operations. Exceptions are the logical operators (`and_`, `or_`) which follow three-valued logic.

| Operation | Result |
|---|---|
| `null + 1` | `null` |
| `null * "hello"` | `null` |
| `null.someKey` | `null` |
| `null[1]` | `null` |
| `null = null` | `false` (null is not equal to null) |
| `null != null` | `false` |
| `null instance of Null` | `true` |
| `true and null` | `null` |
| `false and null` | `false` |
| `true or null` | `true` |
| `false or null` | `null` |

## Null Equality

`BlNull` is **not** equal to itself: `null = null` evaluates to `BlBoolean.FALSE`. This mirrors SQL NULL semantics. To test for null, use `instance_of("Null")` in expressions or `is_null()` in host code.

## `instance of` Check

The type name for null is `Null` (capital N). `value instance of Null` evaluates to `BlBoolean.TRUE` if and only if `value` is `BlNull.INSTANCE`.

## Producing Null

The following operations produce `BlNull` at evaluation time:

- Missing key in a `BlContext` or `ExecutionContext`
- Out-of-range index access on a `BlList`
- Division by zero
- `sqrt()` of a negative number
- `log()` of zero or a negative number
- `power()` producing a complex result
- Any arithmetic or path expression with a null operand

## Edge Cases

- `BlNull` cannot be stored as a `BlContext` value for a key whose contract (`InputContract`, `OutputContract`, or nested `ContextContract`) marks the field as required — a `DataContractValidationError` is thrown at write time.
- Passing `BlNull.INSTANCE` to a factory that requires a non-null argument (e.g. `Bl.String(null)`) produces a `BlTypeError`.
- `__str__()` returns the string `"null"` — the literal representation used throughout blkit's text rendering.
