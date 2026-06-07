---
name: bl.BlDaysTimeDuration
description: The days-and-time duration type in the blkit expression language (ISO 8601 PnDTnHnMnS). Covers construction, component access, the arithmetic/comparison operators, the duration built-ins, and the Go layer (bl.BlDaysTimeDuration + expr registrations).
targets:
  - ../../expr_days_time_duration.go
---

# bl.BlDaysTimeDuration — the days-and-time `duration`

A days-and-time duration covers days, hours, minutes, and seconds (ISO 8601 `PnDTnHnMnS`) — no
years or months. The Go value type backing it is `bl.BlDaysTimeDuration`. It is distinct from
`bl.BlYearsMonthsDuration` ([years_months_duration.spec.md](years_months_duration.spec.md)): the two
**cannot** be added to each other.

See [bl-expr.spec.md](bl-expr.spec.md) for engine internals and component-access syntax.

---

## Construction

There is **no dedicated duration literal** — days-and-time duration values are produced by the
`dtDuration(...)` built-in. For example, the `dtDuration("P1DT2H")` in
`date("2025-03-28") + dtDuration("P1DT2H")` constructs the duration that is then added to the
date. The constructor accepts an ISO 8601 string using only D/T designators:

```
// expression-language
dtDuration("P1DT2H30M")            // 1 day, 2 hours, 30 minutes
dtDuration("PT90M")                // → dtDuration("PT1H30M") (minute overflow normalises on input)
dtDuration("PT3600S")              // → dtDuration("PT1H")
dtDuration("PT1.5S")               // fractional seconds
dtDuration("PT0.123456789S")       // sub-nanosecond precision preserved (no float rounding)
dtDuration("PT1.000000001S")       // 1 second + 1 nanosecond — exact
dtDuration("-PT1H")                // negative
dtDuration("PT0S")                 // zero
dtDuration("P1.5D")                // → dtDuration("P1DT12H") (fractional day reduces to whole hours)
dtDuration("PT1.5H")               // → dtDuration("PT1H30M")
dtDuration("PT0.5H30S")            // → dtDuration("PT30M30S") (fractions accepted on any designator)
dtDuration("p1dt2h30m")            // → dtDuration("P1DT2H30M") (designators are case-insensitive on input)
dtDuration("P1DT2h30m")            // → dtDuration("P1DT2H30M")
dtDuration("P1Y")                  // → bl.ParseError (year/month designators not allowed here)
```

blkit accepts a decimal fraction on any of the `D`/`H`/`M`/`S` designators — this is a deliberate
relaxation of ISO 8601, which permits a fraction only on the smallest unit used. Fractional
components combine as `days*86400 + hours*3600 + minutes*60 + seconds` into the internal total,
stored exactly using arbitrary-precision decimal (no float rounding).

The designator letters `P` / `T` / `D` / `H` / `M` / `S` are **case-insensitive on input**
(`P1DT2H30M`, `p1dt2h30m`, `P1DT2h30M`, etc. all parse identically) — also a deliberate
relaxation of ISO 8601, matching the case-insensitive parsing rule used for the `true` /
`false` and `null` literals, and the parallel ymDuration rule. Canonical output (`bl.String()`)
always emits uppercase designators.

`dtDuration` is paired with `ymDuration`
([years_months_duration.spec.md](years_months_duration.spec.md)) for the sibling years-months
duration. The two are separate functions — the typed return makes downstream usage statically
checkable, where a single polymorphic `duration(string)` would force the call site to inspect
the runtime type — and a Y/M string passed to `dtDuration` (or vice versa) is a `bl.ParseError`.

The companion built-in `dtDurationBetween(from, to)` computes the days-time span between two
dates or datetimes (registered in [datetime.spec.md](datetime.spec.md), which owns the operand
types):

```
// expression-language
dtDurationBetween(date("2025-01-01"), date("2025-03-28"))                   // → dtDuration("P86D")
dtDurationBetween(datetime("2025-01-01T00:00:00"), datetime("2025-01-01T12:30:00"))  // → dtDuration("PT12H30M")
```

When both operands are **zoned** `bl.BlDate`s, each is projected to **midnight in its own zone**
and the result is the UTC-instant gap between those projections — so the same calendar date in
two different zones can yield a non-zero, sub-day duration:

```
// expression-language
// Same calendar date, different offsets — UTC midnights are 10h30m apart:
dtDurationBetween(date("2025-03-28-05:00"), date("2025-03-28+05:30"))
// → dtDuration("-PT10H30M")    (the +05:30 midnight is 10.5h earlier in UTC)

// Crossing the IANA London DST boundary on 2025-03-30 01:00 UTC:
dtDurationBetween(
    date("2025-03-29[Europe/London]"),     // 00:00 GMT = 2025-03-29T00:00:00Z
    date("2025-03-31[Europe/London]"))     // 00:00 BST = 2025-03-30T23:00:00Z
// → dtDuration("P1DT23H")    (not P2D — one hour vanished at the DST transition)

// Naive dates use plain calendar arithmetic — zone is not involved:
dtDurationBetween(date("2025-03-28"), date("2025-03-30"))  // → dtDuration("P2D")
```

