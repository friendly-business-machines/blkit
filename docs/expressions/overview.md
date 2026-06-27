# Overview

> What blkit's expression language is, where it comes from, and the complete set
> of values it works with — its literal syntax at a glance, and how to test a
> value's type.

Expressions are the foundation blkit is built on. A decision rule, a unary test,
the condition that routes a process — all of them are blkit expressions. This
page introduces the language: where it comes from, and the closed set of values
every expression produces and consumes. Each value type then has its own guide
page that goes deep on its literals, operators, and built-in functions.

## What is an expression language?

An **expression language** is a domain-specific language (DSL) designed to
evaluate expressions — typically within a host environment like an application,
framework, or runtime — rather than to write full programs. The key distinction
from a general-purpose programming language is scope: expression languages are
deliberately limited to computing a value from some context (variables,
functions, data), without side effects like I/O, loops, or system calls. This
makes them safe to embed in configuration, rules engines, templates, and query
systems where you want to let users or operators define logic without exposing
the full power of a programming language.

Expression languages typically support arithmetic and logical operators,
property/field access, function calls, and conditional logic. What they usually
omit is variable assignment, I/O, class/function definitions, and other stateful
constructs. A single fragment like

```text
age >= 18 and income > 25000
```

is compiled once and then evaluated against many different contexts, each time
producing a value — here a boolean, but it could equally be a number, a string,
or a list.

The narrowness is the point. Software is full of small, self-contained
calculations and conditions —

- *"is this applicant over 18 and earning more than £25,000?"*
- *"what is 20% off this price?"*
- *"does this date fall inside the policy period?"*

— and these rules tend to change far more often, and need to be understood by far
more people, than the program that runs them. An expression language lets you
lift that logic out of the host program's control flow and treat it as **data**:
something you can store in a database, edit in a spreadsheet-like table, ship
without recompiling, and hand to an analyst rather than a programmer.

### Where you've already met them

Expression languages are everywhere, even where they aren't advertised as
languages:

- **Spreadsheet formulas** — Excel and Google Sheets put an expression language
  in front of hundreds of millions of non-programmers. `=IF(A1>18, "adult",
  "minor")` is an expression, evaluated against the cells it references.
- **SQL (`WHERE` / `SELECT` expressions)** — the most widely used expression
  language in the world, though embedded inside a query language. SQL's
  expression sublanguage — covering predicates, aggregates, `CASE`, and scalar
  functions — has deeply influenced everything that followed.
- **Path and query languages** — XPath selects nodes from an XML document;
  JSONPath and JMESPath do the same for JSON; search strings like
  Lucene/Elasticsearch and Jira's JQL filter records.
- **Configuration and policy** — CEL (Common Expression Language, used across
  Kubernetes and Google Cloud), Open Policy Agent's Rego, and the rule engines
  embedded in CI systems all evaluate expressions to make allow/deny decisions.
- **Jinja2 / Twig / Nunjucks** — template languages that embed expression
  evaluation. Jinja2 (Python/Ansible) is probably the most widely deployed in
  infrastructure tooling. The expression sublanguage — filters, tests,
  conditionals — is the influential core.
- **Embedded scripting** — engines built specifically to let an application
  accept user-authored logic safely, without handing over a full programming
  language. From the Java ecosystem come MVEL (MVFLEX Expression Language, used
  by rule engines such as Drools) and SpEL (Spring Expression Language, built
  into the Spring framework); CEL (above) is Google's, embeddable across Go, C++,
  and Java.
- **Lua** — while a full scripting language, Lua is most famous for its use as an
  embedded expression/scripting language in games (World of Warcraft, Roblox),
  databases (Redis), and web servers (Nginx/OpenResty). Its design prioritised
  being embedded cleanly.

What unites them is the same trade: give up general-purpose programming power
(no side effects, no unbounded loops, no I/O) in exchange for expressions that
are short, safe to evaluate on untrusted input, easy to reason about, and cheap
to run many times over.

## FEEL and DMN

