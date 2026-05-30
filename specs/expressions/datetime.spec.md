---
name: BlDateTime
description: The date-and-time type in the blkit expression language. Covers the datetime constructor, date/time component access, duration arithmetic, difference, timezone normalisation, comparison, and the Go layer (BlDateTime + expr registrations).
targets:
  - ../../expr/datetime.go
---

# BlDateTime — the `datetime` type

`datetime` is a combined date and time with an optional UTC offset or IANA timezone. The textual
form follows [ISO 8601](https://www.iso.org/iso-8601-date-and-time-format.html) for the
date/time portion and [RFC 9557 (IXDTF)](https://datatracker.ietf.org/doc/html/rfc9557) for the
`[Zone]` suffix used to attach an IANA timezone name. This is the same format produced by Java's
`ZonedDateTime`, JavaScript's Temporal, and Python 3.13's `datetime.isoformat()`. The Go value
type backing it is `BlDateTime`. (Constructor named `datetime`, not `date and time` — see
[bl-expr.spec.md](bl-expr.spec.md#relationship-to-feel-and-future-direction).)

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
datetime("2025-03-28T14:30:00[Europe/Paris]") // IANA zone (RFC 9557)
datetime(date("2025-03-28"), time("14:30:00")) // combine a date and a time
now()                                       // current datetime
```

The `T` separator is required; space-separated forms are rejected.

`[@test] ../../expr/datetime_test.go`

---

## Component access & calendar properties

All calendar properties are accessed via **dot syntax**. There is no function-call form
(`dayName(dt)` etc. are not registered as user-callable functions). The component-access
patcher recognises these names on `BlDateTime` operands and dispatches to the corresponding
internal accessor.

```
datetime("2025-03-28T14:30:00+01:00").year             // → 2025
datetime("2025-03-28T14:30:00+01:00").month            // → 3
datetime("2025-03-28T14:30:00+01:00").day              // → 28
datetime("2025-03-28T14:30:00+01:00").hour             // → 14
datetime("2025-03-28T14:30:00+01:00").minute           // → 30
datetime("2025-03-28T14:30:00+01:00").second           // → 0
datetime("2025-03-28T14:30:00+01:00").offset           // → duration("PT1H")             (ext)
datetime("2025-03-28T14:30:00[Europe/Paris]").timezone // → "Europe/Paris"               (ext)
datetime("2019-09-17T00:00:00").dayName              // → "Tuesday"
datetime("2019-09-17T00:00:00").dayNameShort         // → "Tue"                        (ext)
datetime("2019-09-17T00:00:00").dayOfYear              // → 260
datetime("2019-09-17T00:00:00").weekOfYear             // → 38                           (ext; simple Jan-1 anchor)
datetime("2025-12-29T00:00:00").isoWeekOfYear          // → 1                            (ISO 8601 — week 1 of 2026)
datetime("2025-12-29T00:00:00").isoYearWeek            // → "2026W1"                     (ext; combined ISO year+week, parallel to .yearQuarter)
datetime("2019-09-17T00:00:00").monthName            // → "September"
datetime("2019-09-17T00:00:00").monthNameShort       // → "Sep"                        (ext)
datetime("2025-09-17T00:00:00").quarter                // → 3                            (ext; 1–4 calendar quarter)
datetime("2025-09-17T00:00:00").yearQuarter            // → "2025Q3"                     (ext)
```

`date` and `time` extraction stay as function calls — they're type conversions, not properties,
and they share names with the constructors:

| Function | Example | Result |
|---|---|---|
| `date(dt)` | extract the date | a `date` |
| `time(dt)` | extract the time | a `time` |

`[@test] ../../expr/datetime_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration | `datetime("2025-01-31T12:00:00") + duration("P1M")` | `datetime("2025-02-28T12:00:00")` (day clamped) |
| `-` (datetime) | days-time difference | `dt2 - dt1` | a days-time `duration` |
| `< <= > >= = !=` | comparison | `submittedAt <= deadline` | `true`/`false` |
| `between a and b` | inclusive range | `submittedAt between startDt and endDt` | `true`/`false` |
| `in` | membership (list / range / calendar) | `submittedAt in [startDt..endDt]` | `true`/`false` |

- A **years-months** duration adjusts year/month (day clamped), leaving the time; a **days-time**
  duration adds precisely, carrying across date boundaries. Both preserve the original zone/offset.
- Mixed arithmetic chains two operations: `dt + duration("P1Y") + duration("P10D")`.
- Duration-returning differences via named functions (`yearsAndMonthsDuration`,
  `daysAndTimeDuration`) are documented under [§ Business-day arithmetic & difference](#business-day-arithmetic--difference-ext).

`[@test] ../../expr/datetime_ops_test.go`

---

## Comparison semantics

- Two zoned/offset datetimes compare as **UTC instants**.
- Two local datetimes compare **wall-clock**.
- Comparing a local datetime to a zoned/offset one → `null`.

---

## Calendar utilities (shared with date)

These functions accept either a `BlDate` or a `BlDateTime` as their first argument. They are
the canonical home for calendar-property navigation; the date type forwards to the same
implementations. For functions returning a date/datetime, the `BlDateTime` form preserves the
time-of-day and zone; the `BlDate` form returns a date. For boolean/number returns, the time
component is ignored.

Examples below use `date(...)` for brevity, but every function also accepts the corresponding
`datetime(...)` value.

### Classification (**ext**)

| Function | Example | Result |
|---|---|---|
| `isWeekday(v)` | `isWeekday(date("2025-03-24"))` | `true` |
| `isWeekend(v)` | `isWeekend(date("2025-03-29"))` | `true` |
| `isPublicHoliday(v, phCalendar)` | `isPublicHoliday(date("2025-04-18"), ukHolidays)` | `true` |
| `isBusinessDay(v[, phCalendar])` | `isBusinessDay(date("2025-04-18"), ukHolidays)` | `false` (Good Friday — weekday but in `phCalendar`). Without `phCalendar`: weekend check only |

### Month boundaries

| Function | Example | Result |
|---|---|---|
| `lastDayOfMonth(v)` | `lastDayOfMonth(date("2024-02-10"))` | `date("2024-02-29")` |
| `firstDayOfMonth(v)` **ext** | `firstDayOfMonth(date("2025-02-14"))` | `date("2025-02-01")` |
| `lastDayOfPrevMonth(v)` **ext** | `lastDayOfPrevMonth(date("2025-01-01"))` | `date("2024-12-31")` |
| `firstDayOfNextMonth(v)` **ext** | `firstDayOfNextMonth(date("2025-12-31"))` | `date("2026-01-01")` |

### Week-in-month navigation (**ext**)

| Function | Example | Result |
|---|---|---|
| `firstDayOfWeekInMonth(v, dow)` | `firstDayOfWeekInMonth(date("2025-03-15"), "Monday")` | `date("2025-03-03")` |
| `lastDayOfWeekInMonth(v, dow)` | `lastDayOfWeekInMonth(date("2025-03-15"), "Friday")` | `date("2025-03-28")` |
| `nthDayOfWeekInMonth(v, n, dow)` | `nthDayOfWeekInMonth(date("2025-03-15"), 2, "Monday")` | `date("2025-03-10")` (`n<0` from end; out of range → `null`) |

### Day navigation (**ext**)

| Function | Example | Result |
|---|---|---|
| `nextDayOfWeek(v, dow)` / `prevDayOfWeek(v, dow)` | `nextDayOfWeek(date("2025-03-24"), "Monday")` | `date("2025-03-31")` (strictly after) |
| `nextWeekday(v)` / `prevWeekday(v)` | `nextWeekday(date("2025-03-28"))` | `date("2025-03-31")` (Fri → Mon) |
| `nextBusinessDay(v[, phCalendar[, strictCalendarRange]])` / `prevBusinessDay(v[, phCalendar[, strictCalendarRange]])` | `nextBusinessDay(date("2025-04-17"), ukHolidays)` | `date("2025-04-22")` (skips weekend + holidays). Without `phCalendar`: weekends only. See [§ Calendar-range strictness](#calendar-range-strictness) for `strictCalendarRange` |

## Business-day arithmetic & difference (**ext**)

These functions accept either a `BlDate` or a `BlDateTime` (shared with date). The `phCalendar`
argument is **optional**: omitted, the functions skip weekends only; supplied, they additionally
skip the dates marked in the calendar.

| Function | Example | Result |
|---|---|---|
| `addBusinessDays(v, n[, phCalendar[, strictCalendarRange]])` | `addBusinessDays(date("2025-04-17"), 2, ukHolidays)` | `date("2025-04-23")` (Good Friday + Easter Monday skipped). Without `phCalendar`: skip weekends only |
| `subtractBusinessDays(v, n[, phCalendar[, strictCalendarRange]])` | `subtractBusinessDays(date("2025-04-23"), 2, ukHolidays)` | `date("2025-04-17")` |
| `weekdaysBetween(a, b)` | `weekdaysBetween(date("2025-03-24"), date("2025-03-28"))` | `5` (inclusive; order-independent) |
| `businessDaysBetween(a, b[, phCalendar[, strictCalendarRange]])` | `businessDaysBetween(date("2025-04-14"), date("2025-04-25"), ukHolidays)` | `8`. Without `phCalendar`: weekdays in range |
| `yearsAndMonthsDuration(a, b)` | `yearsAndMonthsDuration(date("2025-03-28"), date("2026-06-15"))` | `duration("P1Y2M")` |
| `daysAndTimeDuration(a, b)` | `daysAndTimeDuration(date("2025-01-01"), date("2025-03-28"))` | `duration("P86D")` (equivalent to `b - a`) |

For `BlDateTime` inputs, arithmetic functions preserve time-of-day and zone; difference
functions ignore sub-day differences (use `daysBetween` with `includeTime: true` if you want
sub-day precision).

`[@test] ../../expr/business_days_test.go`

---

## Date difference: days, months, years (**ext**)

`daysBetween`, `monthsBetween`, and `yearsBetween` compute the elapsed difference between two
dates or datetimes as a number. `monthsBetween` and `yearsBetween` take an optional `basis`
argument selecting one of several day-count conventions; `daysBetween` doesn't (actual days has
no convention choices). Results are **signed** — positive when `v2 > v1`, negative when
`v2 < v1`. Use `abs()` if you only want the magnitude.

For `BlDateTime` inputs, an optional trailing `includeTime` boolean controls whether sub-day
precision is factored into the result. It defaults to `false` and is not accepted on the
`BlDate` forms (dates have no time component).

| Function | Example | Result |
|---|---|---|
| `daysBetween(v1, v2)` | `daysBetween(date("2025-01-01"), date("2025-03-15"))` | `73` (actual elapsed days) |
| `daysBetween(dt1, dt2, includeTime)` | `daysBetween(datetime("2025-01-15T00:00:00"), datetime("2025-01-16T12:00:00"), true)` | `1.5` (36 hours / 24) |
| `monthsBetween(v1, v2[, basis])` | `monthsBetween(date("2024-01-10"), date("2025-07-25"))` | `18.4839` (default `"calendar"`) |
| `monthsBetween(dt1, dt2, basis, includeTime)` | `monthsBetween(datetime("2025-01-15T18:00:00"), datetime("2025-07-15T06:00:00"), "actual/365", true)` | `≈ 5.9015` |
| `yearsBetween(v1, v2[, basis])` | `yearsBetween(date("2024-01-10"), date("2025-07-25"))` | `1.5370` (default `"calendar"`) |

`daysBetween` always returns an integer-valued number for date inputs and for datetime inputs
with `includeTime: false`; only `includeTime: true` can produce a fractional result. It's
equivalent to `(v2 - v1).days` but returned directly as a `BlNumber` rather than wrapped in
a duration.

### Basis values

`basis` accepts one of six string values:

| basis | How the calculation is done |
|---|---|
| `"calendar"` (default) | Whole calendar units plus a fractional remainder based on day-of-month/day-of-year. The intuitive "human" answer — what people usually want for age, tenure, etc. |
| `"actual/365"` | Actual elapsed days divided by 365 (× 12 for months). Common for pro-rating. |
| `"actual/360"` | Actual elapsed days divided by 360 (equivalently × 12 / 360 = ÷ 30 for months). US money-market convention. |
| `"actual/actual"` | ISDA: split the period at year boundaries, each part contributing `actualDaysInPart / actualDaysInYear`. Properly leap-year aware. |
| `"30/360"` | US/NASD 30-day-month, 360-day-year convention with end-of-month adjustments. Used in US bond and mortgage calculations. |
| `"30E/360"` | European 30/360 — same idea but simpler end-of-month rules. Used in European bond calculations. |

An invalid `basis` string → `BlTypeError`.

### `includeTime` semantics

- **`false` (default)** — the time-of-day portions of both datetime operands are ignored; the result is the same as you'd get from `monthsBetween(date(dt1), date(dt2), basis)`. Most decision logic wants this.
- **`true`** — sub-day differences contribute fractionally. For year-fraction bases (everything except `"calendar"`), the contribution is `(hours×3600 + minutes×60 + seconds) / 86400` of an extra day. For `"calendar"`, the time-of-day refines the partial-month remainder analogously.

A mismatch in zone-kind between two datetime operands (one local, one zoned/offset) → `BlNull`,
same rule as for `<`/`>` comparisons.

`[@test] ../../expr/date_difference_test.go`

---

## Financial year (**ext**)

Two functions return the **financial year** (also called fiscal year or tax year) and the
financial-year quarter that a date or datetime falls in. Financial years vary by jurisdiction;
the `basis` argument selects which convention to use.

| Function | Example | Result |
|---|---|---|
| `financialYear(v, basis)` | `financialYear(date("2024-08-01"), "AU")` | `"FY2025"` (AU FY runs July → June; labelled by year it ends in) |
| `financialYearQuarter(v, basis)` | `financialYearQuarter(date("2024-08-01"), "AU")` | `"FY2025Q1"` |

### Basis argument

`basis` accepts one of two forms:

- **A `BlNumber` 1–12** — the calendar month in which the financial year starts. For example,
  `7` means a July-start fiscal year (Australian convention).
- **A `BlString` jurisdiction code** — convenient shorthand for well-known jurisdictions:

| Code | Jurisdiction | Start | Notes |
|---|---|---|---|
| `"AU"` | Australia | July 1 | |
| `"UK"` | United Kingdom | April 6 | Personal tax year; the "April 6" quirk is preserved |
| `"US"` | US Federal | October 1 | Federal fiscal year |
| `"IN"` | India | April 1 | |
| `"JP"` | Japan | April 1 | |
| `"CA"` | Canada | April 1 | Federal |
| `"NZ"` | New Zealand | April 1 | |

A `basis` outside `1`–`12` or an unrecognised jurisdiction string → `BlTypeError`.

### Labelling convention

The financial year is **labelled by the calendar year it ends in** — the standard convention in
AU, US, UK, and Canada. For a July-start FY:

```
financialYear(date("2024-06-30"), "AU")   // → "FY2024"  (FY 2024 ends today)
financialYear(date("2024-07-01"), "AU")   // → "FY2025"  (FY 2025 begins today)
```

Both functions return strings prefixed with `"FY"`. `financialYear` returns `"FY<year>"`;
`financialYearQuarter` returns `"FY<year>Q<quarter>"`. `<year>` is the four-digit financial
year (labelled by the calendar year it ends in). `<quarter>` is 1–4 within the financial year
(Q1 starts on the FY start date, Q4 ends on the day before the next FY start).

```
financialYearQuarter(date("2024-08-01"), "AU")   // → "FY2025Q1"  (AU FY 2025 Q1: Jul–Sep)
financialYearQuarter(date("2025-01-15"), "AU")   // → "FY2025Q3"  (AU FY 2025 Q3: Jan–Mar)
financialYearQuarter(date("2024-08-01"), 7)      // → "FY2025Q1"  (numeric basis equivalent)
```

`[@test] ../../expr/financial_year_test.go`

---

## Calendar-range strictness

When a business-day function iterates past the supplied `phCalendar`'s `[validFrom, validTo]`
window, the **default** is to silently continue — the calendar simply contributes no holiday
information beyond its bounds, and the arithmetic proceeds using weekend skipping alone.

Pass `strictCalendarRange: true` as the trailing argument to opt in to a `BlCalendarRangeError`
the moment iteration would step past the boundary. Useful when callers need a hard guarantee
that all returned values are inside the calendar's authoritative window.

`strictCalendarRange` is available on every iterating business-day function (`addBusinessDays`,
`subtractBusinessDays`, `nextBusinessDay`, `prevBusinessDay`, `businessDaysBetween`). It does
nothing — and is meaningless — when `phCalendar` is not supplied, since there's no range to
police. `isBusinessDay` does not accept the flag (it checks a single date, never iterates).

See [calendar.spec.md](calendar.spec.md) for the calendar's `validFrom`/`validTo` definition.

---

## Re-zoning (**ext**)

Two functions change the zone of a value while **preserving the instant** — the wall-clock
numbers shift to reflect the new zone. (To preserve the wall-clock numbers and drop the zone
label instead, use [§ Zone stripping](#zone-stripping-ext).)

| Function | Example | Result |
|---|---|---|
| `withOffset(v, off)` | `withOffset(datetime("2025-03-28T14:30:00+01:00"), duration("PT2H"))` | `datetime("2025-03-28T15:30:00+02:00")` (same instant, new offset) |
| `withOffset(v, off)` | `withOffset(time("14:30:00Z"), duration("PT1H"))` | `time("15:30:00+01:00")` (same instant, new offset) |
| `withTimezone(dt, zone)` | `withTimezone(datetime("2025-03-28T14:30:00Z"), "Europe/Paris")` | `datetime("2025-03-28T15:30:00[Europe/Paris]")` (same instant, new zone) |

`withOffset` takes a `BlDaysTimeDuration` and accepts either a `BlTime` or a `BlDateTime` as
its first argument, returning the same type. `withTimezone` takes a `BlString` IANA name
(unknown zone → `BlTypeError`) and is `BlDateTime`-only — wall-clock-only times cannot meaningfully
carry an IANA zone over time. For re-zoning to UTC, pass `duration("PT0H")` to `withOffset`.
Neither function is defined for naive values (no source zone to convert *from*) — calling on a
naive input returns `BlNull`.

`BlDate` is not supported by either function — a date has no time-of-day to shift across a
zone boundary.

`[@test] ../../expr/datetime_rezoning_test.go`

---

## Zone stripping (**ext**)

Three functions strip zone metadata from a date or datetime, returning a naive (timezone-naive)
value. The wall-clock numbers are preserved — these operations drop the zone *label* without
shifting the moment. Use [§ Re-zoning](#re-zoning-ext) first (e.g.
`withOffset(dt, duration("PT0H"))`) if you want UTC-equivalent strip-then-convert semantics
instead.

| Function | Example | Result |
|---|---|---|
| `withoutOffset(v)` | `withoutOffset(datetime("2025-03-28T14:30:00+01:00"))` | `datetime("2025-03-28T14:30:00")` (no-op if no offset) |
| `withoutTimezone(v)` | `withoutTimezone(datetime("2025-03-28T14:30:00[Europe/Paris]"))` | `datetime("2025-03-28T14:30:00")` (no-op if no zone) |
| `withoutOffsetOrTimezone(v)` | `withoutOffsetOrTimezone(datetime("2025-03-28T14:30:00+01:00"))` | `datetime("2025-03-28T14:30:00")` (strips whichever is present) |

Each function accepts either a `BlDate` or a `BlDateTime` and returns the same type. A value
that already has no offset/timezone is returned unchanged.

`[@test] ../../expr/zone_stripping_test.go`

---

## Interval algebra

A datetime is a *point*; the interval-algebra built-ins (`coincides`, `starts`, `during`,
`finishes`, `before`, `after`, …) are in [range.spec.md](range.spec.md), e.g.
`during(datetime("2025-05-15T12:00:00"), [datetime("2025-04-01T00:00:00")..datetime("2025-06-30T23:59:59")]) // → true`.
Mixed endpoint types (e.g. comparing a datetime point against a date-bounded range) → `BlTypeError`;
both endpoints and the point must be the same temporal type.

---

## Go implementation (expr extension)

Lives in `expr/datetime.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlDateTime` is the immutable Go value type that represents a combined date and time inside the
engine and at the host-code boundary. It wraps a Go [`time.Time`](https://pkg.go.dev/time#Time)
— which is the standard library's combined date/time type — and adds a single `naive` boolean
that distinguishes the timezone-naive (local) case from a genuinely UTC-located one. Both fields are
private so callers cannot mutate the underlying value; every operation in the library returns a
fresh `BlDateTime`.

The `naive` flag is needed because Go's `time.Time` always carries a non-nil `*time.Location`,
so there is no built-in way to represent "wall-clock numbers with no timezone interpretation".
When `naive` is true, the `Location()` of the wrapped `time.Time` is `time.UTC` by convention
and must be ignored; the value is interpreted strictly as wall-clock. When `naive` is false,
the `Location()` is meaningful and may be `time.UTC`, a fixed-offset zone from
[`time.FixedZone`](https://pkg.go.dev/time#FixedZone), or an IANA zone loaded via
[`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation).

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` delegates to [`time.Time.Equal`](https://pkg.go.dev/time#Time.Equal) when
  both operands have `naive == false` (stdlib handles the zone-aware instant comparison); when
  both have `naive == true` it compares wall-clock components directly without zone
  interpretation; mixing them returns `BlNull` (see [§ Comparison semantics](#comparison-semantics)).
  `String()` doubles as the `fmt.Stringer`
  implementation, producing the canonical ISO 8601 / RFC 9557 form (e.g.
  `"2025-03-28T14:30:00+01:00"` or `"2025-03-28T14:30:00[Europe/Paris]"`).
- **`DateTime[T DateTimeInput](v T)`** — the generic host constructor. `DateTimeInput` accepts
  `string` (parsed as ISO 8601 / RFC 9557 — `naive` is set based on whether a zone designator
  was present), `time.Time` (Go's native combined type — see the [Key Go Dependencies](../overview.spec.md#key-go-dependencies)
  note that Go uses one `time.Time` type for date, time, and datetime; `naive` is always false
  for this path because a `time.Time` always carries a `Location` — host code wanting a naive
  result from a `time.Time` should pipe it through `ToDateTimeComponentsAsNaive` first), or a
  `DateTimeComponents` struct for explicit component-by-component construction (`naive` is true
  if and only if both `Offset` and `Zone` are absent). The `error` return fires for unparseable
  strings, invalid components (month=13, day=32, etc.), or a `DateTimeComponents` with both
  `Offset` and `Zone` set (they're mutually exclusive).
- **`Time()` accessor** — hands back the wrapped `time.Time`. For non-naive values this is the
  full meaningful representation. For naive values the host receives a `time.Time` with
  `Location() == time.UTC`, which it MUST treat as wall-clock — callers that need to know which
  kind they have should also call `IsNaive()`.
- **`IsNaive()` accessor** — reports whether the value is timezone-naive.
- **`Now()`** — convenience helper that returns the current moment as a non-naive `BlDateTime`.
  Equivalent to `DateTime(time.Now())` minus the error path (which can never fire for
  `time.Now()`).

```go
type BlDateTime struct {
    t     time.Time   // Location() carries offset or IANA zone when naive==false
    naive bool        // true when no offset/zone was specified — Location() is ignored
}

// BlValue interface — required by all Bl* value types.
func (BlDateTime) Type() BlType { return BlTypeDateTime }
func (dt BlDateTime) Equal(other BlValue) BlValue   // UTC-instant or wall-clock; cross-kind → Null
func (dt BlDateTime) String() string                // canonical ISO 8601 / RFC 9557
func (BlDateTime) isBlValue() {}

// Host constructor — accepts an ISO 8601/RFC 9557 string, a Go time.Time, or component bundle.
type DateTimeComponents struct {
    Year, Month, Day, Hour, Minute, Second int
    Offset *time.Duration   // optional; mutually exclusive with Zone
    Zone   string           // optional; IANA name; mutually exclusive with Offset
}
type DateTimeInput interface { string | time.Time | DateTimeComponents }
func DateTime[T DateTimeInput](v T) (BlDateTime, error)

// Decompose a time.Time into wall-clock components with Offset/Zone unset, so the
// resulting DateTimeComponents builds a naive BlDateTime when passed to DateTime.
// Use this when host code has a time.Time but wants to drop its zone interpretation.
func ToDateTimeComponentsAsNaive(t time.Time) DateTimeComponents

// Convenience for current time.
func Now() BlDateTime

// Host accessors (consume an evaluated result).
func (dt BlDateTime) Time() time.Time               // wrapped time.Time; for naive values its Location() is UTC and must be ignored
func (dt BlDateTime) IsNaive() bool                 // true when timezone-naive (wall-clock only)
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlDateTime` and cannot apply Go's native `+`/`-`/`<`/etc.
to blkit values. For every operator that should work on datetimes, blkit supplies a named Go
function that performs the operation and returns the result wrapped as a `BlValue`. The
connection from operator token to function happens in two steps, neither of which is unique to
`BlDateTime`:

1. The Registrations section below calls `expr.Function("addDateTimeDT", typed2(addDateTimeDT), …)`,
   which makes the engine aware of the function under that exact string name and records its type
   signature.
2. A central `operatorBindings()` in [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then
   calls `expr.Operator("+", "addNumbers", "concatStrings", "addDateTimeDT", "addDateTimeYM", …)`,
   which tells the engine "when you see `+` at parse time, try each of these registered functions
   in turn and dispatch to whichever one's signature matches the operand types." Centralised in
   one place because a single operator spans many types — `+` covers number addition, string
   concatenation, and several temporal forms — and `expr.Operator` needs the full list of
   candidates for each operator in a single call.

So when the parser encounters `dt + dur` and the operands type-check to `BlDateTime` +
`BlDaysTimeDuration`, the engine finds `addDateTimeDT` in the `"+"` binding list, sees its
signature matches, and dispatches to it.

Arithmetic impls return `BlDateTime` directly because they cannot yield null (day-clamping
handles the only awkward case: `2025-01-31 + P1M → 2025-02-28`). Comparison impls return
`BlValue` because cross-kind comparison (local vs zoned/offset) yields `BlNull`.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `BlValue` interface, which `BlDateTime`
implements above. That single dispatch path handles null propagation and cross-type comparison
uniformly.

```go
func addDateTimeYM(dt BlDateTime, dur BlYearsMonthsDuration) BlDateTime // "+" dt + YM duration (day clamped)
func addDateTimeDT(dt BlDateTime, dur BlDaysTimeDuration)   BlDateTime  // "+" dt + DT duration
func subDateTimeYM(dt BlDateTime, dur BlYearsMonthsDuration) BlDateTime // "-" dt − YM duration (day clamped)
func subDateTimeDT(dt BlDateTime, dur BlDaysTimeDuration)   BlDateTime  // "-" dt − DT duration
func subDateTimes(a, b BlDateTime) BlDaysTimeDuration                   // "-" dt − dt
func ltDateTimes(a, b BlDateTime) BlValue                               // "<"  ; cross-kind → Null
func leDateTimes(a, b BlDateTime) BlValue                               // "<=" ; cross-kind → Null
func gtDateTimes(a, b BlDateTime) BlValue                               // ">"  ; cross-kind → Null
func geDateTimes(a, b BlDateTime) BlValue                               // ">=" ; cross-kind → Null
// "=" and "!=" go through BlValue.Equal(); see BlDateTime.Equal() above.
```

These are written in clean typed form (`BlDateTime → BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters at registration time.

### Backing implementations (unexported, suffix `Fn`)

The library and constructor functions are implemented as these typed Go functions. They are
wrapped by `typed1`/`typed2`/`typed3` when registered with the engine in the next section.
Variadic implementations (`datetimeFn`) instead implement the engine's `func(...any) (any, error)`
shape directly because they accept multiple input forms that cannot be expressed via a
fixed-arity adapter.

```go
// Datetime-only typed implementations.
func nowFn() BlDateTime
func withOffsetFn(v, off any) any              // BlTime → BlTime, BlDateTime → BlDateTime; dispatch on first-arg type
func withTimezoneFn(dt BlDateTime, zone BlString) BlDateTime         // unknown zone → BlTypeError

// Datetime-only variadic implementation.
func datetimeFn(args ...any) (any, error)                 // datetime("…") or datetime(date, time)

// Calendar utilities — dispatch on first-arg type (BlDate or BlDateTime).
// For functions returning a date/datetime, the BlDateTime path preserves time and zone.
// For boolean/number returns, the time component is ignored.
func lastDayOfMonthFn(v any) any                   // BlDate → BlDate, BlDateTime → BlDateTime
func firstDayOfMonthFn(v any) any
func lastDayOfPrevMonthFn(v any) any
func firstDayOfNextMonthFn(v any) any
func firstDOWInMonthFn(v any, dow BlString) any
func lastDOWInMonthFn(v any, dow BlString) any
func nthDOWInMonthFn(v any, n BlNumber, dow BlString) any
func nextDayOfWeekFn(v any, dow BlString) any
func prevDayOfWeekFn(v any, dow BlString) any
func nextWeekdayFn(v any) any
func prevWeekdayFn(v any) any
func weekdaysBetweenFn(a, b any) BlNumber          // both args same type
func isWeekdayFn(v any) BlBoolean
func isWeekendFn(v any) BlBoolean
func isPublicHolidayFn(v any, ph BlCalendar) BlValue

// Business-day functions — variadic because phCalendar and strictCalendarRange are optional;
// dispatch on first-arg type (BlDate or BlDateTime).
func isBusinessDayFn(args ...any) (any, error)             // (v) | (v, phCal)
func nextBusinessDayFn(args ...any) (any, error)           // (v) | (v, phCal) | (v, phCal, strict)
func prevBusinessDayFn(args ...any) (any, error)           // (v) | (v, phCal) | (v, phCal, strict)
func addBusinessDaysFn(args ...any) (any, error)           // (v, n) | (v, n, phCal) | (v, n, phCal, strict)
func subtractBusinessDaysFn(args ...any) (any, error)      // (v, n) | (v, n, phCal) | (v, n, phCal, strict)
func businessDaysBetweenFn(args ...any) (any, error)       // (a, b) | (a, b, phCal) | (a, b, phCal, strict)

// Date-difference functions — variadic because basis and includeTime are optional;
// dispatch on first-arg type. includeTime is meaningful only for BlDateTime inputs.
func daysBetweenFn(args ...any) (any, error)               // (a, b) | (a, b, includeTime)  — includeTime only valid for BlDateTime
func monthsBetweenFn(args ...any) (any, error)             // (a, b) | (a, b, basis) | (a, b, basis, includeTime)
func yearsBetweenFn(args ...any) (any, error)              // (a, b) | (a, b, basis) | (a, b, basis, includeTime)

// Zone stripping — dispatch on input type. Returns same type as input; wall-clock preserved.
func withoutOffsetFn(v any) any                  // BlDate → BlDate, BlDateTime → BlDateTime
func withoutTimezoneFn(v any) any
func withoutOffsetOrTimezoneFn(v any) any

// Duration-typed difference — dispatch on input type. Both args must be the same type.
func yearsAndMonthsDurationFn(a, b any) BlYearsMonthsDuration   // both BlDate or both BlDateTime
func daysAndTimeDurationFn(a, b any) BlDaysTimeDuration         // both BlDate or both BlDateTime; equivalent to b - a

// Financial year — dispatch on first-arg type (BlDate or BlDateTime); basis is BlNumber or BlString.
func financialYearFn(v, basis any) BlString                     // returns "FY<year>" (labelled by year it ends in)
func financialYearQuarterFn(v, basis any) BlString              // returns "FY<year>Q<quarter>"
```

`v any` in the calendar-utility signatures means "either `BlDate` or `BlDateTime`" — the
implementation type-switches internally and returns the matching type. Same for the variadic
families; the dispatch on first-arg type is done in the Fn body before any type-specific
arithmetic runs.

`datetimeFn` parses ISO 8601 strings via Go's [`time.Parse`](https://pkg.go.dev/time#Parse) using
`time.RFC3339`-compatible layouts; an unparseable string → `BlParseError`. IANA zone lookups go
through [`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation); an unknown zone name →
`BlTypeError`. Calendar-property dot accessors (`.dayName`, `.dayNameShort`, `.dayOfYear`,
`.weekOfYear`, `.isoWeekOfYear`, `.isoYearWeek`, `.monthName`, `.monthNameShort`) are resolved by the component-access
patcher in [bl-expr.spec.md](bl-expr.spec.md#engine-internals-go) and dispatched to internal
accessors defined in [date.spec.md](date.spec.md); `date(dt)`/`time(dt)` extraction is
overloaded with the constructor entries in [date.spec.md](date.spec.md) / [time.spec.md](time.spec.md).
Native Go `time.Time` inputs wrap to `BlDateTime`.

### Registrations (`datetimeOptions`, unexported)

`datetimeOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every datetime-related operator impl and library function. Each
entry is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that `operatorBindings()`
  references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlDateTime, BlDaysTimeDuration) BlDateTime` into
  that shape, type-asserting each argument and boxing the result. The variadic implementations
  declared above already satisfy the shape and are registered directly.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across signatures (`datetime("…")`
  and `datetime(date, time)` are both valid).

The registrations are grouped to reflect their role: operator impls (consumed by
`operatorBindings()`), library functions (called directly by name from expressions), and
re-zoning helpers.

```go
func datetimeOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addDateTimeYM", typed2(addDateTimeYM), new(func(BlDateTime, BlYearsMonthsDuration) BlDateTime)),
        expr.Function("addDateTimeDT", typed2(addDateTimeDT), new(func(BlDateTime, BlDaysTimeDuration) BlDateTime)),
        expr.Function("subDateTimeYM", typed2(subDateTimeYM), new(func(BlDateTime, BlYearsMonthsDuration) BlDateTime)),
        expr.Function("subDateTimeDT", typed2(subDateTimeDT), new(func(BlDateTime, BlDaysTimeDuration) BlDateTime)),
        expr.Function("subDateTimes",  typed2(subDateTimes),  new(func(BlDateTime, BlDateTime) BlDaysTimeDuration)),
        expr.Function("ltDateTimes",   typed2(ltDateTimes),   new(func(BlDateTime, BlDateTime) BlValue)),
        expr.Function("leDateTimes",   typed2(leDateTimes),   new(func(BlDateTime, BlDateTime) BlValue)),
        expr.Function("gtDateTimes",   typed2(gtDateTimes),   new(func(BlDateTime, BlDateTime) BlValue)),
        expr.Function("geDateTimes",   typed2(geDateTimes),   new(func(BlDateTime, BlDateTime) BlValue)),
        // = and != dispatch via BlValue.Equal() — no per-type registration

        // library
        expr.Function("datetime", datetimeFn,
            new(func(BlString) BlDateTime),                  // datetime("…")
            new(func(BlDate, BlTime) BlDateTime)),           // datetime(date, time)
        expr.Function("now",          nowFn,                  new(func() BlDateTime)),

        // re-zoning
        expr.Function("withOffset",   typed2(withOffsetFn),
            new(func(BlTime, BlDaysTimeDuration) BlTime),
            new(func(BlDateTime, BlDaysTimeDuration) BlDateTime)),
        expr.Function("withTimezone", typed2(withTimezoneFn), new(func(BlDateTime, BlString) BlDateTime)),

        // calendar utilities (shared with date) — single registration covers both BlDate and BlDateTime
        expr.Function("lastDayOfMonth",        typed1(lastDayOfMonthFn),     new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("firstDayOfMonth",       typed1(firstDayOfMonthFn),    new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("lastDayOfPrevMonth",    typed1(lastDayOfPrevMonthFn), new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("firstDayOfNextMonth",   typed1(firstDayOfNextMonthFn),new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("firstDayOfWeekInMonth", typed2(firstDOWInMonthFn),    new(func(BlDate, BlString) BlDate), new(func(BlDateTime, BlString) BlDateTime)),
        expr.Function("lastDayOfWeekInMonth",  typed2(lastDOWInMonthFn),     new(func(BlDate, BlString) BlDate), new(func(BlDateTime, BlString) BlDateTime)),
        expr.Function("nthDayOfWeekInMonth",   typed3(nthDOWInMonthFn),      new(func(BlDate, BlNumber, BlString) BlValue), new(func(BlDateTime, BlNumber, BlString) BlValue)),
        expr.Function("nextDayOfWeek",         typed2(nextDayOfWeekFn),      new(func(BlDate, BlString) BlDate), new(func(BlDateTime, BlString) BlDateTime)),
        expr.Function("prevDayOfWeek",         typed2(prevDayOfWeekFn),      new(func(BlDate, BlString) BlDate), new(func(BlDateTime, BlString) BlDateTime)),
        expr.Function("nextWeekday",           typed1(nextWeekdayFn),        new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("prevWeekday",           typed1(prevWeekdayFn),        new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("weekdaysBetween",       typed2(weekdaysBetweenFn),    new(func(BlDate, BlDate) BlNumber), new(func(BlDateTime, BlDateTime) BlNumber)),
        expr.Function("isWeekday",             typed1(isWeekdayFn),          new(func(BlDate) BlBoolean), new(func(BlDateTime) BlBoolean)),
        expr.Function("isWeekend",             typed1(isWeekendFn),          new(func(BlDate) BlBoolean), new(func(BlDateTime) BlBoolean)),
        expr.Function("isPublicHoliday",       typed2(isPublicHolidayFn),    new(func(BlDate, BlCalendar) BlValue), new(func(BlDateTime, BlCalendar) BlValue)),

        // business-day functions (shared with date) — single registration covers both types
        // BlDate forms keep their signatures; BlDateTime forms accept the same optional args
        expr.Function("isBusinessDay", isBusinessDayFn,
            new(func(BlDate) BlValue),
            new(func(BlDate, BlCalendar) BlValue),
            new(func(BlDateTime) BlValue),
            new(func(BlDateTime, BlCalendar) BlValue)),
        expr.Function("nextBusinessDay", nextBusinessDayFn,
            new(func(BlDate) BlDate),
            new(func(BlDate, BlCalendar) BlDate),
            new(func(BlDate, BlCalendar, bool) BlDate),
            new(func(BlDateTime) BlDateTime),
            new(func(BlDateTime, BlCalendar) BlDateTime),
            new(func(BlDateTime, BlCalendar, bool) BlDateTime)),
        expr.Function("prevBusinessDay", prevBusinessDayFn,
            new(func(BlDate) BlDate),
            new(func(BlDate, BlCalendar) BlDate),
            new(func(BlDate, BlCalendar, bool) BlDate),
            new(func(BlDateTime) BlDateTime),
            new(func(BlDateTime, BlCalendar) BlDateTime),
            new(func(BlDateTime, BlCalendar, bool) BlDateTime)),
        expr.Function("addBusinessDays", addBusinessDaysFn,
            new(func(BlDate, BlNumber) BlDate),
            new(func(BlDate, BlNumber, BlCalendar) BlDate),
            new(func(BlDate, BlNumber, BlCalendar, bool) BlDate),
            new(func(BlDateTime, BlNumber) BlDateTime),
            new(func(BlDateTime, BlNumber, BlCalendar) BlDateTime),
            new(func(BlDateTime, BlNumber, BlCalendar, bool) BlDateTime)),
        expr.Function("subtractBusinessDays", subtractBusinessDaysFn,
            new(func(BlDate, BlNumber) BlDate),
            new(func(BlDate, BlNumber, BlCalendar) BlDate),
            new(func(BlDate, BlNumber, BlCalendar, bool) BlDate),
            new(func(BlDateTime, BlNumber) BlDateTime),
            new(func(BlDateTime, BlNumber, BlCalendar) BlDateTime),
            new(func(BlDateTime, BlNumber, BlCalendar, bool) BlDateTime)),
        expr.Function("businessDaysBetween", businessDaysBetweenFn,
            new(func(BlDate, BlDate) BlNumber),
            new(func(BlDate, BlDate, BlCalendar) BlNumber),
            new(func(BlDate, BlDate, BlCalendar, bool) BlNumber),
            new(func(BlDateTime, BlDateTime) BlNumber),
            new(func(BlDateTime, BlDateTime, BlCalendar) BlNumber),
            new(func(BlDateTime, BlDateTime, BlCalendar, bool) BlNumber)),

        // date difference (shared with date) — single registration covers both types
        // BlDateTime forms additionally accept an includeTime flag for sub-day precision
        expr.Function("daysBetween", daysBetweenFn,
            new(func(BlDate, BlDate) BlNumber),
            new(func(BlDateTime, BlDateTime) BlNumber),
            new(func(BlDateTime, BlDateTime, bool) BlNumber)),
        expr.Function("monthsBetween", monthsBetweenFn,
            new(func(BlDate, BlDate) BlNumber),
            new(func(BlDate, BlDate, BlString) BlNumber),
            new(func(BlDateTime, BlDateTime) BlNumber),
            new(func(BlDateTime, BlDateTime, BlString) BlNumber),
            new(func(BlDateTime, BlDateTime, BlString, bool) BlNumber)),
        expr.Function("yearsBetween", yearsBetweenFn,
            new(func(BlDate, BlDate) BlNumber),
            new(func(BlDate, BlDate, BlString) BlNumber),
            new(func(BlDateTime, BlDateTime) BlNumber),
            new(func(BlDateTime, BlDateTime, BlString) BlNumber),
            new(func(BlDateTime, BlDateTime, BlString, bool) BlNumber)),

        // zone stripping (shared with date) — single registration covers both types
        expr.Function("withoutOffset",           typed1(withoutOffsetFn),           new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("withoutTimezone",         typed1(withoutTimezoneFn),         new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),
        expr.Function("withoutOffsetOrTimezone", typed1(withoutOffsetOrTimezoneFn), new(func(BlDate) BlDate), new(func(BlDateTime) BlDateTime)),

        // duration-typed difference (shared with date)
        expr.Function("yearsAndMonthsDuration", typed2(yearsAndMonthsDurationFn),
            new(func(BlDate, BlDate) BlYearsMonthsDuration),
            new(func(BlDateTime, BlDateTime) BlYearsMonthsDuration)),
        expr.Function("daysAndTimeDuration", typed2(daysAndTimeDurationFn),
            new(func(BlDate, BlDate) BlDaysTimeDuration),
            new(func(BlDateTime, BlDateTime) BlDaysTimeDuration)),

        // financial year (shared with date) — basis may be numeric start month or jurisdiction code
        expr.Function("financialYear", financialYearFn,
            new(func(BlDate, BlNumber) BlString),
            new(func(BlDate, BlString) BlString),
            new(func(BlDateTime, BlNumber) BlString),
            new(func(BlDateTime, BlString) BlString)),
        expr.Function("financialYearQuarter", financialYearQuarterFn,
            new(func(BlDate, BlNumber) BlString),
            new(func(BlDate, BlString) BlString),
            new(func(BlDateTime, BlNumber) BlString),
            new(func(BlDateTime, BlString) BlString)),
    }
}
```

These six entries register *additional* overloads for already-named functions; the canonical
`BlDate` registrations live in [date.spec.md](date.spec.md) and remain unchanged. `expr-lang/expr`
combines all entries for the same name into a single overload set (same mechanism used by
`contains` across [string.spec.md](string.spec.md) and [calendar.spec.md](calendar.spec.md)),
and dispatches at parse time based on operand types.

Component access for `date`/`time`/`offset`/`zone` is wired by the
component-access patcher described in [bl-expr.spec.md](bl-expr.spec.md#engine-internals-go),
not via `expr.Function`.

`[@test] ../../expr/datetime_test.go`

---

## Edge cases

- `now()` depends on the system clock; use explicit construction for deterministic tests.
- `re-zoning` (`withOffset`/`withTimezone`) preserves the instant. To re-zone to UTC, use
  `withOffset(dt, duration("PT0H"))`.
- `dt2 - dt1` is negative when `dt2` precedes `dt1`.
- `duration("P1M")` added to `2025-01-31T23:59:59` → `2025-02-28T23:59:59` (day clamped).
