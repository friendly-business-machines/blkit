---
name: BlDate
description: blkit's date type (modelled on FEEL) — a calendar date (year, month, day) with optional UTC offset or IANA timezone; extends BlExpr so all operations are deferred and chainable; includes blkit-specific business-day and calendar-aware arithmetic
targets:
  - ../../expr/date.go
---

# BlDate

`BlDate` represents a calendar date — year, month, day — with an optional UTC offset or IANA timezone identifier. It is modelled on FEEL's `date` type and uses the ISO 8601 format `YYYY-MM-DD`. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

A plain `BlDate` (no offset or timezone) is timezone-naive, matching FEEL. Attaching an offset or timezone is a blkit-specific addition not present in FEEL. Only one of `offset` or `timezone` may be set on a single instance; providing both raises `BlTypeError`.

```go
type BlDate struct {
    BlExpr
}

// Construction — see bl.spec.md.
//   Bl.Date(year, month, day int, opts ...DateOption) BlDate
//     opts may include WithOffset("+05:30") or WithTimezone("Europe/London")
//     (mutually exclusive). Raises BlTypeError if both are provided, or for
//     invalid month/day.
//   Bl.Today(timezone ...string) BlDate
//     Current date in the given (or local) timezone.

// Parsing — accepts ISO 8601 extended form:
//   "2025-03-28"                   — timezone-naive
//   "2025-03-28+05:30"             — UTC offset
//   "2025-03-28Z"                  — UTC (equivalent to "+00:00")
//   "2025-03-28[Europe/London]"    — IANA timezone
// Raises BlParseError on non-conforming input.
func (Bl) ToDate(value string) BlDate { ... }

// Extract the date component from a BlDateTime (deferred). The resulting
// BlDate carries through the offset/timezone of the source datetime.
func (Bl) DateFromDateTime(dt BlExpr) BlDate { ... }

// ------------------------------------------------------------------ //
// Structural metadata — eager (valid on a concrete BlDate)           //
// ------------------------------------------------------------------ //

// Offset  *string  // UTC offset string, e.g. "+05:30"; nil if not set
// Timezone *string // IANA timezone ID, e.g. "Europe/London"; nil if not set

// ------------------------------------------------------------------ //
// Components — deferred; evaluate to BlNumber                       //
// ------------------------------------------------------------------ //

func (d *BlDate) Year() BlNumber { ... }

func (d *BlDate) Month() BlNumber { ... } // 1–12

func (d *BlDate) Day() BlNumber { ... } // 1–31

// Calendar properties — deferred
func (d *BlDate) DayOfWeek() BlString { ... }   // "Monday" … "Sunday"

func (d *BlDate) DayOfYear() BlNumber { ... }   // 1–366

func (d *BlDate) WeekOfYear() BlNumber { ... }  // 1–53 (ISO 8601)

func (d *BlDate) MonthOfYear() BlString { ... } // "January" … "December"

// ------------------------------------------------------------------ //
// Arithmetic — deferred; evaluate to BlDate                         //
// ------------------------------------------------------------------ //

func (d *BlDate) Add(duration BlExpr) *BlDate { ... }
func (d *BlDate) Subtract(duration BlExpr) *BlDate { ... }

func (d *BlDate) AddBusinessDays(
    days BlExpr,
    calendar BlExpr,
    ignoreCalendarRangeErrors bool,
) *BlDate { ... }
// Advances self by the given number of business days (weekdays not in calendar).
// `days` must evaluate to a non-negative BlNumber integer.
// Raises BlCalendarRangeError if iteration goes outside calendar.ValidFrom/ValidTo,
// unless ignoreCalendarRangeErrors is true (treats out-of-range dates as non-holidays).

func (d *BlDate) SubtractBusinessDays(
    days BlExpr,
    calendar BlExpr,
    ignoreCalendarRangeErrors bool,
) *BlDate { ... }
// Retreats self by the given number of business days. Symmetric to AddBusinessDays.

// ------------------------------------------------------------------ //
// Difference — deferred                                               //
// ------------------------------------------------------------------ //

func (d *BlDate) DiffYearsMonths(other BlExpr) *BlYearsMonthsDuration { ... }
// Evaluates to BlYearsMonthsDuration. Positive if other is after self.

func (d *BlDate) DiffDaysTime(other BlExpr) *BlDaysTimeDuration { ... }
// Evaluates to BlDaysTimeDuration (whole days only). Positive if other is after self.

func (d *BlDate) WeekdaysBetween(other BlExpr) BlNumber { ... }
// Evaluates to BlNumber: the count of Monday–Friday days in [self, other] inclusive.
// Always non-negative regardless of which date is earlier.

func (d *BlDate) BusinessDaysBetween(other BlExpr, calendar BlExpr) BlNumber { ... }
// Evaluates to BlNumber: WeekdaysBetween minus calendar holidays in the same range.
// Always non-negative.

// ------------------------------------------------------------------ //
// Month boundary — deferred; evaluate to BlDate                     //
// ------------------------------------------------------------------ //

func (d *BlDate) FirstDayOfMonth() *BlDate { ... }
// The first day of self's month (always day 1).

func (d *BlDate) LastDayOfMonth() *BlDate { ... }
// The last day of self's month (28, 29, 30, or 31 depending on month/year).

func (d *BlDate) LastDayOfPrevMonth() *BlDate { ... }
// The last day of the month immediately before self's month.

func (d *BlDate) FirstDayOfNextMonth() *BlDate { ... }
// The first day of the month immediately after self's month (always day 1).

// ------------------------------------------------------------------ //
// Week-in-month navigation — deferred; evaluate to BlDate           //
// ------------------------------------------------------------------ //

func (d *BlDate) FirstDayOfWeekInMonth(dayOfWeek BlExpr) *BlDate { ... }
// First occurrence of dayOfWeek in self's month/year.
// dayOfWeek evaluates to BlString: "Monday", "Tuesday", … "Sunday".

func (d *BlDate) LastDayOfWeekInMonth(dayOfWeek BlExpr) *BlDate { ... }
// Last occurrence of dayOfWeek in self's month/year.

func (d *BlDate) NthDayOfWeekInMonth(n BlExpr, dayOfWeek BlExpr) *BlDate { ... }
// n-th occurrence of dayOfWeek in self's month/year.
// n is 1-based from the start of the month; negative n counts from the end
// (-1 = last, -2 = second-to-last).
// Evaluates to BlNull if |n| exceeds the number of occurrences in the month.

// ------------------------------------------------------------------ //
// Day navigation — deferred; evaluate to BlDate                     //
// ------------------------------------------------------------------ //

func (d *BlDate) NextDayOfWeek(dayOfWeek BlExpr) *BlDate { ... }
// Next date strictly after self whose dayOfWeek matches.
// If self is already that day, returns the next occurrence 7 days later.

func (d *BlDate) PrevDayOfWeek(dayOfWeek BlExpr) *BlDate { ... }
// Previous date strictly before self whose dayOfWeek matches.

func (d *BlDate) NextWeekday() *BlDate { ... }
// Next Monday–Friday date strictly after self.
// If self is Friday, returns the following Monday.
// If self is Saturday, returns Monday (+2 days); if Sunday, returns Monday (+1 day).

func (d *BlDate) PrevWeekday() *BlDate { ... }
// Previous Monday–Friday date strictly before self.

func (d *BlDate) NextBusinessDay(calendar BlExpr) *BlDate { ... }
// Next date strictly after self that is both a weekday and absent from calendar.

func (d *BlDate) PrevBusinessDay(calendar BlExpr) *BlDate { ... }
// Previous date strictly before self that is both a weekday and absent from calendar.

// ------------------------------------------------------------------ //
// Day classification — deferred; evaluate to BlBoolean             //
// ------------------------------------------------------------------ //

func (d *BlDate) IsWeekday() BlExpr { ... } // true if Monday–Friday
func (d *BlDate) IsWeekend() BlExpr { ... } // true if Saturday or Sunday

func (d *BlDate) IsHoliday(calendar BlExpr) BlExpr { ... }
// True if calendar.Contains(self) evaluates to BlBoolean.TRUE.

// ------------------------------------------------------------------ //
// Comparison — deferred; evaluate to BlBoolean                      //
// ------------------------------------------------------------------ //

func (d *BlDate) Equals(other BlExpr) BlExpr         { ... }
func (d *BlDate) NotEqual(other BlExpr) BlExpr        { ... }
func (d *BlDate) Before(other BlExpr) BlExpr          { ... }
func (d *BlDate) After(other BlExpr) BlExpr           { ... }
func (d *BlDate) BeforeOrEqual(other BlExpr) BlExpr   { ... }
func (d *BlDate) AfterOrEqual(other BlExpr) BlExpr    { ... }

func (d *BlDate) Between(min BlExpr, max BlExpr) BlExpr { ... }
// True if min <= self <= max (both bounds inclusive).

func (d *BlDate) In(test BlExpr) BlExpr { ... }
// Membership test. test may evaluate to:
//   - BlList of BlDate or BlRange values
//   - BlRange
//   - BlCalendar (equivalent to calendar.Contains(self))
//   - BlList
// Evaluates to BlBoolean.

// ------------------------------------------------------------------ //
// Range algebra (modelled on DMN 1.4 §10.3.2) — deferred; → BlBoolean //
// ------------------------------------------------------------------ //

func (d *BlDate) Coincides(other BlExpr) BlExpr { ... }
// other is BlDate: True if self == other.
// other is BlRange: True if the range is a single-point range [x..x] and self == x.

func (d *BlDate) Starts(other BlExpr) BlExpr { ... }
// other must be a BlRange. True if self equals the range's start and
// the range's StartIncluded is true.

func (d *BlDate) During(other BlExpr) BlExpr { ... }
// other must be a BlRange. True if self is contained within the range,
// respecting boundary inclusions. A point at the start satisfies both Starts()
// and During(); a point at the end satisfies both Finishes() and During().

func (d *BlDate) Finishes(other BlExpr) BlExpr { ... }
// other must be a BlRange. True if self equals the range's end and
// the range's EndIncluded is true.

// ------------------------------------------------------------------ //
// Timezone/offset removal — deferred; evaluate to BlDate            //
// ------------------------------------------------------------------ //

func (d *BlDate) WithoutOffset() *BlDate { ... }
// Returns a new BlDate with the same year/month/day and timezone (if any),
// but with the UTC offset removed. No-op if no offset is set.

func (d *BlDate) WithoutTimezone() *BlDate { ... }
// Returns a new BlDate with the same year/month/day and offset (if any),
// but with the IANA timezone removed. No-op if no timezone is set.

func (d *BlDate) WithoutOffsetOrTimezone() *BlDate { ... }
// Returns a plain, timezone-naive BlDate with the same year/month/day only.

// ------------------------------------------------------------------ //
// Combination — deferred; evaluate to BlDateTime                    //
// ------------------------------------------------------------------ //

func (d *BlDate) AtTime(time BlExpr) *BlDateTime { ... }

// ------------------------------------------------------------------ //
// Eager host-language utilities (call only after .Evaluate())          //
// ------------------------------------------------------------------ //

func (d *BlDate) CompareTo(other *BlDate) int { ... } // -1, 0, or 1
func (d *BlDate) String() string { ... }               // ISO 8601 representation
```

