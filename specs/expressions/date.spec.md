---
name: BlDate
description: The date type in the blkit expression language — a calendar date with optional offset/timezone, including business-day and calendar-aware arithmetic. Covers date literals, component access, the (large) date built-in library incl. blkit extensions, and the Go layer (BlDate + expr registrations).
targets:
  - ../../expr/date.go
---

# BlDate — the `date` type

`date` is a calendar date (year, month, day) with an optional UTC offset **or** IANA timezone (ISO
8601 `YYYY-MM-DD`). The Go value type backing it is `BlDate`. A plain date is timezone-naive; only
one of offset/timezone may be set.

`date` carries blkit's richest built-in surface — including **business-day and calendar-aware
arithmetic**, much of which extends DMN FEEL (flagged **ext**). See
[bl-expr.spec.md](bl-expr.spec.md) for the engine and component access, and
[calendar.spec.md](calendar.spec.md) for the calendar values these functions consume.

---

## Literals / construction

```
date("2025-03-28")               // timezone-naive
date("2025-03-28+05:30")         // UTC offset
date("2025-03-28Z")              // UTC (offset +00:00)
date("2025-03-28[Europe/London]")// IANA timezone
date(2025, 3, 28)                // from components
date(datetime("2025-03-28T14:30:00")) // extract date from a datetime
today()                          // current date (local zone)
```

Only the extended form is parsed; supplying both offset and timezone, or an invalid month/day, →
`BlTypeError`/`BlParseError`.

`[@test] ../../expr/date_test.go`

---

## Component access

```
date("2025-03-28+05:30").year      // → 2025
date("2025-03-28").month           // → 3
date("2025-03-28").day             // → 28
date("2025-03-28+05:30").offset    // → "+05:30"  (ext; null if none)
date("2025-03-28[Europe/London]").timezone // → "Europe/London" (ext; null if none)
```

