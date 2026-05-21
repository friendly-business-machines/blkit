---
name: BlNull
description: The null type in the blkit expression language — a singleton meaning absence/unknown, with SQL-style propagation. Covers the null literal, propagation rules, null testing, and the Go layer (BlNull + expr registrations).
targets:
  - ../../expr/null.go
---

# BlNull — the `null` type

`null` represents the absence of a value or an unknown. It is the result of missing context keys,
out-of-range list access, division by zero, and similar. The Go value type backing it is `BlNull`, a
singleton.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and three-valued logic.

---

## Literal

```
null      // → null
```

`[@test] ../../expr/null_test.go`

---

## Propagation

`null` propagates through arithmetic, concatenation, path access, indexing, and most operations. The
exceptions are the short-circuit boolean cases ([boolean.spec.md](boolean.spec.md)).

| Operation | Result |
|---|---|
| `null + 1` | `null` |
| `null * "x"` | `null` |
| `someContext.missingKey` | `null` |
| `[1,2][9]` | `null` |
| `null = null` | `false` (SQL-style: null is not equal to null) |
| `null != null` | `false` |
| `null instance of null` | `true` |
| `true and null` | `null` |
| `false and null` | `false` |
| `true or null` | `true` |
| `false or null` | `null` |

`[@test] ../../expr/null_propagation_test.go`

---

## Testing for null

- **In an expression:** `x instance of null` → `true` iff `x` is null. The convenience built-in
  `isNull(x)` **ext** returns the same boolean. Do **not** use `x = null` — equality with null is
  always `false`.
- **In host code:** the evaluated `BlValue` exposes `IsNull() bool`.

```
isNull(someContext.missingKey)   // → true
isDefined(someContext.missingKey) // → false   (see boolean.spec.md)
```

`[@test] ../../expr/null_testing_test.go`

---

## Producing null

Division by zero; out-of-range index; missing context key; `sqrt` of a negative; `ln`/`log` of
zero-or-negative; `**` with a complex result; any arithmetic/path expression with a null operand.

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `BlNull.INSTANCE` | literal `null` |
| `isNull` (eager host util) | `isNull(x)` **ext** / `x instance of null` in expressions; `IsNull()` in host code |
| `instanceOf("Null")` | `x instance of null` (type name lowercased — see note) |
| `equals` / `notEqual` | `=` / `!=` (both yield `false` against null) |
| `String` | Go host accessor (`"null"`) |

> **Divergence note.** The type name is lowercased to `null` (was `"Null"`), matching the
> lowercase type-name convention used by `instance of` across the language
> ([bl-expr.spec.md](bl-expr.spec.md)).

---

## Go implementation (expr extension)

`BlNull` and the shared null helpers live in `expr/value.go` (alongside the `BlValue` interface — see
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)).

### Value type & host API (exported)

```go
// BlNull is the singleton null value.
type BlNull struct{}
var Null = BlNull{}

func (BlNull) Type() BlType { return BlTypeNull }
func (BlNull) Equal(other BlValue) BlValue   // always BlBoolean(false), even vs Null
func (BlNull) ToMarkdown() string            // "null"
func (BlNull) isBlValue() {}

func (BlNull) IsNull() bool   // host accessor; always true
func (BlNull) String() string // "null"
```

### Propagation helper & registration

```go
// propagatesNull reports whether any arg is Null; operator/function impls call
// it to short-circuit to Null (except the boolean cases in boolean.spec.md).
func propagatesNull(args ...BlValue) bool

func nullOptions() []expr.Option {
    return []expr.Option{
        expr.Function("isNull", typed1(isNullFn), new(func(BlValue) BlBoolean)), // ext
    }
}
```

All operations that produce null return the `Null` singleton. The engine bridge maps Go `nil` and
absent input keys to `Null`; `instance of null` and `isNull(x)` test for it. Every `BlValue` exposes
`IsNull()` for host callers.

`[@test] ../../expr/null_test.go`

---

## Edge cases

- `null = null` and `null != null` are both `false`.
- Writing `null` to a context key whose contract marks it required → `DataContractValidationError`
  at write time (see data specs).
- Type name for `instance of` is `null` (lowercase).
