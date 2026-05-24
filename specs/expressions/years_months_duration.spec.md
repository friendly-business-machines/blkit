---
name: BlYearsMonthsDuration
description: The years-and-months duration type in the blkit expression language (ISO 8601 PnYnM). Covers the duration constructor, component access, arithmetic/comparison operators, and the Go layer (BlYearsMonthsDuration + expr registrations).
targets:
  - ../../expr/years_months_duration.go
---

# BlYearsMonthsDuration — the years-and-months `duration`

A years-and-months duration covers only years and months (ISO 8601 `PnYnM`). The Go value type
backing it is `BlYearsMonthsDuration`. It is distinct from `BlDaysTimeDuration`
([days_time_duration.spec.md](days_time_duration.spec.md)): the two **cannot** be added to each
other, and a years-months duration **cannot** be applied to a `time`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals / construction

There is **no dedicated duration literal**: years-and-months duration values are produced by the
`duration(...)` built-in — for example, the `duration("P1Y6M")` in
`date("2025-03-28") + duration("P1Y6M")`. The constructor accepts an ISO 8601 string using only
Y/M designators:

```
duration("P1Y6M")     // 1 year, 6 months
duration("P1Y")       // 1 year
duration("P6M")       // 6 months
duration("-P1Y6M")    // negative
duration("P1DT2H")    // → BlParseError (day/time designators not allowed here)
```

`duration(from)` returns a `BlYearsMonthsDuration` for Y/M-only strings (and a
`BlDaysTimeDuration` for D/T-only strings). The standard `yearsAndMonthsDuration(from, to)` built-in
computes the years-months span between two dates:

```
yearsAndMonthsDuration(date("2011-12-22"), date("2013-08-24"))   // → duration("P1Y8M")
```

`[@test] ../../expr/years_months_duration_test.go`

---

## Component access

```
duration("P2Y7M").years        // → 2
duration("P2Y7M").months       // → 7
duration("P2Y7M").totalMonths  // → 31   (ext: signed years*12 + months)
```

`[@test] ../../expr/years_months_duration_components_test.go`

---

## Operators & functions

| Form | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` | add / subtract durations | `duration("P1Y") + duration("P6M")` | `duration("P1Y6M")` |
| unary `-` | negate | `-duration("P1Y6M")` | `duration("-P1Y6M")` |
| `*` `/` | scale by a number (rounded to whole months) | `duration("P6M") * 3` | `duration("P1Y6M")` |
| `< <= > >= = !=` | compare (by total months) | `duration("P1Y") = duration("P12M")` | `true` |
| `abs(d)` | absolute value | `abs(duration("-P2Y3M"))` | `duration("P2Y3M")` |
| `isNegative(d)` **ext** | sign test | `isNegative(duration("-P3M"))` | `true` |

`+`/`-` also apply between this duration and a `date`/`datetime` (not `time`). Division by zero →
`null`. Adding a days-time duration → `BlTypeError`.

`[@test] ../../expr/years_months_duration_ops_test.go`

---

## Semantics & behaviour

- Months normalise to 0–11; overflow carries into years (`duration("P0Y15M")` → `P1Y3M`).
- Comparison and equality are by **total months**; `duration("P1Y") = duration("P12M")`.
- `*` and `/` round the resulting total-months to the nearest whole month (half-up); `/0` → `null`.
- Zero duration is not negative.

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.YearsMonths(y,m)` / `Bl.YearsMonthsFromMonths` | Go host constructors (below); expressions use `duration("…")` |
| `Bl.ToYearsMonths(str)` | `duration("…")` built-in |
| `years` / `months` | `.years` / `.months` |
| `totalMonths` | `.totalMonths` **ext** |
| `isNegative` | `isNegative(d)` **ext** |
| `negate` / `abs` | unary `-` / `abs(d)` |
| `add` / `subtract` | `+` / `-` |
| `multiply` / `divide` | `*` / `/` (by a number; rounded to whole months) |
| `equals` / `notEqual` / `lessThan` / `lessThanOrEqual` / `greaterThan` / `greaterThanOrEqual` | `=` `!=` `<` `<=` `>` `>=` |
| `compareTo` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/years_months_duration.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlYearsMonthsDuration struct{ months int } // signed total months

func (BlYearsMonthsDuration) Type() BlType { return BlTypeYearsMonthsDuration }
func (d BlYearsMonthsDuration) Equal(other BlValue) BlValue
func (d BlYearsMonthsDuration) ToMarkdown() string
func (BlYearsMonthsDuration) isBlValue() {}

// Host constructors (host code; not expression syntax)
func YearsMonths(years, months int) BlYearsMonthsDuration
func YearsMonthsFromMonths(totalMonths int) BlYearsMonthsDuration
func (d BlYearsMonthsDuration) CompareTo(other BlYearsMonthsDuration) int
func (d BlYearsMonthsDuration) String() string   // "P1Y2M" / "-P6M"
```

### Operator impl funcs (unexported)

```go
func addYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration   // "+"
func subYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration   // "-"
func negYMDuration(d BlYearsMonthsDuration) BlYearsMonthsDuration      // unary "-"
func scaleYMDuration(d BlYearsMonthsDuration, n BlNumber) BlYearsMonthsDuration // "*" (round to whole months)
func divYMDuration(d BlYearsMonthsDuration, n BlNumber) BlValue        // "/" ; n==0 → Null
func ltYMDuration(a, b BlYearsMonthsDuration) BlValue                  // "<" by total months; le/gt/ge
```

### Registrations (`yearsMonthsDurationOptions`, unexported)

```go
func yearsMonthsDurationOptions() []expr.Option {
    return []expr.Option{
        expr.Function("addYMDuration",   typed2(addYMDuration),   new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),
        expr.Function("scaleYMDuration", typed2(scaleYMDuration), new(func(BlYearsMonthsDuration, BlNumber) BlYearsMonthsDuration)),
        expr.Function("divYMDuration",   typed2(divYMDuration),   new(func(BlYearsMonthsDuration, BlNumber) BlValue)),
        // … subYMDuration, negYMDuration, ltYMDuration, le/gt/ge

        // `duration(from)` and `isNegative` are registered once in days_time_duration.go with the
        // BlYearsMonthsDuration result/overload added here via extra signatures; `abs` overloads too.
        expr.Function("abs", typed1(absYMFn), new(func(BlYearsMonthsDuration) BlYearsMonthsDuration)),
    }
}
```

(The date/datetime `+`/`-` overloads that consume a years-months duration live in those spokes;
applying one to a `time` → `BlTypeError`.) **Components.** `.years/.months/.totalMonths` via the
component-access patcher.

`[@test] ../../expr/years_months_duration_test.go`

---

## Edge cases

- `duration("P1DT…")` (day/time designators) → `BlParseError` for this type.
- Division by a zero factor → `null`.
- `*`/`/` round to the nearest whole month (half-up).
- Zero duration: `isNegative` → `false`.
- Adding a days-time duration → `BlTypeError`; applying to a `time` → `BlTypeError`.
