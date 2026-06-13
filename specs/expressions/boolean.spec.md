---
name: bl.BlBoolean
description: The boolean type in the blkit expression language — true/false with three-valued (null-propagating) logic. Covers boolean literals, the and/or/not operators, the boolean built-ins, and the Go layer (bl.BlBoolean + expr registrations).
targets:
  - ../../expr_boolean.go
---

# bl.BlBoolean — the `boolean` type

`boolean` has two values, `true` and `false`, and participates in **three-valued logic** where
`null` propagates through logical operations (SQL-style — `null` represents "unknown" rather than
being a third boolean value). The Go value type backing it is `bl.BlBoolean`.

See [bl-expr.spec.md](bl-expr.spec.md) for engine internals and operator precedence.

---

## Literals

A **boolean literal** is the syntactic form used inside a blkit expression to write a constant
boolean value — for example, the `true` in `if approved then true else false`. There are exactly
two literals; the canonical form is lowercase, but the parser accepts any casing:

```
// expression-language
true, True, TRUE                   // → true
false, False, FALSE                // → false
```

Lowercase is canonical — `bl.BlBoolean.String()` always emits `"true"` / `"false"`, and the
recommended style for hand-written expressions is lowercase. The mixed-case forms are accepted to
ease porting from SQL-style dialects and to remove a small foot-gun for users who type `True`
out of habit; they do not introduce a third or fourth literal value. Because case variants are
reserved as boolean literals, a variable named `True`, `TRUE`, etc. is **not** addressable — host
input keys that collide with a boolean literal in any casing produce a `bl.ParseError` at
expression compile time.

