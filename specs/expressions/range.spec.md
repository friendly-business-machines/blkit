---
name: BlRange
description: The range (interval) type in the blkit expression language. Covers interval literals and boundary semantics, membership, the interval-algebra built-ins (Allen's algebra), and the Go layer (BlRange + expr registrations).
targets:
  - ../../expr/range.go
---

# BlRange — the `range` type

A `range` is a contiguous interval of comparable values with configurable inclusion at each
endpoint. Ranges drive `in` membership tests, decision-table input entries, and the interval-algebra
built-ins. The Go value type backing it is `BlRange`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals

| Syntax | Start | End | Meaning |
|---|---|---|---|
| `[a..b]` | included | included | `a ≤ x ≤ b` |
| `(a..b)` | excluded | excluded | `a < x < b` |
| `[a..b)` | included | excluded | `a ≤ x < b` |
| `(a..b]` | excluded | included | `a < x ≤ b` |

Ranges work over any comparable type — numbers, strings (code-point order), and temporal values
(chronological):

```
[1..10]                                    // numeric
[date("2025-01-01")..date("2025-12-31"))   // dates, end-exclusive
```

**Unbounded ends** use `null` as an endpoint (`null` start = −∞, `null` end = +∞):

```
[18..null)     // "18 or older"
(null..0)      // "negative"
```

`[@test] ../../expr/range_test.go`

---

## Membership & equality

| Form | Example | Result |
|---|---|---|
| `x in r` | `25 in [18..65]` | `true` |
| `includes(r, x)` | `includes([18..65], 25)` | `true` |
| `=` `!=` | `[1..5] = [1..5]` | `true` |

The value and endpoints must be comparable; an incompatible type → `BlTypeError`. A closed boundary
with a `null` endpoint → `BlTypeError`.

`[@test] ../../expr/range_membership_test.go`

---

## Boundary access

Components are read with the dot operator ([bl-expr.spec.md](bl-expr.spec.md#accessing-components)):

```
[1..10].start            // → 1
[1..10).end              // → 10
[1..10).endIncluded      // → false
[1..10].startIncluded    // → true
```

---

## Interval algebra (built-ins)

The DMN interval-algebra (Allen's) built-ins relate points and ranges. Arguments may be a point, a
range, or both, per DMN. All return `boolean`.

| Function | Example | Result |
|---|---|---|
| `before(a, b)` | `before([1..5], [6..10])` | `true` |
| `after(a, b)` | `after([6..10], [1..5])` | `true` |
| `meets(a, b)` / `metBy(a, b)` | `meets([1..5], [5..10])` | `true` |
| `overlaps(a, b)` | `overlaps([5..10], [1..6])` | `true` |
| `overlapsBefore(a, b)` / `overlapsAfter(a, b)` | `overlapsBefore([1..5], [4..10])` | `true` |
| `includes(r, point)` / `during(point, r)` | `during(5, [1..10])` | `true` |
| `starts(point, r)` / `startedBy(r, point)` | `starts(1, [1..5])` | `true` |
| `finishes(point, r)` / `finishedBy(r, point)` | `finishes(5, [1..5])` | `true` |
| `coincides(a, b)` | `coincides([1..5], [1..5])` | `true` |
| `isEmpty(r)` **ext** | `isEmpty((3..3))` | `true` |

`[@test] ../../expr/range_algebra_test.go`

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Range(start, end, startIncluded, endIncluded)` | interval literal `[a..b]` / `(a..b)` / `[a..b)` / `(a..b]` |
| `includes(value)` | `x in r` operator / `includes(r, x)` |
| `isEmpty` | `isEmpty(r)` **ext** |
| `equals` / `notEqual` | `=` / `!=` |
| `Start` / `End` / `StartIncluded` / `EndIncluded` | `.start` / `.end` / `.startIncluded` / `.endIncluded` |
| (point/range relations) | `before` / `after` / `meets` / `overlaps` / `during` / `starts` / `finishes` / `coincides` … |
| `String` | Go host accessor (literal notation) |

The legacy point-side interval-algebra methods on `BlNumber`/`BlDate` (`before`, `after`, `during`,
…) map to these same built-ins.

---

## Go implementation (expr extension)

Lives in `expr/range.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
// Either endpoint may be Null (unbounded).
type BlRange struct{ start, end BlValue; startIncluded, endIncluded bool }

func (BlRange) Type() BlType { return BlTypeRange }
func (r BlRange) Equal(other BlValue) BlValue
func (r BlRange) ToMarkdown() string  // "[1..10]", "(0..1)", "[2025-01-01..2025-12-31)"
func (BlRange) isBlValue() {}

func (r BlRange) String() string
```

### Syntax (patcher)

Interval literals (`[a..b]`, `(a..b)`, `[a..b)`, `(a..b]`) and `x in [a..b]` membership are produced
by the range patcher ([bl-expr.spec.md](bl-expr.spec.md#patchers-ast-rewriting)) — `expr` has no
open/closed interval syntax. The patcher emits `newRange(a, b, startIncluded, endIncluded)` for a
literal and lowers `x in r` to `includes(r, x)`.

### Registrations (`rangeOptions`, unexported)

```go
func rangeOptions() []expr.Option {
    return []expr.Option{
        expr.Function("newRange", newRangeFn, new(func(BlValue, BlValue, bool, bool) BlRange)), // patcher target

        // interval algebra — each overloaded over point/range arg combinations:
        expr.Function("includes", includesFn, new(func(BlRange, BlValue) BlBoolean)),
        expr.Function("during",   duringFn,   new(func(BlValue, BlRange) BlBoolean), new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("before",   beforeFn,   new(func(BlValue, BlValue) BlBoolean)),
        expr.Function("after",    afterFn,    new(func(BlValue, BlValue) BlBoolean)),
        expr.Function("meets",    meetsFn,    new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("metBy",    metByFn,    new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("overlaps", overlapsFn, new(func(BlRange, BlRange) BlBoolean)), // calendar overload in calendar.go
        expr.Function("overlapsBefore", overlapsBeforeFn, new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("overlapsAfter",  overlapsAfterFn,  new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("starts",     startsFn,     new(func(BlValue, BlRange) BlBoolean), new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("startedBy",  startedByFn,  new(func(BlRange, BlValue) BlBoolean), new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("finishes",   finishesFn,   new(func(BlValue, BlRange) BlBoolean), new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("finishedBy", finishedByFn, new(func(BlRange, BlValue) BlBoolean), new(func(BlRange, BlRange) BlBoolean)),
        expr.Function("coincides",  coincidesFn,  new(func(BlValue, BlValue) BlBoolean)),
        expr.Function("isEmpty",    typed1(rangeIsEmptyFn), new(func(BlRange) BlBoolean)), // ext (list/context overloads elsewhere)
    }
}
```

**Operators.** `=`/`!=` via the shared equality funcs. A closed boundary with a `Null` endpoint, or
mixed endpoint types, → `BlTypeError`.

`[@test] ../../expr/range_test.go`

---

## Edge cases

- `start > end` → empty range: `isEmpty` → `true`, membership always `false`.
- `(3..3)` is empty; `[3..3]` contains exactly one value.
- Closed boundary with a `null` endpoint → `BlTypeError`.
- Mixed endpoint types (e.g. date vs datetime) → `BlTypeError`.