---

## Construction

### `Bl.Date(year, month, day, *, offset?, timezone?)`

Creates a concrete date value.

```go
Bl.Date(2025, 3, 28)
// → BlDate representing 2025-03-28 (timezone-naive)

Bl.Date(2025, 3, 28, WithOffset("+05:30"))
// → BlDate representing 2025-03-28+05:30

Bl.Date(2025, 3, 28, WithTimezone("Europe/London"))
// → BlDate representing 2025-03-28[Europe/London]

Bl.Date(2025, 3, 28, WithOffset("+05:30"), WithTimezone("Asia/Kolkata"))
// → raises BlTypeError (cannot supply both)
```

### `Bl.ToDate(value)`

Parses an ISO 8601 date string. Accepts timezone-naive, UTC-offset, and IANA-timezone forms.

```go
Bl.ToDate("2025-03-28")
// → Bl.Date(2025, 3, 28)

Bl.ToDate("2025-03-28+05:30")
// → Bl.Date(2025, 3, 28, WithOffset("+05:30"))

Bl.ToDate("2025-03-28Z")
// → Bl.Date(2025, 3, 28, WithOffset("+00:00"))

Bl.ToDate("2025-03-28[Europe/London]")
// → Bl.Date(2025, 3, 28, WithTimezone("Europe/London"))

Bl.ToDate("2025-13-01")
// → raises BlParseError (month 13 is invalid)
```