`[@test] ../../expr/date_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration (day clamped for years-months) | `date("2025-01-31") + duration("P1M")` | `date("2025-02-28")` |
| `-` (date) | days-time difference | `date("2025-03-28") - date("2025-01-01")` | `duration("P86D")` |
| `< <= > >= = !=` | comparison | `date("2025-01-01") < date("2025-06-01")` | `true` |
| `between a and b` | inclusive range | `date("2025-03-15") between date("2025-01-01") and date("2025-12-31")` | `true` |
| `in` | membership (list / range / calendar) | `date("2025-04-18") in ukHolidays` | `true` |

A years-months duration adjusts year/month (day clamped); a days-time duration adds whole days (sub-
day components ignored for `date`). Comparing a tz-aware date with a tz-naive one → `null`.

`[@test] ../../expr/date_operators_test.go`

---

## Built-in functions

Standard DMN functions plus blkit extensions (**ext**). Day-of-week arguments are English full names
(`"Monday"`…`"Sunday"`); month names likewise.

### Calendar properties

| Function | Example | Result |
|---|---|---|
| `dayOfWeek(d)` | `dayOfWeek(date("2025-03-24"))` | `"Monday"` |
| `dayOfYear(d)` | `dayOfYear(date("2019-09-17"))` | `260` |
| `weekOfYear(d)` | `weekOfYear(date("2019-09-17"))` | `38` (ISO 8601) |
| `monthOfYear(d)` | `monthOfYear(date("2019-09-17"))` | `"September"` |

### Classification (**ext**)

| Function | Example | Result |
|---|---|---|
| `isWeekday(d)` | `isWeekday(date("2025-03-24"))` | `true` |
| `isWeekend(d)` | `isWeekend(date("2025-03-29"))` | `true` |
| `isPublicHoliday(d, phCalendar)` | `isPublicHoliday(date("2025-04-18"), ukHolidays)` | `true` |
| `isBusinessDay(d, phCalendar)` | `isBusinessDay(date("2025-04-18"), ukHolidays)` | `false` (weekday and not in `phCalendar`; Good Friday → false) |

### Month boundaries

| Function | Example | Result |
|---|---|---|
| `lastDayOfMonth(d)` | `lastDayOfMonth(date("2024-02-10"))` | `date("2024-02-29")` |
| `firstDayOfMonth(d)` **ext** | `firstDayOfMonth(date("2025-02-14"))` | `date("2025-02-01")` |
| `lastDayOfPrevMonth(d)` **ext** | `lastDayOfPrevMonth(date("2025-01-01"))` | `date("2024-12-31")` |
| `firstDayOfNextMonth(d)` **ext** | `firstDayOfNextMonth(date("2025-12-31"))` | `date("2026-01-01")` |

### Week-in-month navigation (**ext**)

| Function | Example | Result |
|---|---|---|
| `firstDayOfWeekInMonth(d, dow)` | `firstDayOfWeekInMonth(date("2025-03-15"), "Monday")` | `date("2025-03-03")` |
| `lastDayOfWeekInMonth(d, dow)` | `lastDayOfWeekInMonth(date("2025-03-15"), "Friday")` | `date("2025-03-28")` |
| `nthDayOfWeekInMonth(d, n, dow)` | `nthDayOfWeekInMonth(date("2025-03-15"), 2, "Monday")` | `date("2025-03-10")` (`n<0` from end; out of range → `null`) |

### Day navigation (**ext**)

| Function | Example | Result |
|---|---|---|
| `nextDayOfWeek(d, dow)` / `prevDayOfWeek(d, dow)` | `nextDayOfWeek(date("2025-03-24"), "Monday")` | `date("2025-03-31")` (strictly after) |
| `nextWeekday(d)` / `prevWeekday(d)` | `nextWeekday(date("2025-03-28"))` | `date("2025-03-31")` (Fri → Mon) |
| `nextBusinessDay(d, phCalendar)` / `prevBusinessDay(d, phCalendar)` | `nextBusinessDay(date("2025-04-17"), ukHolidays)` | `date("2025-04-22")` (skips weekend + holidays) |

### Business-day arithmetic & difference (**ext**)

| Function | Example | Result |
|---|---|---|
| `addBusinessDays(d, n, phCalendar[, ignoreRangeErrors])` | `addBusinessDays(date("2025-04-17"), 2, ukHolidays)` | `date("2025-04-23")` |
| `subtractBusinessDays(d, n, phCalendar[, ignoreRangeErrors])` | `subtractBusinessDays(date("2025-04-23"), 2, ukHolidays)` | `date("2025-04-17")` |
| `weekdaysBetween(a, b)` | `weekdaysBetween(date("2025-03-24"), date("2025-03-28"))` | `5` (inclusive; order-independent) |
| `businessDaysBetween(a, b, phCalendar)` | `businessDaysBetween(date("2025-04-14"), date("2025-04-25"), ukHolidays)` | `8` |
| `yearsAndMonthsDuration(a, b)` | `yearsAndMonthsDuration(date("2025-03-28"), date("2026-06-15"))` | `duration("P1Y2M")` |

Out-of-`phCalendar`-range iteration raises `BlCalendarRangeError` unless `ignoreRangeErrors` is true
(see [calendar.spec.md](calendar.spec.md)).

### Zone stripping (**ext**)

| Function | Example | Result |
|---|---|---|
| `withoutOffset(d)` | strips a UTC offset (no-op if none) | a date |
| `withoutTimezone(d)` | strips an IANA timezone (no-op if none) | a date |
| `withoutOffsetOrTimezone(d)` | strips both → plain naive date | a date |

`[@test] ../../expr/date_functions_test.go`

### Interval algebra & combination

A date is a *point*; the interval-algebra built-ins (`coincides`, `starts`, `during`, `finishes`,
`before`, `after`, …) are in [range.spec.md](range.spec.md), e.g.
`during(date("2025-05-15"), [date("2025-04-01")..date("2025-06-30")]) // → true`. Combine a date and
time with `datetime(date, time)` ([datetime.spec.md](datetime.spec.md)).

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Date(y,m,d, opts)` | `date(y, m, d)` / `date("…")` (offset/zone via string) |
| `Bl.ToDate(str)` / `Bl.DateFromDateTime(dt)` / `Bl.Today` | `date("…")` / `date(dt)` / `today()` |
| `year`/`month`/`day` | `.year`/`.month`/`.day` |
| `offset`/`timezone` | `.offset` **ext** / `.timezone` **ext** |
| `dayOfWeek`/`dayOfYear`/`weekOfYear`/`monthOfYear` | same-named built-ins |
| `add`/`subtract` | `+`/`-` (duration) |
| `addBusinessDays`/`subtractBusinessDays` | same-named built-ins **ext** |
| `diffYearsMonths`/`diffDaysTime` | `yearsAndMonthsDuration(a,b)` / `b - a` |
| `weekdaysBetween`/`businessDaysBetween` | same-named built-ins **ext** |
| `firstDayOfMonth`/`lastDayOfMonth`/`lastDayOfPrevMonth`/`firstDayOfNextMonth` | built-ins (`lastDayOfMonth` standard; rest **ext**) |
| `firstDayOfWeekInMonth`/`lastDayOfWeekInMonth`/`nthDayOfWeekInMonth` | same-named built-ins **ext** |
| `nextDayOfWeek`/`prevDayOfWeek`/`nextWeekday`/`prevWeekday`/`nextBusinessDay`/`prevBusinessDay` | same-named built-ins **ext** |
| `isWeekday` / `isWeekend` | same-named built-ins **ext** |
| `isHoliday(calendar)` | `isPublicHoliday(d, phCalendar)` **ext** (renamed) |
| `equals`/`notEqual`/`before`/`after`/`beforeOrEqual`/`afterOrEqual` | `=` `!=` `<` `>` `<=` `>=` |
| `between` / `in` | `between a and b` / `in` operators |
| `coincides`/`starts`/`during`/`finishes` | interval-algebra built-ins ([range.spec.md](range.spec.md)) |
| `withoutOffset`/`withoutTimezone`/`withoutOffsetOrTimezone` | same-named built-ins **ext** |
| `atTime(t)` | `datetime(date, time)` |
| `compareTo`/`String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/date.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
// BlDate stores a calendar date plus an optional UTC offset OR IANA zone (not both).
type BlDate struct{ y, m, d int; offset *string; zone *string }

