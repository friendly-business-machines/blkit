# Ranges

> Intervals like `[1..10]` — membership tests, open and closed bounds, and how
> ranges drive unary tests.

A **range** is a contiguous interval of comparable values, with each endpoint
independently included or excluded. Ranges are how you express "between these
two bounds" as a single value: an age band, a date window, a price tier. They
power the `in` membership operator, the interval-algebra built-ins, and the
interval form of decision-table unary tests.

Ranges work over any comparable type — numbers, strings (compared in code-point
order), and the temporal values (compared chronologically). The Go value type
backing a range is `bl.BlRange`.

## Range literals

A range literal writes a constant interval between a start and an end bound. The
bracket on each side chooses whether that endpoint is **included** (`[` / `]`)
or **excluded** (`(` / `)`):

| Syntax | Start | End | Meaning |
|---|---|---|---|
| `[a..b]` | included | included | `a ≤ x ≤ b` |
| `(a..b)` | excluded | excluded | `a < x < b` |
| `[a..b)` | included | excluded | `a ≤ x < b` |
| `(a..b]` | excluded | included | `a < x ≤ b` |

```
// expression-language
[1..10]                                    // numeric, both ends inclusive
[1..10)                                    // 1 ≤ x < 10, upper bound excluded
[date("2025-01-01")..date("2025-12-31"))   // a date window, end-exclusive
```

### Unbounded ends

Use `null` as an endpoint to leave that side open: a `null` start means −∞, a
`null` end means +∞. An unbounded side must always be excluded — a closed
boundary against `null` is an error, since "≤ −∞" or "≥ +∞" is not meaningful.

```
// expression-language
[18..null)     // "18 or older"
(null..0)      // "negative" (everything below zero)
```

## Membership: `in` and `between`

`x in r` asks whether a point falls inside a range. It honours each endpoint's
inclusion exactly:

```
// expression-language
25 in [18..65]                     // → true
3 in [1..10]                       // → true
10 in [1..10)                      // → false   (upper bound is exclusive)
```

The value and the endpoints must be comparable; an incompatible type produces a
`bl.TypeError`.

The related `between` operator is shorthand for a pair of inclusive
comparisons — `x between a and b` is exactly `x >= a and x <= b`, so it is
equivalent to membership in a closed range `[a..b]`:

```
// expression-language
5 between 1 and 10                 // → true
5 in [1..10]                       // → true   (same test, written as a range)
```

