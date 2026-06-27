# Values from Go

> Constructing blkit values in host Go code, declaring typed environments, and
> reading results back — the bridge between the expression language and the Go
> program that runs it.

The other pages in this section describe the expression *language*: the syntax
and functions you write inside an expression string. This page covers the
**host side** — how your Go program builds the values an expression reads, hands
them in, and gets a result back out.

Every value that crosses the boundary is a `bl.BlValue`. You build them with the
`bl.*` constructors that mirror each language type, reference them from an
expression through a typed environment struct, and recover the native Go value
from the result.

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"
```

## The constructors

Each language type has a constructor in the `core` package (imported as `bl`):

| Type | Constructor | Accepts |
|---|---|---|
| `number` | `bl.Number` | any Go numeric type, `bool`, `decimal.Decimal`, or a string |
| `string` | `bl.String` | a Go `string` or valid-UTF-8 `[]byte` |
| `boolean` | `bl.Boolean` | a Go `bool`, any integer (`0`→false, non-zero→true), or `"true"`/`"false"` |
| `null` | `bl.Null` | (no argument) the unknown/absent value |
| `list` | `bl.List` | variadic `bl.BlValue`s, or a spread slice |
| `dictionary` | `bl.Dictionary` | a `map[string]bl.BlValue` |
| `range` | `bl.Range` | two endpoint `bl.BlValue`s plus start/end inclusion flags |
| `date` | `bl.Date` | an ISO 8601 string, a `time.Time`, or a components struct |
| `time` | `bl.Time` | an ISO 8601 string, a `time.Time`, or a components struct |
| `date and time` | `bl.DateTime` | an ISO 8601 / RFC 9557 string, a `time.Time`, or components |
| `days and time duration` | `bl.DTDuration` | an ISO 8601 duration string, seconds, or `bl.DHMS(...)` |
| `years and months duration` | `bl.YMDuration` | an ISO 8601 duration string, months, or `bl.YM(...)` |
| `table` | `bl.Table` | a `bl.Cols` header plus positional `bl.Row` values |
| `calendar` | `bl.Calendar` | a slice of `bl.CalendarEntry(...)` values |

Most constructors return `(value, error)`; the error reports an input that can't
be represented (a `NaN`/`Inf` float, an unparseable string, and so on). For
inputs that can't fail, the `_` slot just keeps the call site uniform.

## Scalars

```go
// host-side (Go)
var amount, _ = bl.Number(1500.50)
var price,  _ = bl.Number("$1,234.56")   // → bl.BlNumber(1234.56)

var greeting, _ = bl.String("hello")
var greet2, _   = bl.String([]byte{0x68, 0x65, 0x6C, 0x6C, 0x6F})   // "hello"

var approved, _ = bl.Boolean(true)       // from a Go bool
var flag,     _ = bl.Boolean(1)          // 0 → false, non-zero → true
var fromConf, _ = bl.Boolean("true")     // case-insensitive string
```

The `bl.Number` string form is the forgiving one: it tolerates thousands
separators, currency symbols, and surrounding whitespace. To render a non-string
Go value, format it host-side first (`strconv.Itoa`, `fmt.Sprintf`, …) or convert
it inside the expression with `string(...)`.

To model an **unknown** value — a missing boolean, an absent number — pass
`bl.Null()` rather than a Go `*bool`/`nil`. It then flows through the language's
[three-valued logic](booleans-and-logic.md):

```go
// host-side (Go)
var unknown bl.BlValue = bl.Null()       // unknown, not false
```

## Collections

`bl.List` is variadic, preserves order, and accepts both individual values and a
spread slice. `bl.Dictionary` takes a `map[string]bl.BlValue` and sorts keys into
canonical order at construction, so the map's Go iteration order is irrelevant.

```go
// host-side (Go)
var prices = bl.List(bl.Number(100), bl.Number(150), bl.Number(75))

var names  = []bl.BlValue{bl.String("Alice"), bl.String("Bob")}
var roster = bl.List(names...)           // spread an existing slice
var empty  = bl.List()                   // the empty list

var applicant, _ = bl.Dictionary(map[string]bl.BlValue{
    "name":   bl.String("Alice"),
    "age":    bl.Number(30),
    "income": bl.Number(75000),
})
```

## Ranges

`bl.Range` takes two endpoints and two inclusion flags. Either endpoint may be
any comparable `bl.BlValue` — numbers, strings, temporal values — or `bl.Null()`
for an unbounded side.

```go
// host-side (Go)
// func Range(start, end bl.BlValue, startIncluded, endIncluded bool) (bl.BlRange, error)

