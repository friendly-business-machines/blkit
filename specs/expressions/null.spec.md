---
name: BlNull
description: The null type in the blkit expression language — a fieldless value type meaning absence or unknown, with SQL-style propagation. Covers the null literal, the equality / instance-of operators, propagation semantics, the null-handling built-ins, and the Go layer (BlNull + expr registrations).
targets:
  - ../../expr/null.go
---

# BlNull — the `null` type

`null` represents the absence of a value or an unknown. It is the result of missing dictionary
keys, out-of-range list access, division by zero, an arithmetic / path expression whose operand
was already null, and any other operation whose normal result is undefined. The Go value type
backing it is `BlNull` — a fieldless struct, so every value of it is structurally identical;
the host constructor `Null()` returns one such value, and every "produces null" path in the
engine returns an equivalent one.

See [bl-expr.spec.md](bl-expr.spec.md) for engine internals and three-valued logic.

---

## Literals

The **null literal** is the syntactic form used inside a blkit expression to write the null
value — for example, the `null` in `if x = null then "missing" else x`. It is the only literal
of this type; the canonical form is lowercase, but the parser accepts any casing:

```
// expression-language
null, Null, NULL                   // all parse to the null value
```

Lowercase is canonical — `BlNull.String()` always emits `"null"`, and the recommended style for
hand-written expressions is lowercase. The mixed-case forms are accepted to ease porting from
SQL-style dialects (where `NULL` is the conventional casing) and to remove a small foot-gun for
users who type `Null` out of habit; they do not introduce additional null values. Because case
variants are reserved as the null literal, a variable named `Null`, `NULL`, etc. is **not**
addressable — host input keys that match any casing of `null` produce a `BlParseError` at
expression compile time.