func (BlDate) Type() BlType { return BlTypeDate }
func (d BlDate) Equal(other BlValue) BlValue
func (d BlDate) ToMarkdown() string
func (BlDate) isBlValue() {}

// Host constructors / accessors
func Date(year, month, day int, opts ...DateOption) (BlDate, error) // WithOffset / WithTimezone
func Today(tz ...string) BlDate
func (d BlDate) CompareTo(other BlDate) int
func (d BlDate) String() string  // ISO 8601
```

### Operator impl funcs (unexported)

```go
func addDateYM(d BlDate, dur BlYearsMonthsDuration) BlDate      // "+" (day clamped)
func addDateDT(d BlDate, dur BlDaysTimeDuration) BlDate         // "+" (whole days)
func subDateDur(d BlDate, dur BlValue) BlValue                  // "-" (either duration)
func subDates(a, b BlDate) BlDaysTimeDuration                   // "-" date−date
func ltDates(a, b BlDate) BlValue                               // "<"; le/gt/ge; tz-aware vs naive → Null
```

Bound to `+`/`-`/comparisons in `operatorBindings()`. `between`/`in` are patcher-lowered; `in`
dispatches to list/range membership or `calendarContainsFn` for a calendar operand.

### Registrations (`dateOptions`, unexported)

```go
func dateOptions() []expr.Option {
    return []expr.Option{
        // operator impls (named for operatorBindings)
        expr.Function("addDateYM", typed2(addDateYM), new(func(BlDate, BlYearsMonthsDuration) BlDate)),
        expr.Function("addDateDT", typed2(addDateDT), new(func(BlDate, BlDaysTimeDuration) BlDate)),
        expr.Function("subDates",  typed2(subDates),  new(func(BlDate, BlDate) BlDaysTimeDuration)),
        // … subDateDur, ltDates, …

        // construction / conversion
        expr.Function("date",  dateFn,
            new(func(BlString) BlDate),                 // date("…")
            new(func(BlNumber, BlNumber, BlNumber) BlDate), // date(y, m, d)
            new(func(BlDateTime) BlDate)),              // date(dt) extraction
        expr.Function("today", todayFn, new(func() BlDate)),
        expr.Function("yearsAndMonthsDuration", typed2(yearsAndMonthsDurationFn),
            new(func(BlDate, BlDate) BlYearsMonthsDuration)),

        // calendar properties
        expr.Function("dayOfWeek",   typed1(dayOfWeekFn),   new(func(BlDate) BlString), new(func(BlDateTime) BlString)),
        expr.Function("dayOfYear",   typed1(dayOfYearFn),   new(func(BlDate) BlNumber), new(func(BlDateTime) BlNumber)),
        expr.Function("weekOfYear",  typed1(weekOfYearFn),  new(func(BlDate) BlNumber), new(func(BlDateTime) BlNumber)),
        expr.Function("monthOfYear", typed1(monthOfYearFn), new(func(BlDate) BlString), new(func(BlDateTime) BlString)),
        expr.Function("lastDayOfMonth", typed1(lastDayOfMonthFn), new(func(BlDate) BlDate)),

        // ext: classification
        expr.Function("isWeekday",       typed1(isWeekdayFn),       new(func(BlDate) BlBoolean)),
        expr.Function("isWeekend",       typed1(isWeekendFn),       new(func(BlDate) BlBoolean)),
        expr.Function("isPublicHoliday", typed2(isPublicHolidayFn), new(func(BlDate, BlCalendar) BlValue)),
        expr.Function("isBusinessDay",   typed2(isBusinessDayFn),   new(func(BlDate, BlCalendar) BlValue)),

        // ext: month boundary
        expr.Function("firstDayOfMonth",     typed1(firstDayOfMonthFn),     new(func(BlDate) BlDate)),
        expr.Function("lastDayOfPrevMonth",  typed1(lastDayOfPrevMonthFn),  new(func(BlDate) BlDate)),
        expr.Function("firstDayOfNextMonth", typed1(firstDayOfNextMonthFn), new(func(BlDate) BlDate)),

        // ext: week-in-month navigation
        expr.Function("firstDayOfWeekInMonth", typed2(firstDOWInMonthFn), new(func(BlDate, BlString) BlDate)),
        expr.Function("lastDayOfWeekInMonth",  typed2(lastDOWInMonthFn),  new(func(BlDate, BlString) BlDate)),
        expr.Function("nthDayOfWeekInMonth",   typed3(nthDOWInMonthFn),   new(func(BlDate, BlNumber, BlString) BlValue)),

        // ext: day navigation
        expr.Function("nextDayOfWeek", typed2(nextDayOfWeekFn), new(func(BlDate, BlString) BlDate)),
        expr.Function("prevDayOfWeek", typed2(prevDayOfWeekFn), new(func(BlDate, BlString) BlDate)),
        expr.Function("nextWeekday",   typed1(nextWeekdayFn),   new(func(BlDate) BlDate)),
        expr.Function("prevWeekday",   typed1(prevWeekdayFn),   new(func(BlDate) BlDate)),
        expr.Function("nextBusinessDay", typed2(nextBusinessDayFn), new(func(BlDate, BlCalendar) BlDate)),
        expr.Function("prevBusinessDay", typed2(prevBusinessDayFn), new(func(BlDate, BlCalendar) BlDate)),

        // ext: business-day arithmetic & difference
        expr.Function("addBusinessDays", addBusinessDaysFn,
            new(func(BlDate, BlNumber, BlCalendar) BlDate),
            new(func(BlDate, BlNumber, BlCalendar, bool) BlDate)),
        expr.Function("subtractBusinessDays", subtractBusinessDaysFn,
            new(func(BlDate, BlNumber, BlCalendar) BlDate),
            new(func(BlDate, BlNumber, BlCalendar, bool) BlDate)),
        expr.Function("weekdaysBetween",      typed2(weekdaysBetweenFn),     new(func(BlDate, BlDate) BlNumber)),
        expr.Function("businessDaysBetween",  typed3(businessDaysBetweenFn), new(func(BlDate, BlDate, BlCalendar) BlNumber)),

        // ext: zone stripping
        expr.Function("withoutOffset",          typed1(withoutOffsetFn),          new(func(BlDate) BlDate)),
        expr.Function("withoutTimezone",        typed1(withoutTimezoneFn),        new(func(BlDate) BlDate)),
        expr.Function("withoutOffsetOrTimezone", typed1(withoutOffsetOrTimezoneFn), new(func(BlDate) BlDate)),
    }
}
```

**Components.** `.year/.month/.day/.offset/.timezone` are resolved by the component-access patcher to
internal accessor calls (`dateYearFn`, …). Calendar-aware funcs raise `BlCalendarRangeError` past the
calendar's validity bounds. Native Go `time.Time` (date portion) inputs wrap to `BlDate`.

`[@test] ../../expr/date_test.go`

---

## Edge cases

- Month outside 1–12, or day invalid for the month → `BlTypeError` (no silent rollover at
  construction).
- Year zero is valid (proleptic Gregorian; `0` = 1 BCE).
- `addBusinessDays`/`subtractBusinessDays` with `n = 0` return the date unchanged.
- `nextBusinessDay`/`prevBusinessDay` always return a date strictly different from the input.
- `nthDayOfWeekInMonth` with `n = 0` → `BlTypeError`; `|n|` beyond the month's occurrences → `null`.
- Unrecognised day-of-week string (e.g. `"Mon"`) in navigation built-ins → `BlTypeError`.
- Comparing tz-aware and tz-naive dates → `null` (no implicit coercion).
- `withoutTimezone` on an offset-only date (and vice versa) is a no-op; use `withoutOffsetOrTimezone`
  to strip both.
