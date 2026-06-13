---
name: bl.BlRange
description: The range (interval) type in the blkit expression language. Covers interval literals and boundary semantics, membership, the interval-algebra built-ins, and the Go layer (bl.BlRange + expr registrations).
targets:
  - ../../expr_range.go
---

# bl.BlRange — the `range` type

A `range` is a contiguous interval of comparable values with configurable inclusion at each
endpoint. Ranges drive `in` membership tests, decision-table input entries, and the interval-algebra
built-ins. The Go value type backing it is `bl.BlRange`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals

A **range literal** is the syntactic form used inside a blkit expression to write a constant
interval value — for example, the `[18..65]` in `age in [18..65]`. Each end is independently
included (`[`/`]`) or excluded (`(`/`)`):

| Syntax | Start | End | Meaning |
|---|---|---|---|
| `[a..b]` | included | included | `a ≤ x ≤ b` |
| `(a..b)` | excluded | excluded | `a < x < b` |
| `[a..b)` | included | excluded | `a ≤ x < b` |
| `(a..b]` | excluded | included | `a < x ≤ b` |

Ranges work over any comparable type — numbers, strings (code-point order), and temporal values
(chronological):

```
// expression-language
[1..10]                                    // numeric
[date("2025-01-01")..date("2025-12-31"))   // dates, end-exclusive
```

**Unbounded ends** use `null` as an endpoint (`null` start = −∞, `null` end = +∞):

```
// expression-language
[18..null)     // "18 or older"
(null..0)      // "negative"
```

`[@test] ../../expr_range_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlRange` via the `bl.Range(start, end bl.BlValue, startIncluded,
endIncluded bool) (bl.BlRange, error)` constructor. The endpoints may be any comparable
`bl.BlValue` (numbers, strings, temporal values) or `bl.Null()` for unbounded sides. A closed
boundary paired with an unbounded endpoint, or cross-type endpoints, returns an error.

```go
// host-side (Go)
// Closed-closed numeric range.
var adultAges, _ = bl.Range(bl.Number(18), bl.Number(120), true, true)

// Half-open (closed-open) — typical "from X up to but not including Y".
var quarter, _ = bl.Range(bl.Number(0), bl.Number(0.25), true, false)

// Unbounded upper end — use bl.Null() for the open side and false for inclusion.
var workingAge, _ = bl.Range(bl.Number(18), bl.Null(), true, false)

// Date range — endpoints must be the same type.
var d1, _   = bl.Date("2025-01-01")
var d2, _   = bl.Date("2025-12-31")
var yr2025, _ = bl.Range(d1, d2, true, true)
```