### `BlDate.today(timezone?)`

```go
Bl.Today()
// → current date in the local system timezone

Bl.Today("America/New_York")
// → current date in the Eastern timezone
```

---

## Offset and Timezone

`offset` and `timezone` are eager structural properties — they can be read directly without calling `.evaluate()`.

```go
d := Bl.Date(2025, 3, 28, WithOffset("+05:30"))
d.Offset    // → "+05:30"
d.Timezone  // → nil

d2 := Bl.Date(2025, 3, 28, WithTimezone("Europe/London"))
d2.Offset    // → nil
d2.Timezone  // → "Europe/London"
```

The offset value `"Z"` is normalised to `"+00:00"` at parse time.

### Stripping timezone information

```go
Bl.Date(2025, 3, 28, WithOffset("+05:30")).WithoutOffset().Evaluate()
// → Bl.Date(2025, 3, 28)   (offset removed, same date)

Bl.Date(2025, 3, 28, WithTimezone("Europe/London")).WithoutTimezone().Evaluate()
// → Bl.Date(2025, 3, 28)   (timezone removed)

Bl.Date(2025, 3, 28, WithOffset("+05:30")).WithoutOffsetOrTimezone().Evaluate()
// → Bl.Date(2025, 3, 28)   (equivalent; no timezone was set anyway)

// WithoutOffset is a no-op when no offset is set
Bl.Date(2025, 3, 28).WithoutOffset().Evaluate()
// → Bl.Date(2025, 3, 28)   (unchanged)
```