A zone-kind mismatch on dates (one naive, one zoned) yields `bl.BlNull`, matching the comparison
rule in [date.spec.md § Operators](date.spec.md#operators).

When both operands are **zoned** `bl.BlDateTime`s, the difference is the **UTC-instant gap** —
identical wall-clock readings in different zones are different instants, and the result
reflects that:

```
// expression-language
// Identical wall-clock noons in two zones — different instants:
dtDurationBetween(
    datetime("2025-03-28T12:00:00+01:00"),       // 11:00 UTC
    datetime("2025-03-28T12:00:00-05:00"))       // 17:00 UTC
// → dtDuration("PT6H")

// Same instant across two zone labels — the difference is zero:
dtDurationBetween(
    datetime("2025-03-28T12:00:00Z"),
    datetime("2025-03-28T13:00:00+01:00"))       // → dtDuration("PT0S")

// IANA zones with DST: spans the spring-forward boundary in London (2025-03-30 01:00 UTC)
dtDurationBetween(
    datetime("2025-03-29T12:00:00[Europe/London]"),  // 12:00 GMT → 12:00 UTC
    datetime("2025-03-31T12:00:00[Europe/London]"))  // 12:00 BST → 11:00 UTC
// → dtDuration("P1DT23H")    (not P2D — one hour was skipped at the DST transition)
```

Two **naive** (timezone-less) `bl.BlDateTime`s use **wall-clock** subtraction instead — no zone
adjustment is performed:

```
// expression-language
dtDurationBetween(
    datetime("2025-03-28T08:00:00"),
    datetime("2025-03-28T12:00:00"))             // → dtDuration("PT4H")
```

A **zone-kind mismatch** between operands (one naive, one zoned/offset) yields `bl.BlNull` —
mirroring the `bl.BlDateTime` comparison policy in
[datetime.spec.md § Comparison semantics](datetime.spec.md#comparison-semantics). Use
`withoutOffset` / `withoutTimezone` ([datetime.spec.md § Zone stripping](datetime.spec.md#zone-stripping-ext))
to drop a zone, or `withOffset` / `withTimezone` ([datetime.spec.md § Zone conversion](datetime.spec.md#zone-conversion-ext))
to attach one before calling.

Both operands must additionally be the **same temporal kind** — either both `bl.BlDate` or both
`bl.BlDateTime`. A mixed `(date, datetime)` call is a type error; convert one operand explicitly
via `datetime(d)` or `date(dt)` first. See
[datetime.spec.md § Business-day arithmetic & difference](datetime.spec.md#business-day-arithmetic--difference-ext)
for the registered overloads.

`[@test] ../../expr_days_time_duration_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlDaysTimeDuration` via the generic
`DTDuration[T DTDurationInput](v T) (bl.BlDaysTimeDuration, error)` constructor.
`DTDurationInput` accepts the natural Go shapes for a days-time value, plus the
wrapped-`Bl*` forms:

- **`string`** / **`bl.BlString`** — two parse paths, dispatched on the first non-sign character
  (case-insensitive): if the string starts with `P` / `-P` / `+P` (or `p` / `-p` / `+p`),
  it's parsed as an **ISO 8601 duration** with D/T designators only (`"P1DT2H"`, `"PT1.5S"`,
  `"-PT90S"`, `"p1dt2h"`, …) — Y/M designators are rejected, since those belong to
  `bl.BlYearsMonthsDuration` (see
  [years_months_duration.spec.md](years_months_duration.spec.md)). All designator letters
  (`P` / `T` / `D` / `H` / `M` / `S`) are case-insensitive on input; canonical output is
  uppercase. Any other string is parsed as a **decimal number of total seconds** (`"1.5"`,
  `"-60"`, `"0.000000001"`, …), exactly as if the caller had wrapped it in
  `decimal.NewFromString(...)` themselves. An unparseable string errors.
- **Integer types** (`int`, `int8`–`int64`, `uint`, `uint8`–`uint64`) — interpreted as the
  total seconds. `bl.DTDuration(60)` is one minute; `bl.DTDuration(86400)` is one day.
- **`float32`** / **`float64`** — total seconds; `NaN` / `Inf` errors. Convenient for
  callers already holding a `float64` (e.g. `goDur.Seconds()`) but subject to float-precision
  limits; prefer `decimal.Decimal` when precision matters.
- **`decimal.Decimal`** / **`bl.BlNumber`** — exact decimal total seconds, the preferred form
  when fractional precision matters (matches the internal storage and preserves
  sub-nanosecond seconds without float rounding). `bl.BlNumber` is just the engine's wrapped
  form of `decimal.Decimal`.
- **`time.Duration`** — a Go `time.Duration` (nanosecond-precision integer); converted to
  exact seconds.
- **`bl.DHMS(days, hours, minutes, seconds)`** — generic constructor function for explicit
  components when the caller doesn't want to compute
  `days*86400 + hours*3600 + minutes*60 + seconds` themselves. Each argument is independently
  constrained by `DHMSNumberInput`, which accepts any Go integer type, `float32` / `float64`
  (`NaN` / `Inf` errors), `decimal.Decimal`, a numeric string (parsed via
  `decimal.NewFromString`), `bl.BlNumber`, or `bl.BlString` (treated as the numeric-string
  path). Different types across the four arguments are fine. Type-checking happens at
  compile time via the constraint; there is **no struct-literal form** because Go's type
  system can't express "this field is one of these unrelated types" without `any`, and `any`
  would defeat the compile-time check. Components are not range-restricted (`bl.DHMS(0, 25, 0, 0)`
  is fine; the constructor normalises into the canonical split), and any argument may be
  fractional.

Native Go `time.Duration` inputs also wrap to `bl.BlDaysTimeDuration` automatically via the
engine input bridge when supplied as an input variable to `bl.BlExpr.Evaluate`.

```go
// host-side (Go)
// ISO 8601 string — convenient when the value comes from config or persistence.
var meeting,  _ = bl.DTDuration("PT1H30M")
var weekend,  _ = bl.DTDuration("P2D")
var lowered,  _ = bl.DTDuration("p1dt2h")            // designators are case-insensitive on input
var bad,    err = bl.DTDuration("P1Y6M")             // err != nil — Y/M designators not allowed

// Integer total seconds.
var oneMin,   _ = bl.DTDuration(60)
var oneDay,   _ = bl.DTDuration(86400)

// Float64 total seconds — convenient but subject to float precision.
var floatS,   _ = bl.DTDuration(1.5)                  // 1.5 seconds

// String of a decimal number — parsed as total seconds. Equivalent to wrapping in
// decimal.NewFromString yourself but reads cleaner.
var fromStr,  _ = bl.DTDuration("1.5")

// Exact fractional total via decimal.Decimal — useful when the value already exists as a
// decimal.
var dec1,     _ = decimal.NewFromString("1.5")
var precise,  _ = bl.DTDuration(dec1)
var dec2,     _ = decimal.NewFromString("0.000000001")
var nano,     _ = bl.DTDuration(dec2)

// From a Go time.Duration.
var goDur       = 2*time.Hour + 30*time.Minute
var fromGoDur, _ = bl.DTDuration(goDur)

// From an engine value — bl.BlNumber and bl.BlString accepted directly.
var dec3,     _ = decimal.NewFromString("60")
var n,        _ = bl.Number(dec3)
var fromBlN,  _ = bl.DTDuration(n)                                            // 60 seconds = 1 minute
var s,        _ = bl.String("PT1H30M")
var fromBlS,  _ = bl.DTDuration(s)                                            // parsed via the ISO 8601 path

// Explicit components — the generic bl.DHMS(...) constructor type-checks each argument at compile
// time. Mixed types across the four arguments are fine; each is independently constrained.
var split,    _ = bl.DTDuration(bl.DHMS(1, 2, 30, 0))                              // 1d 2h 30m
var dec4,     _ = decimal.NewFromString("1.5")
var partial,  _ = bl.DTDuration(bl.DHMS(0, 0, 0, dec4))                            // 1.5s exact
var fromStrs, _ = bl.DTDuration(bl.DHMS("1", "2", "30", "0"))                      // integer-valued numeric strings
var fromDecStrs, _ = bl.DTDuration(bl.DHMS("1.5", "2.5", "30.25", "0.001"))        // decimal-valued numeric strings (each parsed via decimal.NewFromString)
var mixed,    _ = bl.DTDuration(bl.DHMS(1, 2, "30", 0))                            // mixed types
var fromBls,  _ = bl.DTDuration(bl.DHMS(n, n, n, n))                               // bl.BlNumber for each
```

`bl.DTDuration(...)` returns `(bl.BlDaysTimeDuration, error)`. The error path fires for an
unparseable or wrong-kind ISO 8601 `string` / `bl.BlString`, and for a `float32` / `float64`
holding `NaN` / `Inf`. Integer / `decimal.Decimal` / `bl.BlNumber` / `time.Duration` / `bl.DHMS(...)`
inputs are infallible. For details on the underlying `decimal.Decimal` total-seconds storage
and the `Native()` / `Days()` / `Hours()` / `Minutes()` / `Seconds()` / `TotalSeconds()`
accessors, see [§ Value type & host API](#value-type--host-api-exported).

---

## Component access

Field-style access reads the normalised components:

```
// expression-language
dtDuration("P2DT3H45M10S").days          // → 2
dtDuration("P2DT3H45M10S").hours         // → 3
dtDuration("P2DT3H45M10S").minutes       // → 45
dtDuration("P2DT3H45M10S").seconds       // → 10
dtDuration("P2DT3H45M10S").totalSeconds  // → 186310               (ext: signed total)
dtDuration("P2DT3H45M10S").totalMinutes  // → 3105.16666...        (ext: signed; possibly fractional)
dtDuration("P2DT3H45M10S").totalHours    // → 51.75277...          (ext)
dtDuration("P2DT3H45M10S").totalDays     // → 2.156866...          (ext)
dtDuration("-P2DT3H45M10S").days         // → -2                   (sign carried on the day component)
dtDuration("-P2DT3H45M10S").hours        // → -3
dtDuration("-P2DT3H45M10S").totalHours   // → -51.75277...         (sign carries through totals)
dtDuration("PT1.5S").seconds             // → 1.5                  (fractional remainder is preserved)
dtDuration("PT1.5S").totalSeconds        // → 1.5
dtDuration("PT90M").totalHours           // → 1.5                  (totals divide exactly when they can)
```

The four `total*` accessors (**ext**) all return the signed exact decimal total expressed in the
named unit — `totalSeconds`, `totalMinutes = totalSeconds / 60`, `totalHours = totalSeconds /
3600`, `totalDays = totalSeconds / 86400`. They preserve full arbitrary-precision decimal
(no float rounding), so a duration constructed from a fractional input round-trips exactly
through the matching total.

Component access is **patcher-lowered** to function calls (`durationDays(d)`, `durationHours(d)`,
`durationMinutes(d)`, `durationSeconds(d)`, `durationTotalSeconds(d)`, `durationTotalMinutes(d)`,
`durationTotalHours(d)`, `durationTotalDays(d)`); see
[bl-expr.spec.md § Patchers](bl-expr.spec.md#patchers-ast-rewriting).

`[@test] ../../expr_days_time_duration_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add two durations | `dtDuration("P1D") + dtDuration("PT12H")` | `dtDuration("P1DT12H")` |
| `-` | subtract two durations | `dtDuration("P1DT12H") - dtDuration("PT12H")` | `dtDuration("P1D")` |
| unary `-` | negate | `-dtDuration("P2DT3H")` | `dtDuration("-P2DT3H")` |
| `*` | scale by a number | `dtDuration("PT1H") * 2.5` | `dtDuration("PT2H30M")` |
| `/` | divide by a number | `dtDuration("PT1H") / 4` | `dtDuration("PT15M")` |
| `<` `<=` `>` `>=` | compare by total seconds | `dtDuration("PT60S") < dtDuration("PT2M")` | `true` |
| `=` `!=` | equality by total seconds | `dtDuration("PT60S") = dtDuration("PT1M")` | `true` |

`+` and `-` also apply between this duration and a `date`, `time`, or `datetime` (those overloads
live in the date/time/datetime spokes). Mixing with a `bl.BlYearsMonthsDuration` → `bl.TypeError`.
Division by zero → `null`.

`*` and `/` scale the total-seconds count by a `bl.BlNumber` using **exact decimal arithmetic** — no
rounding. `dtDuration("PT1H") / 7` yields a duration whose `totalSeconds` is exactly `3600/7` (a
`bl.BlNumber` with arbitrary precision); the canonical string form puts the resulting fraction on
the smallest designator used (here, seconds).

`[@test] ../../expr_days_time_duration_ops_test.go`

---

## Built-in functions

| Function | Example | Result |
|---|---|---|
| `dtDuration(from)` | `dtDuration("P1DT2H")` | the corresponding `bl.BlDaysTimeDuration` (D/T-only strings; Y/M strings → `bl.ParseError`) |
| `dtDurationBetween(from, to)` | `dtDurationBetween(date("2025-01-01"), date("2025-03-28"))` | `dtDuration("P86D")` |
| `abs(d)` | `abs(dtDuration("-PT5H"))` | `dtDuration("PT5H")` |
| `isNegative(d)` **ext** | `isNegative(dtDuration("-PT1H"))` | `true` (zero → `false`) |

`abs` and `isNegative` are overloaded across the two duration types (the registrations in this
spoke add the `bl.BlDaysTimeDuration` signatures; the `bl.BlYearsMonthsDuration` ones live in
[years_months_duration.spec.md](years_months_duration.spec.md)). `dtDurationBetween` is
registered in [datetime.spec.md](datetime.spec.md) (which owns the `bl.BlDate` / `bl.BlDateTime`
operand types).

### Rounding

The six numeric rounding modes from
[number.spec.md § Built-in functions](number.spec.md#built-in-functions) are overloaded on
`bl.BlDaysTimeDuration`: the second argument is a positive duration `step` (rather than a decimal
`scale`), and the result is rounded to the nearest integer multiple of `step`. This lets host
code round to common business granularities — nearest second, nearest minute, nearest 15
minutes, nearest hour, nearest day — without converting through total-seconds arithmetic.

| Function | Example | Result |
|---|---|---|
| `round(d, step)` **ext** | `round(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (alias of `roundHalfUp`) |
| `roundUp(d, step)` **ext** | `roundUp(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT45M")` (away from zero) |
| `roundDown(d, step)` **ext** | `roundDown(dtDuration("PT37M"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (toward zero / truncation) |
| `roundHalfUp(d, step)` **ext** | `roundHalfUp(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (halfway away from zero) |
| `roundHalfDown(d, step)` **ext** | `roundHalfDown(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT15M")` (halfway toward zero) |
| `roundHalfEven(d, step)` **ext** | `roundHalfEven(dtDuration("PT22M30S"), dtDuration("PT15M"))` | `dtDuration("PT30M")` (ties to even multiple — banker's rounding) |

Each impl computes `q = totalSeconds(d) / totalSeconds(step)`, rounds `q` per the chosen mode,
and returns `q * step`. The duration being rounded can be negative; rounding direction respects
sign (e.g. `roundUp(dtDuration("-PT37M"), dtDuration("PT15M"))` → `dtDuration("-PT45M")` —
"away from zero" makes a negative input more negative). Common uses:

```
// expression-language
round(dtDuration("PT1H37M"), dtDuration("PT1H"))     // → dtDuration("PT2H") (nearest hour)
round(dtDuration("PT1H37M"), dtDuration("PT15M"))    // → dtDuration("PT1H30M") (nearest 15 min)
round(dtDuration("PT1H37M"), dtDuration("PT1M"))     // → dtDuration("PT1H37M") (nearest minute)
roundUp(dtDuration("PT0.5S"), dtDuration("PT1S"))    // → dtDuration("PT1S") (next whole second)
roundDown(dtDuration("P1DT23H"), dtDuration("P1D"))  // → dtDuration("P1D") (truncate to whole days)
```

A non-positive `step` (zero or negative) → `bl.TypeError`; rounding to a "nearest zero-sized
multiple" or "nearest negative multiple" has no sensible meaning.

`[@test] ../../expr_days_time_duration_functions_test.go`

---

## Semantics & behaviour

- Components normalise: seconds carry into minutes, minutes into hours, hours into days
  (`dtDuration("PT90M")` → `PT1H30M`). Fractional remainders are preserved on whichever
  component carries them after normalisation (typically seconds — `dtDuration("PT1.5S").seconds`
  → `1.5`).
- Storage is **exact arbitrary-precision decimal** total seconds; no float rounding occurs at
  parse or arithmetic time. Comparison, equality, and `totalSeconds` are all by total signed
  seconds, so `dtDuration("PT60S") = dtDuration("PT1M")` and `dtDuration("PT1.5H") =
  dtDuration("PT1H30M")`.
- `*` and `/` produce exact decimal results; `/0` yields `null`.
- Fractional designators are accepted on **any** of `D`/`H`/`M`/`S` (a deliberate relaxation of
  ISO 8601, which restricts the fraction to the smallest unit used). Components combine into the
  internal total as `days*86400 + hours*3600 + minutes*60 + seconds`.
- Canonical output (`bl.String()`) puts any fractional remainder on the smallest designator used:
  `3600/7` seconds as `bl.String()` becomes `"PT514.2857...S"` (the underlying decimal preserves
  all digits; the formatter emits the minimal exact representation).
- Sign applies to the whole value: a negative `bl.BlDaysTimeDuration` has negative `.days`,
  `.hours`, `.minutes`, and `.seconds` components (not "negative days, positive hours").
- Zero duration (`dtDuration("PT0S")`) is not negative.
- `dtDurationBetween(from, to)` is signed — `from > to` yields a negative result.

---

## Go implementation (expr extension)

Lives in `expr/days_time_duration.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlDaysTimeDuration` is the immutable Go value type that represents a days-and-time duration
inside the engine and at the host-code boundary. It wraps a single signed arbitrary-precision
decimal — the **total seconds** — and derives the day/hour/minute/second components by
normalisation. The field is private so callers cannot mutate the underlying value; every
operation in the library returns a fresh `bl.BlDaysTimeDuration`. The decimal representation mirrors
`bl.BlYearsMonthsDuration`'s exact-months storage and preserves ISO 8601 fractional durations
without float rounding. Go's `time.Duration` is a fixed nanosecond count that conveniently fits
this domain (modulo precision), so the host-code surface includes a `time.Duration` bridge.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares by total seconds, so `dtDuration("PT60S")` equals
  `dtDuration("PT1M")` and `dtDuration("PT1.5H")` equals `dtDuration("PT1H30M")`. `bl.String()`
  doubles as the `fmt.Stringer` implementation, producing the canonical ISO 8601 form: integer
  split `"P1DT2H30M"` when the total has no sub-second fraction, fractional smallest-unit form
  (`"PT1.5S"`, `"PT30.5M"`) otherwise, `"-PT90S"` for negatives, and `"PT0S"` for zero.
- **`DTDuration[T DTDurationInput](v T) (bl.BlDaysTimeDuration, error)`** — the generic host
  constructor. The `DTDurationInput` constraint accepts a `string` / `bl.BlString`
  (dispatched on the first non-sign character: leading `P` → ISO 8601 with D/T designators
  only; otherwise → decimal number of total seconds), every Go integer type (total seconds),
  `float32` / `float64` (total seconds; `NaN` / `Inf` error), `decimal.Decimal` / `bl.BlNumber`
  (exact total seconds, possibly fractional — preferred when precision matters),
  `time.Duration`, and `bl.DHMS(days, hours, minutes, seconds)` (a generic constructor function
  returning explicit components; each argument is independently constrained by
  `DHMSNumberInput` and type-checked at compile time). See [§ Construction
  (host-side)](#construction-host-side) for the worked example. The `error` return fires
  only for an unparseable / wrong-kind ISO 8601 `string` / `bl.BlString` or a `NaN` / `Inf`
  float; the other input shapes are infallible.
- **`Days()` / `Hours()` / `Minutes()` / `Seconds()` / `TotalSeconds()` accessors** — hand the
  normalised components and the signed total back to host code. `Days()` returns the integer
  days portion (truncated toward zero) with sign; `Hours()` returns the hours remainder
  (`-23..23`, same sign as the value); `Minutes()` returns the minutes remainder (`-59..59`);
  `Seconds()` returns the seconds remainder as a `decimal.Decimal` (`-60 < s < 60`, possibly
  fractional, same sign as the value); `TotalSeconds()` returns the signed exact decimal total
  used for all arithmetic and comparison. A `NativeDuration()` helper hands back a
  `time.Duration`, with the documented caveat that durations exceeding `time.Duration`'s ±290
  year range return `time.Duration`'s saturated bounds (and host code can check via
  `TotalSeconds()` before calling).

```go
// host-side (Go)
// bl.BlDaysTimeDuration wraps a signed arbitrary-precision decimal count of total seconds.
type BlDaysTimeDuration struct{ secs decimal.Decimal }

// bl.BlValue interface — required by all Bl* value types.
func (BlDaysTimeDuration) Type() Type { return TypeDaysTimeDuration }
func (d BlDaysTimeDuration) Equal(other BlValue) BlValue   // exact decimal compare on total seconds
func (d BlDaysTimeDuration) String() string                // "P1DT2H30M" / "PT1.5S" / "-PT90S" / "PT0S"
func (BlDaysTimeDuration) isBlValue() {}

// Host constructor — accepts:
//   - string / bl.BlString:    "P..." parses as ISO 8601 (D/T only; designators case-insensitive
//                            on input — "p1dt2h" works too); anything else parses as a
//                            decimal-number total-seconds string ("1.5", "-60", …).
//   - any Go integer:        total seconds.
//   - float32 / float64:     total seconds; NaN / Inf error.
//   - decimal.Decimal:       exact total seconds (preferred for fractional precision).
//   - bl.BlNumber:               engine-wrapped decimal; treated identically to decimal.Decimal.
//   - time.Duration:         a Go time.Duration; converted to exact seconds.
//   - dhmsComponents (built via DHMS(d, h, m, s)):
//                             explicit (days, hours, minutes, seconds); each argument is
//                             independently type-checked against DHMSNumberInput at compile
//                             time. Arguments may carry different types.

// DHMSNumberInput is the per-argument constraint for the DHMS components constructor: any Go
// numeric type, a numeric string, decimal.Decimal, bl.BlNumber, or bl.BlString.
type DHMSNumberInput interface {
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    float32 | float64 |
    decimal.Decimal |
    string |
    BlNumber |
    BlString
}

// dhmsComponents is the opaque value produced by bl.DHMS(...). It can only be constructed via the
// typed DHMS function, which guarantees the field values come from the accepted set.
type dhmsComponents struct {
    days, hours, minutes, seconds any   // typed at construction via DHMSNumberInput
}

// Generic typed constructor — type parameters inferred from the argument types.
func DHMS[D, H, M, S DHMSNumberInput](days D, hours H, minutes M, seconds S) dhmsComponents

type DTDurationInput interface {
    string | BlString |
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    float32 | float64 |
    decimal.Decimal | BlNumber |
    time.Duration |
    dhmsComponents
}
func DTDuration[T DTDurationInput](v T) (BlDaysTimeDuration, error)

// Host accessors (consume an evaluated result).
func (d BlDaysTimeDuration) Days() int                       // integer days; truncated toward zero; sign carries
func (d BlDaysTimeDuration) Hours() int                      // |h| < 24; same sign as the value
func (d BlDaysTimeDuration) Minutes() int                    // |m| < 60; same sign as the value
func (d BlDaysTimeDuration) Seconds() decimal.Decimal        // |s| < 60; same sign; may be fractional
func (d BlDaysTimeDuration) TotalSeconds() decimal.Decimal   // signed exact total used for arithmetic
func (d BlDaysTimeDuration) NativeDuration() time.Duration   // saturates to time.Duration's ±290y bounds on overflow
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `bl.BlDaysTimeDuration` and cannot apply Go's native `+`/`-`/
`<`/etc. to blkit values. For every operator that should work on days-time durations, blkit
supplies a named Go function that performs the operation on the underlying total-seconds counts
and returns the result wrapped as a `bl.BlValue`. The connection from operator token to function
happens in two steps, neither of which is unique to this type:

1. The Registrations section below calls `expr.Function("addDTDuration", typed2(addDTDuration), …)`,
   which makes the engine aware of the function under that exact string name and records its
   type signature.
2. A central `operatorBindings()` in
   [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then calls
   `expr.Operator("+", "addNumbers", "addDTDuration", …)`, which tells the engine "when you see
   `+` at parse time, try each of these registered functions in turn and dispatch to whichever
   one's signature matches the operand types." This step is centralised in one place because a
   single operator spans many types — `+` covers number addition, string concatenation, both
   duration kinds, and several temporal forms — and `expr.Operator` needs the full list of
   candidates per operator in a single call.

So when the parser encounters `a + b` and both operands type-check to `bl.BlDaysTimeDuration`, the
engine finds `addDTDuration` in the `"+"` binding list, sees its signature matches, and
dispatches to it.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `bl.BlValue` interface, which `bl.BlDaysTimeDuration`
implements above (compare by total seconds). That single dispatch path handles null propagation
and cross-type comparison uniformly.

```go
// host-side (Go)
func addDTDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration              // "+"
func subDTDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration              // "-"
func negDTDuration(d BlDaysTimeDuration) BlDaysTimeDuration                 // unary "-"
func scaleDTDuration(d BlDaysTimeDuration, n BlNumber) BlDaysTimeDuration   // "*" — exact decimal
func divDTDuration(d BlDaysTimeDuration, n BlNumber) BlValue                // "/" — n == 0 → Null
func ltDTDuration(a, b BlDaysTimeDuration) BlValue                          // "<" by total seconds
func leDTDuration(a, b BlDaysTimeDuration) BlValue                          // "<="
func gtDTDuration(a, b BlDaysTimeDuration) BlValue                          // ">"
func geDTDuration(a, b BlDaysTimeDuration) BlValue                          // ">="
// "=" and "!=" go through bl.BlValue.Equal(); see bl.BlDaysTimeDuration.Equal() above.
```

These are written in clean typed form for readability and unit testing. The engine cannot
consume them at this shape directly — they're wrapped by the `typed1` / `typed2` adapters at
registration time.

### Backing implementations (unexported, suffix `Fn`)

The library and component-accessor functions are implemented as these typed Go functions. They
are wrapped by `typed1` / `typed2` when registered with the engine in the next section.

```go
// host-side (Go)
// Constructor.
func dtDurationFn(s BlString) (BlDaysTimeDuration, error)   // D/T-only parser; Y/M designators → ParseError

// Component accessors — emitted by the component-access patcher.
func durationDaysDTFn(d BlDaysTimeDuration) BlNumber          // overload; Y/M overload in years_months_duration.spec.md
func durationHoursDTFn(d BlDaysTimeDuration) BlNumber
func durationMinutesDTFn(d BlDaysTimeDuration) BlNumber
func durationSecondsDTFn(d BlDaysTimeDuration) BlNumber       // may be fractional
func durationTotalSecondsFn(d BlDaysTimeDuration) BlNumber    // ext; signed
func durationTotalMinutesFn(d BlDaysTimeDuration) BlNumber    // ext; totalSeconds / 60
func durationTotalHoursFn(d BlDaysTimeDuration) BlNumber      // ext; totalSeconds / 3600
func durationTotalDaysFn(d BlDaysTimeDuration) BlNumber       // ext; totalSeconds / 86400

// Library functions.
func absDTFn(d BlDaysTimeDuration) BlDaysTimeDuration          // overload; numeric/YM overloads elsewhere
func isNegativeDTFn(d BlDaysTimeDuration) BlBoolean            // ext; YM overload in years_months_duration.spec.md

// Rounding family — overloads of the numeric rounding modes from number.spec.md.
// Each rounds totalSeconds(d) / totalSeconds(step) per the chosen mode, then multiplies back.
// A non-positive step returns a bl.TypeError.
func roundDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration            // ext; alias of roundHalfUpDT
func roundUpDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration          // ext
func roundDownDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration        // ext
func roundHalfUpDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration      // ext
func roundHalfDownDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration    // ext
func roundHalfEvenDTFn(d, step BlDaysTimeDuration) BlDaysTimeDuration    // ext
```

The six rounding impls share a single helper that computes the rounded quotient as a
`decimal.Decimal` using the matching mode, then multiplies by `step.TotalSeconds()`. They reuse
the same per-mode decimal-rounding logic as the numeric `round*Fn` impls in `expr/number.go` so
that ties and signs behave identically across types.

### Registrations (`daysTimeDurationOptions`, unexported)

`daysTimeDurationOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every days-time-duration-related operator impl and library
function. Each entry is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (or that
  `operatorBindings()` references for operator dispatch, or that the component-access patcher
  emits for `.days` / `.hours` / `.minutes` / `.seconds` / `.totalSeconds`).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2`
  adapters (defined in
  [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)) wrap a typed
  implementation such as `func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration` into
  that shape.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across signatures (e.g. `abs`
  accepts both `bl.BlDaysTimeDuration` and `bl.BlYearsMonthsDuration`).

The registrations are grouped by role: operator impls (consumed by `operatorBindings()`),
component-accessor impls (emitted by the patcher), the constructor, and the library functions.

```go
// host-side (Go)
func daysTimeDurationOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addDTDuration",   typed2(addDTDuration),   new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),
        expr.Function("subDTDuration",   typed2(subDTDuration),   new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),
        expr.Function("negDTDuration",   typed1(negDTDuration),   new(func(bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),
        expr.Function("scaleDTDuration", typed2(scaleDTDuration), new(func(bl.BlDaysTimeDuration, bl.BlNumber) bl.BlDaysTimeDuration)),
        expr.Function("divDTDuration",   typed2(divDTDuration),   new(func(bl.BlDaysTimeDuration, bl.BlNumber) bl.BlValue)),
        expr.Function("ltDTDuration",    typed2(ltDTDuration),    new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlValue)),
        expr.Function("leDTDuration",    typed2(leDTDuration),    new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlValue)),
        expr.Function("gtDTDuration",    typed2(gtDTDuration),    new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlValue)),
        expr.Function("geDTDuration",    typed2(geDTDuration),    new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlValue)),
        // = and != dispatch via bl.BlValue.Equal() — no per-type registration

        // component-access impls — emitted by the patcher when lowering .days / .hours / .minutes / .seconds /
        //                          .totalSeconds / .totalMinutes / .totalHours / .totalDays
        expr.Function("durationDays",         typed1(durationDaysDTFn),       new(func(bl.BlDaysTimeDuration) bl.BlNumber)),
        expr.Function("durationHours",        typed1(durationHoursDTFn),      new(func(bl.BlDaysTimeDuration) bl.BlNumber)),
        expr.Function("durationMinutes",      typed1(durationMinutesDTFn),    new(func(bl.BlDaysTimeDuration) bl.BlNumber)),
        expr.Function("durationSeconds",      typed1(durationSecondsDTFn),    new(func(bl.BlDaysTimeDuration) bl.BlNumber)),
        expr.Function("durationTotalSeconds", typed1(durationTotalSecondsFn), new(func(bl.BlDaysTimeDuration) bl.BlNumber)),
        expr.Function("durationTotalMinutes", typed1(durationTotalMinutesFn), new(func(bl.BlDaysTimeDuration) bl.BlNumber)),  // ext
        expr.Function("durationTotalHours",   typed1(durationTotalHoursFn),   new(func(bl.BlDaysTimeDuration) bl.BlNumber)),  // ext
        expr.Function("durationTotalDays",    typed1(durationTotalDaysFn),    new(func(bl.BlDaysTimeDuration) bl.BlNumber)),  // ext

        // constructor — D/T-only parser; sibling ymDuration lives in years_months_duration.spec.md
        expr.Function("dtDuration", typed1(dtDurationFn), new(func(bl.BlString) bl.BlDaysTimeDuration)),

        // library — overloads share a name with the years-months spoke's registrations
        expr.Function("abs",        typed1(absDTFn),        new(func(bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // overload
        expr.Function("isNegative", typed1(isNegativeDTFn), new(func(bl.BlDaysTimeDuration) bl.BlBoolean)),            // ext; overload

        // rounding — overloads of the numeric rounding modes from number.spec.md
        expr.Function("round",         typed2(roundDTFn),         new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext; alias of roundHalfUp
        expr.Function("roundUp",       typed2(roundUpDTFn),       new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext
        expr.Function("roundDown",     typed2(roundDownDTFn),     new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext
        expr.Function("roundHalfUp",   typed2(roundHalfUpDTFn),   new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext
        expr.Function("roundHalfDown", typed2(roundHalfDownDTFn), new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext
        expr.Function("roundHalfEven", typed2(roundHalfEvenDTFn), new(func(bl.BlDaysTimeDuration, bl.BlDaysTimeDuration) bl.BlDaysTimeDuration)),  // ext
    }
}
```

The date / time / datetime `+` / `-` overloads that consume a days-time duration live in the
[date](date.spec.md), [time](time.spec.md), and [datetime](datetime.spec.md) spokes (those
spokes own one operand of the pair). Native Go `time.Duration` inputs wrap to
`bl.BlDaysTimeDuration` via the engine's input bridge.

`[@test] ../../expr_days_time_duration_test.go`

---

## Edge cases

- `dtDuration("P1Y…")` or `dtDuration("P1M")` (any year/month designator) → `bl.ParseError` for
  this type.
- Fractional designators are accepted on any of D/H/M/S — a deliberate relaxation of ISO 8601's
  smallest-unit-only rule.
- Division by a zero factor (`d / 0`) → `null`.
- `*` and `/` produce exact decimal results — no rounding. The fractional remainder is preserved
  on the smallest component and emitted on the smallest unit by `bl.String()`.
- `round*` family: `step` must be a positive duration. Zero or negative `step` → `bl.TypeError`.
  Sign of the input is preserved (rounding direction respects it — `roundUp` on a negative
  input rounds away from zero, making it more negative).
- Zero duration: `isNegative` → `false`, `abs` → unchanged, all components are `0`.
- Adding a `bl.BlYearsMonthsDuration` → `bl.TypeError`.
- `dtDurationBetween` is signed: `from > to` yields a negative result rather than swapping the
  operands.
- Sign is held on the whole value, not per component — a negative duration has negative `.days`,
  `.hours`, `.minutes`, and `.seconds`.
- `NativeDuration()` host accessor saturates at `time.Duration`'s ±290-year bounds; host code
  with potentially larger durations should consume `TotalSeconds()` instead.
