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
date("2025-03-28+05:30").offset    // → dtDuration("PT5H30M")  (ext; null if none)
date("2025-03-28[Europe/London]").timezone // → "Europe/London" (ext; null if none)
```

`[@test] ../../expr/date_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` (duration) | add/subtract a duration (day clamped for years-months) | `date("2025-01-31") + ymDuration("P1M")` | `date("2025-02-28")` |
| `-` (date) | days-time difference | `date("2025-03-28") - date("2025-01-01")` | `dtDuration("P86D")` |
| `< <= > >= = !=` | comparison | `date("2025-01-01") < date("2025-06-01")` | `true` |
| `between a and b` | inclusive range | `date("2025-03-15") between date("2025-01-01") and date("2025-12-31")` | `true` |
| `in` | membership (list / range / calendar) | `date("2025-04-18") in ukHolidays` | `true` |

A years-months duration adjusts year/month (day clamped); a days-time duration adds whole days (sub-
day components ignored for `date`). Comparing a tz-aware date with a tz-naive one → `null`.

`[@test] ../../expr/date_operators_test.go`

---

## Built-in functions

DMN-inspired functions plus blkit extensions (**ext**). Day-of-week arguments are English full names
(`"Monday"`…`"Sunday"`); month names likewise.

### Calendar properties

Calendar properties are accessed via **dot syntax**, alongside the basic component accessors
(`.year`, `.month`, etc.). There is no function-call form — `dayName(d)`, `monthName(d)`,
etc. are *not* registered as user-callable functions. The component-access patcher recognises
these names on `BlDate` (and `BlDateTime`) operands and dispatches to the corresponding internal
accessor.

| Accessor | Example | Result |
|---|---|---|
| `.dayName` | `date("2025-03-24").dayName` | `"Monday"` |
| `.dayNameShort` **ext** | `date("2025-03-24").dayNameShort` | `"Mon"` (3-letter English) |
| `.dayOfYear` | `date("2019-09-17").dayOfYear` | `260` |
| `.weekOfYear` **ext** | `date("2019-09-17").weekOfYear` | `38` (simple Jan-1 anchor: week 1 = Jan 1–7, week 2 = Jan 8–14, …; always 1–53; always matches `.year`) |
| `.isoWeekOfYear` | `date("2025-12-29").isoWeekOfYear` | `1` (ISO 8601: week 1 = week containing the year's first Thursday; may belong to the adjacent calendar year — `date("2025-12-29")` is in ISO week 1 of 2026) |
| `.isoYearWeek` **ext** | `date("2025-12-29").isoYearWeek` | `"2026W1"` (combined ISO year + week identifier, year-then-unit format matching `.yearQuarter`. Use this when you need a single unambiguous label for an ISO week — the year part can differ from `.year` near year boundaries, so the combined form is the safe identifier.) |
| `.monthName` | `date("2019-09-17").monthName` | `"September"` |
| `.monthNameShort` **ext** | `date("2019-09-17").monthNameShort` | `"Sep"` (3-letter English) |
| `.quarter` **ext** | `date("2025-09-17").quarter` | `3` (1–4 calendar quarter) |
| `.yearQuarter` **ext** | `date("2025-09-17").yearQuarter` | `"2025Q3"` |

### Calendar utilities, business-day arithmetic, date difference

These function families accept either a `BlDate` or a `BlDateTime` and are documented in
[datetime.spec.md](datetime.spec.md). They cover classification (`isWeekday`, `isWeekend`,
`isBusinessDay`, `isPublicHoliday`), month boundaries (`firstDayOfMonth`, `lastDayOfMonth`,
…), week-in-month navigation (`firstDayOfWeekInMonth`, …), day navigation (`nextWeekday`,
`nextBusinessDay`, …), business-day arithmetic (`addBusinessDays`, `subtractBusinessDays`,
`weekdaysBetween`, `businessDaysBetween`, `ymDurationBetween`), date difference
(`daysBetween`, `monthsBetween`, `yearsBetween`), financial year (`financialYear`,
`financialYearQuarter`), and zone stripping (`withoutOffset`, `withoutTimezone`,
`withoutOffsetOrTimezone`).

The links below jump straight to the relevant section in `datetime.spec.md`:

- [Classification](datetime.spec.md#classification-ext)
- [Month boundaries](datetime.spec.md#month-boundaries)
- [Week-in-month navigation](datetime.spec.md#week-in-month-navigation-ext)
- [Day navigation](datetime.spec.md#day-navigation-ext)
- [Business-day arithmetic & difference](datetime.spec.md#business-day-arithmetic--difference-ext)
- [Date difference: days, months, years](datetime.spec.md#date-difference-days-months-years-ext)
- [Financial year](datetime.spec.md#financial-year-ext)
- [Calendar-range strictness](datetime.spec.md#calendar-range-strictness)
- [Zone stripping](datetime.spec.md#zone-stripping-ext)

### Interval algebra & combination

A date is a *point*; the interval-algebra built-ins (`coincides`, `starts`, `during`, `finishes`,
`before`, `after`, …) are in [range.spec.md](range.spec.md), e.g.
`during(date("2025-05-15"), [date("2025-04-01")..date("2025-06-30")]) // → true`. Combine a date and
time with `datetime(date, time)` ([datetime.spec.md](datetime.spec.md)).