`bl.Range(...)` returns `(bl.BlRange, error)`. Error cases: cross-type endpoints (e.g. `Number`
start with `Date` end), a closed boundary on an unbounded endpoint (e.g. `bl.Range(bl.Number(5),
bl.Null(), true, true)`), and non-comparable endpoint types. See [§ Edge cases](#edge-cases) for
the full list and [§ Empty-range semantics](#empty-range-semantics) for what happens when
`start > end` is technically valid construction but produces an empty range.

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `x in r` | membership (point in range) | `25 in [18..65]` | `true`. `7 in [5..3]` → `null` (empty range — see [§ Empty-range semantics](#empty-range-semantics)) |
| `=` `!=` | equality (structural) | `[1..5] = [1..5]` | `true`. Structural — two empty ranges with the same endpoints/inclusion compare equal. |

The value and endpoints must be comparable; an incompatible type → `bl.TypeError`. A closed boundary
with a `null` endpoint → `bl.TypeError`.

`in` has an equivalent function form, `includes(r, x)`, documented under
[§ Interval algebra](#interval-algebra-built-ins).

`[@test] ../../expr_range_test.go`

---

## Component access

Components are read with the dot operator ([bl-expr.spec.md](bl-expr.spec.md#accessing-components)):

| Accessor | Example | Result |
|---|---|---|
| `.start` | `[1..10].start` | `1` (or `null` for unbounded) |
| `.end` | `[1..10).end` | `10` (or `null` for unbounded) |
| `.startIncluded` | `[1..10].startIncluded` | `true` |
| `.endIncluded` | `[1..10).endIncluded` | `false` |

`[@test] ../../expr_range_test.go`

---

## Interval algebra (built-ins)

The interval-algebra built-ins express relationships between intervals (and between intervals
and points). Their names and shapes are inspired by DMN FEEL's interval-algebra functions
(blkit is DMN-inspired, not DMN-compliant — see [bl-expr.spec.md](bl-expr.spec.md); behaviour
around nulls and empty ranges in particular diverges from the FEEL spec). Arguments may be a
point, a range, or both, depending on the function. All return `boolean`, except when any
range argument is **empty** — in that case the result is `null` (see
[§ Empty-range semantics](#empty-range-semantics)). `isEmpty` itself is the exception: it
returns `true` for an empty range (that's its purpose).

| Function | Example | Result |
|---|---|---|
| `before(a, b)` | `before([1..5], [6..10])` | `true` (a ends before b begins) |
| `after(a, b)` | `after([6..10], [1..5])` | `true` (a begins after b ends) |
| `meets(a, b)` | `meets([1..5], [5..10])` | `true` (a ends exactly where b begins) |
| `metBy(a, b)` | `metBy([5..10], [1..5])` | `true` (b ends exactly where a begins; inverse of `meets`) |
| `overlaps(a, b)` | `overlaps([5..10], [1..6])` | `true` (any non-empty intersection) |
| `overlapsBefore(a, b)` | `overlapsBefore([1..5], [4..10])` | `true` (a starts before b and they overlap) |
| `overlapsAfter(a, b)` | `overlapsAfter([4..10], [1..5])` | `true` (a starts after b and they overlap; inverse of `overlapsBefore`) |
| `includes(r, point)` | `includes([1..10], 5)` | `true` (point is inside r) |
| `during(point, r)` | `during(5, [1..10])` | `true` (point is inside r; inverse-order of `includes`) |
| `starts(point, r)` | `starts(1, [1..5])` | `true` (point is r's start) |
| `startedBy(r, point)` | `startedBy([1..5], 1)` | `true` (r's start is point; inverse-order of `starts`) |
| `finishes(point, r)` | `finishes(5, [1..5])` | `true` (point is r's end) |
| `finishedBy(r, point)` | `finishedBy([1..5], 5)` | `true` (r's end is point; inverse-order of `finishes`) |
| `coincides(a, b)` | `coincides([1..5], [1..5])` | `true` (identical intervals; self-inverse) |
| `isEmpty(r)` **ext** | `isEmpty((3..3))` | `true` (no values in the range) |

`[@test] ../../expr_range_test.go`

---

## Empty-range semantics

A range is **empty** when it contains no values. The two forms:

- **Reversed**: `start > end`, e.g. `[5..3]` — silently accepted, no error.
- **Degenerate exclusive**: same start and end with at least one excluded endpoint —
  `(3..3)`, `[3..3)`, `(3..3]` all contain no values. (`[3..3]` is **not** empty; it contains
  exactly the one value `3`.)

Operations against an empty range follow blkit's broader null-propagation pattern: asking
"how does this value/range relate to a set with no elements?" doesn't have a meaningful
yes/no answer, so the result is `null`.

| Operation | Against empty range | Why |
|---|---|---|
| `x in r` | `null` | Membership against empty has no truth value |
| `includes(r, x)` | `null` | Same as `in` |
| `during(x, r)` | `null` | Same |
| `before(a, b)`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, `starts`, `startedBy`, `finishes`, `finishedBy`, `coincides` | `null` if **either** range argument is empty | Interval-algebra predicates on the empty set are undefined |
| `isEmpty(r)` | `true` | The meta-question; returning null here would defeat its purpose |
| `r1 = r2`, `r1 != r2` | structural | Structural equality is unaffected by emptiness; two `[5..3]` values compare equal, `[5..3]` and `[8..6]` compare unequal |

The distinction matters: `null` (missing data) and an empty range (a well-defined value that
happens to contain nothing) are different concepts. `isEmpty(null)` → `null` (input was
missing); `isEmpty([5..3])` → `true` (input was an empty range).

`[@test] ../../expr_range_test.go`

---

## Go implementation (expr extension)

Lives in `expr/range.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlRange` is the immutable Go value type that represents an interval inside the engine and at
the host-code boundary. It carries two endpoint values (either of which may be `bl.BlNull` for an
unbounded side) plus two booleans recording whether each endpoint is included. All fields are
private so callers cannot mutate the underlying value; every operation in the library returns
a fresh `bl.BlRange`.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares the start and end endpoints and the two inclusion booleans;
  cross-type endpoints return `bl.BlNull`. `bl.String()` doubles as the `fmt.Stringer` implementation,
  producing the canonical literal form (e.g. `"[1..10]"`, `"(0..1)"`, `"[2025-01-01..2025-12-31)"`).
- **`bl.Range(start, end, startIncluded, endIncluded)`** — the host constructor. Endpoints may be
  any comparable `bl.BlValue` (numbers, strings, temporal values) or `bl.BlNull` for unbounded. The
  `error` return fires for endpoint type mismatches (e.g. `bl.BlNumber` start with `bl.BlDate` end),
  or a closed boundary (`startIncluded` or `endIncluded` is `true`) paired with a `bl.BlNull`
  endpoint — `null` can only be combined with an open boundary, since "≤ −∞" or "≥ +∞" is not
  meaningful.
- **Host accessors** — `bl.Start()`, `bl.End()`, `StartIncluded()`, `EndIncluded()` — return the
  individual fields. `bl.Start()` and `bl.End()` return `bl.BlValue` (which may be `bl.BlNull` for an
  unbounded side); the inclusion accessors return Go `bool`.

```go
// host-side (Go)
// Either endpoint may be bl.BlNull (unbounded). Closed boundary with bl.BlNull → bl.TypeError.
type BlRange struct {
    start, end                BlValue
    startIncluded, endIncluded bool
}

// bl.BlValue interface — required by all Bl* value types.
func (BlRange) Type() Type { return TypeRange }
func (r BlRange) Equal(other BlValue) BlValue   // endpoint-wise comparison; cross-type → Null
func (r BlRange) String() string                // canonical literal form
func (BlRange) isBlValue() {}

// Host constructor — endpoints may be any comparable bl.BlValue or bl.BlNull (unbounded).
// Closed boundary with a bl.BlNull endpoint, or cross-type endpoints, → error.
func Range(start, end BlValue, startIncluded, endIncluded bool) (BlRange, error)

// Host accessors (consume an evaluated result).
func (r BlRange) Start() BlValue            // may be BlNull (unbounded)
func (r BlRange) End() BlValue              // may be BlNull (unbounded)
func (r BlRange) StartIncluded() bool
func (r BlRange) EndIncluded() bool
```

### Syntax (patcher)

Interval literals (`[a..b]`, `(a..b)`, `[a..b)`, `(a..b]`) and `x in [a..b]` membership are produced
by the range patcher ([bl-expr.spec.md](bl-expr.spec.md#patchers-expr_patchgo)) — `expr` has no
open/closed interval syntax. The patcher emits `newRange(a, b, startIncluded, endIncluded)` for a
literal and lowers `x in r` to `includes(r, x)`.

### Backing implementations (unexported, suffix `Fn`)

Range has **no per-type operator implementation functions**. Equality (`=` / `!=`) dispatches
through the `bl.BlValue.Equal()` interface method (see [§ Value type & host API](#value-type--host-api-exported)),
and the `in` operator is patcher-lowered to a call to `includes(r, x)` (see
[§ Syntax (patcher)](#syntax-patcher)). Range has no arithmetic operators (`+`/`-`/etc.) and
no ordering operators (`<`/`<=`/etc.) — "which range is less" has no meaningful definition.

The interval-algebra and constructor functions are implemented as these typed/variadic Go
functions, wrapped by `typed1`/`typed2` at registration time:

```go
// host-side (Go)
// Constructor target (called by the range patcher, not by user-written expressions directly).
func newRangeFn(start, end any, startIncluded, endIncluded bool) any   // returns BlRange or TypeError

// Interval-algebra impls — each variadic because different signatures dispatch on arg types
// (point vs range). Return bl.BlValue (bl.BlBoolean on valid inputs; bl.BlNull when any range arg is
// empty — see § Empty-range semantics).
func includesFn(args ...any) (any, error)        // (BlRange, BlValue)
func duringFn(args ...any) (any, error)          // (BlValue, BlRange) | (BlRange, BlRange)
func beforeFn(args ...any) (any, error)          // (BlValue, BlValue) — point or range either side
func afterFn(args ...any) (any, error)           // (BlValue, BlValue)
func meetsFn(args ...any) (any, error)           // (BlRange, BlRange)
func metByFn(args ...any) (any, error)           // (BlRange, BlRange)
func overlapsFn(args ...any) (any, error)        // (BlRange, BlRange)
func overlapsBeforeFn(args ...any) (any, error)  // (BlRange, BlRange)
func overlapsAfterFn(args ...any) (any, error)   // (BlRange, BlRange)
func startsFn(args ...any) (any, error)          // (BlValue, BlRange) | (BlRange, BlRange)
func startedByFn(args ...any) (any, error)       // (BlRange, BlValue) | (BlRange, BlRange)
func finishesFn(args ...any) (any, error)        // (BlValue, BlRange) | (BlRange, BlRange)
func finishedByFn(args ...any) (any, error)      // (BlRange, BlValue) | (BlRange, BlRange)
func coincidesFn(args ...any) (any, error)       // (BlValue, BlValue)
func rangeIsEmptyFn(r BlRange) BlBoolean          // ext; typed1 — list and context overloads live in their own specs
```

A `bl.BlRange` argument with mismatched endpoint types vs the point being compared → `bl.TypeError`.
A `bl.BlRange` with `start > end` (or with degenerate exclusive endpoints like `(3..3)`) is
**empty** — see [§ Empty-range semantics](#empty-range-semantics) for the null-propagation
rules that apply to membership and interval-algebra predicates against empty ranges.

Component accessors (`.start`/`.end`/`.startIncluded`/`.endIncluded`) are resolved by the
component-access patcher described in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)
— they dispatch to internal accessor functions that are not registered as user-callable
`expr.Function`s.

### Registrations (`rangeOptions`, unexported)

`rangeOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the range constructor (called by the patcher) and the
interval-algebra library. Each entry is built with `expr.Function(name, impl, typeHints...)`,
where:

- `name` is the identifier the parser will recognise in expressions (and that the patcher
  emits when lowering interval literals or `in` operators).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` adapter
  (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wraps a typed implementation such as `func(bl.BlRange) bl.BlBoolean` into that shape; the
  interval-algebra impls are variadic because they need to dispatch on multiple
  point-vs-range argument combinations.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures (e.g.
  `during` accepts both `(point, range)` and `(range, range)` forms).

```go
// host-side (Go)
func rangeOptions() []expr.Option {
    return []expr.Option{
        // patcher target — not normally called directly from user expressions
        expr.Function("newRange", newRangeFn, new(func(bl.BlValue, bl.BlValue, bool, bool) bl.BlRange)),

        // interval algebra — each overloaded over point/range arg combinations
        expr.Function("includes",       includesFn,       new(func(bl.BlRange, bl.BlValue) bl.BlValue)),
        expr.Function("during",         duringFn,         new(func(bl.BlValue, bl.BlRange) bl.BlValue),
                                                          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("before",         beforeFn,         new(func(bl.BlValue, bl.BlValue) bl.BlValue)),
        expr.Function("after",          afterFn,          new(func(bl.BlValue, bl.BlValue) bl.BlValue)),
        expr.Function("meets",          meetsFn,          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("metBy",          metByFn,          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("overlaps",       overlapsFn,       new(func(bl.BlRange, bl.BlRange) bl.BlValue)), // calendar overload registered in calendar.spec.md
        expr.Function("overlapsBefore", overlapsBeforeFn, new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("overlapsAfter",  overlapsAfterFn,  new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("starts",         startsFn,         new(func(bl.BlValue, bl.BlRange) bl.BlValue),
                                                          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("startedBy",      startedByFn,      new(func(bl.BlRange, bl.BlValue) bl.BlValue),
                                                          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("finishes",       finishesFn,       new(func(bl.BlValue, bl.BlRange) bl.BlValue),
                                                          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("finishedBy",     finishedByFn,     new(func(bl.BlRange, bl.BlValue) bl.BlValue),
                                                          new(func(bl.BlRange, bl.BlRange) bl.BlValue)),
        expr.Function("coincides",      coincidesFn,      new(func(bl.BlValue, bl.BlValue) bl.BlValue)),
        expr.Function("isEmpty",        typed1(rangeIsEmptyFn), new(func(bl.BlRange) bl.BlBoolean)), // ext; list/context overloads in their own specs
    }
}
```

`[@test] ../../expr_range_test.go`

---

## Edge cases

- `start > end` → empty range: `isEmpty` → `true`; membership, inclusion, and interval-algebra
  predicates → `null` (see [§ Empty-range semantics](#empty-range-semantics)).
- `(3..3)` is empty; `[3..3]` contains exactly one value.
- Closed boundary with a `null` endpoint → `bl.TypeError`.
- Mixed endpoint types (e.g. date vs datetime) → `bl.TypeError`.