blkit's language is modelled on **FEEL** — the **F**riendly **E**nough
**E**xpression **L**anguage — the expression language defined by the **DMN**
(Decision Model and Notation) standard published by the Object Management Group.
DMN is an industry standard for describing business decisions: the decision
tables, rules, and logic that sit behind questions like loan eligibility,
insurance pricing, or discount calculations. FEEL is the language those rules
are written in.

The name is a statement of intent. FEEL is meant to be *friendly enough* that a
business analyst — not just a software engineer — can read a rule and agree that
it says what the business means, while still being precise enough to execute
unambiguously. That goal pushes it away from the conventions of general-purpose
programming languages and towards the needs of business logic:

- **Exact decimal arithmetic.** Money and percentages are computed as exact
  decimals, so `0.1 + 0.2` is exactly `0.3` — they don't drift the way binary
  floating point does in most languages.
- **Readable, business-friendly syntax.** `between`, ranges like `[1..10]`,
  `if/then/else` as an expression, date and duration literals, and list
  comprehensions (`for`, `some`, `every`) read close to how the rule would be
  stated in English.

### How FEEL differs from other expression languages

Most embeddable expression languages — Expr, CEL, MVEL, JEXL — inherit the
semantics of the host platform they grew up on. Numbers are machine `int`s and
IEEE-754 floats, boolean logic is two-valued, and the syntax is broadly
C- or Java-flavoured. They are excellent general-purpose tools.

FEEL is different because it was designed *backwards from business rules* rather
than forwards from a programming language. The defining contrasts:

- **Decimals, not floats**, so financial arithmetic is correct by default rather
  than by careful rounding.
- **`null`-aware three-valued logic**, so partial data is a first-class case
  instead of an exception waiting to happen.
- **A grammar built for ranges and tests** — `[18..65]`, `> 1000`, `not(x)` as a
  *unary test* against an implicit input — which is exactly the shape of a cell
  in a decision table, the artifact FEEL exists to power.
- **Standardised, not vendor-specific.** Because FEEL is part of DMN, the same
  decision model is portable across conforming engines, in the way SQL is
  portable across databases.

blkit doesn't aim to be a conformant FEEL implementation, and it isn't a DMN
engine. It takes FEEL's good ideas — the exact arithmetic, the null handling,
the range-and-test syntax, and its overall *feel* — and adapts them to a
practical, type-safe Go library.

## The complete set of value types

Every blkit expression produces and consumes values drawn from one closed set of
types. A literal like `42` is a number; `"hello"` is a string; `[1, 2, 3]` is a
list; the result of `age >= 18` is a boolean. There are fifteen value types. Most
have a literal you write directly in source; a few are built only by a
constructor function or by host Go code.

| Type | Literal / constructor | Example | Guide |
|---|---|---|---|
| number | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` | [Numbers](numbers.md) |
| string | double quotes | `"hello"` | [Strings](strings.md) |
| boolean | keywords | `true`, `false` | [Booleans & logic](booleans-and-logic.md) |
| null | keyword | `null` | [Booleans & logic](booleans-and-logic.md) |
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
| calendar | host-built (no constructor) | — | [Values from Go](values-from-go.md) |

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

## Testing a value's type: `instance of`

`x instance of T` tests a value's type, where `T` is one of the type names from
the table above — including the non-literal types `regex`, `table`, and
`calendar`. It returns a boolean.

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

The type names are lowercase. The same type tags are available on the host side
for exhaustive matching on a result — see
[Values from Go](values-from-go.md).

## A type index

Each type's dedicated guide goes deep on its literals, operators, and built-in
functions:

- [Numbers](numbers.md) — exact decimals, arithmetic, the numeric library.
- [Strings](strings.md) — text, concatenation, inspection, and `pattern(...)`.
- [Booleans & logic](booleans-and-logic.md) — the truth tables, three-valued
  logic, and `null`.
- [Dates & times](dates-and-times.md) — dates, times, datetimes, and both
  duration kinds.
- [Lists](lists.md) — indexing, filtering, projection, comprehensions.
- [Dictionaries](dictionaries.md) — entries, path access, key presence.
- [Ranges](ranges.md) — intervals, membership, interval algebra.
- [Tables](tables.md) — rows, columns, transformation methods.
- [Values from Go](values-from-go.md) — building these values in host Go code.