---

## Deferred Semantics

```go
expr := Bl.Date(2025, 1, 31).Add(Bl.YearsMonths(0, 1))
result, err := expr.Evaluate()
// result == Bl.Date(2025, 2, 28)  (day clamped to last valid day)
```

Component properties are deferred and can be chained:

```go
expr := Bl.DateVar("startDate").Year().Equals(Bl.Number(2025))
result, err := expr.Evaluate(map[string]any{"startDate": Bl.Date(2025, 6, 1)})
// result == BlBoolean.TRUE
```

---

## ISO 8601 Parsing

`Bl.ToDate()` accepts the extended form only: `"2025-03-28"`. Basic form (`"20250328"`) is not supported. Negative years are accepted: `"-0500-01-01"`. A non-conforming string produces a `BlParseError`.

---

## Date Arithmetic

### `add(duration)` / `subtract(duration)`

Adding or subtracting a `BlYearsMonthsDuration` adjusts the year and/or month, clamping the day to the last valid day of the resulting month (e.g. `2025-01-31 + P1M` → `2025-02-28`).

Adding or subtracting a `BlDaysTimeDuration` adds only the whole-day component — hours, minutes, and seconds are ignored for `BlDate` arithmetic. Use `BlDateTime` for sub-day precision.

```go
Bl.Date(2025, 1, 15).Add(Bl.YearsMonths(0, 3)).Evaluate()
// → Bl.Date(2025, 4, 15)

Bl.Date(2025, 1, 31).Add(Bl.YearsMonths(0, 1)).Evaluate()
// → Bl.Date(2025, 2, 28)   (clamped)

Bl.Date(2025, 3, 28).Subtract(Bl.DaysTime(7, 0, 0, 0)).Evaluate()
// → Bl.Date(2025, 3, 21)
```

### `add_business_days(days, calendar, ignore_calendar_range_errors?)`

Advances self by the given number of business days (Mon–Fri, not in calendar). Each calendar day is checked: if it is a weekday and not a calendar holiday, it counts.

```go
ukHolidays := Bl.Calendar(
    Bl.CalendarEntry(Bl.Date(2025, 4, 18), "Good Friday"),
    Bl.CalendarEntry(Bl.Date(2025, 4, 21), "Easter Monday"),
    Bl.Date(2025, 1, 1),  // validFrom
    Bl.Date(2025, 12, 31), // validTo
)

// Thursday 17 Apr + 2 business days → skips Good Friday (Fri 18) → Wed 23 Apr
Bl.Date(2025, 4, 17).AddBusinessDays(
    Bl.Number(2), ukHolidays, false,
).Evaluate()
// → Bl.Date(2025, 4, 23)

// Fri 19 Dec + 10 business days — crosses Christmas/Boxing Day and New Year
Bl.Date(2025, 12, 19).AddBusinessDays(
    Bl.Number(10), ukHolidays, false,
).Evaluate()
// → raises BlCalendarRangeError (result goes past 2025-12-31)

Bl.Date(2025, 12, 19).AddBusinessDays(
    Bl.Number(10), ukHolidays, true,
).Evaluate()
// → Bl.Date(2026, 1, 2)  (continues past year boundary, no holidays assumed for Jan 2026)
```