---

## Go implementation (expr extension)

Lives in `expr/date.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlDate` is the immutable Go value type that represents a calendar date inside the engine and
at the host-code boundary. It wraps a Go [`time.Time`](https://pkg.go.dev/time#Time) — Go has
no separate date type, so `time.Time` is used by convention with the time portion set to
`00:00:00.000000000` (midnight) and ignored by all `BlDate` operations — plus a `naive`
boolean that distinguishes the timezone-naive case from a genuinely UTC-located one. Both
fields are private so callers cannot mutate the underlying value; every operation in the
library returns a fresh `BlDate`.

Constructors normalise the time portion to midnight on input. Conversions in/out (e.g.
`Time()` accessor returning the wrapped `time.Time`, or callers passing a `time.Time` into
`Date(...)` whose time-of-day is non-zero) discard the time component; callers should not
read the hour/minute/second fields of a `BlDate.Time()` value.

The `naive` flag is needed because Go's `time.Time` always carries a non-nil `*time.Location`,
so there is no built-in way to represent "a calendar date with no timezone interpretation".
When `naive` is true, the `Location()` of the wrapped `time.Time` is `time.UTC` by convention
and must be ignored. When `naive` is false, the `Location()` is meaningful and may be
`time.UTC`, a fixed-offset zone from [`time.FixedZone`](https://pkg.go.dev/time#FixedZone), or
an IANA zone loaded via [`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation).

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares wall-clock dates when both operands are naive, and compares the
  underlying `time.Time` instants (date portion, equivalent location) when both are zoned;
  mixing them returns `BlNull`. `String()` doubles as the `fmt.Stringer` implementation,
  producing the canonical ISO 8601 / RFC 9557 form (e.g. `"2025-03-28"`,
  `"2025-03-28+05:30"`, or `"2025-03-28[Europe/London]"`).
- **`Date[T DateInput](v T)`** — the generic host constructor. `DateInput` accepts `string`
  (parsed as ISO 8601 / RFC 9557 — `naive` is set based on whether a zone designator was
  present), `time.Time` (the date portion is extracted; `naive` is always false because a
  `time.Time` always carries a `Location`), or a `DateComponents` struct for explicit
  component-by-component construction (`naive` is true if and only if both `Offset` and `Zone`
  are absent). The `error` return fires for unparseable strings, invalid components
  (month=13, day=32, etc.), or a `DateComponents` with both `Offset` and `Zone` set (they're
  mutually exclusive). Host code wanting a naive result from a `time.Time` should pipe it
  through `ToDateComponentsAsNaive` first.
- **`Time()` accessor** — hands back the wrapped `time.Time` (with the date portion
  meaningful, time portion conventionally 00:00:00). For non-naive values this is the full
  representation. For naive values the host receives a `time.Time` with `Location() == time.UTC`,
  which it MUST treat as wall-clock — callers that need to know which kind they have should
  also call `IsNaive()`.
- **`IsNaive()` accessor** — reports whether the value is timezone-naive.
- **`Today()`** — convenience helper that returns the current date as a naive `BlDate` in the
  system's local zone (uses `time.Now().Local()` and strips the time portion).

```go
type BlDate struct {
    t     time.Time   // date portion meaningful; Location() carries offset or IANA zone when naive==false
    naive bool        // true when no offset/zone was specified — Location() is ignored
}

// BlValue interface — required by all Bl* value types.
func (BlDate) Type() BlType { return BlTypeDate }
func (d BlDate) Equal(other BlValue) BlValue   // wall-clock or zoned; cross-kind → Null
func (d BlDate) String() string                // canonical ISO 8601 / RFC 9557
func (BlDate) isBlValue() {}

// Host constructor — accepts an ISO 8601/RFC 9557 string, a Go time.Time, or component bundle.
type DateComponents struct {
    Year, Month, Day int
    Offset *time.Duration   // optional; mutually exclusive with Zone
    Zone   string           // optional; IANA name; mutually exclusive with Offset
}
type DateInput interface { string | time.Time | DateComponents }
func Date[T DateInput](v T) (BlDate, error)

// Decompose a time.Time into date components with Offset/Zone unset, so the
// resulting DateComponents builds a naive BlDate when passed to Date. Use this
// when host code has a time.Time but wants to drop its zone interpretation.
func ToDateComponentsAsNaive(t time.Time) DateComponents

// Convenience for current date.
func Today() BlDate

// Host accessors (consume an evaluated result).
func (d BlDate) Time() time.Time               // wrapped time.Time; for naive values its Location() is UTC and must be ignored
func (d BlDate) IsNaive() bool                 // true when timezone-naive
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlDate` and cannot apply Go's native `+`/`-`/`<`/etc. to
blkit values. For every operator that should work on dates, blkit supplies a named Go function
that performs the operation and returns the result wrapped as a `BlValue`. The connection from
operator token to function happens in two steps, neither of which is unique to `BlDate`:

1. The Registrations section below calls `expr.Function("addDateDT", typed2(addDateDT), …)`,
   which makes the engine aware of the function under that exact string name and records its
   type signature.
2. A central `operatorBindings()` in [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then
   calls `expr.Operator("+", "addNumbers", "concatStrings", "addDateDT", "addDateYM", …)`,
   which tells the engine "when you see `+` at parse time, try each of these registered
   functions in turn and dispatch to whichever one's signature matches the operand types."
   Centralised in one place because a single operator spans many types.

So when the parser encounters `d + dur` and the operands type-check to `BlDate` +
`BlDaysTimeDuration`, the engine finds `addDateDT` in the `"+"` binding list, sees its
signature matches, and dispatches to it.

Arithmetic impls return `BlDate` directly because they cannot yield null (day-clamping handles
the only awkward case: `2025-01-31 + P1M → 2025-02-28`). Comparison impls return `BlValue`
because cross-kind comparison (naive vs zoned/offset) yields `BlNull`.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `BlValue` interface, which `BlDate` implements
above. That single dispatch path handles null propagation and cross-type comparison uniformly.

`between` and `in` are patcher-lowered: `between` rewrites to a pair of comparisons; `in`
dispatches to list/range membership or `calendarContainsFn` for a calendar operand.

```go
func addDateYM(d BlDate, dur BlYearsMonthsDuration) BlDate    // "+" d + YM duration (day clamped)
func addDateDT(d BlDate, dur BlDaysTimeDuration)   BlDate     // "+" d + DT duration (whole days; sub-day components ignored)
func subDateYM(d BlDate, dur BlYearsMonthsDuration) BlDate    // "-" d − YM duration (day clamped)
func subDateDT(d BlDate, dur BlDaysTimeDuration)   BlDate     // "-" d − DT duration
func subDates(a, b BlDate) BlDaysTimeDuration                 // "-" d − d
func ltDates(a, b BlDate) BlValue                             // "<"  ; cross-kind → Null
func leDates(a, b BlDate) BlValue                             // "<=" ; cross-kind → Null
func gtDates(a, b BlDate) BlValue                             // ">"  ; cross-kind → Null
func geDates(a, b BlDate) BlValue                             // ">=" ; cross-kind → Null
// "=" and "!=" go through BlValue.Equal(); see BlDate.Equal() above.
```

These are written in clean typed form (`BlDate → BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters at registration time.

### Backing implementations (unexported, suffix `Fn`)

The constructor and today helper are implemented as these typed/variadic Go functions. They
are wrapped by `typed1`/`typed2`/`typed3` when registered with the engine in the next section.
The `dateFn` variadic shape is needed because the constructor accepts multiple input
signatures.

```go
// Datetime-only typed implementations.
func todayFn() BlDate

// Variadic implementation — handles multiple input shapes in expr's raw shape.
func dateFn(args ...any) (any, error)   // date("…") | date(y, m, d) | date(dt) extraction
```

`dateFn` parses ISO 8601 / RFC 9557 strings via Go's [`time.Parse`](https://pkg.go.dev/time#Parse);
an unparseable string → `BlParseError`. Year/month/day component construction validates ranges
(month 1–12, day valid for the month) and rejects invalid combinations with `BlTypeError` (no
silent rollover). IANA zone lookups go through [`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation);
an unknown zone name → `BlTypeError`. Native Go `time.Time` (date portion) inputs wrap to
`BlDate`.

Component accessors (`.year`/`.month`/`.day`/`.offset`/`.timezone` and the calendar properties
`.dayName`/`.dayNameShort`/`.dayOfYear`/`.weekOfYear`/`.isoWeekOfYear`/`.isoYearWeek`/`.monthName`/`.monthNameShort`/
`.quarter`/`.yearQuarter`) are resolved by the component-access patcher described in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go) — they dispatch to
internal accessor functions (`dateYearFn`, …) that are not registered as user-callable
`expr.Function`s.

Operator implementation functions (`addDateYM`, `addDateDT`, `subDateYM`, `subDateDT`,
`subDates`, `ltDates`, `leDates`, `gtDates`, `geDates`) are documented in the previous section.

### Registrations (`dateOptions`, unexported)

`dateOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every date-related operator impl and constructor function. Each
entry is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that
  `operatorBindings()` references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlDate, BlDate) BlDaysTimeDuration` into that
  shape, type-asserting each argument and boxing the result. The variadic implementation
  (`dateFn`) already satisfies the shape and is registered directly.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures.

Date-only registrations are grouped by role: operator impls (consumed by `operatorBindings()`)
and constructor/today. The bulk of date-usable functions (calendar utilities, business-day
arithmetic, date difference, zone stripping, financial year, duration-typed difference) are
**not** registered here — they accept both `BlDate` and `BlDateTime` and are registered once
in [datetime.spec.md](datetime.spec.md) under a single `expr.Function` per name with both
type signatures.

```go
func dateOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addDateYM", typed2(addDateYM), new(func(BlDate, BlYearsMonthsDuration) BlDate)),
        expr.Function("addDateDT", typed2(addDateDT), new(func(BlDate, BlDaysTimeDuration) BlDate)),
        expr.Function("subDateYM", typed2(subDateYM), new(func(BlDate, BlYearsMonthsDuration) BlDate)),
        expr.Function("subDateDT", typed2(subDateDT), new(func(BlDate, BlDaysTimeDuration) BlDate)),
        expr.Function("subDates",  typed2(subDates),  new(func(BlDate, BlDate) BlDaysTimeDuration)),
        expr.Function("ltDates",   typed2(ltDates),   new(func(BlDate, BlDate) BlValue)),
        expr.Function("leDates",   typed2(leDates),   new(func(BlDate, BlDate) BlValue)),
        expr.Function("gtDates",   typed2(gtDates),   new(func(BlDate, BlDate) BlValue)),
        expr.Function("geDates",   typed2(geDates),   new(func(BlDate, BlDate) BlValue)),
        // = and != dispatch via BlValue.Equal() — no per-type registration

        // construction / conversion
        expr.Function("date",  dateFn,
            new(func(BlString) BlDate),                       // date("…")
            new(func(BlNumber, BlNumber, BlNumber) BlDate),   // date(y, m, d)
            new(func(BlDateTime) BlDate)),                    // date(dt) extraction
        expr.Function("today", todayFn, new(func() BlDate)),
    }
}
```

Iterating business-day functions raise `BlCalendarRangeError` past the calendar's validity
bounds only when `strictCalendarRange: true` is supplied (see
[datetime.spec.md § Calendar-range strictness](datetime.spec.md#calendar-range-strictness)).

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
