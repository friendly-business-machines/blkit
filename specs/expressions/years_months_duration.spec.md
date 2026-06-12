---
name: bl.BlYearsMonthsDuration
description: The years-and-months duration type in the blkit expression language (ISO 8601 PnYnM). Covers construction, component access, the arithmetic/comparison operators, the duration built-ins, and the Go layer (bl.BlYearsMonthsDuration + expr registrations).
targets:
  - ../../expr_years_months_duration.go
---

# bl.BlYearsMonthsDuration — the years-and-months `duration`

A years-and-months duration covers only years and months (ISO 8601 `PnYnM`). The Go value type
backing it is `bl.BlYearsMonthsDuration`. It is distinct from `bl.BlDaysTimeDuration`
([days_time_duration.spec.md](days_time_duration.spec.md)): the two **cannot** be added to each
other, and a years-months duration **cannot** be applied to a `time` (only to a `date` or
`datetime`).

See [bl-expr.spec.md](bl-expr.spec.md) for engine internals and component-access syntax.

---

## Construction

There is **no dedicated duration literal** — years-and-months duration values are produced by
the `ymDuration(...)` built-in. For example, the `ymDuration("P1Y6M")` in
`date("2025-03-28") + ymDuration("P1Y6M")` constructs the duration that is then added to the
date. The constructor accepts an ISO 8601 string using only Y/M designators:

```
// expression-language
ymDuration("P1Y6M")                  // 1 year, 6 months
ymDuration("P1Y")                    // 1 year
ymDuration("P6M")                    // 6 months
ymDuration("-P1Y6M")                 // negative
ymDuration("P0Y0M")                  // zero
ymDuration("P13M")                   // → ymDuration("P1Y1M") (month overflow normalises on input)
ymDuration("P1Y18M")                 // → ymDuration("P2Y6M")
ymDuration("P1.5Y")                  // → ymDuration("P1Y6M") (fractional year reduces to whole months)
ymDuration("P0.5M")                  // 0.5 months — fractional months are representable
ymDuration("P1Y0.25M")               // 1 year, 0.25 months
ymDuration("P1.5Y0.5M")              // → ymDuration("P1Y6.5M") (fractions accepted on any designator)
ymDuration("p1y6m")                  // → ymDuration("P1Y6M")   (designators are case-insensitive on input)
ymDuration("P1Y6m")                  // → ymDuration("P1Y6M")
ymDuration("P1DT2H")                 // → bl.ParseError (day/time designators not allowed here)
```

blkit accepts a decimal fraction on either designator — this is a deliberate relaxation of ISO
8601, which permits a fraction only on the smallest unit used. Fractional components combine as
`years*12 + months` into the internal total, stored exactly using arbitrary-precision decimal
(no float rounding).

The designator letters `P` / `Y` / `M` are **case-insensitive on input** (`P1Y6M`, `p1y6m`,
`P1Y6m`, etc. all parse identically) — also a deliberate relaxation of ISO 8601, matching the
case-insensitive parsing rule used for the `true` / `false` and `null` literals. Canonical
output (`bl.String()`) always emits uppercase designators.

`ymDuration` is paired with `dtDuration` ([days_time_duration.spec.md](days_time_duration.spec.md))
for the sibling days-time duration. The two are separate functions — the typed return makes
downstream usage statically checkable, where a single polymorphic `duration(string)` would force
the call site to inspect the runtime type — and a D/T string passed to `ymDuration` (or vice
versa) is a `bl.ParseError`.

The companion built-in `ymDurationBetween(from, to)` computes the years-months span between
two dates or datetimes (registered in [datetime.spec.md](datetime.spec.md), which owns the
operand types):

```
// expression-language
ymDurationBetween(date("2011-12-22"), date("2013-08-24"))   // → ymDuration("P1Y8M")
ymDurationBetween(date("2025-06-01"), date("2024-06-01"))   // → ymDuration("-P1Y")  (signed)
```

