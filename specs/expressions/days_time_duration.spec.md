---
name: BlDaysTimeDuration
description: The days-and-time duration type in the blkit expression language (ISO 8601 PnDTnHnMnS). Covers the duration constructor, component access, arithmetic/comparison operators, and the Go layer (BlDaysTimeDuration + expr registrations).
targets:
  - ../../expr/days_time_duration.go
---

# BlDaysTimeDuration — the days-and-time `duration`

A days-and-time duration covers days, hours, minutes, and seconds (ISO 8601 `PnDTnHnMnS`) — no years
or months. The Go value type backing it is `BlDaysTimeDuration`. It is distinct from
`BlYearsMonthsDuration` ([years_months_duration.spec.md](years_months_duration.spec.md)): the two
**cannot** be added to each other.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals / construction

Constructed with the `duration(...)` built-in from an ISO 8601 string using only D/T designators:

```
duration("P1DT2H30M")     // 1 day, 2 hours, 30 minutes
duration("PT90M")         // → normalises to 1h30m
duration("PT1.5S")        // fractional seconds
duration("-PT1H")         // negative
duration("P1Y")           // → BlParseError (year designator not allowed here)
```

`duration(from)` returns a `BlDaysTimeDuration` when the string uses only D/T designators, and a
`BlYearsMonthsDuration` when it uses only Y/M.

`[@test] ../../expr/days_time_duration_test.go`

---

## Component access

Read components with the dot operator; values are `number`:

```
duration("P2DT3H45M10S").days       // → 2
duration("P2DT3H45M10S").hours      // → 3
duration("P2DT3H45M10S").minutes    // → 45
duration("P2DT3H45M10S").seconds    // → 10
duration("P2DT3H45M10S").totalSeconds  // → 186310   (ext: signed total)
```

`[@test] ../../expr/days_time_duration_components_test.go`

---

## Operators & functions

| Form | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` | add / subtract durations | `duration("P1D") + duration("PT12H")` | `duration("P1DT12H")` |
| unary `-` | negate | `-duration("P2DT3H")` | `duration("-P2DT3H")` |
| `*` `/` | scale by a number | `duration("PT1H") * 2.5` | `duration("PT2H30M")` |
| `< <= > >= = !=` | compare (by total seconds) | `duration("PT60S") = duration("PT1M")` | `true` |
| `abs(d)` | absolute value | `abs(duration("-PT5H"))` | `duration("PT5H")` |
| `isNegative(d)` **ext** | sign test | `isNegative(duration("-PT1H"))` | `true` |

`+`/`-` also apply between a duration and a `date`/`time`/`datetime` (see those spokes). Division by
zero → `null`. Adding a years-months duration → `BlTypeError`.

`[@test] ../../expr/days_time_duration_ops_test.go`

---

## Semantics & behaviour

- Components normalise (seconds → minutes → hours → days); `duration("PT90M") = duration("PT1H30M")`.
- Comparison and equality are by **total seconds**; structurally different but equal-total durations
  are equal.
- Multiplication may produce fractional seconds (precision preserved); division by zero → `null`.
- Zero duration is not negative.

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.DaysTime(d,h,m,s)` / `Bl.DaysTimeFromSeconds` | Go host constructors (below); expressions use `duration("…")` |
| `Bl.ToDaysTime(str)` | `duration("…")` built-in |
| `days` / `hours` / `minutes` / `seconds` | `.days` / `.hours` / `.minutes` / `.seconds` |
| `totalSeconds` | `.totalSeconds` **ext** |
| `isNegative` | `isNegative(d)` **ext** |
| `negate` / `abs` | unary `-` / `abs(d)` |
| `add` / `subtract` | `+` / `-` |
| `multiply` / `divide` | `*` / `/` (by a number) |
| `equals` / `notEqual` / `lessThan` / `lessThanOrEqual` / `greaterThan` / `greaterThanOrEqual` | `=` `!=` `<` `<=` `>` `>=` |
| `compareTo` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/days_time_duration.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlDaysTimeDuration struct{ secs decimal.Decimal } // signed total seconds (fractional allowed)

func (BlDaysTimeDuration) Type() BlType { return BlTypeDaysTimeDuration }
func (d BlDaysTimeDuration) Equal(other BlValue) BlValue
func (d BlDaysTimeDuration) ToMarkdown() string
func (BlDaysTimeDuration) isBlValue() {}

// Host constructors (host code; not expression syntax)
func DaysTime(days, hours, minutes, seconds int) BlDaysTimeDuration
func DaysTimeFromSeconds(totalSeconds float64) BlDaysTimeDuration
func (d BlDaysTimeDuration) CompareTo(other BlDaysTimeDuration) int
func (d BlDaysTimeDuration) String() string   // "P1DT2H30M" / "-PT90S"
```

### Operator impl funcs (unexported)

```go
func addDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration  // "+"  (also bound for YM in its own spoke)
func subDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration  // "-"
func negDuration(d BlDaysTimeDuration) BlDaysTimeDuration     // unary "-"
func scaleDuration(d BlDaysTimeDuration, n BlNumber) BlDaysTimeDuration // "*"
func divDuration(d BlDaysTimeDuration, n BlNumber) BlValue    // "/" ; n==0 → Null
func ltDuration(a, b BlDaysTimeDuration) BlValue              // "<" by total seconds; le/gt/ge
```

(The date/time `+`/`-` overloads that consume a duration live in those spokes; this spoke owns the
duration-with-duration and duration-with-number forms.)

### Registrations (`daysTimeDurationOptions`, unexported)

```go
func daysTimeDurationOptions() []expr.Option {
    return []expr.Option{
        expr.Function("addDuration",  typed2(addDuration),  new(func(BlDaysTimeDuration, BlDaysTimeDuration) BlDaysTimeDuration)),
        expr.Function("scaleDuration", typed2(scaleDuration), new(func(BlDaysTimeDuration, BlNumber) BlDaysTimeDuration)),
        expr.Function("divDuration",  typed2(divDuration),  new(func(BlDaysTimeDuration, BlNumber) BlValue)),
        // … subDuration, negDuration, ltDuration, le/gt/ge

        // `duration(from)` is registered once (here) with both result kinds; it returns a
        // BlDaysTimeDuration for D/T strings and a BlYearsMonthsDuration for Y/M strings.
        expr.Function("duration", durationFn, new(func(BlString) BlValue)),
        expr.Function("abs",        typed1(absDurationFn), new(func(BlDaysTimeDuration) BlDaysTimeDuration)), // abs overloads numeric in number.go
        expr.Function("isNegative", typed1(isNegativeFn),  new(func(BlDaysTimeDuration) BlBoolean)),          // ext (YM overload too)
    }
}
```

**Components.** `.days/.hours/.minutes/.seconds/.totalSeconds` via the component-access patcher.
Native Go `time.Duration` inputs wrap to `BlDaysTimeDuration`. Mixing with a years-months duration →
`BlTypeError`.

`[@test] ../../expr/days_time_duration_test.go`

---

## Edge cases

- `duration("P1Y…")` (year/month designators) → `BlParseError` for this type.
- Division by a zero factor → `null`.
- Fractional seconds accepted (`PT1.5S`); fractional minutes/hours are not.
- Zero duration: `isNegative` → `false`.
- Adding a years-months duration → `BlTypeError`.