### `subtract_business_days(days, calendar, ignore_calendar_range_errors?)`

Symmetric to `add_business_days`. Retreats self by counting backwards through business days.

```go
Bl.Date(2025, 4, 23).SubtractBusinessDays(
    Bl.Number(2), ukHolidays, false,
).Evaluate()
// → Bl.Date(2025, 4, 17)   (skips Easter Monday Tue 21, Good Friday Fri 18)
```

---

## Difference

### `diff_years_months(other)` / `diff_days_time(other)`

- `diff_years_months(other)` — returns whole years and months between two dates.
- `diff_days_time(other)` — returns number of whole days (time component is zero).
- Both return a positive duration when `other` is after `self`, negative when before.

```go
Bl.Date(2025, 1, 1).DiffDaysTime(Bl.Date(2025, 3, 28)).Evaluate()
// → BlDaysTimeDuration of 86 days

Bl.Date(2025, 3, 28).DiffYearsMonths(Bl.Date(2026, 6, 15)).Evaluate()
// → BlYearsMonthsDuration of 1 year 2 months
```

### `weekdays_between(other)`

Counts Monday–Friday days in `[min(self, other), max(self, other)]` inclusive. Order of arguments does not affect the result.

```go
// Monday to Friday (same week): 5 weekdays
Bl.Date(2025, 3, 24).WeekdaysBetween(Bl.Date(2025, 3, 28)).Evaluate()
// → BlNumber("5")

// Friday to following Monday: Fri + Mon = 2 weekdays (Sat/Sun excluded)
Bl.Date(2025, 3, 28).WeekdaysBetween(Bl.Date(2025, 3, 31)).Evaluate()
// → BlNumber("2")

// Same date: 1 weekday (if it's a weekday), or 0 (if weekend)
Bl.Date(2025, 3, 29).WeekdaysBetween(Bl.Date(2025, 3, 29)).Evaluate()
// → BlNumber("0")   (Saturday)
```

### `business_days_between(other, calendar)`

`weekdays_between` minus any calendar holidays that fall within the same inclusive range.

```go
// Easter week (Good Friday 18 Apr, Easter Monday 21 Apr are both holidays)
// Mon 14 Apr to Fri 25 Apr: 10 weekdays, minus 2 holidays = 8 business days
Bl.Date(2025, 4, 14).BusinessDaysBetween(
    Bl.Date(2025, 4, 25), ukHolidays,
).Evaluate()
// → BlNumber("8")
```

---

## Month Boundary Methods

```go
Bl.Date(2025, 2, 14).FirstDayOfMonth().Evaluate()
// → Bl.Date(2025, 2, 1)

Bl.Date(2025, 2, 14).LastDayOfMonth().Evaluate()
// → Bl.Date(2025, 2, 28)

Bl.Date(2024, 2, 14).LastDayOfMonth().Evaluate()
// → Bl.Date(2024, 2, 29)   (2024 is a leap year)

Bl.Date(2025, 3, 15).LastDayOfPrevMonth().Evaluate()
// → Bl.Date(2025, 2, 28)

Bl.Date(2025, 1, 1).LastDayOfPrevMonth().Evaluate()
// → Bl.Date(2024, 12, 31)

Bl.Date(2025, 2, 14).FirstDayOfNextMonth().Evaluate()
// → Bl.Date(2025, 3, 1)

Bl.Date(2025, 12, 31).FirstDayOfNextMonth().Evaluate()
// → Bl.Date(2026, 1, 1)
```

---

## Week-in-Month Navigation

`day_of_week` arguments must evaluate to a `BlString` using the English full name: `"Monday"`, `"Tuesday"`, `"Wednesday"`, `"Thursday"`, `"Friday"`, `"Saturday"`, or `"Sunday"`. An unrecognised value raises `BlTypeError` at evaluation time.

### `first_day_of_week_in_month(day_of_week)` / `last_day_of_week_in_month(day_of_week)`

