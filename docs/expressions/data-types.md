# Data Types

> The blkit value system — numbers, strings, booleans, null, dates and times,
> durations, lists, dictionaries, ranges, and tables — and their null-aware
> semantics.

Every blkit expression produces and consumes values drawn from one closed set of
types. A literal like `42` is a number; `"hello"` is a string; `[1, 2, 3]` is a
list; the result of `age >= 18` is a boolean. This page is the orientation map
for that value system: it lists every type, shows the literal syntax for each at
a glance, explains how `null` threads through everything, and covers how to test
a value's type with `instance of`. Each type then has its own guide page that
goes deep on its literals, operators, and built-in functions.

If you want to understand *why* the value system is shaped this way — exact
decimals, a sealed type set, three-valued logic — read
[Architecture → Expressions](../architecture/expressions.md) first. This page is
the user-facing tour.

## The complete set of value types

There are fifteen value types. Most have a literal you write directly in source;
a few are built only by a constructor function or by host Go code.

| Type | Literal / constructor | Example | Guide |
|---|---|---|---|
| number | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` | [Numbers](numbers.md) |
| string | double quotes | `"hello"` | [Strings](strings.md) |
| boolean | keywords | `true`, `false` | [Booleans & logic](booleans-and-logic.md) |
| null | keyword | `null` | this page |
| date | `date(...)` | `date("2025-03-28")` | [Dates & times](dates-and-times.md) |
| time | `time(...)` | `time("11:45:30+02:00")` | [Dates & times](dates-and-times.md) |
| datetime | `datetime(...)` | `datetime("2025-03-28T11:45:30")` | [Dates & times](dates-and-times.md) |
| days-time duration | `dtDuration(...)` | `dtDuration("P4DT12H")` | [Dates & times](dates-and-times.md) |
| years-months duration | `ymDuration(...)` | `ymDuration("P1Y6M")` | [Dates & times](dates-and-times.md) |
| list | `[ ... ]` | `[1, 2, 3]` | [Lists](lists.md) |
| dictionary | `{ ... }` | `{name: "Alice", age: 30}` | [Dictionaries](dictionaries.md) |
| range | interval notation | `[1..10]`, `(1..10)`, `[1..10)` | [Ranges](ranges.md) |
| table | `table(...)` / `tableFromDicts(...)` | `table(["a"], [1], [2])` | [Tables](tables.md) |
| regex | `pattern(...)` | `pattern("[0-9]+")` | [Strings](strings.md) |
| calendar | host-built (no constructor) | — | — |

A few notes that hold across the whole set:

- **Numbers are exact decimals, never floats.** Precision is preserved through
  every arithmetic operation, so money and percentages don't drift. See
  [Numbers](numbers.md).
- **The last three rows have no literal syntax.** A regex is built with
  `pattern(...)`, a table with `table(...)` / `tableFromDicts(...)`, and a
  calendar is constructed only by host Go code and then referenced as a
  variable. They still participate fully in the language — you can compare them,
  pass them to functions, and test them with `instance of`.
- **The set is closed.** Every expression result is one of these types; there is
  no "other". That is what makes null propagation and `instance of` total — every
  value has a well-defined type tag.

## Literal syntax at a glance

You can write most types directly as literals. Here is each form, with results
shown in the `expression  // → result` notation used throughout the guide.

### Numbers

```
// expression-language
42            // → 42
3.14          // → 3.14
-5            // → -5
1500.50       // → 1500.5
1.2e3         // → 1200      (scientific notation)
```

Integer and decimal literals are both exact. Hexadecimal literals (`0xff`) are
**not** supported. Full detail in [Numbers](numbers.md).

### Strings

```
// expression-language
"hello"               // → "hello"
"line1\nline2"        // → two lines
"quote: \""           // → quote: "
```

String literals use double quotes and are immutable Unicode sequences. See
[Strings](strings.md).

### Booleans and null

```
// expression-language
true                  // → true
false                 // → false
null                  // → null
```

