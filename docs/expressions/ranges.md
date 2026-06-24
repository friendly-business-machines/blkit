# Ranges

> Intervals like `[1..10]` — membership tests, open and closed bounds, the
> point/interval relations, and how ranges drive unary tests.

A **range** is a contiguous interval of comparable values, with each endpoint
independently included or excluded. Ranges are how you express "between these
two bounds" as a single value: an age band, a date window, a price tier. They
power the `in` membership operator, the interval-relation built-ins, and the
interval form of decision-table unary tests.

Ranges work over any comparable type — numbers, strings (compared in code-point
order), and the temporal values (compared chronologically).

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
boundary against `null` is not meaningful.

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

The related `between` operator is shorthand for a pair of inclusive
comparisons — `x between a and b` is exactly `x >= a and x <= b`, so it is
equivalent to membership in a closed range `[a..b]`:

```
// expression-language
5 between 1 and 10                 // → true
5 in [1..10]                       // → true   (same test, written as a range)
```

`in` also has a function form, `includes(r, x)`, with the operands in the other
order. It is listed under [Interval relations](#interval-relations) below.

## Equality

Two ranges compare with `=` and `!=` **structurally** — they are equal when they
have the same start, the same end, and the same inclusion on both sides:

```
// expression-language
[1..5] = [1..5]                    // → true
[1..5] = [1..5)                    // → false  (different end inclusion)
```

Ranges have no ordering operators (`<`, `<=`, …) and no arithmetic — "which range
is less" has no meaningful definition.

## Reading a range's parts

The components of a range are read with the dot operator:

| Accessor | Example | Result |
|---|---|---|
| `.start` | `[1..10].start` | `1` |
| `.end` | `[1..10).end` | `10` |
| `.startIncluded` | `[1..10].startIncluded` | `true` |
| `.endIncluded` | `[1..10).endIncluded` | `false` |

## Interval relations

blkit's interval-relation built-ins describe how two intervals — or an interval
and a point — relate to one another. Arguments may be a point, a range, or both,
depending on the function. Every one returns a `boolean`.

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

Several of the functions come in inverse-order pairs (`includes`/`during`,
`starts`/`startedBy`, `finishes`/`finishedBy`, `meets`/`metBy`,
`overlapsBefore`/`overlapsAfter`) so you can write a relationship whichever way
round reads more naturally.

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
`bl.TypeTime`, `bl.TypeDateTime`, and the two duration types.

See [Decisions → Decision Tables](../decisions/decision-tables.md) for how
interval cells slot into a full table.

## Ranges from Go

Host Go code builds `range` values with the `bl.Range` constructor — two endpoint
`bl.BlValue`s (or `bl.Null()` for an unbounded side) plus start/end inclusion
flags. See [Values from Go](values-from-go.md) for the full host-side story.