Both operands must be the **same** temporal kind — either both `bl.BlDate` or both `bl.BlDateTime`. A
mixed `(date, datetime)` call is a type error; convert one operand explicitly via `datetime(d)`
or `date(dt)` first. See [datetime.spec.md § Business-day arithmetic & difference](datetime.spec.md#business-day-arithmetic--difference-ext)
for the registered overloads.

`[@test] ../../expr_years_months_duration_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlYearsMonthsDuration` via the generic
`YMDuration[T YMDurationInput](v T) (bl.BlYearsMonthsDuration, error)` constructor.
`YMDurationInput` accepts the natural Go shapes for a years-months value, plus the
wrapped-`Bl*` forms:

- **`string`** / **`bl.BlString`** — two parse paths, dispatched on the first non-sign character
  (case-insensitive): if the string starts with `P` / `-P` / `+P` (or `p` / `-p` / `+p`),
  it's parsed as an **ISO 8601 duration** with Y/M designators only (`"P1Y6M"`, `"-P6M"`,
  `"P0.5M"`, `"p1y6m"`, …) — D/T designators are rejected, since those belong to
  `bl.BlDaysTimeDuration` (see [days_time_duration.spec.md](days_time_duration.spec.md)). All
  designator letters (`P` / `Y` / `M`) are case-insensitive on input; canonical output is
  uppercase. Any other string is parsed as a **decimal number of total months** (`"12.25"`,
  `"-6"`, `"100.5"`, …), exactly as if the caller had wrapped it in
  `decimal.NewFromString(...)` themselves. An unparseable string (matches neither shape)
  errors.
- **Integer types** (`int`, `int8`–`int64`, `uint`, `uint8`–`uint64`) — interpreted as the
  total months. `bl.YMDuration(6)` is six months; `bl.YMDuration(30 * 12)` is thirty years.
- **`float32`** / **`float64`** — total months. Subject to float-precision limits; prefer
  `decimal.Decimal` when precision matters. A `float` holding `NaN` or `Inf` errors.
- **`decimal.Decimal`** / **`bl.BlNumber`** — exact decimal total months, the preferred form when
  fractional precision matters (matches the internal storage and preserves fractional months
  without float rounding). `bl.BlNumber` is just the engine's wrapped form of `decimal.Decimal`.
- **`bl.YM(years, months)`** — generic constructor function for explicit
  `(years, months)` components when the caller doesn't want to compute `years*12 + months`
  themselves. Each argument is independently constrained by `YMNumberInput`, which accepts
  any Go integer type, `float32` / `float64` (`NaN` / `Inf` errors), `decimal.Decimal`, a
  numeric string (parsed via `decimal.NewFromString`), `bl.BlNumber`, or `bl.BlString` (treated
  as the numeric-string path). Different types across the two arguments are fine —
  `bl.YM(1, "6.5")` is a typed call where `years` is `int` and `months` is `string`.
  Type-checking happens at compile time via the constraint; there is **no struct-literal
  form** because Go's type system can't express "this field is one of these unrelated types"
  without `any`, and `any` would defeat the compile-time check. Components are not range-
  restricted (`bl.YM(0, 15)` is fine; the constructor normalises into the canonical
  `(Y, M)` form), and either argument may be fractional.

```go
// host-side (Go)
// ISO 8601 string — convenient when the value comes from config or persistence.
var mortgage, _  = bl.YMDuration("P30Y")
var grace,    _  = bl.YMDuration("P1Y6M")
var lowered,  _  = bl.YMDuration("p1y6m")            // designators are case-insensitive on input
var bad,    err  = bl.YMDuration("P1DT2H")           // err != nil — D/T designators not allowed

// Integer total months.
var halfYear, _  = bl.YMDuration(6)                   // 6 months
var century,  _  = bl.YMDuration(100 * 12)            // 100 years

// Float64 total months — convenient but subject to float precision.
var floatY,   _  = bl.YMDuration(12.25)               // 1y 0.25m

// String of a decimal number — parsed as total months. Equivalent to wrapping in
// decimal.NewFromString yourself but reads cleaner.
var fromStr,    _ = bl.YMDuration("12.25")            // → 1y 0.25m

// Exact fractional total via decimal.Decimal — equivalent shape, useful when the value
// already exists as a decimal.
var dec1,       _ = decimal.NewFromString("12.25")
var fractional, _ = bl.YMDuration(dec1)

// From an engine value — bl.BlNumber and bl.BlString are accepted directly.
var dec2,  _     = decimal.NewFromString("18.5")
var n,     _     = bl.Number(dec2)
var fromBlN, _   = bl.YMDuration(n)                                          // 18.5 total months

var s,   _       = bl.String("P1Y6M")
var fromBlS, _   = bl.YMDuration(s)                                          // parsed via the ISO 8601 path

// Integer components — the simplest case. YM is a generic function; type params
// are inferred from the arguments, so the call site reads cleanly.
var split,    _  = bl.YMDuration(bl.YM(1, 6))

// Fractional months — pass a decimal.Decimal for the argument that needs it.
var dec3,    _   = decimal.NewFromString("3.5")
var partial, _   = bl.YMDuration(bl.YM(2, dec3))                            // 2y 3.5m → 27.5 total months

// String components — numeric strings are parsed via decimal.NewFromString.
var strYrs,   _  = bl.YMDuration(bl.YM("1", "6.5"))

// Mixed types across the two arguments — each is independently typed via the constraint.
var mixed,    _  = bl.YMDuration(bl.YM(2, "3.5"))

// Components from bl.BlNumber / bl.BlString values — accepted directly, no .Decimal() unwrap.
var yearsN,  _    = bl.Number(1)
var monthsN, _    = bl.Number(6)
var fromBl, _    = bl.YMDuration(bl.YM(yearsN, monthsN))

var yearsS, _    = bl.String("1")
var monthsS, _   = bl.String("6.5")
var fromBlS2, _  = bl.YMDuration(bl.YM(yearsS, monthsS))
```