`true`, `false`, and `null` are keywords. The casing of `null` is forgiving on
input — `null`, `Null`, and `NULL` all parse to the same value — but lowercase is
canonical on output. See [Booleans & logic](booleans-and-logic.md) and
[Null and propagation](#null-and-propagation) below.

### Temporal values and durations

Temporal values are created by constructor functions that accept either an
ISO 8601 string or numeric components.

```
// expression-language
date("2025-03-28")                  // → a date
date(2025, 3, 28)                   // → a date (numeric components)
time("11:45:30+02:00")              // → a time with offset
datetime("2025-03-28T11:45:30")     // → a datetime
dtDuration("P4DT12H")               // → days-time duration (4 days, 12 hours)
ymDuration("P1Y6M")                 // → years-months duration (1 year, 6 months)
```

There are two duration kinds because they measure fundamentally different
things: a days-time duration is an exact span of time, while a years-months
duration is a calendar span (a "month" has no fixed number of days). See
[Dates & times](dates-and-times.md).

### Lists and dictionaries

```
// expression-language
[1, 2, 3, 4]                        // → [1, 2, 3, 4]
[[1, 2], [3, 4]]                    // → nested lists
{}                                  // → empty dictionary
{name: "Alice", age: 30}            // → a dictionary
{"name": "Alice"}                   // → quoted keys are allowed too
```

Lists are ordered and heterogeneous; dictionaries are ordered maps of named
entries. See [Lists](lists.md) and [Dictionaries](dictionaries.md).

### Ranges

A range is a bounded interval. Each end is open `( )` or closed `[ ]`
independently.

```
// expression-language
[1..10]      // 1 to 10, both inclusive
(1..10)      // 1 to 10, both exclusive
[1..10)      // 1 inclusive, 10 exclusive
(1..10]      // 1 exclusive, 10 inclusive
```

Ranges work over numbers and ordered temporal values and are used for membership
tests with `in`. See [Ranges](ranges.md).

### Tables, regexes, and calendars

These three have no literal form. Build a table or regex with a constructor;
receive a calendar as an input variable from host code.

```
// expression-language
table(["a"], [1], [2])              // → a one-column table with two rows
pattern("[0-9]+")                   // → a precompiled regex
ukHolidays                          // → a calendar passed in by the host
```

See [Tables](tables.md) for `table(...)` / `tableFromDicts(...)`,
[Strings](strings.md) for `pattern(...)`, and the calendar specification under
`specs/expressions/calendar.spec.md` for the host-built calendar type.

## Passing values in from host Go code

Inside an expression you write literals; on the host side you construct the same
values with the `bl.` constructors that mirror each type — `bl.Number`,
`bl.String`, `bl.Boolean`, `bl.Null`, `bl.List`, `bl.Dictionary`, `bl.Date`, and
so on. The variables an expression may reference are the exported fields of a Go
env struct, each renamed to its source-level name by an `expr:"..."` tag.

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

type ApplicantEnv struct {
    Age    bl.BlNumber `expr:"age"`
    Income bl.BlNumber `expr:"income"`
}

var eligible, _ = bl.Expr[ApplicantEnv](`age >= 18 and income > 50000`)

var age, _    = bl.Number(21)
var income, _ = bl.Number(60000)
var result, _ = eligible.Evaluate(ApplicantEnv{Age: age, Income: income})
// result is the bl.BlBoolean true
```

The constructors return a typed `Bl*` value (`bl.BlNumber`, `bl.BlString`, …),
each implementing the sealed `bl.BlValue` interface. `Evaluate` always returns a
`bl.BlValue`; type-assert or inspect it to get the concrete value back.

The input bridge is also forgiving about native Go values: when the host passes a
plain `map[string]any`, the bridge wraps each value into the matching `Bl*` type
automatically.

| Native Go input | Wrapped as |
|---|---|
| `int`, `int64`, `float64`, `decimal.Decimal`, decimal `string` | `bl.BlNumber` |
| `string` | `bl.BlString` |
| `bool` | `bl.BlBoolean` |
| `[]any` | `bl.BlList` |
| `map[string]any` | `bl.BlDictionary` |
| `time.Time` | `bl.BlDate` / `bl.BlDateTime` (per precision) |
| `time.Duration` | `bl.BlDaysTimeDuration` |
| `nil` / absent input key | `bl.BlNull` |
| an already-`bl.BlValue` | itself |

Numbers stay arbitrary-precision decimals end to end — they are **never**
collapsed to `float64` inside the engine.

For a variable-free expression there is no env to build; compile with
`bl.ExprNoEnv` and evaluate against `bl.NoEnv{}`.

```go
// host-side (Go)
var sum, _ = bl.ExprNoEnv(`1 + 1`)
var two, _ = sum.Evaluate(bl.NoEnv{}) // the bl.BlNumber 2
```

## Null and propagation

`null` is the value that means "absent" or "unknown". It is what you get from a
missing dictionary key, an out-of-range list index, division by zero, or any
operation whose normal result is undefined. blkit follows SQL-style **three-valued
logic**: `null` flows through almost every operation, so a calculation built on a
missing input yields `null` rather than throwing or silently defaulting.

### Propagation

For most operations, if any operand is `null`, the result is `null`.

```
// expression-language
null + 1                  // → null
null * 2                  // → null
null + "x"                // → null   (string concatenation)
someDictionary.missingKey // → null   (missing key)
[1, 2][9]                 // → null   (out-of-range index)
null.foo                  // → null   (path on a null receiver)
```

Ordering comparisons against `null` are also `null`, because there is no defined
order:

```
// expression-language
null < 5                  // → null
null <= 5                 // → null
```

### Equality with null is always false

Equality is the one comparison that does **not** yield `null`. Following SQL,
`null` is never equal to anything — not even another `null`.

```
// expression-language
null = null               // → false
null != null              // → false
null = 5                  // → false
null != 5                 // → false
```

The practical consequence: **never write `x = null` to test for null** — it is
always `false`, so `if x = null then ...` never matches. Use `isNull(x)` or
`x instance of null` instead (see [Testing for null](#testing-for-null)).

### The boolean short-circuit exceptions

The only operators that do not propagate `null` are `and` and `or`, which
short-circuit to a definite answer whenever the non-null operand already settles
the result.

```
// expression-language
false and null            // → false   (short-circuit)
true  or  null            // → true    (short-circuit)

true  and null            // → null
null  and true            // → null
false or  null            // → null
null  or  false           // → null

not(null)                 // → null
```

The full three-valued truth tables live in
[Booleans & logic](booleans-and-logic.md).

### Testing for null

There are three idiomatic ways to test for null; all return the same answer.

| Form | Where it's used |
|---|---|
| `isNull(x)` | inside expressions — the briefer form, preferred in most code |
| `x instance of null` | inside expressions — reads naturally alongside other `instance of` calls |
| `v.IsNull()` (Go) | host code consuming an evaluated `bl.BlValue` |

```
// expression-language
isNull(someDictionary.missingKey)    // → true
isNull(0)                            // → false   (zero is a defined value)
isNull("")                           // → false   (empty string is a defined value)
```

```go
// host-side (Go)
var v, _ = eligible.Evaluate(env)
if v.IsNull() {
    // handle absent / unknown
}
```

Note the distinction between *absence* and *null*: `isNull` tests whether a
*present* value is the null value. To test whether a dictionary key is present at
all — which is different from a key present with a null value — use
`isDefined(d.key)`, covered in [Dictionaries](dictionaries.md).

### Supplying a fallback

To replace a `null` with a default, use `getOrElse` rather than an `if`.

```
// expression-language
getOrElse(null, 1)                   // → 1
getOrElse(42, 1)                     // → 42
getOrElse(applicant.middleName, "")  // → "" if the key is missing or null
```

`getOrElse` fires **only** on `null`. A defined-but-empty value — the empty
string `""`, the empty list `[]`, the number `0`, `false` — is returned as-is, not
treated as missing.

### Where null comes from

The engine produces `null` in these situations:

- A missing dictionary key (`d.absent`, `getValue(d, "absent")`).
- An out-of-range list index (`[1, 2][9]`, including negative indices that fall
  off the start).
- An arithmetic operation with a `null` operand.
- Division by zero (`1 / 0`, `5 / 0.0`).
- Numeric operations whose result is undefined — `sqrt` of a negative number,
  `ln`/`log` of zero-or-negative, a `**` whose result would be complex.
- A date/datetime comparison or subtraction that mixes naive and zoned operands.
- Any path expression whose receiver is `null` (`null.foo`, `null[0]`).

On the host side, the input bridge maps Go `nil`, an absent input-map key, an
untyped JSON `null`, and a nil pointer all to `bl.BlNull`, so an operator
implementation never sees a Go-level `nil`.

## Testing a value's type: `instance of`

`x instance of T` tests a value's type, where `T` is one of the type names from
the table above — including the non-literal types `regex`, `table`, and
`calendar`. It returns a `bl.BlBoolean`.

```
// expression-language
42 instance of number                  // → true
"x" instance of number                 // → false
date("2025-01-01") instance of date    // → true
pattern("[0-9]+") instance of regex    // → true
myTable instance of table              // → true
ukHolidays instance of calendar        // → true
null instance of null                  // → true
null instance of number                // → false
```

The type names are lowercase. `x instance of null` is equivalent to `isNull(x)`;
use whichever reads better in context.

On the host side, the same type tags are available as the `bl.Type` enum and via
the `Type()` method every `bl.BlValue` carries, so host code can switch on a
result's type exhaustively.

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

## A worked example: compile once, evaluate many

The value system is most useful in the typical blkit pattern — compile an
expression once, then evaluate it repeatedly against different inputs. A single
compiled `bl.BlExpr` is safe to reuse, and the expensive parsing and compilation
work happens exactly once.

```go
// host-side (Go)
type Order struct {
    Total    bl.BlNumber `expr:"total"`
    Currency bl.BlString `expr:"currency"`
}

// Compile once.
var discountable, _ = bl.Expr[Order](
    `total > 100 and currency = "GBP"`,
)

// Evaluate many times, against different values.
var t1, _ = bl.Number(150)
var c1, _ = bl.String("GBP")
var r1, _ = discountable.Evaluate(Order{Total: t1, Currency: c1}) // → true

var t2, _ = bl.Number(80)
var r2, _ = discountable.Evaluate(Order{Total: t2, Currency: c1}) // → false
```

If `total` arrives as a missing value, the boolean logic stays well-defined:
`null > 100` is `null`, and `null and currency = "GBP"` short-circuits to `null`
rather than erroring — so a downstream `if` treats the rule as not matched
instead of crashing.

## Where to go next

Each type's dedicated guide goes deep on its literals, operators, and built-in
functions:

- [Numbers](numbers.md) — exact decimals, arithmetic, the numeric library.
- [Strings](strings.md) — text, concatenation, inspection, and `pattern(...)`.
- [Booleans & logic](booleans-and-logic.md) — the three-valued truth tables.
- [Dates & times](dates-and-times.md) — dates, times, datetimes, and both
  duration kinds.
- [Lists](lists.md) — indexing, filtering, projection, comprehensions.
- [Dictionaries](dictionaries.md) — entries, path access, key presence.
- [Ranges](ranges.md) — intervals, membership, interval algebra.
- [Tables](tables.md) — rows, columns, transformation methods.

For the engine that compiles and runs all of this, see
[Architecture → Expressions](../architecture/expressions.md). The exhaustive,
authoritative detail lives in the type specifications under
`specs/expressions/` (notably `bl-expr.spec.md` for the engine and value system,
and `null.spec.md` for null semantics), and the generated API listing is in the
[Reference](../reference/blkit.md).
