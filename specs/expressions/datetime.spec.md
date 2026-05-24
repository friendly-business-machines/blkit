---
name: BlDateTime
description: The date-and-time type in the blkit expression language. Covers the datetime constructor, date/time component access, duration arithmetic, difference, timezone normalisation, comparison, and the Go layer (BlDateTime + expr registrations).
targets:
  - ../../expr/datetime.go
---

# BlDateTime — the `datetime` type

`datetime` is a combined date and time with an optional UTC offset or IANA timezone (ISO 8601
combined form). The Go value type backing it is `BlDateTime`. (Constructor named `datetime`, not
`date and time` — see [bl-expr.spec.md](bl-expr.spec.md#relationship-to-feel-and-future-direction).)

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals / construction

There is **no dedicated datetime literal**: datetime values are produced by the `datetime(...)`
built-in — for example, the `datetime("2025-03-28T14:30:00Z")` in
`datetime("2025-03-28T14:30:00Z").hour`. The constructor accepts an ISO 8601 combined string or a
date/time pair.

```
datetime("2025-03-28T14:30:00")            // local
datetime("2025-03-28T14:30:00Z")           // UTC
datetime("2025-03-28T14:30:00+01:00")      // offset
datetime("2025-03-28T14:30:00@Europe/Paris") // IANA zone
datetime(date("2025-03-28"), time("14:30:00")) // combine a date and a time
now()                                       // current datetime
```

The `T` separator is required; space-separated forms are rejected.

`[@test] ../../expr/datetime_test.go`

---

## Component access & calendar properties

```
datetime("2025-03-28T14:30:00+01:00").year       // → 2025
datetime("2025-03-28T14:30:00+01:00").month      // → 3
datetime("2025-03-28T14:30:00+01:00").day        // → 28
datetime("2025-03-28T14:30:00+01:00").hour       // → 14
datetime("2025-03-28T14:30:00+01:00").minute     // → 30
datetime("2025-03-28T14:30:00+01:00").second     // → 0
datetime("2025-03-28T14:30:00+01:00").timeOffset // → duration("PT1H")   (ext)
datetime("2025-03-28T14:30:00@Europe/Paris").timezone // → "Europe/Paris" (ext)
```

Calendar-property and extraction built-ins (shared with [date.spec.md](date.spec.md)):

| Function | Example | Result |
|---|---|---|
| `dayOfWeek(dt)` | `dayOfWeek(datetime("2019-09-17T00:00:00"))` | `"Tuesday"` |
| `dayOfYear(dt)` | … | `260` |
| `weekOfYear(dt)` | … | `38` |
| `monthOfYear(dt)` | … | `"September"` |
| `date(dt)` | extract the date | a `date` |
| `time(dt)` | extract the time | a `time` |

`[@test] ../../expr/datetime_components_test.go`

---

## Operators & functions

| Form | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration | `datetime("2025-01-31T12:00:00") + duration("P1M")` | `datetime("2025-02-28T12:00:00")` (day clamped) |
| `-` (datetime) | days-time difference | `dt2 - dt1` | a days-time `duration` |
| `yearsAndMonthsDuration(a, b)` | years-months difference | `yearsAndMonthsDuration(dt1, dt2)` | a years-months `duration` |
| `< <= > >= = !=` | comparison | `submittedAt <= deadline` | `true`/`false` |
| `toUTC(dt)` **ext** | normalise to UTC | `toUTC(datetime("…+01:00"))` | the same instant in UTC |
| `withOffset(dt, off)` / `withTimezone(dt, zone)` **ext** | re-zone (same instant) | … | … |

- A **years-months** duration adjusts year/month (day clamped), leaving the time; a **days-time**
  duration adds precisely, carrying across date boundaries. Both preserve the original zone/offset.
- Mixed arithmetic chains two operations: `dt + duration("P1Y") + duration("P10D")`.

`[@test] ../../expr/datetime_ops_test.go`

---

## Comparison semantics

- Two zoned/offset datetimes compare as **UTC instants**.
- Two local datetimes compare **wall-clock**.
- Comparing a local datetime to a zoned/offset one → `null`.

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.DateTime(...)` / `Bl.DateTimeFromComponents(d, t)` | `datetime("…")` / `datetime(date, time)` |
| `Bl.Now` | `now()` |
| `Bl.ToDateTime(str)` | `datetime("…")` |
| `year`/`month`/`day`/`hour`/`minute`/`second` | `.year`/`.month`/`.day`/`.hour`/`.minute`/`.second` |
| `offset` / `timezone` | `.timeOffset` **ext** / `.timezone` **ext** |
| `dayOfWeek`/`dayOfYear`/`weekOfYear`/`monthOfYear` | `dayOfWeek(dt)` / `dayOfYear(dt)` / `weekOfYear(dt)` / `monthOfYear(dt)` |
| `date` / `time` (extraction) | `date(dt)` / `time(dt)` |
| `add` / `subtract` | `+` / `-` (duration) |
| `diffYearsMonths` / `diffDaysTime` | `yearsAndMonthsDuration(a, b)` / `dt2 - dt1` |
| `toUTC` / `withOffset` / `withTimezone` | `toUTC(dt)` / `withOffset(dt, off)` / `withTimezone(dt, zone)` **ext** |
| `equals`/`notEqual`/`before`/`after`/`beforeOrEqual`/`afterOrEqual` | `=` `!=` `<` `>` `<=` `>=` |
| `compareTo` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/datetime.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlDateTime struct{ date BlDate; tod BlTime } // tod carries the offset/zone

func (BlDateTime) Type() BlType { return BlTypeDateTime }
func (dt BlDateTime) Equal(other BlValue) BlValue
func (dt BlDateTime) ToMarkdown() string
func (BlDateTime) isBlValue() {}

func DateTime(year, month, day, hour, minute, second int, opts ...DateTimeOption) (BlDateTime, error)
func Now(tz ...string) BlDateTime
func (dt BlDateTime) CompareTo(other BlDateTime) int
func (dt BlDateTime) String() string  // "2025-03-28T14:30:00" / "…+01:00"
```

### Operator impl funcs (unexported)

```go
func addDateTimeYM(dt BlDateTime, dur BlYearsMonthsDuration) BlDateTime // "+" (day clamped)
func addDateTimeDT(dt BlDateTime, dur BlDaysTimeDuration) BlDateTime    // "+"
func subDateTimes(a, b BlDateTime) BlDaysTimeDuration                  // "-" dt−dt
func subDateTimeDur(dt BlDateTime, dur BlValue) BlValue                // "-" dt−duration
func ltDateTimes(a, b BlDateTime) BlValue                              // "<"; le/gt/ge; cross-kind → Null
```

### Registrations (`datetimeOptions`, unexported)

```go
func datetimeOptions() []expr.Option {
    return []expr.Option{
        expr.Function("addDateTimeDT", typed2(addDateTimeDT), new(func(BlDateTime, BlDaysTimeDuration) BlDateTime)),
        expr.Function("subDateTimes",  typed2(subDateTimes),  new(func(BlDateTime, BlDateTime) BlDaysTimeDuration)),
        // … addDateTimeYM, subDateTimeDur, ltDateTimes, le/gt/ge

        expr.Function("datetime", datetimeFn,
            new(func(BlString) BlDateTime),            // datetime("…")
            new(func(BlDate, BlTime) BlDateTime)),     // datetime(date, time)
        expr.Function("now", nowFn, new(func() BlDateTime)),
        // calendar props / extraction are registered with overloads in date.go / time.go:
        //   dayOfWeek, dayOfYear, weekOfYear, monthOfYear  (BlDateTime overload)
        //   date(dt), time(dt)                              (extraction overloads)
        // ext: re-zoning
        expr.Function("toUTC",        typed1(toUTCFn),        new(func(BlDateTime) BlDateTime)),
        expr.Function("withOffset",   typed2(dtWithOffsetFn), new(func(BlDateTime, BlDaysTimeDuration) BlDateTime)),
        expr.Function("withTimezone", typed2(withTimezoneFn), new(func(BlDateTime, BlString) BlDateTime)),
    }
}
```

`yearsAndMonthsDuration(a, b)` (years-months difference) is registered in [date.spec.md](date.spec.md)
with a `BlDateTime` overload. **Components.** date/time/offset/zone via the component-access patcher.
Native Go `time.Time` inputs wrap to `BlDateTime`.

`[@test] ../../expr/datetime_test.go`

---

## Edge cases

- `now()` depends on the system clock; use explicit construction for deterministic tests.
- `re-zoning` (`toUTC`/`withOffset`/`withTimezone`) preserves the instant.
- `dt2 - dt1` is negative when `dt2` precedes `dt1`.
- `duration("P1M")` added to `2025-01-31T23:59:59` → `2025-02-28T23:59:59` (day clamped).