```go
// First Monday of March 2025
Bl.Date(2025, 3, 15).FirstDayOfWeekInMonth(Bl.String("Monday")).Evaluate()
// → Bl.Date(2025, 3, 3)

// Last Friday of March 2025
Bl.Date(2025, 3, 15).LastDayOfWeekInMonth(Bl.String("Friday")).Evaluate()
// → Bl.Date(2025, 3, 28)

// Last Sunday of February 2025
Bl.Date(2025, 2, 1).LastDayOfWeekInMonth(Bl.String("Sunday")).Evaluate()
// → Bl.Date(2025, 2, 23)
```

### `nth_day_of_week_in_month(n, day_of_week)`

```go
// 2nd Monday of March 2025
Bl.Date(2025, 3, 15).NthDayOfWeekInMonth(
    Bl.Number(2), Bl.String("Monday"),
).Evaluate()
// → Bl.Date(2025, 3, 10)

// Last (-1) Tuesday of February 2025
Bl.Date(2025, 2, 1).NthDayOfWeekInMonth(
    Bl.Number(-1), Bl.String("Tuesday"),
).Evaluate()
// → Bl.Date(2025, 2, 25)

// 5th Monday of February 2025 — February 2025 has only 4 Mondays
Bl.Date(2025, 2, 1).NthDayOfWeekInMonth(
    Bl.Number(5), Bl.String("Monday"),
).Evaluate()
// → BlNull
```

---

## Day Navigation

### `next_day_of_week(day_of_week)` / `prev_day_of_week(day_of_week)`

Always returns a date strictly after (or before) self. If self is already the requested day, skips to the next (or previous) occurrence.

```go
// Monday 24 Mar — next Monday is 31 Mar
Bl.Date(2025, 3, 24).NextDayOfWeek(Bl.String("Monday")).Evaluate()
// → Bl.Date(2025, 3, 31)

// Tuesday 25 Mar — next Friday is 28 Mar
Bl.Date(2025, 3, 25).NextDayOfWeek(Bl.String("Friday")).Evaluate()
// → Bl.Date(2025, 3, 28)

// Tuesday 25 Mar — previous Monday is 24 Mar
Bl.Date(2025, 3, 25).PrevDayOfWeek(Bl.String("Monday")).Evaluate()
// → Bl.Date(2025, 3, 24)
```

### `next_weekday()` / `prev_weekday()`

Returns the immediately adjacent weekday, skipping over weekends.

```go
Bl.Date(2025, 3, 28).NextWeekday().Evaluate()   // Friday → Monday
// → Bl.Date(2025, 3, 31)

Bl.Date(2025, 3, 29).NextWeekday().Evaluate()   // Saturday → Monday
// → Bl.Date(2025, 3, 31)

Bl.Date(2025, 3, 30).NextWeekday().Evaluate()   // Sunday → Monday
// → Bl.Date(2025, 3, 31)

Bl.Date(2025, 3, 31).PrevWeekday().Evaluate()   // Monday → Friday
// → Bl.Date(2025, 3, 28)
```

### `next_business_day(calendar)` / `prev_business_day(calendar)`

Like `next_weekday`/`prev_weekday` but also skips calendar holidays.

```go
// Thursday 17 Apr 2025 — next day is Good Friday (holiday), skip to Monday
Bl.Date(2025, 4, 17).NextBusinessDay(ukHolidays).Evaluate()
// → Bl.Date(2025, 4, 22)   (Tuesday — Easter Monday 21 Apr also skipped)

Bl.Date(2025, 4, 22).PrevBusinessDay(ukHolidays).Evaluate()
// → Bl.Date(2025, 4, 17)   (Thursday — skips Easter Monday and Good Friday)
```

---

## Day Classification

```go
Bl.Date(2025, 3, 24).IsWeekday().Evaluate()   // Monday
// → BlBoolean.TRUE

Bl.Date(2025, 3, 29).IsWeekday().Evaluate()   // Saturday
// → BlBoolean.FALSE

Bl.Date(2025, 3, 29).IsWeekend().Evaluate()   // Saturday
// → BlBoolean.TRUE

Bl.Date(2025, 4, 18).IsHoliday(ukHolidays).Evaluate()   // Good Friday
// → BlBoolean.TRUE

Bl.Date(2025, 4, 19).IsHoliday(ukHolidays).Evaluate()   // Saturday — not in calendar
// → BlBoolean.FALSE
```

