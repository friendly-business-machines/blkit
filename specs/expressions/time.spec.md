---
name: BlTime
description: The time-of-day type in the blkit expression language. Covers the time constructor, component access, duration arithmetic, comparison, timezone/offset semantics, and the Go layer (BlTime + expr registrations).
targets:
  - ../../expr/time.go
---

# BlTime — the `time` type

`time` is a time of day: hours, minutes, seconds (optionally fractional), with an optional UTC
offset or IANA timezone. The Go value type backing it is `BlTime`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals / construction

```
time("14:30:00")              // local time
time("14:30:00.500")          // fractional seconds
time("14:30:00Z")             // UTC
time("11:45:30+02:00")        // fixed offset
time("14:30:00@Europe/Paris") // IANA timezone (DST-aware)
time(23, 59, 0)               // from components
time(now())                   // current time of day (from current datetime)
time(datetime("2025-03-28T14:30:00"))  // extract time component
```

blkit distinguishes a fixed **offset** (`+01:00`, not DST-aware), a named **timezone**
(`@Europe/Paris`, DST-aware), and a **local** time (no UTC relationship).

`[@test] ../../expr/time_test.go`

---

## Component access

```
time("11:45:30+02:00").hour         // → 11
time("11:45:30+02:00").minute       // → 45
time("11:45:30+02:00").second       // → 30
time("11:45:30+02:00").timeOffset   // → duration("PT2H")   (ext)
time("11:45:30@Europe/Paris").timezone  // → "Europe/Paris" (ext)
```

`[@test] ../../expr/time_components_test.go`

---

## Operators & functions

| Form | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` | add/subtract a days-time duration (wraps at midnight) | `time("23:00:00") + duration("PT2H")` | `time("01:00:00")` |
| `< <= > >= = !=` | comparison | `time("14:30:00") < time("17:00:00")` | `true` |
| `withOffset(t, offset)` **ext** | same instant at a new offset | `withOffset(time("14:30:00Z"), duration("PT1H"))` | `time("15:30:00+01:00")` |

Only a **days-time** duration may be applied (time has no year/month concept). The date component is
not tracked — adding `PT25H` to `23:00:00` gives `00:00:00` (day advance discarded); use `datetime`
for day-tracking arithmetic.

`[@test] ../../expr/time_ops_test.go`

---

## Comparison semantics

- Two times that both carry offset/timezone are compared as **UTC instants**.
- Two local times are compared **wall-clock**.
- Comparing a local time to a zoned/offset time → `null`.

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Time(h,m,s, opts)` | `time(h, m, s)` / `time("…")` (offset/zone via the string form) |
| `Bl.ToTime(str)` | `time("…")` |
| `Bl.TimeNow` | `time(now())` |
| `Bl.TimeFromDateTime(dt)` | `time(dt)` |
| `hour` / `minute` / `second` | `.hour` / `.minute` / `.second` |
| `offset` / `timezone` | `.timeOffset` **ext** / `.timezone` **ext** |
| `add` / `subtract` | `+` / `-` (days-time duration) |
| `withOffset` | `withOffset(t, offset)` **ext** |
| `equals` / `notEqual` / `before` / `after` / `beforeOrEqual` / `afterOrEqual` | `=` `!=` `<` `>` `<=` `>=` |
| `compareTo` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/time.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlTime struct{ h, m, s, frac int; offset *time.Duration; zone *string }

func (BlTime) Type() BlType { return BlTypeTime }
func (t BlTime) Equal(other BlValue) BlValue
func (t BlTime) ToMarkdown() string
func (BlTime) isBlValue() {}

func Time(hour, minute, second int, opts ...TimeOption) (BlTime, error) // WithOffset / WithTimezone
func (t BlTime) CompareTo(other BlTime) int
func (t BlTime) String() string  // "14:30:00" / "14:30:00+01:00" / "14:30:00@Europe/Paris"
```

### Operator impl funcs (unexported)

```go
func addTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime  // "+" (wraps at midnight)
func subTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime  // "-"
func ltTimes(a, b BlTime) BlValue                          // "<"; le/gt/ge; cross-kind (zoned vs local) → Null
```

### Registrations (`timeOptions`, unexported)

```go
func timeOptions() []expr.Option {
    return []expr.Option{
        expr.Function("addTimeDur", typed2(addTimeDur), new(func(BlTime, BlDaysTimeDuration) BlTime)),
        expr.Function("ltTimes",    typed2(ltTimes),    new(func(BlTime, BlTime) BlValue)),
        // … subTimeDur, le/gt/ge

        expr.Function("time", timeFn,
            new(func(BlString) BlTime),                          // time("…")
            new(func(BlNumber, BlNumber, BlNumber) BlTime),      // time(h, m, s)
            new(func(BlDateTime) BlTime)),                       // time(dt) extraction
        expr.Function("withOffset", typed2(timeWithOffsetFn),    new(func(BlTime, BlDaysTimeDuration) BlTime)), // ext (datetime overload too)
    }
}
```

**Components.** `.hour/.minute/.second/.timeOffset/.timezone` via the component-access patcher.
Native Go `time.Time` (time-of-day) inputs wrap to `BlTime`. Only a `BlDaysTimeDuration` may be added
(years-months → `BlTypeError`).

`[@test] ../../expr/time_test.go`

---

## Edge cases

- Hour outside 0–23, or minute/second outside 0–59 (component form) → `BlTypeError`.
- `time("24:00:00")` (end-of-day) is valid ISO 8601, normalised to `00:00:00`.
- Unknown IANA zone id → `BlTypeError`.
- Applying a years-months duration → `BlTypeError`.
