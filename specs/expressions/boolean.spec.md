---
name: BlBoolean
description: The boolean type in the blkit expression language — true/false with three-valued (null-propagating) logic. Covers the boolean literals, the and/or/not operators, the boolean built-ins, and the Go layer (BlBoolean + expr registrations).
targets:
  - ../../expr/boolean.go
---

# BlBoolean — the `boolean` type

`boolean` has two values, `true` and `false`, and participates in **three-valued logic** where
`null` propagates through logical operations (SQL-style). The Go value type backing it is
`BlBoolean`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and operator precedence.

---

## Literals

A **boolean literal** is the syntactic form used inside a blkit expression to write a constant
boolean value — for example, the `true` in `if approved then true else false`. There are exactly
two:

```
true      // → true
false     // → false
```

Case-sensitive: `True`/`TRUE` are not boolean literals.

`[@test] ../../expr/boolean_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `and` | logical and (three-valued) | `true and false` | `false` |
| `or` | logical or (three-valued) | `true or false` | `true` |
| `not(x)` | logical negation | `not(true)` | `false` |
| `=` `!=` | equality | `true = true` | `true` |

There is **no truthy/falsy coercion**: non-boolean operands to `and`/`or`/`not` evaluate to `null`.

### Three-valued logic

| `a` | `b` | `a and b` | `a or b` |
|---|---|---|---|
| `true` | `true` | `true` | `true` |
| `true` | `false` | `false` | `true` |
| `true` | `null` | `null` | `true` |
| `false` | `false` | `false` | `false` |
| `false` | `null` | `false` | `null` |
| `null` | `null` | `null` | `null` |

Short-circuits: `false and null → false`, `true or null → true`. `not(null) → null`. Equality with
`null` is `false`, never `null` (see [null.spec.md](null.spec.md)).

`[@test] ../../expr/boolean_logic_test.go`

---

## Built-in functions

| Function | Example | Result |
|---|---|---|
| `not(b)` | `not(2 = 4)` | `true` |
| `isDefined(value)` | `isDefined(null)` | `true` (the value exists; it is null) |
| `getOrElse(value, default)` | `getOrElse(null, 1)` | `1` |

`[@test] ../../expr/boolean_functions_test.go`

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `BlBoolean.TRUE` / `BlBoolean.FALSE` | literals `true` / `false` |
| `and` / `or` (inherited) | `and` / `or` operators |
| `not_` (inherited) | `not(x)` operator/built-in |
| `equals` / `notEqual` | `=` / `!=` |
| `toNativeBoolean` / `String` | Go host accessors on `BlBoolean` (below) |

---

## Go implementation (expr extension)

Lives in `expr/boolean.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
// BlBoolean wraps a Go bool. (BlNull, not a third enum value, models "unknown".)
type BlBoolean struct{ b bool }

func (BlBoolean) Type() BlType { return BlTypeBoolean }
func (b BlBoolean) Equal(other BlValue) BlValue
func (b BlBoolean) ToMarkdown() string
func (BlBoolean) isBlValue() {}

func Boolean(b bool) BlBoolean      // host constructor
func (b BlBoolean) ToNativeBool() bool
func (b BlBoolean) String() string  // "true" / "false"
```

### Logic & registrations

**Logic.** The three-valued connectives are plain Go functions defined in this spoke's target
(`expr/boolean.go`):

```go
func blAnd(a, b BlValue) BlValue   // three-valued; false short-circuits to false
func blOr(a, b BlValue) BlValue    // three-valued; true short-circuits to true
func blNot(x BlValue) BlValue      // not(null) → null
```

`and`/`or` **cannot** be wired with `expr.Operator` — that overloads binary arithmetic/comparison,
not the short-circuit logical operators, and our operands are wrapped `Bl*` values (possibly
`BlNull`) rather than Go `bool`. Instead the engine's AST patcher
([bl-expr.spec.md](bl-expr.spec.md#patchers-ast-rewriting)) rewrites `a and b` / `a or b` into
`blAnd(a, b)` / `blOr(a, b)`. `not` is exposed to source as the `expr.Function` `not`, backed by
`blNot`. All three implement the three-valued table above; a non-boolean operand → `BlNull`.

**Registrations (`booleanOptions`, unexported).**

```go
func booleanOptions() []expr.Option {
    return []expr.Option{
        expr.Function("blAnd", typed2(blAnd), new(func(BlValue, BlValue) BlValue)), // patcher targets
        expr.Function("blOr",  typed2(blOr),  new(func(BlValue, BlValue) BlValue)),
        expr.Function("not",       typed1(blNot),       new(func(BlValue) BlValue)),
        expr.Function("isDefined", typed1(isDefinedFn), new(func(BlValue) BlBoolean)),
        expr.Function("getOrElse", typed2(getOrElseFn), new(func(BlValue, BlValue) BlValue)),
    }
}
```

`blAnd`/`blOr` are the patcher's rewrite targets for `and`/`or` (not operator-bound — see above);
`not` is the source-callable function. Native Go `bool` inputs wrap to `BlBoolean`.

`[@test] ../../expr/boolean_test.go`

---

## Edge cases

- No truthy/falsy coercion; logical ops on non-booleans → `null`.
- `true`/`false` are case-sensitive.
- Equality against `null` → `false` (never `null`).