`in` also has a function form, `includes(r, x)`, with the operands in the other
order. It is listed under [Interval algebra](#interval-algebra) below.

## Equality

Two ranges compare with `=` and `!=` **structurally** — they are equal when they
have the same start, the same end, and the same inclusion on both sides:

```
// expression-language
[1..5] = [1..5]                    // → true
[1..5] = [1..5)                    // → false  (different end inclusion)
```

Structural equality is unaffected by emptiness: two empty ranges with identical
endpoints and inclusion compare equal (see [Empty ranges](#empty-ranges)).
Ranges have no ordering operators (`<`, `<=`, …) and no arithmetic — "which range
is less" has no meaningful definition.

## Reading a range's parts

The components of a range are read with the dot operator:

| Accessor | Example | Result |
|---|---|---|
| `.start` | `[1..10].start` | `1` (or `null` if unbounded) |
| `.end` | `[1..10).end` | `10` (or `null` if unbounded) |
| `.startIncluded` | `[1..10].startIncluded` | `true` |
| `.endIncluded` | `[1..10).endIncluded` | `false` |

## Interval algebra

The interval-algebra built-ins describe how two intervals — or an interval and a
point — relate to one another. Their names follow DMN FEEL's interval functions
(blkit is FEEL-inspired, not FEEL-compliant; the behaviour around `null` and
empty ranges in particular diverges). Arguments may be a point, a range, or both,
depending on the function.

Every one of these returns a `boolean`, with one important exception: if any
range argument is **empty**, the result is `null` instead (see
[Empty ranges](#empty-ranges)). `isEmpty` is itself the exception to that
exception — it returns `true` for an empty range, which is its whole purpose.

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
| `isEmpty(r)` | `isEmpty((3..3))` | `true` (no values in the range) |

Several of the functions come in inverse-order pairs (`includes`/`during`,
`starts`/`startedBy`, `finishes`/`finishedBy`, `meets`/`metBy`,
`overlapsBefore`/`overlapsAfter`) so you can write a relationship whichever way
round reads more naturally.

## Empty ranges

A range is **empty** when it contains no values. There are two ways to write one:

- **Reversed** — `start > end`, e.g. `[5..3]`. This is accepted silently; it is
  not an error.
- **Degenerate exclusive** — the same value at both ends with at least one end
  excluded: `(3..3)`, `[3..3)`, and `(3..3]` all contain nothing. (`[3..3]` is
  *not* empty — it contains exactly the one value `3`.)

Operations against an empty range follow blkit's broader null-propagation rule.
Asking "how does this value relate to a set that has no elements?" has no
meaningful yes/no answer, so the result is `null`:

| Operation | Against an empty range | Why |
|---|---|---|
| `x in r` | `null` | Membership against the empty set has no truth value |
| `includes(r, x)` | `null` | Same as `in` |
| `during(x, r)` | `null` | Same |
| `before`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, `starts`, `startedBy`, `finishes`, `finishedBy`, `coincides` | `null` if **either** range argument is empty | Interval-algebra predicates on the empty set are undefined |
| `isEmpty(r)` | `true` | The meta-question; returning `null` here would defeat its purpose |
| `r1 = r2`, `r1 != r2` | structural | Emptiness doesn't affect structural equality |

```
// expression-language
7 in [5..3]                        // → null   (empty range — reversed bounds)
isEmpty([5..3])                    // → true
isEmpty((3..3))                    // → true
isEmpty([3..3])                    // → false  (contains exactly 3)
[5..3] = [5..3]                    // → true   (structural equality still holds)
```

The distinction is deliberate: `null` means "data was missing", while an empty
range is a well-defined value that simply happens to contain nothing. So
`isEmpty(null)` is `null` (the input was missing) but `isEmpty([5..3])` is `true`
(the input was a real, empty range).

## Constructing ranges from Go

Host code builds a range with the `bl.Range` constructor. The endpoints may be
any comparable `bl.BlValue` — numbers, strings, temporal values — or `bl.Null()`
for an unbounded side:

```go
// host-side (Go)
// func Range(start, end bl.BlValue, startIncluded, endIncluded bool) (bl.BlRange, error)

// Closed-closed numeric range.
var adultAges, _ = bl.Range(bl.Number(18), bl.Number(120), true, true)

// Half-open (closed-open) — the typical "from X up to but not including Y".
var quarter, _ = bl.Range(bl.Number(0), bl.Number(0.25), true, false)

// Unbounded upper end — bl.Null() for the open side, and false for its inclusion.
var workingAge, _ = bl.Range(bl.Number(18), bl.Null(), true, false)

// Date range — both endpoints must be the same type.
var d1, _ = bl.Date("2025-01-01")
var d2, _ = bl.Date("2025-12-31")
var yr2025, _ = bl.Range(d1, d2, true, true)
```

`bl.Range` returns `(bl.BlRange, error)`. The error fires for:

- **cross-type endpoints** — e.g. a `Number` start with a `Date` end;
- a **closed boundary on an unbounded endpoint** — e.g.
  `bl.Range(bl.Number(5), bl.Null(), true, true)`, because `null` may only pair
  with an excluded boundary; and
- **non-comparable endpoint types**.

Note that a reversed range such as `bl.Range(bl.Number(5), bl.Number(3), true, true)`
constructs successfully — it is simply an [empty range](#empty-ranges), not an
error.

Once you have an evaluated `bl.BlRange`, the host accessors `bl.Start()`,
`bl.End()`, `StartIncluded()`, and `EndIncluded()` read its parts. `Start()` and
`End()` return a `bl.BlValue` (which may be `bl.Null()` for an unbounded side);
the two inclusion accessors return a Go `bool`.

## Worked example: compile once, evaluate many

Ranges shine when a single compiled expression is run against many inputs. The
expression is parsed and type-checked once; each `Evaluate` reuses the compiled
program:

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

type applicant struct {
    Age bl.BlNumber `expr:"age"`
}

var eligible, _ = bl.Expr[applicant](`age in [18..65]`)

var r1, _ = eligible.Evaluate(applicant{Age: bl.Number(40)}) // → true
var r2, _ = eligible.Evaluate(applicant{Age: bl.Number(70)}) // → false
```

A range literal can also be a value the expression produces and returns, not just
a test:

```go
// host-side (Go)
var window, _ = bl.ExprNoEnv(`[date("2025-01-01")..date("2025-12-31")]`)
var result, _ = window.Evaluate(bl.NoEnv{}) // → a bl.BlRange
```

## Ranges in unary tests

A **unary test** is a condition evaluated against an implicit input — the value
being matched is supplied automatically, so the test reads as a bare condition.
This is how decision-table input cells are written, and the interval form is one
of the recognised shapes:

| Form | Matches when the input… | Example |
|---|---|---|
| value | equals the value | `"valid"` |
| `<` `<=` `>` `>=` | compares true | `< 10`, `>= date("2020-04-06")` |
| interval | falls in the range | `[2..5]`, `(2..5]` |
| comma list | equals any (disjunction) | `2, 3, 4`, `< 10, > 50` |
| `not(...)` | does **not** match the inner test | `not("valid")` |
| `-` | always (wildcard) | `-` |

A bare interval is read as membership of the implicit input: `[18..65]` means
"the input is between 18 and 65 inclusive".

```
// expression-language
[18..65]              // 18 ≤ input ≤ 65
(0..null)             // any positive input
< 10, [20..30]        // input < 10, or input is in [20..30]
```

The interval and ordering forms need an ordered domain, so they only apply to the
comparable scalar types — `bl.TypeNumber`, `bl.TypeString`, `bl.TypeDate`,
`bl.TypeTime`, `bl.TypeDateTime`, and the two duration types. Using an interval
form against an unordered input type (boolean, list, dictionary, table, range,
calendar, regex) is rejected as a `bl.ParseError` when the test is constructed.

Host code compiles a unary test once and evaluates it against many inputs:

```go
// host-side (Go)
var inBand, _ = bl.UnaryTest[bl.BlNumber](`[18..65]`)

var ok, _ = inBand.Evaluate(bl.Number(30)) // → true
var no, _ = inBand.Evaluate(bl.Number(80)) // → false
```

See [Decisions → Decision Tables](../decisions/decision-tables.md) for how
interval cells slot into a full table.

## Where to look next

This guide covers everything you need to write and read ranges. For the
exhaustive, authoritative detail — every edge case, the Go value type, and the
function registrations — see the range spec at
`specs/expressions/range.spec.md`, and the generated
[Reference](../reference/blkit.md). For how the engine compiles and runs any
expression, including ranges, see
[Architecture → Expressions](../architecture/expressions.md).