---

## Comparison and Range Algebra

### `between(min, max)`

```go
Bl.Date(2025, 3, 15).Between(
    Bl.Date(2025, 1, 1), Bl.Date(2025, 12, 31),
).Evaluate()
// → BlBoolean.TRUE

Bl.Date(2025, 1, 1).Between(
    Bl.Date(2025, 1, 1), Bl.Date(2025, 12, 31),
).Evaluate()
// → BlBoolean.TRUE   (both bounds inclusive)
```

### `in_(test)`

```go
// List of dates
Bl.Date(2025, 12, 25).In(
    Bl.List(
        Bl.Date(2025, 12, 25),
        Bl.Date(2025, 12, 26),
    ),
).Evaluate()
// → BlBoolean.TRUE

// BlRange
Bl.Date(2025, 6, 15).In(
    Bl.Range(Bl.Date(2025, 1, 1), Bl.Date(2025, 12, 31), true, true),
).Evaluate()
// → BlBoolean.TRUE

// BlCalendar
Bl.Date(2025, 4, 18).In(ukHolidays).Evaluate()
// → BlBoolean.TRUE   (Good Friday is in the calendar)
```

### Range Algebra (modelled on DMN 1.4)

```go
q2 := Bl.Range(Bl.Date(2025, 4, 1), Bl.Date(2025, 6, 30), true, true)

Bl.Date(2025, 4, 1).Starts(q2).Evaluate()
// → BlBoolean.TRUE   (equals inclusive start)

Bl.Date(2025, 6, 30).Finishes(q2).Evaluate()
// → BlBoolean.TRUE   (equals inclusive end)

Bl.Date(2025, 5, 15).During(q2).Evaluate()
// → BlBoolean.TRUE   (contained within range)

Bl.Date(2025, 4, 1).During(q2).Evaluate()
// → BlBoolean.TRUE   (boundary is included, so During() is also true)

Bl.Date(2025, 3, 31).During(q2).Evaluate()
// → BlBoolean.FALSE  (before range start)

// coincides: point vs point
Bl.Date(2025, 6, 1).Coincides(Bl.Date(2025, 6, 1)).Evaluate()
// → BlBoolean.TRUE

// coincides: point vs single-point range [2025-06-01..2025-06-01]
Bl.Date(2025, 6, 1).Coincides(
    Bl.Range(Bl.Date(2025, 6, 1), Bl.Date(2025, 6, 1), true, true),
).Evaluate()
// → BlBoolean.TRUE

Bl.Date(2025, 6, 1).Coincides(
    Bl.Range(Bl.Date(2025, 6, 1), Bl.Date(2025, 6, 2), true, true),
).Evaluate()
// → BlBoolean.FALSE  (range spans more than one point)
```

---

## Day/Week Properties

- `day_of_week` evaluates to the English full name: `"Monday"`, …, `"Sunday"`.
- `week_of_year` follows ISO 8601 week numbering (Monday is the first day of the week; week 1 contains the first Thursday of the year).
- `month_of_year` evaluates to the English full name: `"January"`, …, `"December"`.

---

## Edge Cases

- Month values outside `1–12` in `of()` produce a `BlTypeError`.
- Day values outside the valid range for the given month produce a `BlTypeError` (no silent day-rollover at construction time).
- `today()` returns the date in the local system timezone when no timezone argument is supplied.
- Year zero is a valid `BlDate` value (ISO 8601 proleptic Gregorian calendar, matching FEEL): year `0` is 1 BCE.
- `add_business_days` and `subtract_business_days` with `days` equal to `0` return self unchanged.
- `next_business_day` / `prev_business_day` always return a date strictly different from self — they never return self even if self is already a business day.
- `nth_day_of_week_in_month` with `n = 0` produces a `BlTypeError`.
- An unrecognised `day_of_week` string (e.g. `"Mon"`, `"monday"`) in week navigation methods raises `BlTypeError` at evaluation time.
- Comparing a timezone-aware `BlDate` with a timezone-naive one using `before`, `after`, etc. evaluates to `BlNull` — timezone-aware and timezone-naive dates are not implicitly coerced.
- `without_timezone()` on a date that has an offset (not a timezone) is a no-op; `without_offset()` on a date that has a timezone is a no-op. Use `without_offset_or_timezone()` to unconditionally strip both.
