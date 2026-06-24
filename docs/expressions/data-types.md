# Data Types

> The blkit value system — numbers, strings, booleans, null, dates and times,
> durations, lists, dictionaries, ranges, and tables — its literal syntax, and
> how typed environments feed values into expressions.

Every blkit expression produces and consumes values drawn from one closed set of
types. A literal like `42` is a number; `"hello"` is a string; `[1, 2, 3]` is a
list; the result of `age >= 18` is a boolean. This page is the orientation map
for that value system: it lists every type, shows the literal syntax for each at
a glance, and covers how to test a value's type with `instance of`. Each type
then has its own guide page that goes deep on its literals, operators, and
built-in functions.

blkit defines its own closed set of value types. Most have a literal you write
directly in source; a few are built only by a constructor function or by host Go
code.

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
| datetime (date and time) | `datetime(...)` | `datetime("2025-03-28T11:45:30")` | [Dates & times](dates-and-times.md) |
| days-time duration | `dtDuration(...)` | `dtDuration("P4DT12H")` | [Dates & times](dates-and-times.md) |
| years-months duration | `ymDuration(...)` | `ymDuration("P1Y6M")` | [Dates & times](dates-and-times.md) |
| list | `[ ... ]` | `[1, 2, 3]` | [Lists](lists.md) |
| dictionary | `{ ... }` | `{name: "Alice", age: 30}` | [Dictionaries](dictionaries.md) |
| range | interval notation | `[1..10]`, `(1..10)`, `[1..10)` | [Ranges](ranges.md) |
| table | `table(...)` / `tableFromDicts(...)` | `table(["a"], [1], [2])` | [Tables](tables.md) |
| regex | `pattern(...)` | `pattern("[0-9]+")` | [Strings](strings.md) |
| calendar | host-built (no constructor) | — | — |

The **table** type is blkit-specific: a tabular value with named columns and rows.

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
  no "other". That is what makes `instance of` total — every value has a
  well-defined type tag.

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

`true`, `false`, and `null` are keywords. See
[Booleans & logic](booleans-and-logic.md).

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

Lists are ordered and heterogeneous; dictionaries are ordered
maps of named entries. See [Lists](lists.md) and [Dictionaries](dictionaries.md).

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

See [Tables](tables.md) for `table(...)` / `tableFromDicts(...)` and
[Strings](strings.md) for `pattern(...)`.

## Referencing typed environment fields

The variables an expression may reference are supplied by a typed environment.
A name written bare in an expression — `age`, `currency`, `ukHolidays` — resolves
to a field of that environment, each carrying its own value type.

```
// expression-language
age >= 18 and income > 50000        // age, income are number fields
currency = "GBP"                    // currency is a string field
total in [100..500]                 // total is a number field
```

How the environment is declared on the host side — and how each field is named —
is covered in the short Go section at the end of this page.

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

The type names are lowercase.

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

## A type index

Each type's dedicated guide goes deep on its literals, operators, and built-in
functions:

- [Numbers](numbers.md) — exact decimals, arithmetic, the numeric library.
- [Strings](strings.md) — text, concatenation, inspection, and `pattern(...)`.
- [Booleans & logic](booleans-and-logic.md) — the truth tables.
- [Dates & times](dates-and-times.md) — dates, times, datetimes, and both
  duration kinds.
- [Lists](lists.md) — indexing, filtering, projection, comprehensions.
- [Dictionaries](dictionaries.md) — entries, path access, key presence.
- [Ranges](ranges.md) — intervals, membership, interval algebra.
- [Tables](tables.md) — rows, columns, transformation methods.
- [Values from Go](values-from-go.md) — building these values in host Go code.

## Values from Go

On the host side you construct values with the `bl.*` constructors that mirror
each type (`bl.Number`, `bl.String`, `bl.Date`, …), declare the variables an
expression may reference as the `expr:"..."`-tagged fields of a Go env struct,
and pass an instance of that struct to `Evaluate`. See
[Values from Go](values-from-go.md) for the full host-side story.
