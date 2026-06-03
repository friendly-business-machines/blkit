---
name: BlYearsMonthsDuration
description: The years-and-months duration type in the blkit expression language (ISO 8601 PnYnM). Covers construction, component access, the arithmetic/comparison operators, the duration built-ins, and the Go layer (BlYearsMonthsDuration + expr registrations).
targets:
  - ../../expr/years_months_duration.go
---

# BlYearsMonthsDuration — the years-and-months `duration`

A years-and-months duration covers only years and months (ISO 8601 `PnYnM`). The Go value type
backing it is `BlYearsMonthsDuration`. It is distinct from `BlDaysTimeDuration`
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
ymDuration("P1DT2H")                 // → BlParseError (day/time designators not allowed here)
```

blkit accepts a decimal fraction on either designator — this is a deliberate relaxation of ISO
8601, which permits a fraction only on the smallest unit used. Fractional components combine as
`years*12 + months` into the internal total, stored exactly using arbitrary-precision decimal
(no float rounding).

`ymDuration` is paired with `dtDuration` ([days_time_duration.spec.md](days_time_duration.spec.md))
for the sibling days-time duration. The two are separate functions — the typed return makes
downstream usage statically checkable, where a single polymorphic `duration(string)` would force
the call site to inspect the runtime type — and a D/T string passed to `ymDuration` (or vice
versa) is a `BlParseError`.

The companion built-in `ymDurationBetween(from, to)` computes the years-months span between
two dates or datetimes (registered in [datetime.spec.md](datetime.spec.md), which owns the
operand types):

```
// expression-language
ymDurationBetween(date("2011-12-22"), date("2013-08-24"))   // → ymDuration("P1Y8M")
ymDurationBetween(date("2025-06-01"), date("2024-06-01"))   // → ymDuration("-P1Y")  (signed)
```

Both operands must be the **same** temporal kind — either both `BlDate` or both `BlDateTime`. A
mixed `(date, datetime)` call is a type error; convert one operand explicitly via `datetime(d)`
or `date(dt)` first. See [datetime.spec.md § Business-day arithmetic & difference](datetime.spec.md#business-day-arithmetic--difference-ext)
for the registered overloads.

`[@test] ../../expr/years_months_duration_test.go`

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
[bl-expr.spec.md § Patchers](bl-expr.spec.md#patchers-ast-rewriting).

`[@test] ../../expr/years_months_duration_components_test.go`

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
the date/datetime spokes). Mixing with a `BlDaysTimeDuration` → `BlTypeError`; applying to a
`time` → `BlTypeError`. Division by zero → `null`.

`*` and `/` scale the total-months count by a `BlNumber` using **exact decimal arithmetic** — no
rounding. `ymDuration("P1Y") / 7` yields a duration whose `totalMonths` is exactly `12/7` (a
`BlNumber` with arbitrary precision); the canonical string form puts the resulting fraction on
the smallest designator used (here, months).

`[@test] ../../expr/years_months_duration_ops_test.go`

---

## Built-in functions

| Function | Example | Result |
|---|---|---|
| `ymDuration(from)` | `ymDuration("P1Y6M")` | the corresponding `BlYearsMonthsDuration` (Y/M-only strings; D/T strings → `BlParseError`) |
| `ymDurationBetween(from, to)` | `ymDurationBetween(date("2024-01-01"), date("2025-06-01"))` | `ymDuration("P1Y5M")` |
| `abs(d)` | `abs(ymDuration("-P2Y3M"))` | `ymDuration("P2Y3M")` |
| `isNegative(d)` **ext** | `isNegative(ymDuration("-P3M"))` | `true` (zero → `false`) |

`abs` and `isNegative` are overloaded across the two duration types (the registrations in this
spoke add the `BlYearsMonthsDuration` signatures; the `BlDaysTimeDuration` ones live in
[days_time_duration.spec.md](days_time_duration.spec.md)).

### Rounding

The six numeric rounding modes from [number.spec.md § Built-in functions](number.spec.md#built-in-functions)
are overloaded on `BlYearsMonthsDuration`: the second argument is a positive duration `step`
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

A non-positive `step` (zero or negative) → `BlTypeError`; rounding to a "nearest zero-sized
multiple" or "nearest negative multiple" has no sensible meaning.

`[@test] ../../expr/years_months_duration_functions_test.go`

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
- Canonical output (`String()`) puts any fractional remainder on the smallest designator used:
  `12/7` months as `String()` becomes `"P1.7142857...M"` (the underlying decimal preserves all
  digits; the formatter emits the minimal exact representation).
- Sign applies to the whole value: a negative `BlYearsMonthsDuration` has negative `.years` and
  negative `.months` components (not "negative years, positive months").
- Zero duration (`ymDuration("P0Y0M")`) is not negative.
- `ymDurationBetween(from, to)` is signed — `from > to` yields a negative result. The result
  is always integer months because date arithmetic produces whole-month spans.

---

## Go implementation (expr extension)

Lives in `expr/years_months_duration.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlYearsMonthsDuration` is the immutable Go value type that represents a years-and-months
duration inside the engine and at the host-code boundary. It wraps a single signed
arbitrary-precision decimal — the **total months** — and derives the year/month components by
normalisation. The field is private so callers cannot mutate the underlying value; every
operation in the library returns a fresh `BlYearsMonthsDuration`. The decimal representation
mirrors `BlDaysTimeDuration`'s exact-seconds storage and preserves ISO 8601 fractional
durations without float rounding. Go has no built-in years-months duration type
(`time.Duration` is a fixed nanosecond count and cannot represent calendar units), so the
host-code surface stands alone.

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares by total months, so `ymDuration("P1Y")` equals `ymDuration("P12M")`
  and `ymDuration("P1.5Y")` equals `ymDuration("P1Y6M")`. `String()` doubles as the `fmt.Stringer`
  implementation, producing the canonical ISO 8601 form: integer split `"P1Y6M"` when the total
  is a whole month count, fractional smallest-unit form (`"P1Y0.25M"`, `"P0.5M"`) otherwise,
  `"-P6M"` for negatives, and `"P0Y0M"` for zero.
- **`YearsMonths(years, months int)` / `YearsMonthsFromTotalMonths(totalMonths decimal.Decimal)`** —
  the host constructors. `YearsMonths` is the integer-component convenience form; inputs are
  not range-restricted, and the constructor normalises by summing `years*12 + months` into the
  internal total. `YearsMonthsFromTotalMonths` accepts an exact decimal total, including
  fractional values — host code that needs `P1Y0.25M` constructs it as
  `YearsMonthsFromTotalMonths(decimal.NewFromFloat(12.25))` or equivalent. The older
  integer-only `YearsMonthsFromMonths(totalMonths int)` form remains for ergonomics.
- **`Years()` / `Months()` / `TotalMonths()` accessors** — hand the normalised components and the
  signed total back to host code. `Years()` returns the integer years portion (truncated toward
  zero) with sign; `Months()` returns the months remainder as a `decimal.Decimal`
  (`-12 < m < 12`, possibly fractional, same sign as the value); `TotalMonths()` returns the
  signed exact decimal total used for all arithmetic and comparison.

```go
// host-side (Go)
// BlYearsMonthsDuration wraps a signed arbitrary-precision decimal count of total months.
type BlYearsMonthsDuration struct{ months decimal.Decimal }