`bl.YMDuration(...)` returns `(bl.BlYearsMonthsDuration, error)`. The error path fires for an
unparseable or wrong-kind ISO 8601 `string` / `bl.BlString`, and for a `float32` / `float64`
holding `NaN` / `Inf`. Integer / `decimal.Decimal` / `bl.BlNumber` / `YM`
inputs are infallible. For details on the underlying `decimal.Decimal` total-months storage
and the `Years()` / `Months()` / `TotalMonths()` / `TotalYears()` accessors, see [§ Value
type & host API](#value-type--host-api-exported).

---

## Component access

Field-style access reads the normalised components:

```
// expression-language
ymDuration("P2Y7M").years            // → 2
ymDuration("P2Y7M").months           // → 7
ymDuration("P2Y7M").totalMonths      // → 31         (ext: signed years*12 + months)
ymDuration("P2Y7M").totalYears       // → 2.58333... (ext: signed; possibly fractional)
ymDuration("-P2Y7M").years           // → -2         (sign carried on the year component)
ymDuration("-P2Y7M").months          // → -7
ymDuration("-P2Y7M").totalYears      // → -2.58333... (sign carries through totals)
ymDuration("P0Y15M").years           // → 1          (normalised — see § Semantics)
ymDuration("P0Y15M").months          // → 3
ymDuration("P1Y0.25M").years         // → 1
ymDuration("P1Y0.25M").months        // → 0.25       (fractional remainder is preserved)
ymDuration("P1Y0.25M").totalMonths   // → 12.25
ymDuration("P24M").totalYears        // → 2          (totals divide exactly when they can)
```

The two `total*` accessors (**ext**) both return the signed exact decimal total expressed in
the named unit — `totalMonths`, and `totalYears = totalMonths / 12`. They preserve full
arbitrary-precision decimal (no float rounding), so a duration constructed from a fractional
input round-trips exactly through the matching total.

Component access is **patcher-lowered** to function calls (`durationYears(d)`,
`durationMonths(d)`, `durationTotalMonths(d)`, `durationTotalYears(d)`); see
[bl-expr.spec.md § Patchers](bl-expr.spec.md#patchers-expr_patchgo).

`[@test] ../../expr_years_months_duration_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add two durations | `ymDuration("P1Y") + ymDuration("P6M")` | `ymDuration("P1Y6M")` |
| `-` | subtract two durations | `ymDuration("P1Y6M") - ymDuration("P6M")` | `ymDuration("P1Y")` |
| unary `-` | negate | `-ymDuration("P1Y6M")` | `ymDuration("-P1Y6M")` |
| `*` | scale by a number | `ymDuration("P6M") * 3` | `ymDuration("P1Y6M")` |
| `/` | divide by a number | `ymDuration("P1Y") / 4` | `ymDuration("P3M")` |
| `<` `<=` `>` `>=` | compare by total months | `ymDuration("P1Y") > ymDuration("P11M")` | `true` |
| `=` `!=` | equality by total months | `ymDuration("P1Y") = ymDuration("P12M")` | `true` |

`+` and `-` also apply between this duration and a `date` or `datetime` (those overloads live in
the date/datetime spokes). Mixing with a `bl.BlDaysTimeDuration` → `bl.TypeError`; applying to a
`time` → `bl.TypeError`. Division by zero → `null`.

`*` and `/` scale the total-months count by a `bl.BlNumber` using **exact decimal arithmetic** — no
rounding. `ymDuration("P1Y") / 7` yields a duration whose `totalMonths` is exactly `12/7` (a
`bl.BlNumber` with arbitrary precision); the canonical string form puts the resulting fraction on
the smallest designator used (here, months).

`[@test] ../../expr_years_months_duration_ops_test.go`

---

## Built-in functions

| Function | Example | Result |
|---|---|---|
| `ymDuration(from)` | `ymDuration("P1Y6M")` | the corresponding `bl.BlYearsMonthsDuration` (Y/M-only strings; D/T strings → `bl.ParseError`) |
| `ymDurationBetween(from, to)` | `ymDurationBetween(date("2024-01-01"), date("2025-06-01"))` | `ymDuration("P1Y5M")` |
| `abs(d)` | `abs(ymDuration("-P2Y3M"))` | `ymDuration("P2Y3M")` |
| `isNegative(d)` **ext** | `isNegative(ymDuration("-P3M"))` | `true` (zero → `false`) |

`abs` and `isNegative` are overloaded across the two duration types (the registrations in this
spoke add the `bl.BlYearsMonthsDuration` signatures; the `bl.BlDaysTimeDuration` ones live in
[days_time_duration.spec.md](days_time_duration.spec.md)).

### Rounding

The six numeric rounding modes from [number.spec.md § Built-in functions](number.spec.md#built-in-functions)
are overloaded on `bl.BlYearsMonthsDuration`: the second argument is a positive duration `step`
(rather than a decimal `scale`), and the result is rounded to the nearest integer multiple of
`step`. This lets host code round to common business granularities — nearest month, nearest
quarter, nearest year, nearest half-year — without converting through total-months arithmetic.

| Function | Example | Result |
|---|---|---|
| `round(d, step)` **ext** | `round(ymDuration("P5M"), ymDuration("P3M"))` | `ymDuration("P6M")` (alias of `roundHalfUp`) |
| `roundUp(d, step)` **ext** | `roundUp(ymDuration("P5M"), ymDuration("P3M"))` | `ymDuration("P6M")` (away from zero) |
| `roundDown(d, step)` **ext** | `roundDown(ymDuration("P5M"), ymDuration("P3M"))` | `ymDuration("P3M")` (toward zero / truncation) |
| `roundHalfUp(d, step)` **ext** | `roundHalfUp(ymDuration("P4.5M"), ymDuration("P3M"))` | `ymDuration("P6M")` (halfway away from zero) |
| `roundHalfDown(d, step)` **ext** | `roundHalfDown(ymDuration("P4.5M"), ymDuration("P3M"))` | `ymDuration("P3M")` (halfway toward zero) |
| `roundHalfEven(d, step)` **ext** | `roundHalfEven(ymDuration("P1Y6M"), ymDuration("P1Y"))` | `ymDuration("P2Y")` (ties to even multiple — banker's rounding) |

Each impl computes `q = totalMonths(d) / totalMonths(step)`, rounds `q` per the chosen mode, and
returns `q * step`. The duration being rounded can be negative; rounding direction respects sign
(e.g. `roundUp(ymDuration("-P5M"), ymDuration("P3M"))` → `ymDuration("-P6M")` — "away from zero" makes
a negative input more negative). Common uses:

```
// expression-language
round(ymDuration("P14M"), ymDuration("P1Y"))           // → ymDuration("P1Y") (nearest year)
round(ymDuration("P14M"), ymDuration("P3M"))           // → ymDuration("P1Y3M") (nearest quarter)
round(ymDuration("P14M"), ymDuration("P6M"))           // → ymDuration("P1Y") (nearest half-year)
roundUp(ymDuration("P1Y0.5M"), ymDuration("P1M"))      // → ymDuration("P1Y1M") (next whole month)
roundDown(ymDuration("P1Y11M"), ymDuration("P1Y"))     // → ymDuration("P1Y") (truncate to whole years)
```

A non-positive `step` (zero or negative) → `bl.TypeError`; rounding to a "nearest zero-sized
multiple" or "nearest negative multiple" has no sensible meaning.

`[@test] ../../expr_years_months_duration_functions_test.go`

---

## Semantics & behaviour

- Months normalise to the half-open interval `0 ≤ |months| < 12`; integer overflow carries into
  years (`ymDuration("P0Y15M")` → `P1Y3M`). Fractional remainders are preserved on the months
  component (`ymDuration("P1Y0.25M").months` → `0.25`).
- Storage is **exact arbitrary-precision decimal** total months; no float rounding occurs at
  parse or arithmetic time. Comparison, equality, and `totalMonths` are all by total signed
  months, so `ymDuration("P1Y") = ymDuration("P12M")` and `ymDuration("P1.5Y") = ymDuration("P1Y6M")`.
- `*` and `/` produce exact decimal results; `/0` yields `null`.
- Fractional designators are accepted on **either** the years or the months component (a
  deliberate relaxation of ISO 8601, which restricts the fraction to the smallest unit used):
  `P1.5Y`, `P0.5M`, `P1Y0.5M`, and `P1.5Y0.5M` are all valid. Components combine into the
  internal total as `years*12 + months`.
- Canonical output (`bl.String()`) puts any fractional remainder on the smallest designator used:
  `12/7` months as `bl.String()` becomes `"P1.7142857...M"` (the underlying decimal preserves all
  digits; the formatter emits the minimal exact representation).
- Sign applies to the whole value: a negative `bl.BlYearsMonthsDuration` has negative `.years` and
  negative `.months` components (not "negative years, positive months").
- Zero duration (`ymDuration("P0Y0M")`) is not negative.
- `ymDurationBetween(from, to)` is signed — `from > to` yields a negative result. The result
  is always integer months because date arithmetic produces whole-month spans.

---

## Go implementation (expr extension)

Lives in `expr/years_months_duration.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlYearsMonthsDuration` is the immutable Go value type that represents a years-and-months
duration inside the engine and at the host-code boundary. It wraps a single signed
arbitrary-precision decimal — the **total months** — and derives the year/month components by
normalisation. The field is private so callers cannot mutate the underlying value; every
operation in the library returns a fresh `bl.BlYearsMonthsDuration`. The decimal representation
mirrors `bl.BlDaysTimeDuration`'s exact-seconds storage and preserves ISO 8601 fractional
durations without float rounding. Go has no built-in years-months duration type
(`time.Duration` is a fixed nanosecond count and cannot represent calendar units), so the
host-code surface stands alone.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares by total months, so `ymDuration("P1Y")` equals `ymDuration("P12M")`
  and `ymDuration("P1.5Y")` equals `ymDuration("P1Y6M")`. `bl.String()` doubles as the `fmt.Stringer`
  implementation, producing the canonical ISO 8601 form: integer split `"P1Y6M"` when the total
  is a whole month count, fractional smallest-unit form (`"P1Y0.25M"`, `"P0.5M"`) otherwise,
  `"-P6M"` for negatives, and `"P0Y0M"` for zero.
- **`YMDuration[T YMDurationInput](v T) (bl.BlYearsMonthsDuration, error)`** — the generic host
  constructor. The `YMDurationInput` constraint accepts a `string` / `bl.BlString`
  (dispatched on the first non-sign character: leading `P` → ISO 8601 with Y/M designators
  only; otherwise → decimal number of total months), every Go integer type (total months),
  `float32` / `float64` (total months; `NaN` / `Inf` error), `decimal.Decimal` / `bl.BlNumber`
  (exact total months, possibly fractional), and a
  `YM{Years, Months decimal.Decimal}` struct (explicit components — not
  range-restricted, and either field may be fractional). See [§ Construction
  (host-side)](#construction-host-side) for the worked example. The `error` return fires
  only for an unparseable string / bl.BlString or a `NaN` / `Inf` float; integer / decimal /
  bl.BlNumber / components inputs are infallible.
- **`Years()` / `Months()` / `TotalMonths()` accessors** — hand the normalised components and the
  signed total back to host code. `Years()` returns the integer years portion (truncated toward
  zero) with sign; `Months()` returns the months remainder as a `decimal.Decimal`
  (`-12 < m < 12`, possibly fractional, same sign as the value); `TotalMonths()` returns the
  signed exact decimal total used for all arithmetic and comparison.

```go
// host-side (Go)
// bl.BlYearsMonthsDuration wraps a signed arbitrary-precision decimal count of total months.
type BlYearsMonthsDuration struct{ months decimal.Decimal }

// bl.BlValue interface — required by all Bl* value types.
func (BlYearsMonthsDuration) Type() Type { return TypeYearsMonthsDuration }
func (d BlYearsMonthsDuration) Equal(other BlValue) BlValue   // exact decimal compare on total months
func (d BlYearsMonthsDuration) String() string                // "P1Y6M" / "P0.5M" / "-P6M" / "P0Y0M"
func (BlYearsMonthsDuration) isBlValue() {}

// Host constructor — accepts:
//   - string / bl.BlString:    "P..." parses as ISO 8601 (Y/M only; designators case-insensitive
//                            on input — "p1y6m" works too); anything else parses as a
//                            decimal-number total-months string ("12.25", "-6", …).
//   - any Go integer:        total months.
//   - float32 / float64:     total months; NaN / Inf error.
//   - decimal.Decimal:       exact total months (preferred for fractional precision).
//   - bl.BlNumber:               engine-wrapped decimal; treated identically to decimal.Decimal.
//   - ymComponents (built via YM(years, months)):
//                             explicit (years, months); each argument is independently
//                             type-checked against YMNumberInput at compile time. The two
//                             arguments may carry different types.

// YMNumberInput is the per-argument constraint for the YM constructor: any Go
// numeric type, a numeric string, decimal.Decimal, bl.BlNumber, or bl.BlString.
type YMNumberInput interface {
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    float32 | float64 |
    decimal.Decimal |
    string |
    BlNumber |
    BlString
}

// ymComponents is the opaque value produced by bl.YM(...). It can only be constructed
// via the typed YM function, which guarantees the field values come from the
// accepted set.
type ymComponents struct {
    years  any   // typed at construction via the YMNumberInput constraint
    months any
}

// Generic typed constructor — Y and M are inferred from the argument types.
func YM[Y, M YMNumberInput](years Y, months M) ymComponents

type YMDurationInput interface {
    string | BlString |
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    float32 | float64 |
    decimal.Decimal | BlNumber |
    ymComponents
}
func YMDuration[T YMDurationInput](v T) (BlYearsMonthsDuration, error)

// Host accessors (consume an evaluated result).
func (d BlYearsMonthsDuration) Years() int                  // integer years; truncated toward zero; sign carries
func (d BlYearsMonthsDuration) Months() decimal.Decimal     // |m| < 12; same sign as the value; may be fractional
func (d BlYearsMonthsDuration) TotalMonths() decimal.Decimal // signed exact total used for arithmetic
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `bl.BlYearsMonthsDuration` and cannot apply Go's native
`+`/`-`/`<`/etc. to blkit values. For every operator that should work on years-months durations,
blkit supplies a named Go function that performs the operation on the underlying total-months
counts and returns the result wrapped as a `bl.BlValue`. The connection from operator token to
function happens in two steps, neither of which is unique to this type:

1. The Registrations section below calls `expr.Function("addYMDuration", typed2(addYMDuration), …)`,
   which makes the engine aware of the function under that exact string name and records its type
   signature.
2. A central `operatorBindings()` in
   [bl-expr.spec.md](bl-expr.spec.md#operators) then calls
   `expr.Operator("+", "addNumbers", "addYMDuration", …)`, which tells the engine "when you see
   `+` at parse time, try each of these registered functions in turn and dispatch to whichever
   one's signature matches the operand types." This step is centralised in one place because a
   single operator spans many types — `+` covers number addition, string concatenation, both
   duration kinds, and several temporal forms — and `expr.Operator` needs the full list of
   candidates per operator in a single call.

So when the parser encounters `a + b` and both operands type-check to `bl.BlYearsMonthsDuration`,
the engine finds `addYMDuration` in the `"+"` binding list, sees its signature matches, and
dispatches to it.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `bl.BlValue` interface, which
`bl.BlYearsMonthsDuration` implements above (compare by total months). That single dispatch path
handles null propagation and cross-type comparison uniformly.

```go
// host-side (Go)
func addYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration              // "+"
func subYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration              // "-"
func negYMDuration(d BlYearsMonthsDuration) BlYearsMonthsDuration                 // unary "-"
func scaleYMDuration(d BlYearsMonthsDuration, n BlNumber) BlYearsMonthsDuration   // "*" — exact decimal
func divYMDuration(d BlYearsMonthsDuration, n BlNumber) BlValue                   // "/" — n == 0 → Null
func ltYMDuration(a, b BlYearsMonthsDuration) BlValue                             // "<" by total months
func leYMDuration(a, b BlYearsMonthsDuration) BlValue                             // "<="
func gtYMDuration(a, b BlYearsMonthsDuration) BlValue                             // ">"
func geYMDuration(a, b BlYearsMonthsDuration) BlValue                             // ">="
// "=" and "!=" go through bl.BlValue.Equal(); see bl.BlYearsMonthsDuration.Equal() above.
```

These are written in clean typed form for readability and unit testing. The engine cannot consume
them at this shape directly — they're wrapped by the `typed1` / `typed2` adapters at
registration time.

### Backing implementations (unexported, suffix `Fn`)

The library and component-accessor functions are implemented as these typed Go functions. They
are wrapped by `typed1` / `typed2` when registered with the engine in the next section.

```go
// host-side (Go)
// Component accessors — emitted by the component-access patcher.
func durationYearsYMFn(d BlYearsMonthsDuration) BlNumber          // overload; D/T overload in days_time_duration.spec.md
func durationMonthsYMFn(d BlYearsMonthsDuration) BlNumber         // overload
func durationTotalMonthsFn(d BlYearsMonthsDuration) BlNumber      // ext; signed
func durationTotalYearsFn(d BlYearsMonthsDuration) BlNumber       // ext; totalMonths / 12

// Library functions.
func ymDurationBetweenFn(from, to BlDate) BlYearsMonthsDuration  // signed; also overloads on BlDateTime in datetime.spec.md
func absYMFn(d BlYearsMonthsDuration) BlYearsMonthsDuration           // overload; numeric/D-T overloads elsewhere
func isNegativeYMFn(d BlYearsMonthsDuration) BlBoolean                // ext; D/T overload in days_time_duration.spec.md

// Rounding family — overloads of the numeric rounding modes from number.spec.md.
// Each rounds totalMonths(d) / totalMonths(step) per the chosen mode, then multiplies back.
// A non-positive step returns a bl.TypeError.
func roundYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration            // ext; alias of roundHalfUpYM
func roundUpYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration          // ext
func roundDownYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration        // ext
func roundHalfUpYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration      // ext
func roundHalfDownYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration    // ext
func roundHalfEvenYMFn(d, step BlYearsMonthsDuration) BlYearsMonthsDuration    // ext
```

The six rounding impls share a single helper that computes the rounded quotient as a
`decimal.Decimal` using the matching mode, then multiplies by `step.TotalMonths()`. They reuse
the same per-mode decimal-rounding logic as the numeric `round*Fn` impls in `expr/number.go` so
that ties and signs behave identically across types.

The Y/M-only parser `ymDurationFn(s bl.BlString) (bl.BlYearsMonthsDuration, error)` lives in this
spoke. It accepts only ISO 8601 strings whose designators are restricted to `Y` and `M` (with an
optional `T`-less leading sign); any `D`/`T`/`H`/`S` designator yields a `bl.ParseError`. The
sibling `dtDuration` parser in [days_time_duration.spec.md](days_time_duration.spec.md) applies
the mirror restriction.

### Registrations (`yearsMonthsDurationOptions`, unexported)

`yearsMonthsDurationOptions()` returns the slice of `expr.Option` values the engine consumes
during initialisation to learn about every years-months-duration-related operator impl and
library function. Each entry is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (or that
  `operatorBindings()` references for operator dispatch, or that the component-access patcher
  emits for `.years` / `.months` / `.totalMonths`).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2`
  adapters (defined in
  [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)) wrap a typed
  implementation such as `func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration`
  into that shape.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across signatures (e.g. `abs`
  accepts both `bl.BlYearsMonthsDuration` and `bl.BlDaysTimeDuration`).

The registrations are grouped by role: operator impls (consumed by `operatorBindings()`),
component-accessor impls (emitted by the patcher), and the library functions.

```go
// host-side (Go)
func yearsMonthsDurationOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addYMDuration",   typed2(addYMDuration),   new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),
        expr.Function("subYMDuration",   typed2(subYMDuration),   new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),
        expr.Function("negYMDuration",   typed1(negYMDuration),   new(func(bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),
        expr.Function("scaleYMDuration", typed2(scaleYMDuration), new(func(bl.BlYearsMonthsDuration, bl.BlNumber) bl.BlYearsMonthsDuration)),
        expr.Function("divYMDuration",   typed2(divYMDuration),   new(func(bl.BlYearsMonthsDuration, bl.BlNumber) bl.BlValue)),
        expr.Function("ltYMDuration",    typed2(ltYMDuration),    new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlValue)),
        expr.Function("leYMDuration",    typed2(leYMDuration),    new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlValue)),
        expr.Function("gtYMDuration",    typed2(gtYMDuration),    new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlValue)),
        expr.Function("geYMDuration",    typed2(geYMDuration),    new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlValue)),
        // = and != dispatch via bl.BlValue.Equal() — no per-type registration

        // component-access impls — emitted by the patcher when lowering .years / .months / .totalMonths / .totalYears
        expr.Function("durationYears",       typed1(durationYearsYMFn),     new(func(bl.BlYearsMonthsDuration) bl.BlNumber)),
        expr.Function("durationMonths",      typed1(durationMonthsYMFn),    new(func(bl.BlYearsMonthsDuration) bl.BlNumber)),
        expr.Function("durationTotalMonths", typed1(durationTotalMonthsFn), new(func(bl.BlYearsMonthsDuration) bl.BlNumber)),
        expr.Function("durationTotalYears",  typed1(durationTotalYearsFn),  new(func(bl.BlYearsMonthsDuration) bl.BlNumber)),  // ext

        // constructor — Y/M-only parser; sibling dtDuration lives in days_time_duration.spec.md
        expr.Function("ymDuration", typed1(ymDurationFn), new(func(bl.BlString) bl.BlYearsMonthsDuration)),

        // library — overloads share a name with the days-time spoke's registrations
        expr.Function("ymDurationBetween", typed2(ymDurationBetweenFn), new(func(bl.BlDate, bl.BlDate) bl.BlYearsMonthsDuration)),
        expr.Function("abs",        typed1(absYMFn),        new(func(bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // overload
        expr.Function("isNegative", typed1(isNegativeYMFn), new(func(bl.BlYearsMonthsDuration) bl.BlBoolean)),               // ext; overload

        // rounding — overloads of the numeric rounding modes from number.spec.md
        expr.Function("round",         typed2(roundYMFn),         new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext; alias of roundHalfUp
        expr.Function("roundUp",       typed2(roundUpYMFn),       new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext
        expr.Function("roundDown",     typed2(roundDownYMFn),     new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfUp",   typed2(roundHalfUpYMFn),   new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfDown", typed2(roundHalfDownYMFn), new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfEven", typed2(roundHalfEvenYMFn), new(func(bl.BlYearsMonthsDuration, bl.BlYearsMonthsDuration) bl.BlYearsMonthsDuration)),  // ext
    }
}
```

The date / datetime `+` / `-` overloads that consume a years-months duration live in the
[date](date.spec.md) and [datetime](datetime.spec.md) spokes (those spokes own one operand of
the pair); applying a years-months duration to a `time` → `bl.TypeError`.

`[@test] ../../expr_years_months_duration_test.go`

---

## Edge cases

- `dtDuration("P1DT…")` (any day or time designator) → `bl.ParseError` for this type.
- Fractional designators are accepted on either Y or M (or both) — a deliberate relaxation of
  ISO 8601's smallest-unit-only rule.
- Division by a zero factor (`d / 0`) → `null`.
- `*` and `/` produce exact decimal results — no rounding. The fractional remainder is preserved
  on the months component and emitted on the smallest unit by `bl.String()`.
- `round*` family: `step` must be a positive duration. Zero or negative `step` → `bl.TypeError`.
  Sign of the input is preserved (rounding direction respects it — `roundUp` on a negative input
  rounds away from zero, making it more negative).
- Zero duration: `isNegative` → `false`, `abs` → unchanged, `.years` is `0`, `.months` is `0`.
- Adding a `bl.BlDaysTimeDuration` → `bl.TypeError`; applying any years-months duration to a `time`
  → `bl.TypeError`.
- `ymDurationBetween` is signed: `from > to` yields a negative result rather than swapping
  the operands; its output is always whole months because date arithmetic produces integer spans.
- Sign is held on the whole value, not per component — a negative duration has both negative
  `.years` and negative `.months`.
