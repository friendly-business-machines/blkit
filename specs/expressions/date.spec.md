---
name: BlDate
description: The date type in the blkit expression language — a calendar date with optional offset/timezone, including business-day and calendar-aware arithmetic. Covers date literals, component access, the (large) date built-in library incl. blkit extensions, and the Go layer (BlDate + expr registrations).
targets:
  - ../../expr/date.go
---

# BlDate — the `date` type

`date` is a calendar date (year, month, day) with an optional UTC offset **or** IANA timezone.
The textual form follows [ISO 8601](https://www.iso.org/iso-8601-date-and-time-format.html)
(`YYYY-MM-DD`) for the date portion and [RFC 9557 (IXDTF)](https://datatracker.ietf.org/doc/html/rfc9557)
for the `[Zone]` suffix used to attach an IANA timezone name. The Go value type backing it is
`BlDate`. A plain date is timezone-naive; only one of offset/timezone may be set.

`date` carries blkit's richest built-in surface — including **business-day and calendar-aware
arithmetic**, much of which extends DMN FEEL (flagged **ext**). See
[bl-expr.spec.md](bl-expr.spec.md) for the engine and component access, and
[calendar.spec.md](calendar.spec.md) for the calendar values these functions consume.

---

## Literals / construction

There is **no dedicated date literal**: date values are produced by the `date(...)` built-in — for
example, the `date("2025-03-28")` in `date("2025-03-28").year`. The constructor accepts an ISO
8601 string, year/month/day components, or another temporal value to extract from.

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
date("2025-03-28+05:30").offset    // → duration("PT5H30M")  (ext; null if none)
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

Calendar properties are accessed via **dot syntax**, alongside the basic component accessors
(`.year`, `.month`, etc.). There is no function-call form — `dayOfWeek(d)`, `monthOfYear(d)`,
etc. are *not* registered as user-callable functions. The component-access patcher recognises
these names on `BlDate` (and `BlDateTime`) operands and dispatches to the corresponding internal
accessor.

| Accessor | Example | Result |
|---|---|---|
| `.dayOfWeek` | `date("2025-03-24").dayOfWeek` | `"Monday"` |
| `.dayOfWeekShort` **ext** | `date("2025-03-24").dayOfWeekShort` | `"Mon"` (3-letter English) |
| `.dayOfYear` | `date("2019-09-17").dayOfYear` | `260` |
| `.weekOfYear` | `date("2019-09-17").weekOfYear` | `38` (ISO 8601) |
| `.monthOfYear` | `date("2019-09-17").monthOfYear` | `"September"` |
| `.monthOfYearShort` **ext** | `date("2019-09-17").monthOfYearShort` | `"Sep"` (3-letter English) |

### Calendar utilities, business-day arithmetic, date difference

These function families accept either a `BlDate` or a `BlDateTime` and are documented in
[datetime.spec.md](datetime.spec.md). They cover classification (`isWeekday`, `isWeekend`,
`isBusinessDay`, `isPublicHoliday`), month boundaries (`firstDayOfMonth`, `lastDayOfMonth`,
…), week-in-month navigation (`firstDayOfWeekInMonth`, …), day navigation (`nextWeekday`,
`nextBusinessDay`, …), business-day arithmetic (`addBusinessDays`, `subtractBusinessDays`,
`weekdaysBetween`, `businessDaysBetween`, `yearsAndMonthsDuration`), date difference
(`daysBetween`, `monthsBetween`, `yearsBetween`), and zone stripping (`withoutOffset`,
`withoutTimezone`, `withoutOffsetOrTimezone`).

The links below jump straight to the relevant section in `datetime.spec.md`:

- [Classification](datetime.spec.md#classification-ext)
- [Month boundaries](datetime.spec.md#month-boundaries)
- [Week-in-month navigation](datetime.spec.md#week-in-month-navigation-ext)
- [Day navigation](datetime.spec.md#day-navigation-ext)
- [Business-day arithmetic & difference](datetime.spec.md#business-day-arithmetic--difference-ext)
- [Date difference: days, months, years](datetime.spec.md#date-difference-days-months-years-ext)
- [Calendar-range strictness](datetime.spec.md#calendar-range-strictness)
- [Zone stripping](datetime.spec.md#zone-stripping-ext)

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
        // yearsAndMonthsDuration and daysAndTimeDuration are registered in datetime.spec.md
        // with both BlDate and BlDateTime signatures.

        // calendar properties are accessed via dot syntax — the component-access patcher
        // (bl-expr.spec.md § Engine internals) rewrites .dayOfWeek, .dayOfWeekShort, .dayOfYear,
        // .weekOfYear, .monthOfYear, and .monthOfYearShort into calls to the internal accessor
        // functions (dayOfWeekFn, …), which are not registered as user-callable expr.Functions.

        // Calendar utilities, business-day arithmetic, date-difference, and zone-stripping
        // functions that accept BOTH BlDate and BlDateTime are registered in datetime.spec.md
        // (under a single expr.Function per name with both type signatures). See datetime.spec.md
        // § Calendar utilities and the surrounding sections for the consolidated registration
        // code.
    }
}
```

**Components.** `.year/.month/.day/.offset/.timezone` are resolved by the component-access patcher to
internal accessor calls (`dateYearFn`, …). Iterating business-day funcs raise `BlCalendarRangeError`
past the calendar's validity bounds only when `strictCalendarRange: true` is supplied (see
[§ Calendar-range strictness](#calendar-range-strictness)). Native Go `time.Time` (date portion) inputs
wrap to `BlDate`.

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