// BlValue interface — required by all Bl* value types.
func (BlYearsMonthsDuration) Type() BlType { return BlTypeYearsMonthsDuration }
func (d BlYearsMonthsDuration) Equal(other BlValue) BlValue   // exact decimal compare on total months
func (d BlYearsMonthsDuration) String() string                // "P1Y6M" / "P0.5M" / "-P6M" / "P0Y0M"
func (BlYearsMonthsDuration) isBlValue() {}

// Host constructors.
func YearsMonths(years, months int) BlYearsMonthsDuration                              // integer convenience
func YearsMonthsFromMonths(totalMonths int) BlYearsMonthsDuration                      // integer total
func YearsMonthsFromTotalMonths(totalMonths decimal.Decimal) BlYearsMonthsDuration     // fractional total

// Host accessors (consume an evaluated result).
func (d BlYearsMonthsDuration) Years() int                  // integer years; truncated toward zero; sign carries
func (d BlYearsMonthsDuration) Months() decimal.Decimal     // |m| < 12; same sign as the value; may be fractional
func (d BlYearsMonthsDuration) TotalMonths() decimal.Decimal // signed exact total used for arithmetic
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlYearsMonthsDuration` and cannot apply Go's native
`+`/`-`/`<`/etc. to blkit values. For every operator that should work on years-months durations,
blkit supplies a named Go function that performs the operation on the underlying total-months
counts and returns the result wrapped as a `BlValue`. The connection from operator token to
function happens in two steps, neither of which is unique to this type:

1. The Registrations section below calls `expr.Function("addYMDuration", typed2(addYMDuration), …)`,
   which makes the engine aware of the function under that exact string name and records its type
   signature.
2. A central `operatorBindings()` in
   [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then calls
   `expr.Operator("+", "addNumbers", "addYMDuration", …)`, which tells the engine "when you see
   `+` at parse time, try each of these registered functions in turn and dispatch to whichever
   one's signature matches the operand types." This step is centralised in one place because a
   single operator spans many types — `+` covers number addition, string concatenation, both
   duration kinds, and several temporal forms — and `expr.Operator` needs the full list of
   candidates per operator in a single call.

So when the parser encounters `a + b` and both operands type-check to `BlYearsMonthsDuration`,
the engine finds `addYMDuration` in the `"+"` binding list, sees its signature matches, and
dispatches to it.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `BlValue` interface, which
`BlYearsMonthsDuration` implements above (compare by total months). That single dispatch path
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
// "=" and "!=" go through BlValue.Equal(); see BlYearsMonthsDuration.Equal() above.
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
// A non-positive step returns a BlTypeError.
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

The Y/M-only parser `ymDurationFn(s BlString) (BlYearsMonthsDuration, error)` lives in this
spoke. It accepts only ISO 8601 strings whose designators are restricted to `Y` and `M` (with an
optional `T`-less leading sign); any `D`/`T`/`H`/`S` designator yields a `BlParseError`. The
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
  implementation such as `func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration`
  into that shape.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across signatures (e.g. `abs`
  accepts both `BlYearsMonthsDuration` and `BlDaysTimeDuration`).

The registrations are grouped by role: operator impls (consumed by `operatorBindings()`),
component-accessor impls (emitted by the patcher), and the library functions.

```go
// host-side (Go)
func yearsMonthsDurationOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addYMDuration",   typed2(addYMDuration),   new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),
        expr.Function("subYMDuration",   typed2(subYMDuration),   new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),
        expr.Function("negYMDuration",   typed1(negYMDuration),   new(func(BlYearsMonthsDuration) BlYearsMonthsDuration)),
        expr.Function("scaleYMDuration", typed2(scaleYMDuration), new(func(BlYearsMonthsDuration, BlNumber) BlYearsMonthsDuration)),
        expr.Function("divYMDuration",   typed2(divYMDuration),   new(func(BlYearsMonthsDuration, BlNumber) BlValue)),
        expr.Function("ltYMDuration",    typed2(ltYMDuration),    new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlValue)),
        expr.Function("leYMDuration",    typed2(leYMDuration),    new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlValue)),
        expr.Function("gtYMDuration",    typed2(gtYMDuration),    new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlValue)),
        expr.Function("geYMDuration",    typed2(geYMDuration),    new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlValue)),
        // = and != dispatch via BlValue.Equal() — no per-type registration

        // component-access impls — emitted by the patcher when lowering .years / .months / .totalMonths / .totalYears
        expr.Function("durationYears",       typed1(durationYearsYMFn),     new(func(BlYearsMonthsDuration) BlNumber)),
        expr.Function("durationMonths",      typed1(durationMonthsYMFn),    new(func(BlYearsMonthsDuration) BlNumber)),
        expr.Function("durationTotalMonths", typed1(durationTotalMonthsFn), new(func(BlYearsMonthsDuration) BlNumber)),
        expr.Function("durationTotalYears",  typed1(durationTotalYearsFn),  new(func(BlYearsMonthsDuration) BlNumber)),  // ext

        // constructor — Y/M-only parser; sibling dtDuration lives in days_time_duration.spec.md
        expr.Function("ymDuration", typed1(ymDurationFn), new(func(BlString) BlYearsMonthsDuration)),

        // library — overloads share a name with the days-time spoke's registrations
        expr.Function("ymDurationBetween", typed2(ymDurationBetweenFn), new(func(BlDate, BlDate) BlYearsMonthsDuration)),
        expr.Function("abs",        typed1(absYMFn),        new(func(BlYearsMonthsDuration) BlYearsMonthsDuration)),  // overload
        expr.Function("isNegative", typed1(isNegativeYMFn), new(func(BlYearsMonthsDuration) BlBoolean)),               // ext; overload

        // rounding — overloads of the numeric rounding modes from number.spec.md
        expr.Function("round",         typed2(roundYMFn),         new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext; alias of roundHalfUp
        expr.Function("roundUp",       typed2(roundUpYMFn),       new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext
        expr.Function("roundDown",     typed2(roundDownYMFn),     new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfUp",   typed2(roundHalfUpYMFn),   new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfDown", typed2(roundHalfDownYMFn), new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext
        expr.Function("roundHalfEven", typed2(roundHalfEvenYMFn), new(func(BlYearsMonthsDuration, BlYearsMonthsDuration) BlYearsMonthsDuration)),  // ext
    }
}
```

The date / datetime `+` / `-` overloads that consume a years-months duration live in the
[date](date.spec.md) and [datetime](datetime.spec.md) spokes (those spokes own one operand of
the pair); applying a years-months duration to a `time` → `BlTypeError`.

`[@test] ../../expr/years_months_duration_test.go`

---

## Edge cases

- `dtDuration("P1DT…")` (any day or time designator) → `BlParseError` for this type.
- Fractional designators are accepted on either Y or M (or both) — a deliberate relaxation of
  ISO 8601's smallest-unit-only rule.
- Division by a zero factor (`d / 0`) → `null`.
- `*` and `/` produce exact decimal results — no rounding. The fractional remainder is preserved
  on the months component and emitted on the smallest unit by `String()`.
- `round*` family: `step` must be a positive duration. Zero or negative `step` → `BlTypeError`.
  Sign of the input is preserved (rounding direction respects it — `roundUp` on a negative input
  rounds away from zero, making it more negative).
- Zero duration: `isNegative` → `false`, `abs` → unchanged, `.years` is `0`, `.months` is `0`.
- Adding a `BlDaysTimeDuration` → `BlTypeError`; applying any years-months duration to a `time`
  → `BlTypeError`.
- `ymDurationBetween` is signed: `from > to` yields a negative result rather than swapping
  the operands; its output is always whole months because date arithmetic produces integer spans.
- Sign is held on the whole value, not per component — a negative duration has both negative
  `.years` and negative `.months`.
