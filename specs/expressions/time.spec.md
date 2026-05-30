---
name: BlTime
description: The time-of-day type in the blkit expression language. Covers the time constructor, component access, duration arithmetic, comparison, timezone/offset semantics, and the Go layer (BlTime + expr registrations).
targets:
  - ../../expr/time.go
---

# BlTime — the `time` type

`time` is a time of day: hours, minutes, seconds (optionally fractional), with an optional UTC
offset or IANA timezone. The textual form follows [ISO 8601](https://www.iso.org/iso-8601-date-and-time-format.html)
for the time portion and [RFC 9557 (IXDTF)](https://datatracker.ietf.org/doc/html/rfc9557) for
the `[Zone]` suffix used to attach an IANA timezone name. The Go value type backing it is
`BlTime`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and component-access syntax.

---

## Literals / construction

There is **no dedicated time literal**: time values are produced by the `time(...)` built-in — for
example, the `time("14:30:00")` in `time("14:30:00").hour`. The constructor accepts an ISO 8601
time string, hour/minute/second components, or another temporal value to extract from.

```
time("14:30:00")              // local time
time("14:30:00.500")          // fractional seconds
time("14:30:00Z")             // UTC
time("11:45:30+02:00")        // fixed offset
time("14:30:00[Europe/Paris]") // IANA timezone (RFC 9557, DST-aware)
time(23, 59, 0)               // from components
time(now())                   // current time of day (from current datetime)
time(datetime("2025-03-28T14:30:00"))  // extract time component
```

blkit distinguishes a fixed **offset** (`+01:00`, not DST-aware), a named **timezone**
(`[Europe/Paris]`, DST-aware), and a **local** time (no UTC relationship).

`[@test] ../../expr/time_test.go`

---

## Component access

```
time("11:45:30+02:00").hour         // → 11
time("11:45:30+02:00").minute       // → 45
time("11:45:30+02:00").second       // → 30
time("11:45:30+02:00").offset       // → duration("PT2H")   (ext)
time("11:45:30[Europe/Paris]").timezone  // → "Europe/Paris" (ext)
```

`[@test] ../../expr/time_components_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` `-` | add/subtract a days-time duration (wraps at midnight) | `time("23:00:00") + duration("PT2H")` | `time("01:00:00")` |
| `< <= > >= = !=` | comparison | `time("14:30:00") < time("17:00:00")` | `true` |
| `between a and b` | inclusive range | `time("12:00:00") between time("09:00:00") and time("17:00:00")` | `true` |
| `in` | membership (list / range) | `now() in [time("09:00:00")..time("17:00:00")]` | `true`/`false` |

Only a **days-time** duration may be applied (time has no year/month concept). The date component is
not tracked — adding `PT25H` to `23:00:00` gives `00:00:00` (day advance discarded); use `datetime`
for day-tracking arithmetic.

`withOffset(t, offset)` for re-zoning a `BlTime` is documented in
[datetime.spec.md § Re-zoning](datetime.spec.md#re-zoning-ext) — the function accepts both
`BlTime` and `BlDateTime`.

`[@test] ../../expr/time_ops_test.go`

---

## Comparison semantics

- Two times that both carry offset/timezone are compared as **UTC instants**.
- Two local times are compared **wall-clock**.
- Comparing a local time to a zoned/offset time → `null`.

---

## Go implementation (expr extension)

Lives in `expr/time.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlTime` is the immutable Go value type that represents a time of day inside the engine and at
the host-code boundary. It wraps a Go [`time.Time`](https://pkg.go.dev/time#Time) — Go has no
separate time-of-day type, so `time.Time` is used by convention with the date portion set to
`0001-01-01` (year 1) and ignored by all `BlTime` operations — plus a `naive` boolean that
distinguishes the timezone-naive (local) case from a genuinely UTC/zoned one. Both fields are
private so callers cannot mutate the underlying value; every operation in the library returns
a fresh `BlTime`.

Constructors normalise the date portion to `0001-01-01` on input. Conversions in/out (e.g.
`Native()` accessor returning the wrapped `time.Time`, or callers passing a `time.Time` into
`Time(...)` whose date portion is non-zero) discard the date component; callers should not
read the year/month/day fields of a `BlTime.Native()` value.

The `naive` flag is needed because Go's `time.Time` always carries a non-nil `*time.Location`,
so there is no built-in way to represent "a time of day with no timezone interpretation".
When `naive` is true, the `Location()` of the wrapped `time.Time` is `time.UTC` by convention
and must be ignored. When `naive` is false, the `Location()` is meaningful and may be
`time.UTC`, a fixed-offset zone from [`time.FixedZone`](https://pkg.go.dev/time#FixedZone), or
an IANA zone loaded via [`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation).

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` compares wall-clock times when both operands are naive, and compares as
  UTC instants when both are zoned/offset; mixing them returns `BlNull`. `String()` doubles as
  the `fmt.Stringer` implementation, producing the canonical ISO 8601 / RFC 9557 form (e.g.
  `"14:30:00"`, `"14:30:00+01:00"`, or `"14:30:00[Europe/Paris]"`).
- **`Time[T TimeInput](v T)`** — the generic host constructor. `TimeInput` accepts `string`
  (parsed as ISO 8601 / RFC 9557 — `naive` is set based on whether a zone designator was
  present), `time.Time` (the time portion is extracted; `naive` is always false because a
  `time.Time` always carries a `Location`), or a `TimeComponents` struct for explicit
  component-by-component construction (`naive` is true if and only if both `Offset` and `Zone`
  are absent). The `error` return fires for unparseable strings, invalid components (hour ≥
  24, minute/second ≥ 60, etc.), or a `TimeComponents` with both `Offset` and `Zone` set
  (they're mutually exclusive). Host code wanting a naive result from a `time.Time` should
  pipe it through `ToTimeComponentsAsNaive` first.
- **`Native()` accessor** — hands back the wrapped `time.Time` (with the time portion
  meaningful, date portion conventionally `0001-01-01`). For non-naive values this is the full
  representation. For naive values the host receives a `time.Time` with `Location() == time.UTC`,
  which it MUST treat as wall-clock — callers that need to know which kind they have should
  also call `IsNaive()`. The accessor is named `Native` (not `Time`) to avoid a collision with
  the `Time` constructor.
- **`IsNaive()` accessor** — reports whether the value is timezone-naive.

```go
type BlTime struct {
    t     time.Time   // time portion meaningful; Location() carries offset or IANA zone when naive==false
    naive bool        // true when no offset/zone was specified — Location() is ignored
}

// BlValue interface — required by all Bl* value types.
func (BlTime) Type() BlType { return BlTypeTime }
func (t BlTime) Equal(other BlValue) BlValue   // wall-clock or UTC-instant; cross-kind → Null
func (t BlTime) String() string                // canonical ISO 8601 / RFC 9557
func (BlTime) isBlValue() {}

// Host constructor — accepts an ISO 8601/RFC 9557 string, a Go time.Time, or component bundle.
type TimeComponents struct {
    Hour, Minute, Second int
    Nanosecond int               // optional (default 0); for fractional seconds
    Offset *time.Duration        // optional; mutually exclusive with Zone
    Zone   string                // optional; IANA name; mutually exclusive with Offset
}
type TimeInput interface { string | time.Time | TimeComponents }
func Time[T TimeInput](v T) (BlTime, error)

// Decompose a time.Time into time-of-day components with Offset/Zone unset, so the
// resulting TimeComponents builds a naive BlTime when passed to Time. Use this when
// host code has a time.Time but wants to drop its zone interpretation.
func ToTimeComponentsAsNaive(t time.Time) TimeComponents

// Host accessors (consume an evaluated result).
func (t BlTime) Native() time.Time             // wrapped time.Time; for naive values its Location() is UTC and must be ignored
func (t BlTime) IsNaive() bool                 // true when timezone-naive
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlTime` and cannot apply Go's native `+`/`-`/`<`/etc. to
blkit values. For every operator that should work on times, blkit supplies a named Go function
that performs the operation and returns the result wrapped as a `BlValue`. The connection from
operator token to function happens in two steps, neither of which is unique to `BlTime`:

1. The Registrations section below calls `expr.Function("addTimeDur", typed2(addTimeDur), …)`,
   which makes the engine aware of the function under that exact string name and records its
   type signature.
2. A central `operatorBindings()` in [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then
   calls `expr.Operator("+", "addNumbers", "concatStrings", "addTimeDur", …)`, which tells
   the engine "when you see `+` at parse time, try each of these registered functions in turn
   and dispatch to whichever one's signature matches the operand types." Centralised in one
   place because a single operator spans many types.

So when the parser encounters `t + dur` and the operands type-check to `BlTime` +
`BlDaysTimeDuration`, the engine finds `addTimeDur` in the `"+"` binding list, sees its
signature matches, and dispatches to it.

Arithmetic impls return `BlTime` directly because they cannot yield null (midnight wrap
handles any overflow case: `time("23:00:00") + duration("PT2H") → time("01:00:00")`, day
advance discarded). Comparison impls return `BlValue` because cross-kind comparison (naive vs
zoned/offset) yields `BlNull`.

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `BlValue` interface, which `BlTime` implements
above. That single dispatch path handles null propagation and cross-type comparison uniformly.

`between` and `in` are patcher-lowered: `between` rewrites to a pair of comparisons; `in`
dispatches to list/range membership.

```go
func addTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime  // "+" t + DT duration (wraps at midnight)
func subTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime  // "-" t − DT duration (wraps at midnight)
func ltTimes(a, b BlTime) BlValue                          // "<"  ; cross-kind → Null
func leTimes(a, b BlTime) BlValue                          // "<=" ; cross-kind → Null
func gtTimes(a, b BlTime) BlValue                          // ">"  ; cross-kind → Null
func geTimes(a, b BlTime) BlValue                          // ">=" ; cross-kind → Null
// "=" and "!=" go through BlValue.Equal(); see BlTime.Equal() above.
```

These are written in clean typed form (`BlTime → BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters at registration time.

### Backing implementations (unexported, suffix `Fn`)

The constructor is implemented as a variadic Go function (multiple input signatures). The
`timeFn` shape is needed because the constructor accepts string, components, or extraction
from a datetime.

```go
// Variadic implementation — handles multiple input shapes in expr's raw shape.
func timeFn(args ...any) (any, error)   // time("…") | time(h, m, s) | time(dt) extraction
```

`timeFn` parses ISO 8601 / RFC 9557 strings via Go's [`time.Parse`](https://pkg.go.dev/time#Parse);
an unparseable string → `BlParseError`. Hour/minute/second component construction validates
ranges (hour 0–23, minute/second 0–59) and rejects invalid combinations with `BlTypeError`.
IANA zone lookups go through [`time.LoadLocation`](https://pkg.go.dev/time#LoadLocation); an
unknown zone name → `BlTypeError`. Native Go `time.Time` (time-of-day portion) inputs wrap to
`BlTime`.

Component accessors (`.hour`/`.minute`/`.second`/`.offset`/`.timezone`) are resolved by the
component-access patcher described in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go)
— they dispatch to internal accessor functions (`timeHourFn`, …) that are not registered as
user-callable `expr.Function`s.

Operator implementation functions (`addTimeDur`, `subTimeDur`, `ltTimes`, `leTimes`,
`gtTimes`, `geTimes`) are documented in the previous section.

The `withOffset` function (re-zoning) accepts both `BlTime` and `BlDateTime` and is registered
in [datetime.spec.md § Re-zoning](datetime.spec.md#re-zoning-ext) under a single
`expr.Function` with both type signatures.

### Registrations (`timeOptions`, unexported)

`timeOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every time-related operator impl and constructor function. Each
entry is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that
  `operatorBindings()` references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlTime, BlDaysTimeDuration) BlTime` into that
  shape, type-asserting each argument and boxing the result. The variadic implementation
  (`timeFn`) already satisfies the shape and is registered directly.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures.

Time-only registrations are grouped by role: operator impls (consumed by `operatorBindings()`)
and the constructor. Re-zoning (`withOffset`) accepts both `BlTime` and `BlDateTime` and is
registered once in [datetime.spec.md](datetime.spec.md) with both type signatures.

```go
func timeOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addTimeDur", typed2(addTimeDur), new(func(BlTime, BlDaysTimeDuration) BlTime)),
        expr.Function("subTimeDur", typed2(subTimeDur), new(func(BlTime, BlDaysTimeDuration) BlTime)),
        expr.Function("ltTimes",    typed2(ltTimes),    new(func(BlTime, BlTime) BlValue)),
        expr.Function("leTimes",    typed2(leTimes),    new(func(BlTime, BlTime) BlValue)),
        expr.Function("gtTimes",    typed2(gtTimes),    new(func(BlTime, BlTime) BlValue)),
        expr.Function("geTimes",    typed2(geTimes),    new(func(BlTime, BlTime) BlValue)),
        // = and != dispatch via BlValue.Equal() — no per-type registration

        // construction / extraction
        expr.Function("time", timeFn,
            new(func(BlString) BlTime),                          // time("…")
            new(func(BlNumber, BlNumber, BlNumber) BlTime),      // time(h, m, s)
            new(func(BlDateTime) BlTime)),                       // time(dt) extraction
    }
}
```

`[@test] ../../expr/time_test.go`

---

## Edge cases

- Hour outside 0–23, or minute/second outside 0–59 (component form) → `BlTypeError`.
- `time("24:00:00")` (end-of-day) is valid ISO 8601, normalised to `00:00:00`.
- Unknown IANA zone id → `BlTypeError`.
- Applying a years-months duration → `BlTypeError`.
