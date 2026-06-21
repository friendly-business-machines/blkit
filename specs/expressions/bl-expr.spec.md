---
name: bl.BlExpr
description: The blkit expression language — a string-based expression syntax (based on DMN FEEL) parsed and evaluated into Bl* values. This is the hub spec covering the engine API, operators, control flow, unary tests, lexical rules, semantics, and the Go layer that extends expr-lang/expr; each data type's literals, functions, and Go implementation are detailed in its own spoke spec.
targets:
  - ../../core/engine.go
---

# The blkit expression language

This spec defines **blkit's expression language**: a compact, business-readable syntax for writing
expressions as text — `age >= 18 and income > 50000`, `sum(lineItems.amount)`,
`if score >= 700 then "approve" else "review"` — that evaluate to blkit value types (`bl.BlNumber`,
`bl.BlString`, `bl.BlList`, …, each documented in its own spoke spec; see [§ Data types](#data-types)).

Expressions are **strings**, parsed and evaluated at runtime — there is no programmatic
expression-builder API. The language is based on **DMN 1.4 FEEL** (Friendly Enough Expression
Language) and reproduces its syntax and semantics as the v1 baseline. The runtime is built on
[`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which blkit extends with its own
value types, functions, and operators — see [§ Engine internals](#engine-internals-go).

This document is the **hub**: it specifies the engine API, operators, control flow, unary tests,
lexical rules, semantics, and the shared Go extension layer. Each data type's literals, type-specific
functions, and Go implementation live in that type's **spoke** spec (e.g. [number.spec.md](number.spec.md),
[string.spec.md](string.spec.md)).

Throughout, examples use the notation `expression  // → result`, where the result is shown in the
same source syntax (e.g. a `bl.BlNumber` renders as `42`, a `bl.BlString` as `"foo"`).

---

## Using the engine

```go
// host-side (Go)
// Expr compiles a source string once against the concrete env struct E. E's
// exported fields — renamed by `expr:"name"` tags — declare the variables the
// source may reference; an undeclared name is a compile-time error. The returned
// bl.BlExpr[E] can be evaluated repeatedly.
func Expr[E any](source string) (*BlExpr[E], error)

// BlExpr is a compiled, type-checked expression over a concrete env struct E.
type BlExpr[E any] struct { /* unexported: original source + compiled program */ }
func (e *BlExpr[E]) Evaluate(env E) (BlValue, error)
func (e *BlExpr[E]) Source() string

// NoEnv is the env type for a variable-free expression; ExprNoEnv is shorthand
// for Expr[NoEnv].
type NoEnv = struct{}
func ExprNoEnv(source string) (*BlExpr[NoEnv], error)
```

A `bl.BlExpr[E]` is a compiled expression: parse a source string once with `bl.Expr[E]`, then
`Evaluate` it repeatedly. The variables an expression may reference are the **exported fields of the
concrete Go env struct `E`**, each renamed to its FEEL name by an `expr:"…"` tag. Because the env is a
real Go type, the variable names and their types are checked at **Go compile time**: passing the wrong
env to `Evaluate` is a build error, and referencing an undeclared name is a parse error. The result is
a `bl.BlValue` (see [§ Engine internals](#engine-internals-go) for the bridging rules and host
accessors). There are no `bl.Number(…)`-style expression factories.

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
// result is the bl.BlBoolean true
```

A variable-free expression needs no env type: compile it with `bl.ExprNoEnv` and evaluate it against
`bl.NoEnv{}`.

```go
// host-side (Go)
var sum, _ = bl.ExprNoEnv(`1 + 1`)
var two, _ = sum.Evaluate(bl.NoEnv{}) // the bl.BlNumber 2
```

`[@test] ../../core/engine_test.go`

---

## Data types

Every value belongs to one of the following types. Each has a literal or constructor syntax.

| Type | `Bl*` type | Literal / constructor | Example |
|---|---|---|---|
| number | `bl.BlNumber` | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` |
| string | `bl.BlString` | double quotes | `"hello"` |
| boolean | `bl.BlBoolean` | keywords | `true`, `false` |
| date | `bl.BlDate` | `date(...)` | `date("2025-03-28")` |
| time | `bl.BlTime` | `time(...)` | `time("11:45:30")`, `time("11:45:30+02:00")` |
| datetime | `bl.BlDateTime` | `datetime(...)` | `datetime("2025-03-28T11:45:30")` |
| dtDuration | `bl.BlDaysTimeDuration` | `dtDuration(...)` | `dtDuration("P4DT12H")` |
| ymDuration | `bl.BlYearsMonthsDuration` | `ymDuration(...)` | `ymDuration("P1Y6M")` |
| list | `bl.BlList` | `[ ... ]` | `[1, 2, 3]` |
| dictionary | `bl.BlDictionary` | `{ ... }` | `{name: "Alice", age: 30}` |
| range | `bl.BlRange` | interval notation | `[1..10]`, `(1..10)`, `[1..10)` |
| null | `bl.BlNull` | keyword | `null` |
| regex | `bl.BlRegex` | `pattern(...)` | `pattern("[0-9]+")` |
| table | `bl.BlTable` | `table(...)` / `tableFromDicts(...)` | `table(["a"], [1], [2])` |
| calendar | `bl.BlCalendar` | host-built (no constructor) | — |

- **Numbers** are arbitrary-precision decimals — never floats; precision is preserved through all
  arithmetic (see [number.spec.md](number.spec.md)).
- The last three rows have **no literal syntax** in v1: `bl.BlRegex`
  ([string.spec.md § Precompiled patterns](string.spec.md#precompiled-patterns-patterns--blblregex))
  is built by `pattern(...)`; `bl.BlTable` ([table.spec.md](table.spec.md)) by
  `table(...)` / `tableFromDicts(...)`; `bl.BlCalendar` ([calendar.spec.md](calendar.spec.md)) is
  host-built only. All three are produced by a constructor or host code and referenced as variables.

`[@test] ../../core/engine_test.go`

### Numbers

```
// expression-language
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
// expression-language
"hello"               // → "hello"
"line1\nline2"        // → two lines
"quote: \""           // → quote: "
```

### Temporal values

Temporal values are created with constructor functions, accepting either an ISO 8601 string or
numeric components (see [§ Built-in function library](#built-in-function-library)).

```
// expression-language
date("2025-03-28")                       // → a date
date(2025, 3, 28)                        // → a date
time("11:45:30+02:00")                   // → a time with offset
datetime("2025-03-28T11:45:30")          // → a datetime
dtDuration("P4DT12H")                      // → days-time duration (4 days, 12 hours)
ymDuration("P1Y6M")                        // → years-months duration (1 year, 6 months)
```

---

## Variables

A bare name references an input variable. Names follow conventional identifier rules — letters,
digits, and underscores, **no spaces** — and are case-sensitive. (This is a deliberate divergence
from FEEL, which allows multi-word names with embedded spaces; it also applies to built-in function
names. See [§ Relationship to FEEL](#relationship-to-feel-and-future-direction).)

```
// expression-language
age                   // value of the `age` variable
loanAmount * 12       // identifier used in arithmetic
```

Variables are supplied at evaluation time via the `input` map and, when an expression is compiled
with a `bl.BlSchema`, are type-checked at parse time. A reference to a name that is neither in scope
nor declared is a parse error (see [§ Errors and null](#errors-and-null)).

`[@test] ../../core/engine_test.go`

### Name resolution: `isDefined`

Variable names are the fields of a concrete env struct, fixed at Go-compile time, so a top-level name
is **always** bound: `isDefined(name)` on a declared field is statically `true`, and naming an
undeclared variable is a compile-time error rather than a runtime `false`. `isDefined` remains useful
for **dictionary paths**, where it probes whether a dictionary actually contains a key — a missing key
is "not defined", distinct from a key present with a `bl.BlNull` value.

```
// expression-language, with `applicant` a declared bl.BlDictionary field
isDefined(applicant)             // → true  (a declared field is always defined)
isDefined(applicant.name)        // → true  when the dictionary has key "name"
isDefined(applicant.middleName)  // → false when the dictionary has no "middleName" key
isDefined(undeclaredName)        // compile error — not a declared field
```

`isDefined` operates at the resolution layer, not the value layer — to test whether a *present* value
*is* `bl.BlNull`, use `isNull(x)` ([null.spec.md § Testing for null](null.spec.md#testing-for-null));
to supply a fallback for a `bl.BlNull`, use `getOrElse(x, default)`
([null.spec.md § Default for null](null.spec.md#built-in-functions)).

Because the distinction is structural, `isDefined` cannot be a normal `bl.BlValue → bl.BlValue` impl —
by the time a normal impl runs, a missing dictionary key has already collapsed to `bl.BlNull`. Instead
`normalise` lowers `isDefined(name)` to a static bound check (the strict env still rejects an
undeclared name) and `isDefined(d.key)` to a key-presence probe on the dictionary `d`. The impls are
registered in `engine.go` alongside the other engine-level options.

`[@test] ../../core/typetest_test.go`

---

## Arithmetic

The operators `+`, `-`, `*`, `/`, `**` (exponent), and unary `-` operate on numbers, with decimal
precision preserved. `+` and `-` also apply to temporal/duration combinations.

```
// expression-language
2 + 3                              // → 5
10 - 4                             // → 6
3 * 4                              // → 12
10 / 4                             // → 2.5
2 ** 8                             // → 256
-(7)                               // → -7

date("2025-01-01") + ymDuration("P1Y")    // → date("2026-01-01")
dtDuration("P1D") + dtDuration("PT12H")      // → dtDuration("P1DT12H")
```

`null` propagates: `null + 1 // → null`. Division by zero yields `null`.

`[@test] ../../core/engine_test.go`

---

## Comparison

The operators `<`, `<=`, `>`, `>=`, `=`, `!=` compare numbers, strings, and temporal/duration
values; `=` and `!=` apply to all types. `x between a and b` is shorthand for `x >= a and x <= b`.
The result is a `bl.BlBoolean` (or `null` when operands are incomparable).

```
// expression-language
5 < 10                             // → true
10 >= 10                           // → true
"abc" = "abc"                      // → true
date("2025-01-01") < date("2025-06-01")   // → true
5 between 1 and 10                 // → true
```

`[@test] ../../core/engine_test.go`

---

## Boolean logic

`and`, `or`, and `not(...)` use **three-valued logic** — `true`, `false`, and `null` — matching
[boolean.spec.md](boolean.spec.md) and [null.spec.md](null.spec.md). `and`/`or` short-circuit so a
definite result is returned even when the other operand is `null`.

```
// expression-language
true and false                    // → false
true or false                     // → true
not(true)                         // → false

false and null                    // → false   (short-circuit)
true or null                      // → true    (short-circuit)
true and null                     // → null
null or false                     // → null
```

`[@test] ../../core/boolean_test.go`

---

## String expressions

Strings concatenate with `+`. Concatenation is **string-only**: to join a non-string value, convert
it first with `string(...)`.

```
// expression-language
"foo" + "bar"                     // → "foobar"
"order-" + string(123)            // → "order-123"
```

Inspection and transformation use the [string.spec.md § Built-in functions](string.spec.md#built-in-functions).

```
// expression-language
upperCase("aBc")                 // → "ABC"
contains("foobar", "oob")         // → true
substring("foobar", 3, 2)         // → "ob"
```

`[@test] ../../core/string_test.go`

---

## Conditional

`if c then a else b` evaluates `a` when `c` is `true`, otherwise `b`. A `null` or non-boolean
condition takes the `else` branch. The two branches may have different types; the result type is
their union.

```
// expression-language
if 5 < 10 then "low" else "high"           // → "low"
if 12 < 10 then "low" else "high"          // → "high"
if null then "low" else "high"             // → "high"

if age >= 18 then "adult" else "minor"     // depends on `age`
```

Conditionals nest:

```
// expression-language
if score >= 750 then "prime"
else if score >= 650 then "standard"
else "subprime"
```

`[@test] ../../core/engine_test.go`

---

## Ranges and intervals

A range is a bounded interval with open `( )` or closed `[ ]` boundaries on each side. Ranges are
used for membership tests (with `in`) and the [range.spec.md § Interval algebra](range.spec.md#interval-algebra-built-ins). See
[range.spec.md](range.spec.md).

```
// expression-language
[1..10]      // 1 to 10, both inclusive
(1..10)      // 1 to 10, both exclusive
[1..10)      // 1 inclusive, 10 exclusive
(1..10]      // 1 exclusive, 10 inclusive
```

Ranges work over numbers and ordered temporal values:

```
// expression-language
[date("2025-01-01")..date("2025-12-31")]
```

`[@test] ../../core/range_test.go`

---

## Sequences: the `:` operator

`start:end` materialises a numeric **`bl.BlList`** running from `start` to `end` inclusive in
steps of `1` — a strict-list counterpart to the range `[start..end]` (which stays as a
`bl.BlRange` for containment / interval-algebra purposes). The shorthand is sugar for
`seq(start, end, 1)`; the parser lowers `start:end` to `seq(start, end, 1)` at patch time, so
the two forms are exactly equivalent. For a non-default step (or for clarity in dense
expressions), call `seq(start, end, step)` explicitly. Full semantics — auto-reverse on
`start > end`, fractional steps, the zero-step rejection — live in
[list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator).

```
// expression-language
5:10                 // → [5, 6, 7, 8, 9, 10]
10:5                 // → [10, 9, 8, 7, 6, 5]    (auto-reversed)
1.5:3.5              // → [1.5, 2.5, 3.5]        (fractional)
-2:2                 // → [-2, -1, 0, 1, 2]      (unary minus binds tightest, so this is (-2):2)
seq(0, 1, 0.25)      // → [0, 0.25, 0.5, 0.75, 1]   (non-default step requires seq)
```

**Precedence.** `:` binds tighter than `+` / `-` but looser than `*` / `/`, so
`1+2:5*2` parses as `(1+2):(5*2)` → `[3, 4, 5, 6, 7, 8, 9, 10]`. It matches the position of
the range constructor `..` in the precedence ladder (see [§ Operator
precedence](#operator-precedence)).

**Dictionary literals.** `:` is also the dictionary key/value separator inside `{...}`. To
avoid ambiguity, a bare sequence `start:end` is **not allowed in dictionary value
positions** — the first `:` after a key is always the dict separator, and any further `:`
inside that value is a parse error. Use `seq(start, end)` (or parenthesise:
`{a: (5:10)}`) when you need a sequence as a dictionary value.

```
// expression-language
{a: seq(5, 10)}              // OK — explicit function call
{a: (5:10)}                  // OK — parenthesised sequence expression
{a: 5:10}                    // bl.ParseError — bare `:` in a dict value position
```

`[@test] ../../core/list_test.go`

---

## Membership: the `in` operator

`x in y` tests whether `x` is a member of a list, falls within a range, or is covered by a
calendar.

```
// expression-language
5 in [1, 2, 3, 4, 5]               // → true
3 in [1..10]                       // → true
10 in [1..10)                      // → false   (upper bound exclusive)
"US" in ["US", "CA", "MX"]         // → true
date("2025-12-25") in ukHolidays   // → true   (calendar membership; see calendar.spec.md)
```

For a `bl.BlCalendar` right operand, `point in calendar` is **patcher-lowered** to
`contains(calendar, point)` and inherits its semantics — see
[calendar.spec.md § Operators](calendar.spec.md#operators) for the zone-kind and
cross-temporal-kind rules. The left operand for calendar membership must be a `bl.BlDate` or
`bl.BlDateTime`; a range left operand → `bl.TypeError` (use the explicit
`overlaps(c, r)` / `entriesIn(c, r)` for range-on-calendar queries).

`[@test] ../../core/list_test.go`

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
| leading-dot path | the path access on the input compares true | `.status = "active"`, `.amount > 100` |
| `?` expression | the expression (with `?` = input) is true | `contains(?, "good")`, `endsWith(?, "@blkit.io")` |
| `-` | always (wildcard) | `-` |

```
// expression-language
< 10                  // input < 10
[18..65]              // 18 <= input <= 65
"low", "medium"       // input = "low" or input = "medium"
not(0)                // input != 0
contains(?, "urgent") // the input string contains "urgent"
-                     // matches anything
```

### Form applicability

Not every form applies to every input type. The ordering and interval forms
need an ordered domain; the equality, disjunction, `not`, `?` expression, and
wildcard forms work against any type:

| Form | Comparable scalars\* | Unordered types\*\* |
|---|---|---|
| value (equality literal) | ✅ | ✅ |
| comma list (disjunction) | ✅ | ✅ |
| `not(...)` | ✅ inherits inner | ✅ inherits inner |
| `?` expression | ✅ | ✅ |
| wildcard `-` | ✅ | ✅ |
| `<` `<=` `>` `>=` | ✅ | ❌ `bl.ParseError` at construction |
| interval `[a..b]` | ✅ | ❌ `bl.ParseError` at construction |

\* **Comparable scalars**: `bl.TypeNumber`, `bl.TypeString`, `bl.TypeDate`,
`bl.TypeTime`, `bl.TypeDateTime`, `bl.TypeDaysTimeDuration`, `bl.TypeYearsMonthsDuration`.

\*\* **Unordered types**: `bl.TypeBoolean`, `bl.TypeList`, `bl.TypeDictionary`,
`bl.TypeTable`, `bl.TypeRange`, `bl.TypeCalendar`, `bl.TypeRegex`.

`bl.TypeAny` accepts every form — the parse-time type-checker can't statically
rule any of them out when the input type is unknown.

### The `?` placeholder

Inside a unary test, `?` is the **implicit input** — the value the test is being evaluated
against. Simple tests like `< 10` or `"valid"` use the input implicitly (the engine inserts
the comparison or equality), but when the test body needs to call a built-in on the input,
`?` is the addressable name:

```
// expression-language
contains(?, "urgent")               // input.contains("urgent")
endsWith(?, "@blkit.io")            // input.endsWith("@blkit.io")
?.year >= 2025                      // input is a date; check its year
isPublicHoliday(?, ukHolidays)      // input is a date; check against a calendar
```

For implicit-input forms (`< 10`, `"valid"`, `[18..65]`, etc.), writing `?` explicitly is
redundant — `< 10` and `? < 10` are equivalent. A **leading dot** is shorthand for path
access on the implicit input: `.status = "active"` is equivalent to `?.status = "active"`,
and `.items.amount > 100` is equivalent to `?.items.amount > 100`. Use `?` explicitly when
the input needs to be passed as a function argument (e.g. `contains(?, "x")`) rather than
projected by a path.

Because a `?` expression can be **any** language expression that yields a
`bl.BlBoolean`, the unary-test grammar is fully expressive over every value
type. Every FEEL idiom for list / context / table queries — `listContains`,
`count`, predicate filter (`xs[item.field > n]`), projection (`xs.field`),
`for … return`, `some` / `every`, path access, `getValue`, `getEntries`,
`dictionaryPut`, `dictionaryMerge`, `instance of` — composes into a cell
predicate via the `?` form.

### Tests over structured inputs

When a decision-table column has a list, dictionary, or table type, the cells
use the `?` expression form. Typical idioms:

```
// expression-language — input is a list

count(?) > 0                          // non-empty list
listContains(?, "urgent")             // membership
count(?[item.amount > 100]) >= 1      // at least one matching element (filter + count)
every x in ? satisfies x.qty > 0      // every element passes
some  x in ? satisfies x.is_priority  // at least one element passes
```

```
// expression-language — input is a dictionary

.tier = "gold"                        // field equality (leading-dot shorthand for ?.tier)
.applicant.income >= 50000            // nested path
has(?, "approver")                    // key presence
getValue(?, "tier") = "gold"          // explicit lookup (equivalent to .tier)
size(?) > 0                           // non-empty dictionary
```

```
// expression-language — input is a table (BlTable = list of uniformly-keyed rows)

count(?) > 0                            // non-empty
some r in ? satisfies r.amount > 1000   // any row predicate
every r in ? satisfies r.status = "ok"  // every row predicate
count(?[item.flagged]) = 0              // no flagged rows (filter shorthand)
```

The functions and operators used above are documented in their type's spoke
spec ([list.spec.md](list.spec.md), [dictionary.spec.md](dictionary.spec.md),
[table.spec.md](table.spec.md)). The cross-cutting forms (`for … return`,
`some` / `every`, filter `[predicate]`) live in [§ Loops](#loops-for--return)
and [§ Quantified expressions](#quantified-expressions-some--every) above.

### Host-side usage

Host Go code compiles a unary test once and evaluates it against many inputs — typical
pattern when a decision-table row's cells are checked against repeated input data. The
host API is the same `bl.BlExpr` returned by `bl.Expr`; the only difference is the
constructor accepts the unary-test grammar and an `inputType` (the type the implicit `?`
will hold).

```go
// host-side (Go)

// bl.UnaryTest compiles a unary-test source string over a single typed input T
// (the implicit ?). The result is verified to be a bl.BlBoolean (or bl.BlNull);
// forms that require an ordered domain (<, <=, >, >=, interval [a..b]) are
// rejected at construction when T is an unordered type. Use T = bl.BlValue for
// inputs whose type isn't known statically.
type BlUnaryTest[T BlValue] struct { /* unexported: source + program */ }
func UnaryTest[T BlValue](source string) (*BlUnaryTest[T], error)
func (u *BlUnaryTest[T]) Evaluate(input T) (BlValue, error)
func (u *BlUnaryTest[T]) Source() string
```

The returned `bl.BlUnaryTest[T]` is evaluated by passing the typed input value directly to
`Evaluate` — there is only the single implicit `?`, so no dictionary wrapping.

Worked example:

```go
// host-side (Go) — compile-once, test-many.
var atLeast18, _ = bl.UnaryTest[bl.BlNumber](`>= 18`)
var isUrgent,  _ = bl.UnaryTest[bl.BlString](`contains(?, "urgent")`)
var inRange,   _ = bl.UnaryTest[bl.BlNumber](`[18..65]`)
var wildcard,  _ = bl.UnaryTest[bl.BlValue](`-`)

// Evaluate against typed inputs. The engine has verified at construction that
// every result will be bl.BlBoolean (true / false) or bl.BlNull (when null
// propagation kicks in — see null.spec.md § Propagation).
var n21, _    = bl.Number(21)
var ok,  _    = atLeast18.Evaluate(n21)                          // → bl.BlBoolean(true)

var noteIn, _ = bl.String("urgent notice")
var ok2, _    = isUrgent.Evaluate(noteIn)                        // → bl.BlBoolean(true)

var n70, _    = bl.Number(70)
var ok3, _    = inRange.Evaluate(n70)                            // → bl.BlBoolean(false)
var ok4, _    = wildcard.Evaluate(n70)                           // → bl.BlBoolean(true) — wildcard matches anything
var ok5, _    = wildcard.Evaluate(bl.Null())                     // → bl.BlBoolean(true) — even Null
```

The fallible cases:

- **Parse-time errors** — `bl.UnaryTest[T]` returns `(nil, bl.ParseError)` if the source
  string isn't a valid unary test. Common causes: unknown identifier, malformed range,
  type mismatch between the test body and the input type `T` (e.g. `>= 18` with
  `bl.BlString`), or a body whose result isn't a boolean.
- **Evaluation-time errors** — `Evaluate(input)` returns `(nil, bl.TypeError)` for a
  runtime type mismatch inside the test body. The input type itself is fixed by the Go
  type parameter `T`, so supplying the wrong input type is a build error, not a runtime one.
- **`bl.BlNull` results** — a test against a null input returns `bl.BlNull`, not an
  error. Wildcards (`-`) explicitly return `true` for null; all other tests propagate
  null per the standard rules (see [null.spec.md](null.spec.md)).

The engine's pre-validation guarantees the result type, but `Evaluate` returns
`bl.BlValue` like any other `bl.BlExpr`; callers that need a `bl.BlBoolean` value can
type-assert the result.

The decision-table runtime ([decision-table.spec.md](../decision-tasks/decision-table.spec.md))
uses this API internally: at table-construction time, each cell's unary-test source is
compiled once via `bl.UnaryTest` and the resulting `bl.BlExpr` is cached on the cell. At
evaluation time, the column-input value is fed through `Evaluate(input)`, and the
per-cell booleans are combined per the table's hit policy.

**Relationship to `bl.Expr`.** `bl.UnaryTest[T]` is a constructor variant of `bl.Expr`:
it shares the same parse / patch / type-check / compile pipeline, with two extra steps
in front. A **source normaliser** rewrites the unary-test forms into ordinary
expressions referencing `?` (e.g. `< 10` → `? < 10`, `2, 3` → `? = 2 or ? = 3`, `-` →
`true`, `[18..65]` → `? in [18..65]`, `.status = "active"` → `?.status = "active"`,
`contains(?, "urgent")` left as-is), and a single-field struct env binding the implicit
`?` placeholder with type `T` is supplied for the type-check step. The result is a
`bl.BlUnaryTest[T]` whose evaluation receives the input value bound to `?`. The separate
constructor exists so the unary-test grammar
(the left-implicit and comma-disjunction forms) doesn't have to be valid
plain-expression syntax — those forms only parse in unary-test mode. `Source()`
returns the original (un-normalised) string the caller supplied.

`[@test] ../../core/unarytest_test.go`

---

## Lists

### Literals and indexing

Lists are ordered and heterogeneous. Indexing is **1-based**; negative indexes count from the end;
an out-of-range index yields `null`.

```
// expression-language
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
// expression-language
[1, 2, 3, 4][item > 2]             // → [3, 4]
[1, 2, 3, 4][even(item)]           // → [2, 4]
```

### Projection

Accessing a field on a list of dictionaries projects that field across every element.

```
// expression-language
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].name   // → ["Alice", "Bob"]
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].age     // → [30, 34]
```

List operations are covered by the [list.spec.md § Built-in functions](list.spec.md#built-in-functions).

`[@test] ../../core/list_test.go`

---

## Loops: `for … return`

`for x in xs return e` evaluates `e` once per element of `xs`, collecting the results into a new
list.

```
// expression-language
for x in [1, 2, 3] return x * 2          // → [2, 4, 6]
```

**Multiple iterators** pair each element of the first list with each element of the second
(every combination), iterating the rightmost fastest:

```
// expression-language
for x in [1, 2], y in [3, 4] return x * y    // → [3, 4, 6, 8]
```

A loop may iterate a numeric range:

```
// expression-language
for x in 0..8 return 2 ** x              // → [1, 2, 4, 8, 16, 32, 64, 128, 256]
```

The keyword **`partial`** refers to the list of results accumulated so far, enabling running
computations:

```
// expression-language
for i in 1..10 return if i <= 2 then 1 else partial[-1] + partial[-2]
// → [1, 1, 2, 3, 5, 8, 13, 21, 34, 55]   (Fibonacci)
```

Each loop result is a `bl.BlList`.

`[@test] ../../core/comprehensions_test.go`

---

## Quantified expressions: `some` / `every`

`some x in xs satisfies c` is `true` when `c` holds for at least one element; `every x in xs
satisfies c` is `true` when `c` holds for all of them.

```
// expression-language
some x in [1, 2, 3] satisfies x > 2           // → true
every x in [1, 2, 3] satisfies x >= 1         // → true
every x in [1, 2, 3] satisfies x > 2          // → false

some order in orders satisfies order.total > 1000
```

**Multiple iterators** pair each element of the first list with each element of the second
(same as `for`), and the condition is tested against every combination:

```
// expression-language
some x in [1, 2], y in [3, 4] satisfies x + y > 5     // → true   (2 + 4 > 5)
every x in [1, 2], y in [3, 4] satisfies x + y >= 4   // → true   (every pair has sum ≥ 4)
```

`[@test] ../../core/comprehensions_test.go`

---

## Dictionaries

A dictionary is an ordered map of named entries. Keys may be unquoted names or strings; values may be
any type. Later entries can reference earlier ones in the same literal.

```
// expression-language
{}                                 // → empty dictionary
{a: 1, b: 2}                       // → {a: 1, b: 2}
{"a": 1, "b": 2}                   // → {a: 1, b: 2}   (quoted keys)
{a: 1, b: {c: 2}}                  // → nested dictionary
{a: 2, b: a * 2}                   // → {a: 2, b: 4}   (b references a)
```

### Path access

The dot operator navigates into a dictionary. Chains traverse nested dictionaries; a missing key yields
`null`.

```
// expression-language
{a: 2}.a                           // → 2
{a: {b: 3}}.a.b                    // → 3
{a: 1}.b                           // → null
applicant.address.postcode         // navigate input variables
```

See [dictionary.spec.md](dictionary.spec.md). Dictionary manipulation uses the
[dictionary.spec.md § Built-in functions](dictionary.spec.md#built-in-functions).

`[@test] ../../core/dictionary_test.go`

---

## Accessing components

The dot operator also reads named **components** of temporal, duration, and range values — not just
dictionary entries. This is the standard FEEL way to pull a field out of a date, time, duration, or
range.

```
// expression-language
date("2025-03-28").year            // → 2025
date("2025-03-28").month           // → 3
date("2025-03-28").day             // → 28

time("11:45:30+02:00").hour        // → 11
time("11:45:30+02:00").minute      // → 45
time("11:45:30+02:00").offset // → dtDuration("PT2H")

dtDuration("P1DT2H3M4S").days        // → 1
dtDuration("P1DT2H3M4S").hours       // → 2
dtDuration("P1DT2H3M4S").minutes     // → 3
ymDuration("P1Y6M").years            // → 1
ymDuration("P1Y6M").months           // → 6

[1..10].start                      // → 1
[1..10].end                        // → 10
[1..10).endIncluded               // → false
```

Available components follow the relevant `Bl*` type spec (e.g. [date.spec.md](date.spec.md),
[range.spec.md](range.spec.md)).

`[@test] ../../core/engine_test.go`

---

## Function invocation

Functions (built-ins or in-scope functions) are invoked with **positional** or **named**
arguments.

```
// expression-language
substring("foobar", 3, 2)                                  // positional → "ob"
substring(string: "foobar", startPosition: 3, length: 2)  // named → "ob"
```

### User-defined functions

FEEL inline functions have the form `function(params) body`:

```
// expression-language
function(x, y) x + y
listReplace([2, 4, 7], function(i) i < 5, 5)   // → [5, 5, 7]   (predicate form)
```

Inline functions are a v1 feature; the engine's `BlFunc` value type is consumed by every
higher-order built-in in the library (the predicate forms of `remove` / `listReplace`,
etc.). To keep the semantics simple and the engine surface auditable, blkit
restricts the form to a minimal subset of what FEEL allows:

- **Anonymous only.** The syntactic form is always `function(params) body`. There is no
  named-function-definition form (no `let f = function(...) ...`, no top-level `fun`
  declarations) — FEEL itself doesn't have one. Functions exist purely as anonymous values
  inside expressions.
- **First-class as values, but not addressable by name.** A function value can be passed as
  an argument to a higher-order built-in, stored as a dictionary value, or returned from an
  `if`/`for`/`some` expression. It cannot be looked up by name from outside the expression
  (host code that wants a callable should register an `expr.Function` instead — see [§
  Registering built-in functions](#registering-built-in-functions)).
- **No recursion.** The body cannot reference the function itself — there's no name to refer
  to, since the function is anonymous, and the engine does not insert an implicit
  self-reference. For recursive-style computation, use the bounded `for i in 1..n return …`
  form ([§ Loops: for … return](#loops-for--return)), which accumulates results
  iteratively and inherits the engine's step limit.
- **Pure, bounded execution.** The body sees only its parameters and the surrounding lexical
  scope; it has no I/O, no access to host state, no mutable references. The function shares
  the engine VM's existing step / stack / recursion limits (see [§ Environment &
  errors](#environment--errors)), so a runaway body terminates with a `BlEvalError` rather
  than hanging.

These constraints fall out naturally from `expr-lang/expr`'s sandboxed VM model — the engine
already disallows I/O, host calls, and arbitrary recursion at the VM layer. The restrictions
above are stated for clarity at the language-spec level so callers understand the surface
they're getting, not to add engine work.

`[@test] ../../core/func_test.go`

---

## Type checking: `instance of`

`x instance of T` tests a value's type, where `T` is a type name from
[§ Data types](#data-types) — including the non-literal types `regex`, `table`, and
`calendar`. Returns a `bl.BlBoolean`.

```
// expression-language
42 instance of number              // → true
"x" instance of number             // → false
date("2025-01-01") instance of date    // → true
pattern("[0-9]+") instance of regex    // → true
myTable instance of table              // → true
ukHolidays instance of calendar        // → true
```

`[@test] ../../core/typetest_test.go`

---

## Built-in function library

The language ships a large built-in function library. Each function is documented in the spec for
the type it operates on or produces — that spoke gives its full signature, worked examples, edge
cases, and Go registration. This catalogue is the index:

| Group | Representative functions | Spoke |
|---|---|---|
| Conversion | `string`, `number`, `date`, `time`, `datetime`, `duration` | the target type's spoke ([number](number.spec.md), [string](string.spec.md), [date](date.spec.md), …) |
| Boolean | `not` | [boolean.spec.md](boolean.spec.md) |
| Null | `isNull`, `getOrElse` | [null.spec.md](null.spec.md) |
| Resolution | `isDefined` | this spec ([§ Name resolution](#name-resolution-isdefined)) |
| String | `substring`, `stringLength`, `upperCase`, `contains`, `matches`, `replace`, `split`, `stringJoin`, `pattern` (precompiled `bl.BlRegex`), … | [string.spec.md](string.spec.md) |
| Numeric | `clamp`, `floor`, `ceiling`, `round*`, `abs`, `modulo`, `sqrt`, `log`, `ln`, `exp`, `odd`, `even`, … | [number.spec.md](number.spec.md) |
| List | `count`, `min`, `max`, `sum`, `mean`, `sublist`, `append`, `concatenate`, `union`, `distinct`, `flatten`, `sort`, `seq` (and the `:` sequence operator), … | [list.spec.md](list.spec.md) |
| Dictionary | `getValue`, `getEntries`, `dictionaryPut`, `dictionaryMerge` | [dictionary.spec.md](dictionary.spec.md) |
| Temporal | `now`, `today`, `lastDayOfMonth`, `addBusinessDays`, `is*`, … (calendar properties such as `.dayName`, `.monthName` are dot accessors, not function calls — see [date.spec.md § Calendar properties](date.spec.md#calendar-properties)) | [date](date.spec.md) / [time](time.spec.md) / [datetime](datetime.spec.md) |
| Duration | `ymDuration`, `dtDuration`, `ymDurationBetween`, `dtDurationBetween`, components, `abs`, `isNegative`, `round*` (overloaded) | [days_time_duration](days_time_duration.spec.md) / [years_months_duration](years_months_duration.spec.md) |
| Range (interval algebra) | `before`, `after`, `meets`, `overlaps`, `includes`, `during`, `starts`, `finishes`, `coincides` | [range.spec.md](range.spec.md) |
| Table | `table`, `tableFromDicts`, `hasColumn`, `union`, `join`, `asc` / `desc` / `inOrder` (sort keys); transformation **methods** `t.filter` / `t.select` / `t.sort` / `t.withColumn` / `t.groupBy`, … | [table.spec.md](table.spec.md) |
| Calendar (host-constructed) | `entries`, `find`, `contains`, `overlaps`, `entriesFor`, `entriesIn`, `validFrom` / `validTo` / `validRange`, `calendarDrop` / `calendarKeep` / `calendarMerge` | [calendar.spec.md](calendar.spec.md) |

Multi-word names are lowerCamelCase ([§ Relationship to FEEL](#relationship-to-feel-and-future-direction)).
Built-ins that exceed the DMN standard (blkit extensions, e.g. `clamp`, `padLeading`, `addBusinessDays`)
are flagged as such in their spoke. Every built-in is registered with `expr` via the mechanism in
[§ Engine internals](#engine-internals-go).

---

## Operator precedence

From lowest to highest binding:

1. `or`
2. `and`
3. comparison — `< <= > >= = !=`, `between`, `in`, `instance of`
4. additive — `+`, `-`
5. sequence — `:` (see [§ Sequences](#sequences-the--operator))
6. multiplicative — `*`, `/`
7. exponent — `**`
8. unary — `-`, `not(...)`
9. postfix — path (`.`), filter/index (`[ ]`), invocation (`( )`)

Parentheses `( )` group sub-expressions explicitly.

```
// expression-language
2 + 3 * 4             // → 14    (* binds tighter than +)
(2 + 3) * 4           // → 20
a or b and c          // → a or (b and c)
1 + 2:5 * 2           // → [3, 4, 5, 6, 7, 8, 9, 10]   (: between additive and multiplicative)
```

`[@test] ../../core/engine_test.go`

---

## Errors and null

- **`null` propagation** — most operations involving `null` produce `null`
  ([null.spec.md](null.spec.md)); the exceptions are the short-circuit boolean cases (see
  [§ Boolean logic](#boolean-logic)).
- **Missing dictionary key** → `null`, not an error.
- **Division by zero** → `null`.
- **Parse / type-check errors** — malformed syntax, an unknown variable (when a `bl.BlSchema` is given),
  or a static type mismatch — are returned by `bl.Expr` as a `bl.ParseError`.
  `[@test] ../../core/engine_test.go`
- **Evaluation errors** — a type mismatch only detectable with concrete inputs (e.g. comparing
  incompatible types) — are returned by `Evaluate` as a `bl.TypeError`.
  `[@test] ../../core/engine_test.go`

The full error-type catalogue (all defined in `errors.go` / `schema.go` — see
[§ Engine internals](#engine-internals-go)):

| Error | Raised when |
|---|---|
| `bl.ParseError` | `bl.Expr` fails to parse or type-check the source. |
| `bl.TypeError` | `Evaluate` hits a runtime type mismatch (also returned by host constructors on bad input). |
| `bl.RegexError` | a malformed pattern in `matches` / `replace` / `extract` / `pattern(...)` ([string.spec.md](string.spec.md)). |
| `bl.CalendarRangeError` | business-day iteration steps past a calendar's validity bounds under `strictCalendarRange` ([datetime.spec.md § Calendar-range strictness](datetime.spec.md#calendar-range-strictness)). |
| `bl.SchemaError` | a value fails `bl.BlSchema` validation, carrying a path to the offending node ([schema.spec.md](schema.spec.md)). |

---

## Engine internals (Go)

The engine is built on [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which
provides lexing, parsing, optional static type-checking, compilation, and a sandboxed evaluation VM.
`expr` supplies **none** of FEEL's types, functions, or syntax — blkit builds those on top. This
section is the **authoritative Go-implementation contract** shared by every spoke; each spoke's
*Go implementation* section then gives only its own concrete value type, host API, and registrations,
referring back here for the mechanics.

All code lives in the **root `blkit`** package (Go module path
`github.com/friendly-business-machines/blkit`). Callers conventionally import as `bl`, so
call sites read `bl.Expr(...)`, `bl.Number(...)`, etc.

### Package layout

| File | Contents |
|---|---|
| `engine.go` | `Expr`, `bl.BlExpr`, `Type`; the option-assembly, operator binding, patcher install, and the input/output bridge. `bl.BlSchema` lives in `schema.go`. |
| `value.go` | The `bl.BlValue` interface, the `bl.BlNull` singleton, and shared helpers (null propagation, equality, wrapping). |
| `errors.go` | `ParseError`, `TypeError`, `RegexError`, `CalendarRangeError` (`SchemaError` lives in `schema.go`). |
| `patch.go` | The `ast.Visitor` patcher(s) for FEEL-only syntax. |
| `<type>.go` | One per value type (`number.go`, `string.go`, `date.go`, …): the `Bl*` value type, its exported host API, its unexported `…Options()` registrations, and its backing impl funcs. |
| `*_test.go` | Tests — the `[@test]` targets throughout these specs. |

### Visibility & naming conventions

- **Exported** (the public API): the value types (`bl.BlNumber`, `bl.BlString`, …); their host
  constructors (`Number`, `Date`, `Today`, …) and accessors (`ToNativeFloat`, `ToNativeString`,
  `CompareTo`, …); the engine surface (`Expr`, `bl.BlExpr`, `bl.BlSchema`, `bl.BlValue`, `Type`); and the
  error types.
- **Unexported** (package-internal): built-in implementation funcs (suffix `Fn`, e.g. `dayNameFn`);
  operator implementation funcs (e.g. `addNumbers`, `ltDates`); each type's `…Options()` assembler;
  the patcher; and the bridge helpers (`wrap`/`unwrap`).

### Engine entry points (`engine.go`)

```go
// host-side (Go)
// All identifiers below live in package core (imported as `bl`). Callers conventionally
// import as `bl` (so call sites read bl.Expr, bl.Number, etc.).

func Expr[E any](source string) (*BlExpr[E], error)
func ExprNoEnv(source string) (*BlExpr[NoEnv], error) // E = NoEnv = struct{}

// bl.BlExpr is a compiled expression over the concrete env struct E (wraps a *vm.Program).
type BlExpr[E any] struct { /* unexported: original source + compiled program */ }
func (e *BlExpr[E]) Evaluate(env E) (BlValue, error)
func (e *BlExpr[E]) Source() string

// An expression's variables are E's exported fields, each renamed by an `expr:"name"`
// tag; their names and types are checked at Go compile time. (BlSchema is now a
// separate runtime value-validation utility — see schema.spec.md.)

// Type identifies a language type for parse-time checking and `instance of`.
type Type int
const (
    TypeNull Type = iota
    TypeNumber; TypeString; TypeBoolean
    TypeDate; TypeTime; TypeDateTime
    TypeDaysTimeDuration; TypeYearsMonthsDuration
    TypeList; TypeDictionary; TypeRange; TypeTable; TypeCalendar
    TypeRegex
    TypeGroupedTable  // transient t.groupBy(...) handle; only valid as the .agg(...) receiver
    TypeSortKey       // transient asc/desc/inOrder sort key; only valid as a t.sort(...) argument
    TypeAny
)
```

**Pipeline.** `bl.Expr[E]` runs: **normalise** (source-level fixups `expr`'s lexer needs — see
Operators) → **check** (reject any name that is not a declared field of `E`) → **parse** (`expr`'s
parser) → **patch** (`expr.Patch`, rewrite FEEL-only syntax) → **type-check** (against the `E` struct
env) → **compile** to a `*vm.Program`. `Evaluate(env E)` binds the struct's fields as the variables,
runs the program on the sandboxed VM, and returns the result as a `bl.BlValue`.

**Source normalisation (`normalise`).** `expr`'s lexer/parser is fixed, so anything it can't
lex or parse — and anything that must be captured *before* lexing — is rewritten in the source
string first. `normalise` is the only stage that still has the original text, so it owns:

- **`=` → `==`** — FEEL single-equals to `expr`'s equality token (leaving `==` / `<=` / `>=` /
  `!=` untouched; see [§ Operators](#operators)).
- **Unary-test forms** — the decision-table input-entry shorthands (see [§ Unary tests](#unary-tests)).
- **Numeric-literal capture** — `expr` lexes a decimal/exponent literal to a Go `float64` whose
  AST node keeps **no source text**, so precision beyond ~15 significant digits is lost before
  any patch can run (and `NewFromFloat` can only recover that ~15-digit shortest round-trip).
  `normalise` rewrites each fractional/exponent literal to its exact constructor form — `0.1` →
  `number("0.1")` — so it is parsed with `decimal.NewFromString` and the full decimal128
  precision survives. Integer literals are already exact (`expr` lexes them to `int`) and are
  left untouched. See [number.spec.md § Literals](number.spec.md#literals).
- **Two-slot table bracket** — `expr` indexing is a single expression, so the comma forms
  `t[r, c]` / `t[, c]` are not parseable, and a post-parse patch can't fix a parse error.
  `normalise` rewrites the comma form to a backing call before parsing — `t[r, c]` →
  `tableIndex(t, r, c)`, with the empty row slot of `t[, c]` becoming an all-rows marker —
  distinguishing it from a list-literal row selector `t[[a, b]]` and skipping strings and
  nested brackets. The `t[]` / `t[a, b, c]` arity errors are raised by the rewrite; the
  no-comma `t[i]` stays as ordinary single-index access. See
  [table.spec.md § Row and column indexing](table.spec.md#row-and-column-indexing).

These are deliberately source-string rewrites, **not** `expr.Patch` visitors: each needs the
original text or must precede the parse.

```go
// host-side (Go)
func Expr[E any](source string) (*BlExpr[E], error) {
    if source == "" {
        return nil, &ParseError{Source: source, Err: errEmptySource}
    }
    program, err := compileWithEnv(source, *new(E), envFieldNames(reflect.TypeOf((*E)(nil)).Elem()))
    if err != nil {
        return nil, &ParseError{Source: source, Err: err}
    }
    return &BlExpr[E]{source: source, program: program}, nil
}

// compileWithEnv normalises, rejects any reference to a name not in declared
// (blkit's own undefined-name discipline — expr's overloaded operators/functions
// leave its built-in strict checker lenient in operand/argument position), then
// compiles against the strict struct env. *new(E) is the zero env exemplar.
func compileWithEnv(source string, env any, declared map[string]bool) (*vm.Program, error) {
    src, err := normalise(source)
    if err != nil {
        return nil, err
    }
    if name, bad := firstUndefined(src, declared); bad {
        return nil, fmt.Errorf("unknown name %s", name)
    }
    return expr.Compile(src, buildOptionsEnv(env)...)
}

// buildOptionsEnv assembles the strict env plus every spoke's registrations, the
// operator dispatch functions, and the patcher.
func buildOptionsEnv(env any) []expr.Option {
    opts := []expr.Option{expr.Env(env)}
    for _, reg := range typeRegistrations() { // numberOptions, stringOptions, dateOptions, …
        opts = append(opts, reg()...)
    }
    opts = append(opts, operatorRegistrations()...)
    opts = append(opts, expr.Patch(newFeelPatcher()))
    return opts
}
```

### The `bl.BlValue` contract (`value.go`)

Every `Bl*` value type implements `bl.BlValue`, so they pass through the VM as `any`:

```go
// host-side (Go)
type BlValue interface {
    Type() Type                // the language type tag
    Equal(other BlValue) BlValue // three-valued: BlBoolean or BlNull (see null.spec.md)
    String() string              // canonical literal-form rendering
    isBlValue()                  // sealing method — only this package's types implement BlValue
}

// The null value (see null.spec.md).
type BlNull struct{}
func Null() BlNull
```

Type-specific host accessors (`ToNativeFloat`, `Hour`, `ToArray`, …) are declared on the concrete
types in their spokes, not on the interface.

### Bridging native ↔ `Bl*` (`value.go`)

`wrap` converts host inputs to `Bl*`; `unwrap` is the inverse for results that cross back out.

```go
// host-side (Go)
func wrap(v any) (BlValue, error)   // native Go → Bl*
func unwrap(v BlValue) any          // Bl* → native (used by host code)
```

| Native Go input | Wrapped as |
|---|---|
| `int`, `int64`, `float64`, `decimal.Decimal`, decimal `string` via `Number(...)` | `bl.BlNumber` |
| `string` | `bl.BlString` |
| `bool` | `bl.BlBoolean` |
| `[]any` | `bl.BlList` |
| `map[string]any` | `bl.BlDictionary` |
| `time.Time` | `bl.BlDate` / `bl.BlDateTime` (per precision) |
| `time.Duration` | `bl.BlDaysTimeDuration` |
| `nil` / absent input key | `Null` |
| an already-`bl.BlValue` | itself |

`bl.BlNumber` stays an arbitrary-precision decimal inside the VM — **never** collapsed to `float64`.
`Null` propagates per [null.spec.md](null.spec.md).

### Registering built-in functions

`expr.Function(name, fn, signatures…)` takes an impl in `expr`'s calling convention plus one or more
typed signatures used by the checker (multiple = overloads). Each spoke's `…Options()` assembles its
registrations:

```go
// host-side (Go)
// expr's required impl shape:
type exprFn = func(args ...any) (any, error)

// dateOptions is unexported; the engine calls it from buildOptions.
func dateOptions() []expr.Option {
    return []expr.Option{
        expr.Function("dayName", dayNameFn,
            new(func(bl.BlDate) bl.BlString), new(func(bl.BlDateTime) bl.BlString)),
        expr.Function("addBusinessDays", addBusinessDaysFn,
            new(func(bl.BlDate, bl.BlNumber, bl.BlCalendar) bl.BlDate),
            new(func(bl.BlDate, bl.BlNumber, bl.BlCalendar, bool) bl.BlDate)),
        // … one entry per function in the spoke's § Built-in functions
    }
}

// Backing impl (unexported, suffix Fn), in expr's calling convention:
func dayNameFn(args ...any) (any, error) { /* type-asserts args, returns BlValue */ }
```

A small set of generic adapters (`typed1`, `typed2`, `typed3`, `variadic`) wrap statically-typed Go
funcs into `exprFn` and supply the matching signature pointer, so most impls are written with real
types and the `args ...any` boilerplate is confined to the adapters.

### Operators

`expr.Operator(op, fnNames…)` overloads a **binary** operator: the listed functions (each registered
via `expr.Function`) are tried by operand type. Binding is centralised in `operatorBindings()` because
one operator spans many types; each spoke contributes the named funcs.

> **Implementation note — patcher-based dispatch (divergence).** `expr.Operator`'s overload
> resolution is *static*: it matches each operand's compile-time type against the registered
> signatures. Because every operator impl returns the `bl.BlValue` **interface** (so it can yield
> `bl.BlNull`), a chained expression like `a + b + c` type-checks the inner `a + b` to `bl.BlValue`,
> and no `addNumbers(bl.BlNumber, bl.BlNumber)` overload matches an interface-typed operand — `expr`
> rejects it (`mismatched types`). The implementation therefore **does not use `expr.Operator`**.
> Instead the patcher lowers every binary operator to a call of a single per-operator dispatch
> function typed `func(bl.BlValue, bl.BlValue) bl.BlValue` (`__add`, `__sub`, `__lt`, `__eq`, …) that
> switches on the operands' *runtime* types and delegates to the spoke impls (`addNumbers`,
> `concatStrings`, …). Interface→interface chains type-check cleanly, and operand-type errors move
> from parse time to evaluation time (a `bl.TypeError`). The per-spoke typed impls
> (`addNumbers(a, b bl.BlNumber) bl.BlValue`, etc.) are retained exactly as specified — only the
> binding mechanism differs. For the same reason, library functions are registered with `bl.BlValue`
> parameter hints (asserting concrete operand types at runtime), since their arguments are commonly
> operator results of static type `bl.BlValue`.

```go
// host-side (Go)
func operatorBindings() []expr.Option {
    return []expr.Option{
        expr.Operator("+",  "addNumbers", "concatStrings",
                            "addDateYM", "addDateDT", "addDateTimeDur",
                            "addTimeDur", "addYMDuration", "addDTDuration"),
        expr.Operator("-",  "subNumbers", "subDates", "subDateTimes",
                            "subDateDur", "subDateTimeDur",
                            "subYMDuration", "subDTDuration"),
        expr.Operator("*",  "mulNumbers", "scaleYMDuration", "scaleDTDuration"),
        expr.Operator("/",  "divNumbers", "divYMDuration", "divDTDuration"),
        expr.Operator("**", "powNumber"),
        expr.Operator("<",  "ltNumbers", "ltStrings", "ltDates", "ltDateTimes",
                            "ltTimes", "ltYMDuration", "ltDTDuration"),
        // <=, >, >=, ==, != likewise
    }
}
```

Three operator concerns are **not** handled by `expr.Operator`:

- **`=` (single-equals).** FEEL uses `=`/`!=`; `expr`'s lexer expects `==`. The `normalise` step
  rewrites a single `=` to `==` (leaving `==`, `<=`, `>=`, `!=` untouched) before parsing.
- **Unary `-`.** `expr.Operator` is binary-only; the patcher rewrites unary `-x` into `negate(x)`,
  a registered function overloaded over `bl.BlNumber`/duration types.
- **`and` / `or`.** Short-circuit logical operators; `expr`'s native `&&` / `||` need Go `bool`
  operands, but ours are wrapped `Bl*` values that may be `bl.BlNull`. The patcher lowers them to
  a **lazy conditional** (an `ast.ConditionalNode`, **not** a function call), so the second
  operand is evaluated only when the first doesn't already decide the result:
  `a and b` → `let va = a in (if va == false then false else blAnd(va, b))`;
  `a or b` → `let va = a in (if va == true then true else blOr(va, b))`. Laziness is structural —
  `b` (and the helper call) sit in the else-branch, which `expr` evaluates only when taken. The
  guard fires only for a genuine `false` / `true`; a null left operand falls through and
  evaluates `b` (`null == false` → `false` via `BlNull.Equal`), so the three-valued table holds
  and `false and X` / `true or X` never evaluate `X` ([boolean.spec.md](boolean.spec.md)).
  `blAnd` / `blOr` remain as the three-valued truth-table **helpers** the else-branch delegates
  to, but the conditional — not a function call — owns evaluation order. `not(x)` is an ordinary
  `expr.Function`.

### Patchers (`patch.go`)

FEEL constructs absent from `expr`'s grammar are produced by an `expr` patcher (`ast.Visitor` via
`expr.Patch`) that rewrites the parsed tree before compilation:

- interval/range membership with open/closed boundaries (`x in [a..b)`, range literals);
- unary tests (decision-table input entries);
- `if…then…else`; and the **comprehensions** — `for x in a, y in b return …` and
  `some` / `every … satisfies` — which lower to nested iteration because `expr` has no FEEL
  `for`/quantifier syntax (multi-generator forms become nested maps);
- the **filter form** `list[predicate]` — when the bracket holds a *boolean* expression rather
  than a numeric index, the patcher rewrites it to a comprehension that binds the magic `item`
  to each element (`list[item > 2]`), distinguished from the numeric `list[i]` index;
- the boolean connectives `and`/`or` and unary `-` (above);
- **numeric literal nodes** — `IntegerNode`s are replaced with a `bl.BlNumber` `ast.ConstantNode`
  (`NewFromInt`), so operators only ever see `Bl*` operands; decimal/exponent literals were
  already rewritten to `number("…")` by `normalise` (and may be constant-folded here);
- **component access** — `x.year`, `d.minutes`, `r.start` lower to a single runtime-dispatching
  accessor `componentAccess(x, "year")` that switches on `x`'s runtime `Type()` (the per-type
  `dateYearFn` / `durationDaysFn` / … are its internal arms), because the correct accessor
  depends on the operand type, which isn't reliably available at patch time; dictionary member
  access (`d.key`) lowers to `getValue(d, "key")`;
- **dictionary forward-references** — a literal whose entry references an earlier sibling
  (`{a: 2, b: a*2}`) lowers to sequential `let` bindings so each key is in scope for the entries
  to its right (see [dictionary.spec.md](dictionary.spec.md));
- the **sequence operator** `start:end` lowers to `seq(start, end, 1)` (see
  [§ Sequences](#sequences-the--operator)); the patcher also enforces the "no bare `:` in a
  dictionary value position" rule by rejecting any `:` it finds inside a dict-literal value
  expression that isn't parenthesised.

Forbidding spaces in identifiers (see [§ Relationship to FEEL](#relationship-to-feel-and-future-direction))
removes what would otherwise be the hardest rewrite — multi-word names colliding with `and`/`or` —
and lets identifiers ride directly on `expr`'s lexer.

### Environment & errors

The env struct `E` is passed straight to `expr.Env(*new(E))`: expr reflects its exported fields (each
renamed by its `expr:"…"` tag) so every reference is type-checked at compile time against the field's
Go type. Because expr's overloaded operators and functions accept the `bl.BlValue` interface, its own
strict checker is lenient about an undefined name in operand or argument position, so `bl.Expr` runs
its own pre-pass (`firstUndefined`) that rejects any reference to a name that is not a declared field.
A top-level field typed `bl.BlDictionary` is the way to model nested data: `applicant.address.postalCode`
resolves through the dictionary at runtime (member access is not deep-type-checked). Errors:

```go
// host-side (Go)
type ParseError struct { Source string; Err error } // from Expr (parse / undefined name / type-check)
type TypeError  struct { /* op, types */ }           // from Evaluate (runtime type mismatch)
type RegexError struct { Pattern string; Err error } // bad regex in matches/replace/extract
type CalendarRangeError struct { /* date, bounds */ }// business-day iteration past validity
type SchemaError struct { Path string; Err error }   // from bl.BlSchema validation (schema.go)
```

`[@test] ../../core/engine_test.go`

---

## Integration points

These are forward-looking notes; the referenced specs are **not** modified by this document.

- **`DecisionExpression`** ([decision-expression.spec.md](../decision-tasks/decision-expression.spec.md))
  compiles each entry's source string with `Expr`.
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
  FEEL function name is spelled in lowerCamelCase: `string length` → `stringLength`, `day of year` →
  `dayOfYear`, and `date and time` → `datetime` (a recognised compound, kept whole). *Rationale:*
  lowerCamelCase is the convention used by `expr`'s own built-ins (`hasPrefix`, `sortBy`, `toJSON`)
  and matches the casing of the host Go `Bl*` methods, so blkit functions read as native on both
  layers. The catalogue and the spokes use these spellings.
- **Renamed for clarity.** A handful of FEEL function names are renamed where the literal
  lowerCamelCase translation reads awkwardly or hides the function's role:
  - FEEL's `years and months duration(from, to)` becomes `ymDurationBetween(from, to)` — the
    `Between` suffix makes the date-difference role explicit and matches the parallel
    `dtDurationBetween` for the sibling duration type.
  - FEEL's polymorphic `duration(string)` (which returns either kind depending on the literal's
    designators) is split into typed sibling constructors **`ymDuration(string)`** and
    **`dtDuration(string)`**. Each accepts only its kind's designators and has a single typed
    return, so downstream usage is statically checkable; a wrong-kind string is a
    `ParseError`. This avoids the runtime-dispatch cost of a polymorphic constructor for a
    case (variable input strings) that's rare in practice.

  The catalogue and spokes use the renamed forms.

**Open decisions:**

- Which vendor extensions (the non-standard built-ins flagged in the spokes, e.g. `dateAdd`,
  `intersection`) are in scope.

### References

- OMG DMN 1.4 specification — the FEEL grammar and standard built-in function library.
- Trisotech Digital Enterprise Suite FEEL docs — standard FEEL function reference and temporal/list
  extensions.
- Drools / KIE [DMN FEEL handbook](https://kiegroup.github.io/dmn-feel-handbook/) — function
  reference and numeric-literal extensions.
- Camunda FEEL language guide — function signatures and worked examples.

---

## Edge cases

- An empty source string is a `ParseError`.
- An expression that evaluates to `null` is a valid result.
- A variable-free expression (`1 + 1`, `date("2025-01-01")`) needs no env: compile it with
  `bl.ExprNoEnv` and evaluate it against `bl.NoEnv{}`.
- A list index out of range returns `null`; a missing dictionary key returns `null`.

`[@test] ../../core/engine_test.go`