`[@test] ../../expr_boolean_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlBoolean` via the generic `Boolean[T BooleanInput](v T)
(bl.BlBoolean, error)` constructor. `BooleanInput` accepts a Go `bool` (direct), every Go integer
type (`int`, `int8`–`int64`, `uint`, `uint8`–`uint64` — `0` → `false`, any non-zero → `true`,
matching the C convention), or a `string` (case-insensitive `"true"` / `"false"`, mirroring
the case-insensitive literal-parsing rule in [§ Literals](#literals)). `bool` and integer
inputs are infallible; the `error` return only fires for a `string` whose value is not a
recognised boolean literal in any casing.

```go
// host-side (Go)
// Direct construction from a Go bool — infallible.
var approved, _ = bl.Boolean(true)
var rejected, _ = bl.Boolean(false)

// From an integer — C convention: 0 → false, non-zero → true. Infallible.
var flag,  _ = bl.Boolean(1)               // → bl.BlBoolean(true)
var noFlag, _ = bl.Boolean(0)              // → bl.BlBoolean(false)

// From a string — case-insensitive; an unrecognised string returns an error.
var fromConf,  _ = bl.Boolean("true")      // → bl.BlBoolean(true)
var fromMixed, _ = bl.Boolean("True")      // → bl.BlBoolean(true)   (case-insensitive)
var fromSQL,   _ = bl.Boolean("FALSE")     // → bl.BlBoolean(false)
var bad,   err   = bl.Boolean("yes")       // err != nil — "yes" is not a recognised literal

// To model an unknown boolean from host code, pass bl.Null() rather than constructing a bl.BlBoolean.
var hasConsent bl.BlValue
if maybeConsent != nil {
    hasConsent, _ = bl.Boolean(*maybeConsent)
} else {
    hasConsent = bl.Null()
}
```

`bl.Boolean(...)` returns `(bl.BlBoolean, error)`. Failure mode: a `string` input that doesn't
match a case-variant of `"true"` or `"false"` — `bl.Boolean("yes")`, `bl.Boolean("1")`, and
`bl.Boolean("")` all error. Integer-shaped strings like `"1"` and `"0"` are intentionally
rejected so the string path mirrors the language's literal-parsing rule; convert the string
to an integer first if you want `0` / non-zero coercion. For details on how unknown booleans
propagate through three-valued logic, see [§ Three-valued logic](#three-valued-logic).

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `and` | logical and (three-valued) | `true and false` | `false` |
| `or` | logical or (three-valued) | `true or false` | `true` |
| `not(x)` | logical negation (function form) | `not(true)` | `false` |
| `=` `!=` | equality | `true = true` | `true` |

Booleans have **no truthy/falsy coercion**: a non-boolean operand to `and` / `or` / `not` produces
`null` rather than being converted. Booleans have no arithmetic operators (`+`/`-`/etc.), no
ordering operators (`<`/`<=`/`>`/`>=`), and no `in` operator.

`and` and `or` are **patcher-lowered** to internal function calls (`blAnd` / `blOr`) rather than
bound through `expr.Operator`, because their three-valued behaviour can't be expressed as an
overload of a Go boolean operator — see
[§ Logical operator implementations](#logical-operator-implementations-unexported).

### Three-valued logic

`null` represents an unknown boolean — propagation matches SQL semantics:

| `a` | `b` | `a and b` | `a or b` |
|---|---|---|---|
| `true`  | `true`  | `true`  | `true`  |
| `true`  | `false` | `false` | `true`  |
| `true`  | `null`  | `null`  | `true`  |
| `false` | `true`  | `false` | `true`  |
| `false` | `false` | `false` | `false` |
| `false` | `null`  | `false` | `null`  |
| `null`  | `true`  | `null`  | `true`  |
| `null`  | `false` | `false` | `null`  |
| `null`  | `null`  | `null`  | `null`  |

Short-circuits: `false and X → false` (X never evaluated), `true or X → true`. `not(null) → null`.
Equality with `null` yields `false`, never `null` (see [null.spec.md](null.spec.md)).

`[@test] ../../expr_boolean_test.go`

---

## Built-in functions

| Function | Example | Result |
|---|---|---|
| `not(b)` | `not(2 = 4)` | `true` (logical negation; `not(null) → null`) |

Related null-handling helpers live alongside the type they introspect:
[`isDefined`](bl-expr.spec.md#name-resolution-isdefined) (name-resolution, in
[bl-expr.spec.md](bl-expr.spec.md)) and [`getOrElse`](null.spec.md#built-in-functions) (null
fallback, in [null.spec.md](null.spec.md)).

`[@test] ../../expr_boolean_test.go`

---

## Go implementation (expr extension)

Lives in `expr/boolean.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlBoolean` is the immutable Go value type that represents a boolean inside the engine and at the
host-code boundary. Its only field is private (`b`) so callers cannot mutate the underlying value —
every operation in the library returns a fresh `bl.BlBoolean`. There is **no third "unknown" enum
value**: an unknown boolean is modelled as `bl.BlNull` (see [null.spec.md](null.spec.md)), which
propagates through the three-valued logic table.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` is structural (`true = true`, `false = false`); cross-type equality returns
  `false`, never `null` (see [null.spec.md](null.spec.md)). `bl.String()` doubles as the
  `fmt.Stringer` implementation, producing `"true"` or `"false"`.
- **`Boolean[T BooleanInput](v T)`** — the generic host constructor. The `BooleanInput`
  constraint accepts `bool` (direct), every Go integer type (`int`, `int8`–`int64`, `uint`,
  `uint8`–`uint64` — `0` → `false`, any non-zero → `true`), and `string` (case-insensitive
  `"true"` / `"false"` — see [§ Literals](#literals) for the matching case-insensitive parsing
  rule used for source-text literals). All other Go types are rejected at compile time. The
  `error` return only fires at runtime for a `string` whose value is not a recognised boolean
  literal in any casing; integer and `bool` inputs are infallible. To model an unknown boolean
  from host code, pass `bl.Null()` (not a `*bool`) — see
  [null.spec.md § Construction (host-side)](null.spec.md#construction-host-side).
- **`Native()` accessor** — hands the underlying Go `bool` back to host code. From there, Go's
  native `&&` / `||` / `!` are all that's needed.

```go
// host-side (Go)
// bl.BlBoolean wraps a native Go bool. bl.BlNull, not a third enum value, models "unknown".
type BlBoolean struct{ b bool }

// bl.BlValue interface — required by all Bl* value types.
func (BlBoolean) Type() Type { return TypeBoolean }
func (b BlBoolean) Equal(other BlValue) BlValue
func (b BlBoolean) String() string                // "true" / "false"
func (BlBoolean) isBlValue() {}

// Host constructor — accepts bool, any Go integer type, or a case-insensitive "true"/"false" string.
// Integer inputs use the C convention: 0 → false, any non-zero → true.
// String inputs match the case-insensitive boolean literal parser (see § Literals).
type BooleanInput interface {
    bool |
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    string
}
func Boolean[T BooleanInput](v T) (BlBoolean, error)

// Host accessor (consume an evaluated result).
func (b BlBoolean) Native() bool                  // underlying Go bool
```

### Logical operator implementations (unexported)

`and` and `or` cannot be wired with `expr.Operator` — that mechanism overloads binary arithmetic /
comparison operators, not the short-circuit logical operators, and our operands are wrapped
`bl.BlValue`s (possibly `bl.BlNull`) rather than Go `bool`s. Instead, the engine's AST patcher (see
[bl-expr.spec.md § Patchers](bl-expr.spec.md#patchers-expr_patchgo)) lowers `a and b` and `a or b`
to a **lazy conditional**, *not* a function call — a function call would evaluate both operands and
defeat short-circuiting. The lowering binds the left operand once and is:

- `a and b` → `if a == false then false else blAnd(a, b)`
- `a or b` → `if a == true then true else blOr(a, b)`

The second operand sits in the else-branch, so it — and the helper call — run **only** when the
left operand doesn't already decide the result (`false and X` / `true or X` never evaluate `X`).
The guard fires only for a genuine `false` / `true`; a null left operand falls through (`null ==
false` → `false`) and evaluates the right operand, preserving the three-valued table above. `blAnd`
/ `blOr` are the three-valued truth-table **helpers** the else-branch delegates to once both
operands are known (a non-boolean operand → `bl.BlNull`); they do not themselves short-circuit.

`not` is different: the source-level form is already a function call (`not(x)`), so it's registered
directly under its plain name as an `expr.Function` rather than going through the patcher.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl — it dispatches through the
`bl.BlValue.Equal()` interface method (see [§ Value type & host
API](#value-type--host-api-exported)). That single dispatch path handles null propagation and
cross-type comparison uniformly.

```go
// host-side (Go)
// Truth-table helpers — registered under names blAnd / blOr; invoked from the else-branch of
// the and/or conditional lowering (so they only run for the non-short-circuit cases). Not
// source-callable. Three-valued; a non-boolean operand → BlNull. They do NOT short-circuit —
// the patcher's conditional owns evaluation order.
func blAndFn(a, b BlValue) BlValue
func blOrFn(a, b BlValue) BlValue

// Source-callable — registered under name "not".
func notFn(x BlValue) BlValue        // not(null) → null; non-boolean → null
```

### Registrations (`booleanOptions`, unexported)

`booleanOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the boolean library. Each entry is built with
`expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (for `not`), or that the
  patcher emits when lowering `and` / `or` (`blAnd`, `blOr`).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` adapters
  (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)) wrap a
  typed implementation such as `func(bl.BlValue, bl.BlValue) bl.BlValue` into that shape, type-asserting
  each argument and boxing the result.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost.

The registrations are grouped by role: the patcher targets (consumed by the AST patcher when
lowering `and` / `or`) and the source-callable functions.

```go
// host-side (Go)
func booleanOptions() []expr.Option {
    return []expr.Option{
        // patcher targets — emitted when the AST patcher rewrites `a and b` / `a or b`
        expr.Function("blAnd",     typed2(blAndFn),     new(func(bl.BlValue, bl.BlValue) bl.BlValue)),
        expr.Function("blOr",      typed2(blOrFn),      new(func(bl.BlValue, bl.BlValue) bl.BlValue)),

        // source-callable
        expr.Function("not",       typed1(notFn),       new(func(bl.BlValue) bl.BlValue)),
    }
}
```

Native Go `bool` inputs wrap to `bl.BlBoolean` via the engine's input bridge.

`[@test] ../../expr_boolean_test.go`

---

## Edge cases

- No truthy/falsy coercion — a non-boolean operand to `and` / `or` / `not` → `null`, not a
  coerced boolean.
- Boolean literals are case-insensitive on input (`true`, `True`, `TRUE` all parse to the same value); lowercase is canonical on output. Host input keys that match any casing of `true` / `false` are rejected at compile time.
- Equality against `null` → `false` (never `null`); cross-type equality (`true = 1`) → `false`.
- Short-circuit evaluation: `false and X` and `true or X` never evaluate `X`, so a side-effecting
  or error-producing right-hand operand is skipped.
