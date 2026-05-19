---
name: BlDateTime
description: blkit's date-and-time type — a combined date and time with optional timezone; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/datetime.go
---

# BlDateTime

`BlDateTime` represents a combined date and time with an optional timezone offset or IANA zone identifier. It uses the ISO 8601 combined datetime format. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

```go
type BlDateTime struct {
    BlExpr
}

// Construction — see bl.spec.md.
//   Bl.DateTime(year, month, day, hour, minute, second int, opts ...DateTimeOption) BlDateTime
//     opts may include WithOffset("+05:30") or WithTimezone("Europe/Paris").
//   Bl.DateTimeFromComponents(date BlDate, time BlTime) BlDateTime
//     Combines an existing BlDate and BlTime into a BlDateTime.
//   Bl.Now(timezone ...string) BlDateTime
//     Current datetime in the given (or local) timezone. Pass "UTC" for current UTC.

// Parsing — accepts an ISO 8601 combined datetime string.
func (Bl) ToDateTime(value string) BlDateTime { ... }

// Date components — deferred; evaluate to BlNumber
func (dt *BlDateTime) Year() BlNumber { ... }

func (dt *BlDateTime) Month() BlNumber { ... }

func (dt *BlDateTime) Day() BlNumber { ... }

// Time components — deferred; evaluate to BlNumber or BlNull
func (dt *BlDateTime) Hour() BlNumber { ... }

func (dt *BlDateTime) Minute() BlNumber { ... }

func (dt *BlDateTime) Second() BlNumber { ... }     // includes fractional seconds

func (dt *BlDateTime) Offset() BlExpr { ... }       // evaluates to BlDaysTimeDuration or BlNull

func (dt *BlDateTime) Timezone() BlExpr { ... }     // evaluates to BlString or BlNull

// Calendar properties — deferred (delegates to date component)
func (dt *BlDateTime) DayOfWeek() BlString { ... }  // "Monday" … "Sunday"

func (dt *BlDateTime) DayOfYear() BlNumber { ... }

func (dt *BlDateTime) WeekOfYear() BlNumber { ... }

func (dt *BlDateTime) MonthOfYear() BlString { ... } // "January" … "December"

// Extraction — deferred
func (dt *BlDateTime) Date() *BlDate { ... }

func (dt *BlDateTime) Time() *BlTime { ... }

// Arithmetic — deferred; evaluate to BlDateTime
func (dt *BlDateTime) Add(duration BlExpr) *BlDateTime { ... }
func (dt *BlDateTime) Subtract(duration BlExpr) *BlDateTime { ... }

// Difference — deferred; evaluate to duration type
func (dt *BlDateTime) DiffYearsMonths(other BlExpr) *BlYearsMonthsDuration { ... }
func (dt *BlDateTime) DiffDaysTime(other BlExpr) *BlDaysTimeDuration { ... }

// Normalisation — deferred; evaluate to BlDateTime
func (dt *BlDateTime) ToUTC() *BlDateTime { ... }
func (dt *BlDateTime) WithOffset(offset BlExpr) *BlDateTime { ... }
func (dt *BlDateTime) WithTimezone(timezone BlExpr) *BlDateTime { ... }

// Comparison — deferred; evaluate to BlBoolean or BlNull
func (dt *BlDateTime) Equals(other BlExpr) BlExpr         { ... }
func (dt *BlDateTime) NotEqual(other BlExpr) BlExpr        { ... }
func (dt *BlDateTime) Before(other BlExpr) BlExpr          { ... }
func (dt *BlDateTime) After(other BlExpr) BlExpr           { ... }
func (dt *BlDateTime) BeforeOrEqual(other BlExpr) BlExpr   { ... }
func (dt *BlDateTime) AfterOrEqual(other BlExpr) BlExpr    { ... }

// Eager host-language utilities — only valid on a concrete BlDateTime after .evaluate()
func (dt *BlDateTime) CompareTo(other *BlDateTime) int { ... }  // -1, 0, or 1
func (dt *BlDateTime) String() string { ... }  // ISO 8601: "2025-03-28T14:30:00" or "2025-03-28T14:30:00+01:00"
```

## Deferred semantics

```go
deadline := Bl.DateTime(2025, 12, 31, 23, 59, 59)
expr := Bl.DateTimeVar("submittedAt").BeforeOrEqual(deadline)
result := expr.Evaluate(map[string]any{"submittedAt": Bl.ToDateTime("2025-11-01T10:00:00")})
// result == BlBoolean.TRUE
```

Property chaining:

```go
expr := Bl.DateTimeVar("event").Date().Year().Equals(Bl.Number(2025))
```

## ISO 8601 Parsing

`Bl.ToDateTime()` accepts:
- Local: `"2025-03-28T14:30:00"`, `"2025-03-28T14:30:00.500"`
- UTC: `"2025-03-28T14:30:00Z"`
- Offset: `"2025-03-28T14:30:00+01:00"`, `"2025-03-28T14:30:00-05:00"`
- IANA zone: `"2025-03-28T14:30:00@Europe/Paris"`

The `T` separator is required. Space-separated forms are not accepted.

## Arithmetic

- Adding a `BlYearsMonthsDuration` adjusts year and/or month (day clamped if needed), leaving time unchanged.
- Adding a `BlDaysTimeDuration` adds days, hours, minutes, seconds precisely, carrying across date boundaries.
- Mixed arithmetic is supported by chaining two `add()` calls.
- All arithmetic preserves the original timezone/offset.

## Comparison Semantics

- Two datetimes are compared as instants if both have timezone/offset (converted to UTC).
- Two local datetimes are compared wall-clock.
- Comparing a local datetime to a zoned/offset datetime evaluates to `BlNull`.

## Edge Cases

- `now()` and `now_utc()` depend on the system clock; use explicit construction for deterministic values in tests.
- `with_offset()` and `with_timezone()` convert to the same instant at the new offset/zone.
- `diff_days_time()` returns a negative duration if `self` is after `other`.
- `P1M` added to `2025-01-31T23:59:59` → `2025-02-28T23:59:59` (day clamped).