var adultAges,  _ = bl.Range(bl.Number(18), bl.Number(120), true, true)   // [18..120]
var quarter,    _ = bl.Range(bl.Number(0), bl.Number(0.25), true, false)  // [0..0.25)
var workingAge, _ = bl.Range(bl.Number(18), bl.Null(), true, false)       // 18 and up
```

## Temporal values

Each temporal constructor accepts an ISO 8601 / RFC 9557 string, a Go
`time.Time`, or an explicit components struct. `bl.Today()` and `bl.Now()` are
shorthands for the current date and date-time.

```go
// host-side (Go)
var d,   _ = bl.Date("2025-03-28")                  // also bl.Today()
var t,   _ = bl.Time("11:45:30+01:00")
var dt,  _ = bl.DateTime(time.Now())                // also bl.Now()
var dur, _ = bl.DTDuration(bl.DHMS(1, 2, 30, 0))    // 1d 2h 30m
var age, _ = bl.YMDuration(bl.YM(1, 6))             // 1y 6m
```

A **calendar** is built from `bl.CalendarEntry` values, optionally scoped to a
validity range:

```go
// host-side (Go)
var ukHolidays, _ = bl.Calendar(
    []bl.BlCalendarEntry{
        bl.CalendarEntry(d, "Good Friday"),
    },
    bl.WithValidity(adultAges))   // any bl.BlRange of dates
```

Calendars can also be imported from RFC 5545 iCalendar (`.ics`) data with
`bl.ImportICal(...)` — the recommended path for holiday feeds. Host-side
mutation uses the `(bl.BlCalendar).Drop` / `.Keep` methods and the package-level
`bl.CalendarMerge`.

## Tables

`bl.Table(columns bl.Cols, rows ...bl.Row)` takes a typed header — column names
and types — then positional value rows:

```go
// host-side (Go)
var shippingRates, _ = bl.Table(
    bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}},
    //      region      rate
    bl.Row{"domestic",  5.99},
    bl.Row{"europe",    15.99},
)
```

## Typed environments

The variables an expression may reference are the exported fields of a Go
**environment struct**. Each field is renamed to its source-level name by an
`expr:"..."` tag and must hold a `bl.BlValue`. You compile against that struct
type with `bl.Expr[Env]`, then pass an instance to `Evaluate`:

```go
// host-side (Go)
type ApplicantEnv struct {
    Age    bl.BlNumber `expr:"age"`
    Income bl.BlNumber `expr:"income"`
}

var eligible, _ = bl.Expr[ApplicantEnv](`age >= 18 and income > 50000`)

var age,    _ = bl.Number(21)
var income, _ = bl.Number(60000)
var result, _ = eligible.Evaluate(ApplicantEnv{Age: age, Income: income})
```

A composite value binds to a single field — a `bl.BlDictionary` field is the
idiomatic way to hand a whole nested record in, and a `bl.BlTable` field exposes
a table the expression then operates on:

```go
// host-side (Go)
type ShipmentsEnv struct {
    Shipments bl.BlTable `expr:"shipments"`
}

var pricey, _ = bl.Expr[ShipmentsEnv](`shipments.filter(rate > 6).sort(desc("rate"))`)
var out,    _ = pricey.Evaluate(ShipmentsEnv{Shipments: shippingRates})
```

For a variable-free expression there is no env to build: compile with
`bl.ExprNoEnv` and evaluate against `bl.NoEnv{}`.

## Reading results back

`Evaluate` returns a `bl.BlValue`. Type-assert it to the concrete type and call
its native accessor to get an ordinary Go value:

```go
// host-side (Go)
var d = result.(bl.BlNumber).Decimal()    // shopspring/decimal value
var s = greeting.Native()                  // Go string from a bl.BlString
```

`bl.BlNumber` exposes `Decimal()` (then use the `shopspring/decimal` API —
`IntPart`, `Float64`, `StringFixed`, …); the other scalar types expose a
`Native()` accessor returning the corresponding Go value.

When you don't know a result's type ahead of time, switch on it. Every
`bl.BlValue` carries a `Type()` method returning a `bl.Type` tag — the host-side
mirror of the language's [`instance of`](overview.md#testing-a-values-type-instance-of)
test. Because the value set is closed, the switch can be exhaustive:

```go
// host-side (Go)
var v, _ = eligible.Evaluate(env)
switch v.Type() {
case bl.TypeNumber:
    // ...
case bl.TypeBoolean:
    // ...
case bl.TypeNull:
    // ...
}
```