This parallels the case-insensitive-input / lowercase-canonical rule for `true` / `false`
([boolean.spec.md § Literals](boolean.spec.md#literals)).

`[@test] ../../expr/null_test.go`

---

## Construction (host-side)

Host Go code constructs a `BlNull` via the `Null()` function, matching the same-name-minus-`Bl`
convention used by every other `Bl*` constructor (`Number(v)`, `String(v)`, `Boolean(b)`,
`List(items...)`, `Dictionary(m)`, `Date(...)`, etc.). `Null()` is infallible — there's no
input shape that can fail — and the returned `BlNull` carries no state, so every call is
structurally equivalent.

```go
// host-side (Go)
// Build a null directly with the Null() constructor — useful when wiring it into another
// host-built BlValue.
var middleName = Null()

// Typical use: embed it in a BlDictionary for a known-absent field. The Dictionary(...)
// constructor takes a map[string]BlValue, so the value must already be a BlValue — an
// explicit Null() entry stands in for "this field exists but has no value."
var applicant, _ = Dictionary(map[string]BlValue{
    "applicant_name": String("Alice"),
    "applicant_age":  Number(30),
    "middle_name":    Null(),
})
```

The same `BlNull` value can also reach the engine **via the input bridge** when the host
passes a raw `map[string]any` straight to `BlExpr.Evaluate` — no explicit `Null()` call is
needed at that layer because the bridge resolves several "absence" shapes to `BlNull`
automatically:

- A Go `nil` value in the input map → `BlNull`.
- A key absent from the input map → `BlNull` on access.
- An untyped JSON `null` (from `json.Unmarshal` into `map[string]any`) → `BlNull`.
- A typed Go `*T` whose value is `nil` → `BlNull` (the bridge dereferences pointers).

Call `Null()` explicitly when the host wants the call site to make "this value is
intentionally absent" visible at a glance, especially when constructing a `BlDictionary` /
`BlList` / other `Bl*` value host-side (where the slot must be a `BlValue`); rely on the
bridge's `nil` / missing-key handling when the absence comes naturally from upstream data
(`json.Unmarshal` results, optional fields, etc.). Either path lands on a structurally
identical `BlNull`. To **test** whether an arbitrary `BlValue` is null, use the `IsNull()`
accessor declared on the interface — `v.IsNull()` reads more naturally than a type-assertion
against `BlNull`.

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `=` `!=` | equality (SQL-style — null is never equal to anything, including itself) | `null = null` / `null != null` | `false` / `false` |
| `instance of` | type test | `null instance of null` | `true` |

Null has no arithmetic operators (`+` / `-` / etc.), no ordering operators (`<` / `<=` / `>` /
`>=`), and no `in` operator. Equality (`=` / `!=`) dispatches through the `BlValue.Equal()`
interface method (see [§ Value type & host API](#value-type--host-api-exported)) — `Equal()` on
`BlNull` always returns `BlBoolean(false)`, even against another `BlNull`, which is the
SQL-three-valued-logic convention. Use `instance of null`, `isNull(x)`, or `IsNull()` (Go) to
test for null instead.

Operators on **other** types (numeric addition, string concatenation, path access, etc.) that
encounter a null operand follow the propagation rules in [§ Semantics &
behaviour](#semantics--behaviour) — most return null, with documented exceptions for the
short-circuit boolean cases.

`[@test] ../../expr/null_operators_test.go`

---

## Built-in functions

Null-handling extensions (**ext** — neither in DMN, but standard FEEL has `is null` as the
sibling form).

| Function | Example | Result |
|---|---|---|
| `isNull(value)` **ext** | `isNull(null)` | `true` (`isNull(0)` → `false`) |
| `getOrElse(value, default)` **ext** | `getOrElse(null, 1)` | `1` (`getOrElse(42, 1)` → `42`) |

`isNull(x)` is the canonical test for null and is equivalent to `x instance of null` — both
forms are accepted. Prefer `isNull` for brevity; `instance of null` reads as a generic type
test alongside other `instance of` calls in the same expression.

`getOrElse(value, default)` returns `default` when `value` is `BlNull`, otherwise returns
`value` unchanged. It is the canonical null fallback — preferred over an explicit
`if isNull(x) then d else x`, which is more verbose and re-evaluates `x` twice. It accepts any
`BlValue` for both arguments and only fires on `BlNull` — a defined-but-empty value (the empty
string `""`, the empty list `[]`, the zero number `0`) is returned as-is, **not** treated as
null.

```
// expression-language
isNull(applicant.middleName)         // → true if the key is missing
isNull(0)                            // → false (zero is a defined value)
isNull("")                           // → false (empty string is a defined value)

getOrElse(null, 1)                   // → 1
getOrElse(42, 1)                     // → 42
getOrElse(applicant.middleName, "")  // → "" if the key is missing or null
```

`[@test] ../../expr/null_functions_test.go`

---

## Semantics & behaviour

### Propagation (SQL-style three-valued)

`null` propagates through arithmetic, concatenation, path access, indexing, comparison, and
most operations — the surrounding expression evaluates to `null` whenever any operand is
`null`. The exceptions are the **short-circuit boolean** cases, where the SQL three-valued
logic table determines a definite result without consulting the null operand
([boolean.spec.md § Three-valued logic](boolean.spec.md#three-valued-logic)).

| Operation | Result |
|---|---|
| `null + 1`, `null * 2`, `null - 1`, `null / 1` | `null` |
| `null + "x"` (string concatenation) | `null` |
| `null < 5`, `null = 5`, `null != 5`, `null <= 5` (comparison with non-null) | `null` for ordering, `false` for `=` / `!=` |
| `someDictionary.missingKey` (path access) | `null` |
| `[1,2][9]` (out-of-range index) | `null` |
| `null = null` / `null != null` | `false` / `false` (SQL-style: null is never equal to anything) |
| `null instance of null` | `true` |
| `null instance of number` | `false` |
| `true and null` / `null and true` | `null` |
| `false and null` / `null and false` | `false` (short-circuit) |
| `true or null` / `null or true` | `true` (short-circuit) |
| `false or null` / `null or false` | `null` |
| `not(null)` | `null` |

`[@test] ../../expr/null_propagation_test.go`

### Producing null

The engine produces `null` in the following situations:

- A missing dictionary key (`d.absent`, `d["absent"]`, `getValue(d, "absent")`).
- An out-of-range list index (`[1, 2][9]`, including negative-from-end indices that fall off
  the start of the list).
- A wrong-kind list-projection target (handled per type — see the cross-kind rules in each
  type's spec).
- An arithmetic operation with a `null` operand (per the table above).
- Division by zero (`1 / 0`, `5 / 0.0`).
- Numeric operations whose result is undefined: `sqrt` of a negative number, `ln` / `log` of
  zero-or-negative, `**` whose result would be complex
  ([number.spec.md](number.spec.md)).
- A `BlDate` / `BlDateTime` comparison or subtraction that mixes naive and zoned operands
  (see the comparison-semantics sections of [date.spec.md](date.spec.md) and
  [datetime.spec.md](datetime.spec.md)).
- A range query (`contains`, `overlaps`, `entriesFor`, `entriesIn`) where the per-entry
  temporal kind doesn't match the query argument (see [calendar.spec.md § Cross-kind
  matching](calendar.spec.md#cross-kind-matching)).
- Any path expression whose receiver evaluates to `null` (`null.foo`, `null[0]`).

### Testing for null

Three idiomatic tests; all return the same answer:

| Form | Where it's used |
|---|---|
| `x instance of null` | inside expressions; reads naturally alongside other `instance of` calls |
| `isNull(x)` **ext** | inside expressions; the briefer form, preferred in most code |
| `IsNull() bool` (Go) | host code consuming an evaluated `BlValue` |

**Do not use `x = null`** — equality with null is always `false`, so `if x = null then …`
never matches.

```
// expression-language
isNull(someDictionary.missingKey)    // → true
```

```go
// host-side (Go)
var v, _ = expr.Evaluate(input)
if v.IsNull() {
    // handle absent / unknown
}
```

---

## Go implementation (expr extension)

`BlNull` and the shared null helpers live in `expr/value.go` (alongside the `BlValue` interface
itself — see [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)). The
type has no `expr/null.go` of its own because `BlNull` is trivial enough to live with the
interface it supports; the registrations target file is `expr/null.go` only for symmetry with
the other spokes.

### Value type & host API (exported)

`BlNull` is the immutable Go value type that represents the absence-or-unknown value inside
the engine and at the host-code boundary. The struct has no fields — `BlNull` is a zero-sized
type, so every value of it is structurally identical and equality on the Go side is trivial.
The package exposes a constructor `Null()` that returns a fresh (and therefore
indistinguishable) `BlNull`, matching the same-name-minus-`Bl` constructor convention used by
every other `Bl*` type.

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal(other)` always returns `BlBoolean(false)`, even when `other` is also
  `BlNull` — this is the SQL-style three-valued-logic rule, surfaced through the engine's
  single equality dispatch path so callers don't need to special-case null. `String()`
  doubles as the `fmt.Stringer` implementation, producing `"null"`.
- **`Null()`** — the host constructor. Infallible; takes no arguments; returns a `BlNull`.
  Because `BlNull` is fieldless, the returned value is structurally identical to every other
  `BlNull` ever produced — by the constructor, by the engine, or via the bridge. Host code
  should call `Null()` rather than literal-construct `BlNull{}`, so that the call-site intent
  is uniform with `Number(30)` / `String("x")` / etc.
- **`IsNull() bool` accessor** — a convenience method declared on every `BlValue` (in
  `expr/value.go`) so host code can write `v.IsNull()` against any evaluated result without
  type-asserting first. For `BlNull` it returns `true`; for every other concrete `Bl*` type
  it returns `false`. Prefer this over comparing a `BlValue` against `Null()` directly — the
  interface method works without a concrete-type assertion.

```go
// host-side (Go)
// BlNull is the absence-or-unknown value. The struct has no fields, so every BlNull is
// structurally identical to every other BlNull.
type BlNull struct{}

// BlValue interface — required by all Bl* value types.
func (BlNull) Type() BlType { return BlTypeNull }
func (BlNull) Equal(other BlValue) BlValue   // always BlBoolean(false), even vs another BlNull
func (BlNull) String() string                // "null"
func (BlNull) isBlValue() {}

// Host constructor — matches the Number(...) / String(...) / Boolean(...) convention.
func Null() BlNull

// IsNull is declared on every BlValue (in expr/value.go); for BlNull it returns true.
func (BlNull) IsNull() bool                  // always true for BlNull; always false for other Bl* types
```

### Backing implementations (unexported, suffix `Fn`)

Null has **no per-type operator implementation functions**. Equality (`=` / `!=`) dispatches
through the `BlValue.Equal()` interface method (see [§ Value type & host
API](#value-type--host-api-exported)), and `instance of` is handled by the engine's central
type-test patcher. Null has no arithmetic, ordering, or `in` operators.

The library functions are implemented as these typed Go functions, wrapped by `typed1` /
`typed2` at registration time. A shared `propagatesNull` helper is exposed for use by other
spokes' operator/library impls so that the "any operand is null → return null" rule has a
single source of truth.

```go
// host-side (Go)
// Backing impls (unexported, suffix Fn).
func isNullFn(v BlValue) BlBoolean                     // true iff v is BlNull
func getOrElseFn(v BlValue, fallback BlValue) BlValue  // fallback when v is BlNull, else v

// Shared propagation helper — used by other spokes' impls to short-circuit to a BlNull when
// any operand is BlNull. The short-circuit boolean impls in expr/boolean.go intentionally do
// NOT call this — they implement the three-valued table directly.
func propagatesNull(args ...BlValue) bool              // returns true iff any arg is BlNull
```

The propagation helper has no expression-language surface — it's a Go-internal convenience
for operator impls to keep their `if propagatesNull(a, b) { return Null() }` guard
consistent. The engine bridge maps Go `nil` and absent input-map keys to a `BlNull` at the
input boundary, so operator impls never see a Go-level `nil` `BlValue`.

### Registrations (`nullOptions`, unexported — all ext)

`nullOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the null library. Each entry is built with
`expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions.
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2`
  adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlValue) BlBoolean` into that shape.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost.

```go
// host-side (Go)
func nullOptions() []expr.Option {
    return []expr.Option{
        expr.Function("isNull",    typed1(isNullFn),    new(func(BlValue) BlBoolean)),         // ext
        expr.Function("getOrElse", typed2(getOrElseFn), new(func(BlValue, BlValue) BlValue)),  // ext
    }
}
```

Every operation in the engine that produces null returns a `BlNull` (via `Null()` or
literal `BlNull{}` — they're indistinguishable). The engine bridge maps Go `nil` and absent
input-map keys to a `BlNull` at the input boundary; `instance of null` and `isNull(x)` test
for it; every `BlValue` exposes `IsNull()` for host callers.

`[@test] ../../expr/null_test.go`

---

## Edge cases

- The null literal is case-insensitive on input (`null`, `Null`, `NULL` all parse to the same
  value); lowercase is canonical on output. Host input keys that match any casing of `null`
  are rejected at compile time.
- `null = null` and `null != null` are both `false` (SQL-style: null is never equal to
  anything, including itself).
- `null instance of null` → `true`; `null instance of <any-other-type>` → `false`. The type
  name for `instance of` is `null` lowercase (consistent with the lowercase type-name
  convention used by `instance of` across the language — see [bl-expr.spec.md § Type
  checking](bl-expr.spec.md#type-checking-instance-of)).
- `getOrElse` only fires on `BlNull` — a defined-but-empty value (`""`, `[]`, `0`, `false`) is
  returned as-is, not treated as null. Callers wanting "treat falsy as missing" semantics
  must compose their own predicate (e.g. `if isEmpty(x) or isNull(x) then default else x`).
- Writing `null` to a dictionary key whose data contract marks it required →
  `DataContractValidationError` at write time (see [data-contract.spec.md](../data/data-contract.spec.md)).
- The engine bridge maps Go `nil`, absent input-map keys, and untyped JSON `null` to a
  `BlNull` at the input boundary, so operator impls never see a Go-level `nil` `BlValue` at
  runtime.
- Short-circuit boolean operators are the only operators that **don't** propagate null —
  `false and X` and `true or X` evaluate to the definite answer without consulting `X`, even
  when `X` is `null`. All other operators propagate null per
  [§ Propagation](#propagation-sql-style-three-valued).
