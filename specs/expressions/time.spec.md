---
name: BlTime
description: blkit's time-of-day type (modelled on FEEL) — a time of day with optional timezone; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/time.go
---

# BlTime

`BlTime` represents a time of day: hours, minutes, seconds (with optional fractional seconds), and an optional timezone offset or IANA zone identifier. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

```go
type BlTime struct {
    BlExpr
}

// Construction — see bl.spec.md.
//   Bl.Time(hour, minute, second int, opts ...TimeOption) BlTime
//     opts may include WithOffset("+05:30") or WithTimezone("Europe/Paris").

// Parsing — accepts an ISO 8601 time string.
func (Bl) ToTime(value string) BlTime { ... }

// Current local time at time of call (or in the given timezone).
func (Bl) TimeNow(timezone ...string) BlTime { ... }

// Extract the time component from a BlDateTime (deferred).
func (Bl) TimeFromDateTime(dt BlExpr) BlTime { ... }

// Components — deferred; evaluate to BlNumber
func (t *BlTime) Hour() BlNumber { ... }    // 0–23

func (t *BlTime) Minute() BlNumber { ... }  // 0–59

func (t *BlTime) Second() BlNumber { ... }  // 0–59.999…

func (t *BlTime) Offset() BlExpr { ... }    // evaluates to BlDaysTimeDuration or BlNull

func (t *BlTime) Timezone() BlExpr { ... }  // evaluates to BlString (IANA id) or BlNull

// Arithmetic — deferred; wraps at midnight; evaluates to BlTime
func (t *BlTime) Add(duration BlExpr) *BlTime { ... }
func (t *BlTime) Subtract(duration BlExpr) *BlTime { ... }

// Normalisation — deferred; evaluates to BlTime
func (t *BlTime) WithOffset(offset BlExpr) *BlTime { ... }
// converts to same instant at the given UTC offset

// Comparison — deferred; evaluate to BlBoolean or BlNull
func (t *BlTime) Equals(other BlExpr) BlExpr         { ... }
func (t *BlTime) NotEqual(other BlExpr) BlExpr        { ... }
func (t *BlTime) Before(other BlExpr) BlExpr          { ... }
func (t *BlTime) After(other BlExpr) BlExpr           { ... }
func (t *BlTime) BeforeOrEqual(other BlExpr) BlExpr   { ... }
func (t *BlTime) AfterOrEqual(other BlExpr) BlExpr    { ... }

// Eager host-language utilities — only valid on a concrete BlTime after .evaluate()
func (t *BlTime) CompareTo(other *BlTime) int { ... }  // -1, 0, or 1
func (t *BlTime) String() string { ... }               // ISO 8601: "14:30:00" / "14:30:00+01:00" / "14:30:00@Europe/Paris"
```

## Deferred semantics

```go
expr := Bl.TimeVar("appointmentTime").Before(Bl.Time(17, 0, 0))
result, _ := expr.Evaluate(map[string]any{"appointmentTime": Bl.Time(14, 30, 0)})
// result == BlBoolean.TRUE
```

## ISO 8601 Parsing

`Bl.ToTime()` accepts:
- Local time (no offset): `"14:30:00"`, `"14:30:00.500"`
- UTC offset: `"14:30:00Z"`, `"14:30:00+01:00"`, `"14:30:00-05:00"`
- IANA timezone: `"14:30:00@Europe/Paris"` (notation borrowed from FEEL's DMN 1.3+ extension)

Fractional seconds are accepted to nanosecond precision; stored precision must be at least millisecond.

## Timezone vs. Offset

blkit distinguishes between a fixed UTC **offset** (`+01:00`) and a named **timezone** (`@Europe/Paris`), following FEEL's distinction:

- An **offset** is a fixed displacement from UTC, not DST-aware.
- A **timezone** is an IANA zone identifier; arithmetic and comparisons respect DST transitions.
- A time with neither is a **local time** with no UTC relationship.

## Arithmetic

Adding or subtracting a `BlDaysTimeDuration` adjusts the time and wraps at midnight. The date component is not tracked — adding `PT25H` to `"23:00:00"` evaluates to `"00:00:00"` (day advance is discarded). Use `BlDateTime` for arithmetic that must track day changes.

## Comparison Semantics

- Two times with offset/timezone information are compared as UTC instants.
- Two local times (no offset) are compared wall-clock.
- Comparing a local time to a zoned/offset time evaluates to `BlNull`.

## Edge Cases

- Hour values outside `0–23` in `of()` produce a `BlTypeError`.
- Minute/second values outside `0–59` produce a `BlTypeError`.
- `Bl.ToTime("24:00:00")` (end-of-day midnight) is valid ISO 8601; normalised to `00:00:00` with an end-of-day flag. Implementations must handle this consistently.
- An unknown IANA timezone id in `of_with_zone()` or `parse()` produces a `BlTypeError`.
