---
name: FeelEngine
description: The blkit expression language — a string-based expression syntax (based on DMN FEEL) parsed and evaluated into Bl* values. Covers the data types, operators, control flow, unary tests, and built-in function library, with the runtime built on github.com/expr-lang/expr.
targets:
  - ../expr/feel_engine.go
---

# The blkit expression language

This spec defines **blkit's string expression language**: a compact, business-readable syntax for
writing expressions as text — `age >= 18 and income > 50000`, `sum(line items.amount)`,
`if score >= 700 then "approve" else "review"` — that evaluate to blkit value types (`BlNumber`,
`BlString`, `BlList`, …, see [bl.spec.md](bl.spec.md)).

The language is based on **DMN 1.4 FEEL** (Friendly Enough Expression Language) and reproduces its
syntax and semantics as the v1 baseline. It is the string-based counterpart to the programmatic
`Bl.*` builder API in [bl.spec.md](bl.spec.md): both produce the same `BlValue` results, but this
one is authored as text and parsed at runtime. The runtime is built on
[`github.com/expr-lang/expr`](https://github.com/expr-lang/expr) — see
[§ Implementation](#implementation).

Throughout, examples use the notation `expression  // → result`, where the result is shown in the
same source syntax (e.g. a `BlNumber` renders as `42`, a `BlString` as `"foo"`).

---

## Using the engine

```go
// Expr compiles a source string once, optionally type-checking it against a
// declared environment. The returned BlExpression can be evaluated repeatedly.
func (Bl) Expr(source string, env BlEnv) (BlExpression, error)

// Eval parses and evaluates in one call. Prefer Expr when reusing an expression.
func (Bl) Eval(source string, input map[string]BlValue) (BlValue, error)

// BlEnv declares the variable names and types an expression may reference, so
// type errors surface at parse time. Pass nil to skip static checking.
type BlEnv map[string]BlType

// BlExpression is a compiled source-text expression.
type BlExpression interface {
    Evaluate(input map[string]BlValue) (BlValue, error)
    Source() string
    ToMarkdown() string
}
```

`BlExpression` (a compiled *string* expression) is distinct from `BlExpr` (the programmatic deferred
tree in [bl-expr.spec.md](bl-expr.spec.md)); they are not interchangeable, though both evaluate to a
`BlValue`.

```go
var env = BlEnv{"age": BlTypeNumber, "income": BlTypeNumber}
var eligible, _ = Bl.Expr("age >= 18 and income > 50000", env)

result, err := eligible.Evaluate(map[string]BlValue{
    "age":    Bl.Number(21),
    "income": Bl.Number(60000),
})
// result is BlBoolean.TRUE
```

`[@test] ../expr/feel_engine_test.go`

---

## Data types

Every value belongs to one of the following types. Each has a literal or constructor syntax.

| Type | `Bl*` type | Literal / constructor | Example |
|---|---|---|---|
| number | `BlNumber` | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` |
| string | `BlString` | double quotes | `"hello"` |
| boolean | `BlBoolean` | keywords | `true`, `false` |
| date | `BlDate` | `date(...)` | `date("2025-03-28")` |
| time | `BlTime` | `time(...)` | `time("11:45:30")`, `time("11:45:30+02:00")` |
| date-time | `BlDateTime` | `datetime(...)` | `datetime("2025-03-28T11:45:30")` |
| days-time duration | `BlDaysTimeDuration` | `duration(...)` | `duration("P4DT12H")` |
| years-months duration | `BlYearsMonthsDuration` | `duration(...)` | `duration("P1Y6M")` |
| list | `BlList` | `[ ... ]` | `[1, 2, 3]` |
| context | `BlContext` | `{ ... }` | `{name: "Alice", age: 30}` |
| range | `BlRange` | interval notation | `[1..10]`, `(1..10)`, `[1..10)` |
| null | `BlNull` | keyword | `null` |

- **Numbers** are arbitrary-precision decimals — never floats; precision is preserved through all
  arithmetic (see [number.spec.md](number.spec.md)).
- The blkit-specific types `BlCalendar` ([calendar.spec.md](calendar.spec.md)) and `BlTable`
  ([table.spec.md](table.spec.md)) have **no literal syntax** in v1; they are produced
  programmatically and may be referenced as variables.

`[@test] ../expr/feel_data_types_test.go`

### Numbers

```
42            // → 42
3.14          // → 3.14
-5            // → -5
1500.50       // → 1500.5
```

Scientific notation (`1.2e3`) is also accepted. Hexadecimal literals (`0xff`) are a Drools/KIE
vendor extension and are **not** supported.

### Strings

String literals use double quotes. Strings are immutable Unicode sequences
([string.spec.md](string.spec.md)).

```
"hello"               // → "hello"
"line1\nline2"        // → two lines
"quote: \""           // → quote: "
```

### Temporal values

Temporal values are created with constructor functions, accepting either an ISO 8601 string or
numeric components (see [§ Conversion functions](#conversion-functions)).

```
date("2025-03-28")                       // → a date
date(2025, 3, 28)                        // → a date
time("11:45:30+02:00")                   // → a time with offset
datetime("2025-03-28T11:45:30")          // → a date-time
duration("P4DT12H")                      // → days-time duration (4 days, 12 hours)
duration("P1Y6M")                        // → years-months duration (1 year, 6 months)
```

---

## Variables

A bare name references an input variable. Names follow conventional identifier rules — letters,
digits, and underscores, **no spaces** — and are case-sensitive. (This is a deliberate divergence
from FEEL, which allows multi-word names with embedded spaces; it also applies to built-in function
names. See [§ Relationship to FEEL](#relationship-to-feel-and-future-direction).)

```
age                   // value of the `age` variable
loanAmount * 12       // identifier used in arithmetic
```

Variables are supplied at evaluation time via the `input` map and, when an expression is compiled
with a `BlEnv`, are type-checked at parse time. A reference to a name that is neither in scope nor
declared is a parse error (see [§ Errors and null](#errors-and-null)).

`[@test] ../expr/feel_variables_test.go`

---

## Arithmetic

The operators `+`, `-`, `*`, `/`, `**` (exponent), and unary `-` operate on numbers, with decimal
precision preserved. `+` and `-` also apply to temporal/duration combinations.

```
2 + 3                              // → 5
10 - 4                             // → 6
3 * 4                              // → 12
10 / 4                             // → 2.5
2 ** 8                             // → 256
-(7)                               // → -7

date("2025-01-01") + duration("P1Y")    // → date("2026-01-01")
duration("P1D") + duration("PT12H")      // → duration("P1DT12H")
```

`null` propagates: `null + 1 // → null`. Division by zero yields `null`.

`[@test] ../expr/feel_arithmetic_test.go`

---

## Comparison

The operators `<`, `<=`, `>`, `>=`, `=`, `!=` compare numbers, strings, and temporal/duration
values; `=` and `!=` apply to all types. `x between a and b` is shorthand for `x >= a and x <= b`.
The result is a `BlBoolean` (or `null` when operands are incomparable).

```
5 < 10                             // → true
10 >= 10                           // → true
"abc" = "abc"                      // → true
date("2025-01-01") < date("2025-06-01")   // → true
5 between 1 and 10                 // → true
```

`[@test] ../expr/feel_comparison_test.go`

---

## Boolean logic

`and`, `or`, and `not(...)` use **three-valued logic** — `true`, `false`, and `null` — matching
[boolean.spec.md](boolean.spec.md) and [null.spec.md](null.spec.md). `and`/`or` short-circuit so a
definite result is returned even when the other operand is `null`.

```
true and false                    // → false
true or false                     // → true
not(true)                         // → false

false and null                    // → false   (short-circuit)
true or null                      // → true    (short-circuit)
true and null                     // → null
null or false                     // → null
```

`[@test] ../expr/feel_boolean_logic_test.go`

---

## String expressions

Strings concatenate with `+`. Concatenation is **string-only**: to join a non-string value, convert
it first with `string(...)`.

```
"foo" + "bar"                     // → "foobar"
"order-" + string(123)            // → "order-123"
```

Inspection and transformation use the [§ String functions](#string-functions).

```
upperCase("aBc")                 // → "ABC"
contains("foobar", "oob")         // → true
substring("foobar", 3, 2)         // → "ob"
```

`[@test] ../expr/feel_string_expressions_test.go`

---

## Conditional

`if c then a else b` evaluates `a` when `c` is `true`, otherwise `b`. A `null` or non-boolean
condition takes the `else` branch. The two branches may have different types; the result type is
their union.

```
if 5 < 10 then "low" else "high"           // → "low"
if 12 < 10 then "low" else "high"          // → "high"
if null then "low" else "high"             // → "high"

if age >= 18 then "adult" else "minor"     // depends on `age`
```

Conditionals nest:

```
if score >= 750 then "prime"
else if score >= 650 then "standard"
else "subprime"
```

`[@test] ../expr/feel_conditional_test.go`

---

## Ranges and intervals

A range is a bounded interval with open `( )` or closed `[ ]` boundaries on each side. Ranges are
used for membership tests (with `in`) and the [§ Range functions](#range-functions). See
[range.spec.md](range.spec.md).

```
[1..10]      // 1 to 10, both inclusive
(1..10)      // 1 to 10, both exclusive
[1..10)      // 1 inclusive, 10 exclusive
(1..10]      // 1 exclusive, 10 inclusive
```

Ranges work over numbers and ordered temporal values:

```
[date("2025-01-01")..date("2025-12-31")]
```

`[@test] ../expr/feel_ranges_test.go`

---

## Membership: the `in` operator

`x in y` tests whether `x` is a member of a list or falls within a range.

```
5 in [1, 2, 3, 4, 5]               // → true
3 in [1..10]                       // → true
10 in [1..10)                      // → false   (upper bound exclusive)
"US" in ["US", "CA", "MX"]         // → true
```

`[@test] ../expr/feel_membership_test.go`

---

## Unary tests

A **unary test** is a condition evaluated against an implicit input value. Unary tests are how
decision-table input entries are written (see
[decision-table.spec.md](../decision-tasks/decision-table.spec.md)); the tested column value is
supplied implicitly.

| Form | Matches when input… | Example |
|---|---|---|
| value | equals the value | `"valid"` |
| `<` `<=` `>` `>=` | compares true | `< 10`, `>= date("2020-04-06")` |
| interval | falls in the range | `[2..5]`, `(2..5]` |
| comma list | equals any (disjunction) | `2, 3, 4`, `< 10, > 50` |
| `not(...)` | does **not** match the inner test | `not("valid")`, `not(2, 3)` |
| `?` expression | the expression (with `?` = input) is true | `contains(?, "good")`, `endsWith(?, "@blkit.io")` |
| `-` | always (wildcard) | `-` |

```
< 10                  // input < 10
[18..65]              // 18 <= input <= 65
"low", "medium"       // input = "low" or input = "medium"
not(0)                // input != 0
contains(?, "urgent") // the input string contains "urgent"
-                     // matches anything
```

`[@test] ../expr/feel_unary_tests_test.go`

---

## Lists

### Literals and indexing

Lists are ordered and heterogeneous. Indexing is **1-based**; negative indexes count from the end;
an out-of-range index yields `null`.

```
[1, 2, 3, 4]                       // → [1, 2, 3, 4]
[1, 2, 3, 4][1]                    // → 1     (first element)
[1, 2, 3, 4][-1]                   // → 4     (last element)
[1, 2, 3, 4][5]                    // → null  (out of range)
[[1, 2], [3, 4]]                   // nested lists
```

### Filtering

`a[c]` filters list `a` by condition `c`. Inside the filter, the current element is bound to
`item`.

```
[1, 2, 3, 4][item > 2]             // → [3, 4]
[1, 2, 3, 4][even(item)]           // → [2, 4]
```

### Projection

Accessing a field on a list of contexts projects that field across every element.

```
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].name   // → ["Alice", "Bob"]
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].age     // → [30, 34]
```

List operations are covered by the [§ List functions](#list-functions).

`[@test] ../expr/feel_list_expressions_test.go`

---

## Loops: `for … return`

`for x in xs return e` evaluates `e` once per element of `xs`, collecting the results into a new
list.

```
for x in [1, 2, 3] return x * 2          // → [2, 4, 6]
```

**Multiple iterators** produce the cartesian product, iterating the rightmost fastest:

```
for x in [1, 2], y in [3, 4] return x * y    // → [3, 4, 6, 8]
```

A loop may iterate a numeric range:

```
for x in 0..8 return 2 ** x              // → [1, 2, 4, 8, 16, 32, 64, 128, 256]
```

The keyword **`partial`** refers to the list of results accumulated so far, enabling running
computations:

```
for i in 1..10 return if i <= 2 then 1 else partial[-1] + partial[-2]
// → [1, 1, 2, 3, 5, 8, 13, 21, 34, 55]   (Fibonacci)
```

Each loop result is a `BlList`.

`[@test] ../expr/feel_for_test.go`

---

## Quantified expressions: `some` / `every`

`some x in xs satisfies c` is `true` when `c` holds for at least one element; `every x in xs
satisfies c` is `true` when `c` holds for all of them.

```
some x in [1, 2, 3] satisfies x > 2           // → true
every x in [1, 2, 3] satisfies x >= 1         // → true
every x in [1, 2, 3] satisfies x > 2          // → false

some order in orders satisfies order.total > 1000
```

`[@test] ../expr/feel_quantified_test.go`

---

## Contexts

A context is an ordered map of named entries. Keys may be unquoted names or strings; values may be
any type. Later entries can reference earlier ones in the same literal.

```
{}                                 // → empty context
{a: 1, b: 2}                       // → {a: 1, b: 2}
{"a": 1, "b": 2}                   // → {a: 1, b: 2}   (quoted keys)
{a: 1, b: {c: 2}}                  // → nested context
{a: 2, b: a * 2}                   // → {a: 2, b: 4}   (b references a)
```

### Path access

The dot operator navigates into a context. Chains traverse nested contexts; a missing key yields
`null`.

```
{a: 2}.a                           // → 2
{a: {b: 3}}.a.b                    // → 3
{a: 1}.b                           // → null
applicant.address.postcode         // navigate input variables
```

See [context.spec.md](context.spec.md). Context manipulation uses the
[§ Context functions](#context-functions).

`[@test] ../expr/feel_contexts_test.go`

---

## Accessing components

The dot operator also reads named **components** of temporal, duration, and range values — not just
context entries. This is the standard FEEL way to pull a field out of a date, time, duration, or
range.

```
date("2025-03-28").year            // → 2025
date("2025-03-28").month           // → 3
date("2025-03-28").day             // → 28
date("2025-03-28").weekday         // → 5    (Friday; Monday = 1)

time("11:45:30+02:00").hour        // → 11
time("11:45:30+02:00").minute      // → 45
time("11:45:30+02:00").timeOffset // → duration("PT2H")

duration("P1DT2H3M4S").days        // → 1
duration("P1DT2H3M4S").hours       // → 2
duration("P1DT2H3M4S").minutes     // → 3
duration("P1Y6M").years            // → 1
duration("P1Y6M").months           // → 6

[1..10].start                      // → 1
[1..10].end                        // → 10
[1..10).endIncluded               // → false
```

Available components follow the relevant `Bl*` type spec (e.g. [date.spec.md](date.spec.md),
[range.spec.md](range.spec.md)).

`[@test] ../expr/feel_components_test.go`

---

## Function invocation

Functions (built-ins or in-scope functions) are invoked with **positional** or **named**
arguments.

```
substring("foobar", 3, 2)                                  // positional → "ob"
substring(string: "foobar", startPosition: 3, length: 2)  // named → "ob"
```

### User-defined functions

FEEL inline functions have the form `function(params) body`:

```
function(x, y) x + y
sort([3, 1, 2], function(a, b) a < b)      // → [1, 2, 3]
```

> **Scope note.** User-defined inline functions are a candidate for **deferral** in v1 (sandboxing
> and typing implications). If excluded initially, `sort` and similar higher-order builtins accept a
> restricted comparator form. This is the one open scoping decision; see
> [§ Relationship to FEEL](#relationship-to-feel-and-future-direction).

`[@test] ../expr/feel_functions_test.go`

---

## Type checking: `instance of`

`x instance of T` tests a value's type, where `T` is a type name from
[§ Data types](#data-types). Returns a `BlBoolean`.

```
42 instance of number              // → true
"x" instance of number             // → false
date("2025-01-01") instance of date    // → true
```

`[@test] ../expr/feel_instance_of_test.go`

---

## Built-in function library

Each built-in maps to the corresponding `Bl*` behaviour (the same operations the programmatic API
exposes as methods — see the coverage table in [bl.spec.md](bl.spec.md)). Signatures below use
`name(arg: type): returnType`.

### Conversion functions

| Function | Example | Result |
|---|---|---|
| `string(from: Any): string` | `string(1.1)` | `"1.1"` |
| `number(from, groupingSeparator, decimalSeparator): number` | `number("1 500.5", " ", ".")` | `1500.5` |
| `date(from) / date(year, month, day): date` | `date(2012, 12, 25)` | `date("2012-12-25")` |
| `time(from) / time(hour, minute, second): time` | `time(23, 59, 0)` | `time("23:59:00")` |
| `datetime(from) / datetime(date, time)` | `datetime(d, t)` | a date-time |
| `duration(from): duration` | `duration("P5D")` | days-time duration |
| `yearsAndMonthsDuration(from: date, to: date)` | `yearsAndMonthsDuration(date("2011-12-22"), date("2013-08-24"))` | `duration("P1Y8M")` |

`[@test] ../expr/feel_fn_conversion_test.go`

### Boolean functions

| Function | Example | Result |
|---|---|---|
| `not(negand: boolean): boolean` | `not(true)` | `false` |
| `isDefined(value: Any): boolean` | `isDefined(null)` | `true` (the value exists, it is null) |
| `getOrElse(value, default): Any` | `getOrElse(null, 1)` | `1` |

`[@test] ../expr/feel_fn_boolean_test.go`

### String functions

| Function | Example | Result |
|---|---|---|
| `substring(string, start[, length])` | `substring("foobar", 3, 3)` | `"oba"` |
| `stringLength(string)` | `stringLength("foo")` | `3` |
| `upperCase(string)` | `upperCase("aBc4")` | `"ABC4"` |
| `lowerCase(string)` | `lowerCase("aBc4")` | `"abc4"` |
| `substringBefore(string, match)` | `substringBefore("foobar", "bar")` | `"foo"` |
| `substringAfter(string, match)` | `substringAfter("foobar", "ob")` | `"ar"` |
| `contains(string, match)` | `contains("foobar", "of")` | `false` |
| `startsWith(string, match)` | `startsWith("foobar", "fo")` | `true` |
| `endsWith(string, match)` | `endsWith("foobar", "r")` | `true` |
| `matches(input, pattern[, flags])` | `matches("FooBar", "foo", "i")` | `true` |
| `replace(input, pattern, replacement[, flags])` | `replace("How do you feel?", "feel", "FEEL", "i")` | `"How do you FEEL?"` |
| `split(string, delimiter)` | `split("John Doe", "\s")` | `["John", "Doe"]` |
| `extract(string, pattern)` | `extract("refs 1234, 1256", "12[0-9]*")` | `["1234", "1256"]` |
| `trim(string)` | `trim("  hi  ")` | `"hi"` |
| `stringJoin(list[, delimiter[, prefix, suffix]])` | `stringJoin(["a","b","c"], ", ")` | `"a, b, c"` |
| `isBlank(string)` | `isBlank("")` | `true` |

`[@test] ../expr/feel_fn_string_test.go`

### Numeric functions

| Function | Example | Result |
|---|---|---|
| `decimal(n, scale)` | `decimal(1/3, 2)` | `0.33` |
| `floor(n[, scale])` | `floor(-1.56, 1)` | `-1.6` |
| `ceiling(n[, scale])` | `ceiling(-1.56, 1)` | `-1.5` |
| `roundUp(n, scale)` | `roundUp(5.5, 0)` | `6` |
| `roundDown(n, scale)` | `roundDown(5.5, 0)` | `5` |
| `roundHalfUp(n, scale)` | `roundHalfUp(5.5, 0)` | `6` |
| `roundHalfDown(n, scale)` | `roundHalfDown(5.5, 0)` | `5` |
| `abs(number)` | `abs(-10)` | `10` |
| `modulo(dividend, divisor)` | `modulo(12, 5)` | `2` |
| `sqrt(number)` | `sqrt(16)` | `4` |
| `log(number)` | `log(10)` | `2.302585…` |
| `exp(number)` | `exp(5)` | `148.413…` |
| `odd(number)` | `odd(5)` | `true` |
| `even(number)` | `even(2)` | `true` |

`[@test] ../expr/feel_fn_numeric_test.go`

### List functions

| Function | Example | Result |
|---|---|---|
| `listContains(list, element)` | `listContains([1,2,3], 2)` | `true` |
| `count(list)` | `count([1,2,3])` | `3` |
| `min(list)` / `max(list)` | `max([1,2,3])` | `3` |
| `sum(list)` / `product(list)` | `sum([1,2,3])` | `6` |
| `mean(list)` / `median(list)` | `median([6,1,2,3])` | `2.5` |
| `stddev(list)` / `mode(list)` | `mode([6,1,9,6,1])` | `[1, 6]` |
| `all(list)` / `any(list)` | `all([true,false])` | `false` |
| `sublist(list, start[, length])` | `sublist([1,2,3], 2)` | `[2, 3]` |
| `append(list, items…)` | `append([1], 2, 3)` | `[1, 2, 3]` |
| `concatenate(lists…)` | `concatenate([1,2],[3])` | `[1, 2, 3]` |
| `insertBefore(list, position, item)` | `insertBefore([1,3], 1, 2)` | `[2, 1, 3]` |
| `remove(list, position)` | `remove([1,2,3], 2)` | `[1, 3]` |
| `insertAfter(list, position, item)` | `insertAfter([1,3], 1, 2)` | `[1, 2, 3]` |
| `listReplace(list, position, newItem)` | `listReplace([1,2,3], 2, 9)` | `[1, 9, 3]` |
| `listReplace(list, match, newItem)` | `listReplace([2,4,7], function(i) i<5, 5)` | `[5, 5, 7]` |
| `intersection(lists…)` | `intersection([1,2],[2,3])` | `[2]` |
| `reverse(list)` | `reverse([1,2,3])` | `[3, 2, 1]` |
| `indexOf(list, match)` | `indexOf([1,2,3,2], 2)` | `[2, 4]` |
| `union(lists…)` | `union([1,2],[2,3])` | `[1, 2, 3]` |
| `distinctValues(list)` | `distinctValues([1,2,3,2,1])` | `[1, 2, 3]` |
| `duplicateValues(list)` | `duplicateValues([1,2,3,2,1])` | `[1, 2]` |
| `flatten(list)` | `flatten([[1,2],[[3]],4])` | `[1, 2, 3, 4]` |
| `sort(list, precedes)` | `sort([3,1,2], function(x,y) x < y)` | `[1, 2, 3]` |
| `isEmpty(list)` | `isEmpty([])` | `true` |
| `partition(list, size)` | `partition([1,2,3,4,5], 2)` | `[[1,2],[3,4],[5]]` |

`[@test] ../expr/feel_fn_list_test.go`

### Context functions

| Function | Example | Result |
|---|---|---|
| `getValue(context, key)` | `getValue({foo: 123}, "foo")` | `123` |
| `getValue(context, keys)` | `getValue({x:1, y:{z:0}}, ["y","z"])` | `0` |
| `getEntries(context)` | `getEntries({foo: 123})` | `[{key: "foo", value: 123}]` |
| `contextPut(context, key, value)` | `contextPut({x:1}, "y", 2)` | `{x:1, y:2}` |
| `contextPut(context, keys, value)` | `contextPut({x:1, y:{z:0}}, ["y","z"], 2)` | `{x:1, y:{z:2}}` |
| `contextMerge(contexts)` | `contextMerge([{x:1},{y:2}])` | `{x:1, y:2}` |

`[@test] ../expr/feel_fn_context_test.go`

### Temporal functions

| Function | Example | Result |
|---|---|---|
| `now()` | `now()` | current date-time |
| `today()` | `today()` | current date |
| `dayOfWeek(date)` | `dayOfWeek(date("2019-09-17"))` | `"Tuesday"` |
| `dayOfYear(date)` | `dayOfYear(date("2019-09-17"))` | `260` |
| `weekOfYear(date)` | `weekOfYear(date("2019-09-17"))` | `38` |
| `monthOfYear(date)` | `monthOfYear(date("2019-09-17"))` | `"September"` |
| `abs(duration)` | `abs(duration("-PT5H"))` | `duration("PT5H")` |
| `lastDayOfMonth(date)` | `lastDayOfMonth(date("2022-10-01"))` | `date("2022-10-31")` |
| `is(value1, value2)` | `is(date("2012-12-25"), time("12:00:00"))` | `false` (same value *and* type) |

`[@test] ../expr/feel_fn_temporal_test.go`

### Range functions

These test the relative position of points and ranges (Allen's interval algebra), per
[range.spec.md](range.spec.md).

| Function | Example | Result |
|---|---|---|
| `before(a, b)` | `before([1..5], [6..10])` | `true` |
| `after(a, b)` | `after([6..10], [1..5])` | `true` |
| `meets(a, b)` / `metBy(a, b)` | `meets([1..5], [5..10])` | `true` |
| `overlaps(a, b)` | `overlaps([5..10], [1..6])` | `true` |
| `overlapsBefore(a, b)` / `overlapsAfter(a, b)` | `overlapsBefore([1..5], [4..10])` | `true` |
| `includes(range, point)` / `during(point, range)` | `during(5, [1..10])` | `true` |
| `starts` / `startedBy` | `starts(1, [1..5])` | `true` |
| `finishes` / `finishedBy` | `finishes(5, [1..5])` | `true` |
| `coincides(a, b)` | `coincides([1..5], [1..5])` | `true` |

`[@test] ../expr/feel_fn_range_test.go`

### Vendor extensions

Beyond the DMN-standard library, the language includes widely-used vendor extensions, **clearly
labelled as extensions** (the DMN spec does not define them). Vendor docs (Trisotech, Drools/KIE)
flag these as "not yet standardized".

Temporal arithmetic extensions (Trisotech), mirroring the `BlDate`/`BlDateTime` methods already in
[bl.spec.md](bl.spec.md):

| Function | Example | Result |
|---|---|---|
| `dateAdd(date, duration)` | `dateAdd(date("2025-01-31"), duration("P1M"))` | `date("2025-02-28")` |
| `timeAdd(time, duration)` | `timeAdd(time("10:00:00"), duration("PT90M"))` | `time("11:30:00")` |
| `datetimeAdd(datetime, duration)` | `datetimeAdd(datetime("2025-01-01T00:00:00"), duration("P1DT1H"))` | `datetime("2025-01-02T01:00:00")` |
| `dateDiff(date1, date2)` | `dateDiff(date("2025-01-10"), date("2025-01-01"))` | `9` (days) |

> Exact signatures are pinned to the Trisotech reference during implementation. blkit's own
> calendar-aware / business-day arithmetic ([calendar.spec.md](calendar.spec.md)) is exposed on the
> `Bl*` types programmatically and is **not** part of the v1 expression grammar.

List extensions (Trisotech, flagged non-standard): `insertAfter`, `intersection`, and the
predicate forms of `remove` / `listReplace` — listed in [§ List functions](#list-functions).

`[@test] ../expr/feel_fn_extensions_test.go`

---

## Operator precedence

From lowest to highest binding:

1. `or`
2. `and`
3. comparison — `< <= > >= = !=`, `between`, `in`, `instance of`
4. additive — `+`, `-`
5. multiplicative — `*`, `/`
6. exponent — `**`
7. unary — `-`, `not(...)`
8. postfix — path (`.`), filter/index (`[ ]`), invocation (`( )`)

Parentheses `( )` group sub-expressions explicitly.

```
2 + 3 * 4             // → 14    (* binds tighter than +)
(2 + 3) * 4           // → 20
a or b and c          // → a or (b and c)
```

`[@test] ../expr/feel_precedence_test.go`

---

## Errors and null

- **`null` propagation** — most operations involving `null` produce `null`
  ([null.spec.md](null.spec.md)); the exceptions are the short-circuit boolean cases (see
  [§ Boolean logic](#boolean-logic)).
- **Missing context key** → `null`, not an error.
- **Division by zero** → `null`.
- **Parse / type-check errors** — malformed syntax, an unknown variable (when a `BlEnv` is given),
  or a static type mismatch — are returned by `Bl.Expr` as a `BlParseError`.
  `[@test] ../expr/feel_parse_error_test.go`
- **Evaluation errors** — a type mismatch only detectable with concrete inputs (e.g. comparing
  incompatible types) — are returned by `Evaluate` as a `BlTypeError`.
  `[@test] ../expr/feel_eval_error_test.go`

---

## Implementation

The engine is built on [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which
provides lexing, parsing, optional static type-checking, compilation, and a sandboxed evaluation VM.
The language surface defined above is realised by configuring `expr`:

- **Built-in functions** are registered as `expr` functions that construct/operate on `Bl*` values.
- **Operators** (`+ - * / **`, comparisons, boolean connectives) are overloaded so they carry
  `Bl*` semantics — arbitrary-precision decimals, temporal/duration arithmetic, and three-valued
  logic — rather than `expr`'s native Go behaviour.
- **The `BlEnv`** is translated into an `expr` environment for parse-time type-checking.
- **FEEL constructs `expr` does not natively have** — ranges with open/closed boundaries, unary
  tests, `for…return`, `some/every…satisfies`, and `if…then…else` — are produced by rewriting the
  parsed syntax tree before compilation (an `expr` patcher / AST visitor). Forbidding spaces in
  identifiers (see [§ Relationship to FEEL](#relationship-to-feel-and-future-direction)) removes
  what would otherwise be the hardest rewrite — multi-word names colliding with the `and`/`or`
  keywords — and lets identifiers ride directly on `expr`'s lexer.
- **Values** crossing into the VM stay as `Bl*` types; in particular `BlNumber` is never collapsed
  to a float, and `BlNull` propagates per [null.spec.md](null.spec.md).

`[@test] ../expr/feel_engine_internal_test.go`

---

## Integration points

These are forward-looking notes; the referenced specs are **not** modified by this document.

- **`LiteralExpression`** ([literal-expression.spec.md](../decision-tasks/literal-expression.spec.md))
  could accept a source string compiled with `Bl.Expr` as an alternative to its `BlExpr` body.
- **`DecisionTable`** ([decision-table.spec.md](../decision-tasks/decision-table.spec.md)) rule
  predicates and input entries could be authored as unary tests (see [§ Unary tests](#unary-tests)),
  parsed against each column's type.

---

## Relationship to FEEL and future direction

The v1 language is based on DMN 1.4 FEEL, drawing on the standard plus the well-documented
implementations below. FEEL is the starting point, not a permanent contract: blkit diverges where it
improves the language. Each divergence is recorded here with the FEEL behaviour, the blkit
behaviour, and the rationale.

**Divergences from FEEL (v1):**

- **No spaces in identifiers.** FEEL permits multi-word names with embedded spaces, for both
  variables and built-in functions (`loan amount`, `string length`, `day of week`). blkit forbids
  spaces — identifiers are letters, digits, and underscores. *Rationale:* removes FEEL's hardest
  parsing ambiguity (longest-match names colliding with the `and`/`or` keywords), matches
  conventional programming identifiers, and lets the grammar sit directly on `expr`'s lexer.
- **lowerCamelCase built-in names.** As a direct consequence of the no-spaces rule, every multi-word
  FEEL function name is spelled in lowerCamelCase: `string length` → `stringLength`, `day of week` →
  `dayOfWeek`, `years and months duration` → `yearsAndMonthsDuration`, and `date and time` →
  `datetime` (a recognised compound, kept whole). *Rationale:* lowerCamelCase is the convention used
  by `expr`'s own built-ins (`hasPrefix`, `sortBy`, `toJSON`) and matches the casing of the host Go
  `Bl*` methods, so blkit functions read as native on both layers. The function tables above use
  these spellings.

**Open decisions:**

- Whether user-defined inline functions (see [§ Function invocation](#function-invocation)) are part
  of the baseline or deferred.
- Which vendor [§ extensions](#vendor-extensions) are in scope vs. left to the programmatic `Bl*`
  API.

### References

- OMG DMN 1.4 specification — the FEEL grammar and standard built-in function library.
- Trisotech Digital Enterprise Suite FEEL docs — standard FEEL function reference and temporal/list
  extensions.
- Drools / KIE [DMN FEEL handbook](https://kiegroup.github.io/dmn-feel-handbook/) — function
  reference and numeric-literal extensions.
- Camunda FEEL language guide — function signatures and worked examples.

---

## Edge cases

- An empty source string is a `BlParseError`.
- An expression that evaluates to `null` is a valid result.
- `Bl.Eval` needs no input for expressions that reference no variables (`1 + 1`,
  `date("2025-01-01")`).
- A list index out of range returns `null`; a missing context key returns `null`.

`[@test] ../expr/feel_edge_cases_test.go`
