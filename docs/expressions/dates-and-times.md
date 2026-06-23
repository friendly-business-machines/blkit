# Dates & Times

> Dates, times, date-times, durations, and calendars — construction,
> arithmetic, component access, and comparisons.

blkit ships a full family of temporal types. There are three *points* in time —
`date`, `time`, and `datetime` — two *spans* — a days-time `duration` and a
years-months `duration` — and a host-built `calendar` for holiday sets, blackout
periods, and other business schedules. This page shows how to construct each one,
read its components, do arithmetic and comparisons, and reach the calendar-aware
business-day functions that make blkit useful for real scheduling logic.

A theme runs through all of it: temporal values are either **timezone-naive**
(wall-clock numbers, no UTC relationship) or **zoned** (carrying a UTC offset or
an IANA timezone). Comparing or differencing a naive value against a zoned one is
never silently coerced — it yields `null`. Keep that distinction in mind and the
rest follows.

Textual forms follow [ISO 8601](https://www.iso.org/iso-8601-date-and-time-format.html)
for the date/time portion and [RFC 9557 (IXDTF)](https://datatracker.ietf.org/doc/html/rfc9557)
for the `[Zone]` suffix that attaches an IANA timezone name — the same format
produced by Java's `ZonedDateTime`, JavaScript's Temporal, and Python 3.13's
`datetime.isoformat()`.

## Dates

A `date` is a calendar date — year, month, day — optionally carrying a UTC offset
**or** an IANA timezone (never both). There is no date literal; date values are
produced by the `date(...)` built-in.

```
// expression-language
date("2025-03-28")                     // timezone-naive
date("2025-03-28+05:30")               // UTC offset
date("2025-03-28Z")                    // UTC (offset +00:00)
date("2025-03-28[Europe/London]")      // IANA timezone
date(2025, 3, 28)                      // from components
date(datetime("2025-03-28T14:30:00"))  // extract the date from a datetime
today()                                // current date (naive, local zone)
```

Supplying both an offset and a timezone, or an out-of-range month/day, is an
error (`bl.TypeError` / `bl.ParseError`) — there is no silent rollover.

### Component access

```
// expression-language
date("2025-03-28+05:30").year      // → 2025
date("2025-03-28").month           // → 3
date("2025-03-28").day             // → 28
date("2025-03-28+05:30").offset    // → dtDuration("PT5H30M")   (null if none)
date("2025-03-28[Europe/London]").timezone  // → "Europe/London"  (null if none)
```

### Calendar properties

Alongside the basic accessors, dates (and datetimes) expose a set of calendar
properties via the same dot syntax — there is no function-call form.

| Accessor | Example | Result |
|---|---|---|
| `.dayName` | `date("2025-03-24").dayName` | `"Monday"` |
| `.dayNameShort` | `date("2025-03-24").dayNameShort` | `"Mon"` |
| `.dayOfYear` | `date("2019-09-17").dayOfYear` | `260` |
| `.weekOfYear` | `date("2019-09-17").weekOfYear` | `38` (simple Jan-1 anchor: week 1 = Jan 1–7; always 1–53; always matches `.year`) |
| `.isoWeekOfYear` | `date("2025-12-29").isoWeekOfYear` | `1` (ISO 8601 — week 1 contains the year's first Thursday, so this belongs to 2026) |
| `.isoYearWeek` | `date("2025-12-29").isoYearWeek` | `"2026W1"` (combined ISO year + week — the safe identifier near year boundaries) |
| `.monthName` | `date("2019-09-17").monthName` | `"September"` |
| `.monthNameShort` | `date("2019-09-17").monthNameShort` | `"Sep"` |
| `.quarter` | `date("2025-09-17").quarter` | `3` (1–4 calendar quarter) |
| `.yearQuarter` | `date("2025-09-17").yearQuarter` | `"2025Q3"` |

Day-of-week and month-name strings throughout the date functions are English full
names (`"Monday"`…`"Sunday"`, `"January"`…`"December"`).

### Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration | `date("2025-01-31") + ymDuration("P1M")` | `date("2025-02-28")` (day clamped) |
| `-` (date) | days-time difference | `date("2025-03-28") - date("2025-01-01")` | `dtDuration("P86D")` |
| `< <= > >= = !=` | comparison | `date("2025-01-01") < date("2025-06-01")` | `true` |
| `between a and b` | inclusive range | `date("2025-03-15") between date("2025-01-01") and date("2025-12-31")` | `true` |
| `in` | membership (list / range / calendar) | `date("2025-04-18") in ukHolidays` | `true` |

A years-months duration adjusts the year/month and clamps the day
(`2025-01-31 + P1M → 2025-02-28`); a days-time duration adds whole days (sub-day
components are ignored for a bare `date`). Comparing a zoned date with a naive one
yields `null`.

Date subtraction depends on the zone-kind. When both operands are naive, `b - a`
is plain calendar arithmetic — whole days. When both are zoned, each is projected
to **midnight in its own zone** and the result is the UTC-instant gap between
those projections, which can be sub-day when the zones differ:

```
// expression-language
date("2025-03-28+05:30") - date("2025-03-28-05:00")  // → dtDuration("-PT10H30M")
date("2025-03-29+05:30") - date("2025-03-28-05:00")  // → dtDuration("PT13H30M")
date("2025-03-28")       - date("2025-01-01")        // → dtDuration("P86D")   (naive — whole days)
```

A mismatch (one naive, one zoned) yields `null`, exactly as for comparison.

### Constructing dates host-side

Host Go code builds a `bl.BlDate` via the generic `bl.Date[T](v T) (bl.BlDate, error)`
constructor, which accepts an ISO 8601 / RFC 9557 string, a Go `time.Time`, or a
`bl.DateComponents` struct.

```go
// host-side (Go)
var d1, _ = bl.Date("2025-03-28")                          // naive
var d2, _ = bl.Date("2025-03-28+05:30")                    // zoned, offset
var d3, _ = bl.Date("2025-03-28[Europe/London]")           // zoned, IANA zone

// From a time.Time — the date portion is extracted; the result is zoned.
var now      = time.Now()
var today, _ = bl.Date(now)

// From a time.Time but dropping the zone (wall-clock date, no zone).
var todayNaive, _ = bl.Date(bl.ToDateComponentsAsNaive(now))

// From explicit components.
var christmas, _ = bl.Date(bl.DateComponents{Year: 2025, Month: 12, Day: 25})

// Convenience: today in the local zone, as a naive date.
var t = bl.Today()
```

A `time.Time` always carries a `Location`, so the `time.Time` path always produces
a zoned date; route through `bl.ToDateComponentsAsNaive` first when you want a
naive result. A `bl.DateComponents` with both `Offset` and `Zone` set, or invalid
components, returns an error.

## Times

A `time` is a time of day — hours, minutes, seconds (optionally fractional) — with
an optional UTC offset or IANA timezone, or none at all (a local, wall-clock time).
There is no time literal; values come from the `time(...)` built-in.

```
// expression-language
time("14:30:00")                     // local
time("14:30:00.500")                 // fractional seconds
time("14:30:00Z")                    // UTC
time("11:45:30+02:00")               // fixed offset (not DST-aware)
time("14:30:00[Europe/Paris]")       // IANA timezone (DST-aware)
time(23, 59, 0)                      // from components (naive)
time(14, 30, 0, dtDuration("PT2H"))  // components with a UTC offset → 14:30:00+02:00
time(now())                          // current time of day
time(datetime("2025-03-28T14:30:00"))  // extract the time from a datetime
```

`time("24:00:00")` (end-of-day) is valid ISO 8601 and normalises to `00:00:00`.

### Component access

```
// expression-language
time("11:45:30+02:00").hour          // → 11
time("11:45:30+02:00").minute        // → 45
time("11:45:30+02:00").second        // → 30
time("11:45:30+02:00").offset        // → dtDuration("PT2H")
time("11:45:30[Europe/Paris]").timezone  // → "Europe/Paris"
```

### Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` | add/subtract a days-time duration (wraps at midnight) | `time("23:00:00") + dtDuration("PT2H")` | `time("01:00:00")` |
| `< <= > >= = !=` | comparison | `time("14:30:00") < time("17:00:00")` | `true` |
| `between a and b` | inclusive range | `time("12:00:00") between time("09:00:00") and time("17:00:00")` | `true` |
| `in` | membership (list / range) | `time(now()) in [time("09:00:00")..time("17:00:00")]` | `true`/`false` |

Only a **days-time** duration may be applied — time has no year/month concept, so
a years-months duration is a `bl.TypeError`. A `time` does not track days: adding
`PT25H` to `23:00:00` gives `00:00:00`, with the day advance discarded. Use
`datetime` when you need day-tracking arithmetic.

Comparison follows the usual rule: two zoned/offset times compare as UTC instants,
two local times compare wall-clock, and a local-versus-zoned comparison yields
`null`.

### Constructing times host-side

```go
// host-side (Go)
var morning, _ = bl.Time("09:00:00")                       // naive
var london,  _ = bl.Time("11:45:30+01:00")                 // zoned, offset
var paris,   _ = bl.Time("11:45:30[Europe/Paris]")         // zoned, IANA zone

// From a time.Time — time-of-day portion extracted; result is zoned.
var now       = time.Now()
var nowTime, _ = bl.Time(now)

// Wall-clock time, dropping the zone.
var wallClock, _ = bl.Time(bl.ToTimeComponentsAsNaive(now))

// From explicit components.
var noon, _ = bl.Time(bl.TimeComponents{Hour: 12, Minute: 0, Second: 0})
```

`bl.TimeComponents` also carries an optional `Nanosecond` field for fractional
seconds, and the same mutually-exclusive `Offset` / `Zone` pair as dates.

## Date-times

A `datetime` combines a date and a time, again with an optional offset or IANA
timezone. Values come from the `datetime(...)` built-in.

```
// expression-language
datetime("2025-03-28T14:30:00")             // local
datetime("2025-03-28T14:30:00Z")            // UTC
datetime("2025-03-28T14:30:00+01:00")       // offset
datetime("2025-03-28T14:30:00[Europe/Paris]")  // IANA zone
datetime(date("2025-03-28"), time("14:30:00"))  // combine a date and a time
now()                                        // current datetime (zoned, local zone)
```

The `T` separator is required; space-separated forms are rejected. Note that
`now()` returns a **zoned** datetime in the local zone, whereas `today()` returns
a *naive* date — to get a "wall-clock now", strip the zone with `withoutTimezone`
(below).

### Component access & calendar properties

All the date component accessors and calendar properties work on a `datetime`
too, plus the time-of-day components:

```
// expression-language
datetime("2025-03-28T14:30:00+01:00").year      // → 2025
datetime("2025-03-28T14:30:00+01:00").hour       // → 14
datetime("2025-03-28T14:30:00+01:00").minute     // → 30
datetime("2025-03-28T14:30:00+01:00").second     // → 0
datetime("2025-03-28T14:30:00+01:00").offset     // → dtDuration("PT1H")
datetime("2025-03-28T14:30:00[Europe/Paris]").timezone  // → "Europe/Paris"
datetime("2019-09-17T00:00:00").dayName          // → "Tuesday"
datetime("2025-09-17T00:00:00").yearQuarter      // → "2025Q3"
```

Extracting the date or time portion stays a function call — these are type
conversions, not properties:

| Function | Example | Result |
|---|---|---|
| `date(dt)` | extract the date | a `date` |
| `time(dt)` | extract the time | a `time` |

### Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration | `datetime("2025-01-31T12:00:00") + ymDuration("P1M")` | `datetime("2025-02-28T12:00:00")` (day clamped) |
| `-` (datetime) | days-time difference | `dt2 - dt1` | a days-time `duration` |
| `< <= > >= = !=` | comparison | `submittedAt <= deadline` | `true`/`false` |
| `between a and b` | inclusive range | `submittedAt between startDt and endDt` | `true`/`false` |
| `in` | membership (list / range / calendar) | `submittedAt in [startDt..endDt]` | `true`/`false` |

A years-months duration adjusts year/month (day clamped) and leaves the time
alone; a days-time duration adds precisely, carrying across date boundaries. Both
preserve the original zone/offset. You can chain the two kinds:
`dt + ymDuration("P1Y") + dtDuration("P10D")`.

Comparison: zoned datetimes compare as UTC instants, local ones wall-clock, and a
local-versus-zoned comparison yields `null`. `dt2 - dt1` is negative when `dt2`
precedes `dt1`.

### Constructing date-times host-side

```go
// host-side (Go)
var local,  _ = bl.DateTime("2025-03-28T11:45:30")               // naive
var utc,    _ = bl.DateTime("2025-03-28T11:45:30Z")              // zoned, UTC
var london, _ = bl.DateTime("2025-03-28T11:45:30+01:00")         // zoned, offset
var paris,  _ = bl.DateTime("2025-03-28T11:45:30[Europe/Paris]") // zoned, IANA zone

// From a time.Time — result is zoned.
var now      = time.Now()
var nowDT, _ = bl.DateTime(now)

// Wall-clock semantics, dropping the zone.
var wallClock, _ = bl.DateTime(bl.ToDateTimeComponentsAsNaive(now))

// From explicit components.
var deploy, _ = bl.DateTime(bl.DateTimeComponents{
    Year: 2025, Month: 3, Day: 1, Hour: 3, Minute: 0, Second: 0,
    Zone: "Europe/London",
})

// Convenience for the current moment (zoned).
var n = bl.Now()
```

## Durations

A duration is a *span*, and blkit keeps two distinct kinds because they obey
different arithmetic:

- A **days-time duration** (`dtDuration`) covers days, hours, minutes, and seconds
  — ISO 8601 `PnDTnHnMnS`.
- A **years-months duration** (`ymDuration`) covers only years and months — ISO
  8601 `PnYnM`.

The two **cannot be added to each other** (that is a `bl.TypeError`), because a
month is not a fixed number of days. A days-time duration can be applied to a
`date`, `time`, or `datetime`; a years-months duration can be applied to a `date`
or `datetime` but **not** a `time`.

### Days-time durations

Built by `dtDuration(...)` from an ISO 8601 string using only D/T designators:

```
// expression-language
dtDuration("P1DT2H30M")        // 1 day, 2 hours, 30 minutes
dtDuration("PT90M")            // → dtDuration("PT1H30M")   (minutes overflow into hours on input)
dtDuration("PT3600S")          // → dtDuration("PT1H")
dtDuration("PT1.5S")           // fractional seconds
dtDuration("PT0.123456789S")   // sub-nanosecond precision preserved (no float rounding)
dtDuration("-PT1H")            // negative
dtDuration("PT0S")             // zero
dtDuration("P1.5D")            // → dtDuration("P1DT12H")
dtDuration("p1dt2h30m")        // → dtDuration("P1DT2H30M")  (designators are case-insensitive on input)
dtDuration("P1Y")              // → bl.ParseError (year/month designators not allowed here)
```

Two deliberate relaxations of ISO 8601: a decimal fraction is accepted on **any**
of `D`/`H`/`M`/`S` (not just the smallest unit used), and designator letters are
case-insensitive on input. Canonical output always emits uppercase designators.
Storage is exact arbitrary-precision decimal total seconds — no float rounding.

#### Component access

```
// expression-language
dtDuration("P2DT3H45M10S").days          // → 2
dtDuration("P2DT3H45M10S").hours         // → 3
dtDuration("P2DT3H45M10S").minutes       // → 45
dtDuration("P2DT3H45M10S").seconds       // → 10
dtDuration("P2DT3H45M10S").totalSeconds  // → 186310           (signed total)
dtDuration("P2DT3H45M10S").totalMinutes  // → 3105.16666...    (signed; possibly fractional)
dtDuration("P2DT3H45M10S").totalHours    // → 51.75277...
dtDuration("P2DT3H45M10S").totalDays     // → 2.156866...
dtDuration("-P2DT3H45M10S").days         // → -2              (sign carried on every component)
dtDuration("PT90M").totalHours           // → 1.5             (totals divide exactly when they can)
```

The sign applies to the whole value: a negative duration has negative `.days`,
`.hours`, `.minutes`, and `.seconds` — not "negative days, positive hours". The
four `total*` accessors return the signed exact decimal total expressed in that
unit.

#### Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add two durations | `dtDuration("P1D") + dtDuration("PT12H")` | `dtDuration("P1DT12H")` |
| `-` | subtract two durations | `dtDuration("P1DT12H") - dtDuration("PT12H")` | `dtDuration("P1D")` |
| unary `-` | negate | `-dtDuration("P2DT3H")` | `dtDuration("-P2DT3H")` |
| `*` | scale by a number | `dtDuration("PT1H") * 2.5` | `dtDuration("PT2H30M")` |
| `/` | divide by a number | `dtDuration("PT1H") / 4` | `dtDuration("PT15M")` |
| `< <= > >=` | compare by total seconds | `dtDuration("PT60S") < dtDuration("PT2M")` | `true` |
| `= !=` | equality by total seconds | `dtDuration("PT60S") = dtDuration("PT1M")` | `true` |

`*` and `/` use exact decimal arithmetic — `dtDuration("PT1H") / 7` has a
`totalSeconds` of exactly `3600/7`, with the canonical string putting the fraction
on the smallest designator. Division by zero yields `null`. Mixing with a
years-months duration is a `bl.TypeError`.

#### Rounding

The six numeric rounding modes are overloaded so the second argument is a positive
duration `step`; the result is rounded to the nearest integer multiple of `step`.

| Function | Example | Result |
|---|---|---|
| `round(d, step)` | `round(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (alias of `roundHalfUp`) |
| `roundUp(d, step)` | `roundUp(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT45M")` (away from zero) |
| `roundDown(d, step)` | `roundDown(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (toward zero) |
| `roundHalfUp(d, step)` | `roundHalfUp(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT30M")` |
| `roundHalfDown(d, step)` | `roundHalfDown(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT15M")` |
| `roundHalfEven(d, step)` | `roundHalfEven(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (banker's rounding) |

```
// expression-language
round(dtDuration("PT1H37M"), dtDuration("PT1H"))     // → dtDuration("PT2H")    (nearest hour)
round(dtDuration("PT1H37M"), dtDuration("PT15M"))    // → dtDuration("PT1H30M") (nearest 15 min)
roundDown(dtDuration("P1DT23H"), dtDuration("P1D"))  // → dtDuration("P1D")     (truncate to whole days)
```

Rounding respects sign (`roundUp` of a negative input rounds away from zero,
making it more negative). A non-positive `step` is a `bl.TypeError`.

#### Other functions

| Function | Example | Result |
|---|---|---|
| `abs(d)` | `abs(dtDuration("-PT5H"))` | `dtDuration("PT5H")` |
| `isNegative(d)` | `isNegative(dtDuration("-PT1H"))` | `true` (zero → `false`) |

#### Constructing days-time durations host-side

`bl.DTDuration[T](v T) (bl.BlDaysTimeDuration, error)` is generic over a wide set
of inputs — strings parse as ISO 8601 (leading `P`) or otherwise as a decimal
number of total seconds; integers, floats, `decimal.Decimal`, `bl.BlNumber`, and
`time.Duration` are all total seconds; and `bl.DHMS(...)` supplies explicit
components.

```go
// host-side (Go)
var meeting, _ = bl.DTDuration("PT1H30M")     // ISO 8601 string
var weekend, _ = bl.DTDuration("P2D")
var oneMin,  _ = bl.DTDuration(60)            // integer total seconds
var floatS,  _ = bl.DTDuration(1.5)           // 1.5 seconds
var fromStr, _ = bl.DTDuration("1.5")         // decimal-number string → total seconds

// Exact fractional total via decimal.Decimal.
var dec,     _ = decimal.NewFromString("0.000000001")
var nano,    _ = bl.DTDuration(dec)

// From a Go time.Duration.
var fromGo,  _ = bl.DTDuration(2*time.Hour + 30*time.Minute)

// Explicit components — bl.DHMS(days, hours, minutes, seconds), each argument
// independently type-checked; mixed types across the four arguments are fine.
var split,   _ = bl.DTDuration(bl.DHMS(1, 2, 30, 0))           // 1d 2h 30m
var mixed,   _ = bl.DTDuration(bl.DHMS(1, 2, "30", 0))
```

A `float` holding `NaN`/`Inf` errors; the integer / decimal / `time.Duration` /
`bl.DHMS` paths are infallible. A Go `time.Duration` supplied as an env input
variable also wraps to `bl.BlDaysTimeDuration` automatically.

### Years-months durations

Built by `ymDuration(...)` from an ISO 8601 string using only Y/M designators:

```
// expression-language
ymDuration("P1Y6M")        // 1 year, 6 months
ymDuration("P6M")          // 6 months
ymDuration("-P1Y6M")       // negative
ymDuration("P0Y0M")        // zero
ymDuration("P13M")         // → ymDuration("P1Y1M")  (months overflow into years on input)
ymDuration("P1.5Y")        // → ymDuration("P1Y6M")
ymDuration("P0.5M")        // 0.5 months — fractional months are representable
ymDuration("p1y6m")        // → ymDuration("P1Y6M")  (designators are case-insensitive on input)
ymDuration("P1DT2H")       // → bl.ParseError (day/time designators not allowed here)
```

The same two relaxations apply (fraction on either designator; case-insensitive
input), and storage is exact decimal total months.

#### Component access

```
// expression-language
ymDuration("P2Y7M").years        // → 2
ymDuration("P2Y7M").months       // → 7
ymDuration("P2Y7M").totalMonths  // → 31         (signed years*12 + months)
ymDuration("P2Y7M").totalYears   // → 2.58333...  (signed; possibly fractional)
ymDuration("-P2Y7M").years       // → -2         (sign carried on every component)
ymDuration("P0Y15M").years       // → 1          (normalised)
ymDuration("P1Y0.25M").months    // → 0.25       (fractional remainder preserved)
```

Months normalise to `0 ≤ |months| < 12`, with overflow carrying into years.

#### Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add two durations | `ymDuration("P1Y") + ymDuration("P6M")` | `ymDuration("P1Y6M")` |
| `-` | subtract two durations | `ymDuration("P1Y6M") - ymDuration("P6M")` | `ymDuration("P1Y")` |
| unary `-` | negate | `-ymDuration("P1Y6M")` | `ymDuration("-P1Y6M")` |
| `*` | scale by a number | `ymDuration("P6M") * 3` | `ymDuration("P1Y6M")` |
| `/` | divide by a number | `ymDuration("P1Y") / 4` | `ymDuration("P3M")` |
| `< <= > >=` | compare by total months | `ymDuration("P1Y") > ymDuration("P11M")` | `true` |
| `= !=` | equality by total months | `ymDuration("P1Y") = ymDuration("P12M")` | `true` |

`*` and `/` are exact decimal; `/0` yields `null`. Mixing with a days-time
duration is a `bl.TypeError`, and applying a years-months duration to a `time` is
also a `bl.TypeError`.

#### Rounding & other functions

The same six rounding modes are overloaded with a positive duration `step`,
letting you round to the nearest month, quarter, half-year, or year:

```
// expression-language
round(ymDuration("P14M"), ymDuration("P1Y"))      // → ymDuration("P1Y")    (nearest year)
round(ymDuration("P14M"), ymDuration("P3M"))      // → ymDuration("P1Y3M")  (nearest quarter)
roundDown(ymDuration("P1Y11M"), ymDuration("P1Y"))  // → ymDuration("P1Y")  (truncate to whole years)
```

| Function | Example | Result |
|---|---|---|
| `abs(d)` | `abs(ymDuration("-P2Y3M"))` | `ymDuration("P2Y3M")` |
| `isNegative(d)` | `isNegative(ymDuration("-P3M"))` | `true` (zero → `false`) |

#### Constructing years-months durations host-side

`bl.YMDuration[T](v T) (bl.BlYearsMonthsDuration, error)` mirrors its days-time
sibling: strings parse as ISO 8601 (leading `P`) or as a decimal number of total
months; integers/floats/decimals are total months; `bl.YM(...)` supplies explicit
components.

```go
// host-side (Go)
var mortgage, _ = bl.YMDuration("P30Y")           // ISO 8601 string
var grace,    _ = bl.YMDuration("P1Y6M")
var halfYear, _ = bl.YMDuration(6)                // 6 total months
var fromStr,  _ = bl.YMDuration("12.25")          // decimal-number string → 1y 0.25m

// Explicit components — bl.YM(years, months), each argument independently typed.
var split,    _ = bl.YMDuration(bl.YM(1, 6))
var partial,  _ = bl.YMDuration(bl.YM(2, "3.5"))  // 2y 3.5m → 27.5 total months
```

### Differences as durations

Two named functions compute the span between two dates or datetimes directly as a
duration — handy when you want the result as a `duration` value rather than a raw
number:

```
// expression-language
dtDurationBetween(date("2025-01-01"), date("2025-03-28"))     // → dtDuration("P86D")
ymDurationBetween(date("2011-12-22"), date("2013-08-24"))     // → ymDuration("P1Y8M")
ymDurationBetween(date("2025-06-01"), date("2024-06-01"))     // → ymDuration("-P1Y")  (signed)
```

Both are **signed** (`from > to` gives a negative result) and require both
operands to be the **same** temporal kind — either both `date` or both `datetime`.
A mixed `(date, datetime)` call is a type error; convert one operand first with
`datetime(d)` (lifts a date to midnight) or `date(dt)`. `dtDurationBetween` is
exactly equivalent to the `-` operator on dates/datetimes, including the zoned
midnight-projection and the naive-versus-zoned `→ null` rule.

## Calendars

A `calendar` is a blkit-specific, immutable, chronologically-ordered collection of
temporal entries — each a `date`, `datetime`, or a `range` of either, with an
optional name. Calendars model holiday sets, maintenance windows, blackout
periods, and freeze schedules.

Calendars have **no expression-language literal or constructor** — they are built
host-side in Go and supplied to an expression as an input variable, where they are
queried. The rationale is that holiday sets and schedules are configuration data,
better assembled with the type-safe host API than encoded inline in a rule.

### Building a calendar host-side

```go
// host-side (Go)
var ukHolidays, _ = bl.Calendar(
    []bl.BlCalendarEntry{
        bl.CalendarEntry(blDate("2025-01-01"), "New Year's Day"),
        bl.CalendarEntry(blDate("2025-04-18"), "Good Friday"),
        bl.CalendarEntry(blDate("2025-04-21"), "Easter Monday"),
        bl.CalendarEntry(blDate("2025-12-25"), "Christmas Day"),
        bl.CalendarEntry(blDate("2025-12-26"), "Boxing Day"),
        bl.CalendarEntry(
            blRange(blDate("2025-12-24"), blDate("2026-01-02")),
            "Holiday closure"),
    },
    bl.WithValidity(blRange(blDate("2025-01-01"), blDate("2025-12-31"))))

// Hand it to an expression as an env field.
type HolidayEnv struct {
    ApplicantDate bl.BlDate     `expr:"applicantDate"`
    UkHolidays    bl.BlCalendar `expr:"ukHolidays"`
}
var checkHoliday, _ = bl.Expr[HolidayEnv](`isPublicHoliday(applicantDate, ukHolidays)`)
```

`bl.CalendarEntry(value, name...)` is infallible; all validation — entries must be
temporal points or bounded temporal ranges, must share a single zone-kind, and the
validity range must match that zone-kind — happens at the `bl.Calendar(...)`
assembly step. `bl.WithValidity` marks the period the calendar is authoritative
over (used by the strictness flag below; it does **not** filter queries).

Calendars can also be imported from RFC 5545 iCalendar (`.ics`) data via
`bl.ImportICal(r io.Reader, opts ...bl.ICalOption)` — the recommended path for
holiday feeds, with options for the recurrence-expansion window, validity range,
default entry name, and strict mode.

**Zone-kind homogeneity.** Every entry in a non-empty calendar must share the same
zone-kind — all zoned, or all naive. Mixing them is an error at construction. (A
query against the calendar must likewise share its zone-kind, or it is a
`bl.TypeError`.) Within a zone-kind, temporal kinds may mix freely — date points,
datetime points, and ranges of either side by side; queries that cross temporal
kinds simply produce no match for the mismatched entries rather than poisoning the
result.

### Query & inspection

| Function | Example | Result |
|---|---|---|
| `count(c)` | `count(ukHolidays)` | entry count |
| `isEmpty(c)` | `isEmpty(ukHolidays)` | `true` if no entries |
| `entries(c)` | `entries(c)` | entries in chronological order |
| `names(c)` | `names(c)` | distinct names, chronological (unnamed excluded) |
| `find(c, name)` | `find(ukHolidays, "Christmas Day")` | entries with that name (case-sensitive) |
| `contains(c, point)` | `contains(ukHolidays, date("2025-12-25"))` | `true` if any entry covers `point` |
| `entriesFor(c, point)` | `entriesFor(c, date("2025-05-05"))` | entries covering the point |
| `overlaps(c, range)` | `overlaps(windows, [t1..t2])` | `true` if any entry overlaps the range |
| `entriesIn(c, range)` | `entriesIn(ukHolidays, [date("2025-01-01")..date("2025-03-31")])` | entries overlapping the range |
| `validFrom(c)` / `validTo(c)` | `validFrom(ukHolidays)` | the bound, or `null` |
| `validRange(c)` | `validRange(ukHolidays)` | `[validFrom..validTo]`, or `null` if either unset |
| `entryValue(e)` / `entryName(e)` | `entryName(find(c, "X")[1])` | an entry's value / name (`null` if unnamed) |
| `next(c, point[, n])` | `next(ukHolidays, date("2025-12-01"))` | the *n*th entry strictly **after** `point` (`n` defaults to 1; `null` if too few) |
| `prev(c, point[, n])` | `prev(ukHolidays, date("2025-12-01"), 2)` | the *n*th entry strictly **before** `point` |

```
// expression-language
next(ukHolidays, date("2025-04-01"))       // → the Good Friday entry (2025-04-18)
next(ukHolidays, date("2025-04-01"), 2)    // → Easter Monday (2025-04-21)
prev(ukHolidays, date("2025-04-21"))       // → Good Friday
prev(ukHolidays, date("2025-01-01"))       // → null (nothing before New Year's Day)
```

For `next`/`prev`, an entry is *strictly after* `point` when its position (the
value for a point entry, or `range.start` for a range) is `> point`, and *strictly
before* when its position (or `range.end`) is `< point`. A range that straddles
`point` is the currently-active entry, so it is neither next nor prev. `n` must be
a positive integer.

### The `in` operator

`point in calendar` is sugar for `contains(calendar, point)`:

```
// expression-language
date("2025-12-25") in ukHolidays   // → true
```

The left operand must be a `date` or `datetime` (a range operand is a
`bl.TypeError` — use `overlaps` / `entriesIn` for range queries). Calendars also
support `=`/`!=` (set-equality on entries plus matching validity bounds) but have
no ordering or arithmetic operators.

### Mutation

Each of these returns a fresh calendar; the receiver is never mutated.

| Function | Example | Result |
|---|---|---|
| `calendarDrop(c, target[, options])` | `calendarDrop(ukHolidays, "Boxing Day")` | entries matching `target` **removed** |
| `calendarKeep(c, target[, options])` | `calendarKeep(ukHolidays, "Boxing Day")` | entries matching `target` **retained** (inverse of drop) |
| `calendarMerge(calendars[, options])` | `calendarMerge([england, scotland], {dedupeBy: "value"})` | union of all entries, re-sorted, with optional dedupe |

`calendarDrop` and `calendarKeep` are symmetric inverses. The `target` selects the
match mode by its type: a string matches entry names exactly; a `pattern(...)`
matches names by regex; a `date`/`datetime`/`range` matches by value; and a list
matches any of its items. For a `range` target, an `options` dictionary key
`rangeMatch` chooses how it compares against entries — `"equality"` (default),
`"entryWithin"`, `"entryEncloses"`, or `"overlap"`.

```
// expression-language
calendarDrop(ukHolidays, "Christmas Day")                              // exact name
calendarDrop(ukHolidays, pattern("^Bank Holiday.*"))                   // regex name
calendarDrop(ukHolidays, date("2025-12-25"))                           // by value
calendarKeep(ukHolidays, [date("2025-12-25"), date("2025-12-26")])     // only the Christmas pair
calendarDrop(c, blRange(date("2025-12-22"), date("2025-12-28")), {rangeMatch: "overlap"})
    // drops everything touching that week
```

`calendarMerge` unions the entries of several calendars and re-sorts them.
Supplying `{dedupeBy: "value"}` (or `"valueAndName"`) deduplicates during the
merge, with an optional `{tiebreak: "first" | "name"}` choosing the survivor on a
value clash. Validity bounds are **not** unioned; re-supply them via a follow-up
`bl.Calendar(..., bl.WithValidity(...))` host construction if you need them.

The host-side equivalents are `(c bl.BlCalendar).Drop(target, opts...)`,
`.Keep(target, opts...)`, and the package-level `bl.CalendarMerge(calendars,
opts...)`, with functional options `bl.WithRangeMatch`, `bl.WithDedupe`, and
`bl.WithTiebreak`.

## Calendar-aware date functions

These functions accept either a `date` or a `datetime` as their first argument
(the examples use `date(...)` for brevity). For a `datetime`, value-returning
functions preserve the time-of-day and zone; for boolean or number returns the
time component is ignored.

### Classification

| Function | Example | Result |
|---|---|---|
| `isWeekday(v)` | `isWeekday(date("2025-03-24"))` | `true` |
| `isWeekend(v)` | `isWeekend(date("2025-03-29"))` | `true` |
| `isPublicHoliday(v, phCalendar)` | `isPublicHoliday(date("2025-04-18"), ukHolidays)` | `true` |
| `isBusinessDay(v[, phCalendar])` | `isBusinessDay(date("2025-04-18"), ukHolidays)` | `false` (Good Friday — a weekday but in the calendar). Without `phCalendar`: weekend check only |

### Month boundaries

| Function | Example | Result |
|---|---|---|
| `lastDayOfMonth(v)` | `lastDayOfMonth(date("2024-02-10"))` | `date("2024-02-29")` |
| `firstDayOfMonth(v)` | `firstDayOfMonth(date("2025-02-14"))` | `date("2025-02-01")` |
| `lastDayOfPrevMonth(v)` | `lastDayOfPrevMonth(date("2025-01-01"))` | `date("2024-12-31")` |
| `firstDayOfNextMonth(v)` | `firstDayOfNextMonth(date("2025-12-31"))` | `date("2026-01-01")` |

### Week-in-month navigation

| Function | Example | Result |
|---|---|---|
| `firstDayOfWeekInMonth(v, dow)` | `firstDayOfWeekInMonth(date("2025-03-15"), "Monday")` | `date("2025-03-03")` |
| `lastDayOfWeekInMonth(v, dow)` | `lastDayOfWeekInMonth(date("2025-03-15"), "Friday")` | `date("2025-03-28")` |
| `nthDayOfWeekInMonth(v, n, dow)` | `nthDayOfWeekInMonth(date("2025-03-15"), 2, "Monday")` | `date("2025-03-10")` (`n<0` counts from the end; out of range → `null`) |

### Day navigation

| Function | Example | Result |
|---|---|---|
| `nextDayOfWeek(v, dow)` / `prevDayOfWeek(v, dow)` | `nextDayOfWeek(date("2025-03-24"), "Monday")` | `date("2025-03-31")` (strictly after) |
| `nextWeekday(v)` / `prevWeekday(v)` | `nextWeekday(date("2025-03-28"))` | `date("2025-03-31")` (Fri → Mon) |
| `nextBusinessDay(v[, phCalendar[, strictCalendarRange]])` / `prevBusinessDay(...)` | `nextBusinessDay(date("2025-04-17"), ukHolidays)` | `date("2025-04-22")` (skips weekend + holidays). Without `phCalendar`: weekends only |

### Business-day arithmetic & difference

The `phCalendar` argument is optional — omit it to skip weekends only, supply it
to additionally skip the calendar's dates.

| Function | Example | Result |
|---|---|---|
| `addBusinessDays(v, n[, phCalendar[, strictCalendarRange]])` | `addBusinessDays(date("2025-04-17"), 2, ukHolidays)` | `date("2025-04-23")` (Good Friday + Easter Monday skipped) |
| `subtractBusinessDays(v, n[, phCalendar[, strictCalendarRange]])` | `subtractBusinessDays(date("2025-04-23"), 2, ukHolidays)` | `date("2025-04-17")` |
| `weekdaysBetween(a, b)` | `weekdaysBetween(date("2025-03-24"), date("2025-03-28"))` | `5` (inclusive; order-independent) |
| `businessDaysBetween(a, b[, phCalendar[, strictCalendarRange]])` | `businessDaysBetween(date("2025-04-14"), date("2025-04-25"), ukHolidays)` | `8` |

`addBusinessDays` / `subtractBusinessDays` with `n = 0` return the date unchanged;
`nextBusinessDay` / `prevBusinessDay` always return a strictly different date.

**Calendar-range strictness.** When an iterating business-day function steps past
the calendar's `[validFrom, validTo]` window, the default is to continue silently
(the calendar simply contributes no holiday data beyond its bounds). Pass
`strictCalendarRange: true` as the trailing argument to instead raise a
`bl.CalendarRangeError` the moment iteration would cross the boundary — useful when
you need every returned value guaranteed inside the authoritative window. The flag
is available on `addBusinessDays`, `subtractBusinessDays`, `nextBusinessDay`,
`prevBusinessDay`, and `businessDaysBetween`; it is meaningless without a
`phCalendar`, and `isBusinessDay` (which never iterates) does not accept it.

### Date difference: days, months, years

`daysBetween`, `monthsBetween`, and `yearsBetween` return the elapsed difference as
a **number** (rather than a duration). Results are **signed** — positive when
`v2 > v1` — so use `abs()` for magnitude.

| Function | Example | Result |
|---|---|---|
| `daysBetween(v1, v2)` | `daysBetween(date("2025-01-01"), date("2025-03-15"))` | `73` (actual elapsed days) |
| `daysBetween(dt1, dt2, includeTime)` | `daysBetween(datetime("2025-01-15T00:00:00"), datetime("2025-01-16T12:00:00"), true)` | `1.5` (36 hours ÷ 24) |
| `monthsBetween(v1, v2[, basis])` | `monthsBetween(date("2024-01-10"), date("2025-07-25"))` | `18.4839` (default `"calendar"`) |
| `yearsBetween(v1, v2[, basis])` | `yearsBetween(date("2024-01-10"), date("2025-07-25"))` | `1.5370` (default `"calendar"`) |

`monthsBetween` and `yearsBetween` take an optional `basis` selecting a day-count
convention; `daysBetween` doesn't (actual days has no choices). For `datetime`
inputs, an optional trailing `includeTime` boolean (default `false`) controls
whether sub-day differences contribute fractionally — it isn't accepted on `date`
forms.

The `basis` values: `"calendar"` (default — the intuitive "human" answer for age
or tenure), `"actual/365"`, `"actual/360"`, `"actual/actual"` (ISDA, leap-aware),
`"30/360"` (US/NASD), and `"30E/360"` (European). An invalid `basis` is a
`bl.TypeError`.

### Financial year

| Function | Example | Result |
|---|---|---|
| `financialYear(v, basis)` | `financialYear(date("2024-08-01"), "AU")` | `"FY2025"` |
| `financialYearQuarter(v, basis)` | `financialYearQuarter(date("2024-08-01"), "AU")` | `"FY2025Q1"` |

`basis` is either a number 1–12 (the calendar month the financial year starts) or
a jurisdiction code — `"AU"` (July 1), `"UK"` (April 6), `"US"` (October 1),
`"IN"`, `"JP"`, `"CA"`, `"NZ"` (April 1). The financial year is labelled by the
calendar year it **ends** in:

```
// expression-language
financialYear(date("2024-06-30"), "AU")          // → "FY2024"   (FY 2024 ends here)
financialYear(date("2024-07-01"), "AU")          // → "FY2025"   (FY 2025 begins here)
financialYearQuarter(date("2025-01-15"), "AU")   // → "FY2025Q3" (AU FY 2025 Q3: Jan–Mar)
financialYearQuarter(date("2024-08-01"), 7)      // → "FY2025Q1" (numeric basis equivalent)
```

A `basis` outside 1–12, or an unrecognised jurisdiction, is a `bl.TypeError`.

### Re-zoning and zone stripping

**Re-zoning** changes the zone while preserving the instant (the wall-clock
numbers shift):

| Function | Example | Result |
|---|---|---|
| `withOffset(v, off)` | `withOffset(datetime("2025-03-28T14:30:00+01:00"), dtDuration("PT2H"))` | `datetime("2025-03-28T15:30:00+02:00")` |
| `withTimezone(dt, zone)` | `withTimezone(datetime("2025-03-28T14:30:00Z"), "Europe/Paris")` | `datetime("2025-03-28T15:30:00[Europe/Paris]")` |

`withOffset` takes a days-time duration and accepts a `time` or a `datetime`;
`withTimezone` takes an IANA name and is `datetime`-only. To re-zone to UTC, pass
`dtDuration("PT0H")` to `withOffset`. Neither is defined for naive values (there is
no source zone to convert *from*) — calling on a naive input returns `null`. A
`date` is not supported by either (it has no time-of-day to shift).

**Zone stripping** drops the zone *label* while preserving the wall-clock numbers,
returning a naive value:

| Function | Example | Result |
|---|---|---|
| `withoutOffset(v)` | `withoutOffset(datetime("2025-03-28T14:30:00+01:00"))` | `datetime("2025-03-28T14:30:00")` |
| `withoutTimezone(v)` | `withoutTimezone(datetime("2025-03-28T14:30:00[Europe/Paris]"))` | `datetime("2025-03-28T14:30:00")` |
| `withoutOffsetOrTimezone(v)` | `withoutOffsetOrTimezone(datetime("2025-03-28T14:30:00+01:00"))` | `datetime("2025-03-28T14:30:00")` |

Each accepts a `date` or a `datetime`, returns the same type, and is a no-op on a
value that already has no zone.

## A worked example: compile once, evaluate many

The temporal types slot into the same compile-once / evaluate-many model as the
rest of the language. Declare a typed env, compile the expression once, then run
it against many inputs.

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

type claim struct {
    SubmittedAt bl.BlDateTime `expr:"submittedAt"`
    Deadline    bl.BlDateTime `expr:"deadline"`
}

// On time, and submitted within business hours? Compiled once.
var onTime, _ = bl.Expr[claim](`submittedAt <= deadline
    and time(submittedAt) between time("09:00:00") and time("17:00:00")`)

var submitted, _ = bl.DateTime("2025-03-28T14:30:00Z")
var deadline, _  = bl.DateTime("2025-03-31T17:00:00Z")
var result, _    = onTime.Evaluate(claim{SubmittedAt: submitted, Deadline: deadline})
// result → true
```

A second example combines a calendar with business-day arithmetic to compute a
settlement date:

```
// expression-language
addBusinessDays(today(), 3, ukHolidays)   // three business days from now, skipping UK holidays
```

## Null and edge-case behaviour

A handful of rules recur across all the temporal types — worth keeping in one
place:

- **Naive versus zoned.** Comparing, differencing, or otherwise combining a
  timezone-naive value with a zoned one yields `null`, never a silent coercion.
  This applies to `<`/`<=`/`>`/`>=`, `=`/`!=`, the `-` difference operator,
  `dtDurationBetween` / `ymDurationBetween`, and the `includeTime` date-difference
  forms.
- **Day clamping.** Adding a years-months duration that would overflow a short
  month clamps the day: `date("2025-01-31") + ymDuration("P1M") → date("2025-02-28")`.
- **Construction validates.** An out-of-range month/day (`date(2025, 13, 1)`),
  hour/minute/second (`time` component form), an unknown IANA zone, or a value
  carrying both an offset and a timezone is an error — there is no silent rollover.
- **Duration kinds don't mix.** Adding a `dtDuration` to a `ymDuration` is a
  `bl.TypeError`; applying a `ymDuration` to a `time` is a `bl.TypeError`.
- **Division by zero.** Scaling a duration by `/ 0` yields `null`.
- **Wrong duration string.** A Y/M designator passed to `dtDuration` (or a D/T
  designator to `ymDuration`) is a `bl.ParseError`.
- **Calendar queries are zone-kind-strict.** A query value must share the
  calendar's zone-kind, or it is a `bl.TypeError`; within a zone-kind, temporal-kind
  mismatches between the query and individual entries produce no match (not a
  poisoned result).

## Where the detail lives

This guide covers the user-facing surface. For the exhaustive, authoritative
definition of every operator, function, basis convention, and edge case, see the
specs under `specs/expressions/` — `date.spec.md`, `time.spec.md`,
`datetime.spec.md`, `days_time_duration.spec.md`,
`years_months_duration.spec.md`, and `calendar.spec.md` — and the generated
[Reference](../reference/blkit.md) for the full Go API. For how the
engine compiles and runs these expressions, see
[Architecture → Expressions](../architecture/expressions.md).
