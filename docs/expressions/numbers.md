# Numbers

> Exact decimal arithmetic and the numeric functions — rounding, absolute
> value, min and max, sum, and more.

In blkit, `number` is a single, exact type: an **arbitrary-precision decimal**.
There is no integer-versus-float split, no rounding error from binary floating
point, and no surprise when you add two prices together. Every number is exact
up to 34 significant digits (matching IEEE 754 decimal128), which is exactly the
property business arithmetic needs.

The headline consequence:

```
// expression-language
0.1 + 0.2        // → 0.3   (not 0.30000000000000004)
```

This page covers how to write numeric literals, the arithmetic and comparison
operators, the built-in numeric functions, how `null` propagates through them,
and how to construct numbers from your Go host code.

## Literals

A number literal is just the constant you write inside an expression. Decimal
and scientific forms are both accepted; a leading `-` is the unary minus
operator applied to a non-negative literal.

```
// expression-language
42            // → 42
3.14          // → 3.14
-5            // → -5
1500.50       // → 1500.5
1.5e3         // → 1500     (scientific notation)
```

A few rules worth knowing:

- **Decimal literals are exact.** Writing `0.1 + 0.2` gives you `0.3`, because
  blkit captures the literal's source text and parses it as a decimal before the
  expression is even compiled — it never passes through a lossy `float64`.
  Integer literals are exact too.
- **Scientific notation is accepted** (`1.5e3` is `1500`).
- **Hexadecimal is not supported.**
- **`NaN` and `Infinity` are not representable.** A source or host value of
  either is rejected as a type error rather than producing a number.

## Operators

The arithmetic and comparison operators all work on numbers and return either a
number or a boolean. Comparison and equality follow the language-wide
[three-valued `null` rules](booleans-and-logic.md).

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add | `2 + 3` | `5` |
| `-` | subtract / negate | `10 - 4`, `-(7)` | `6`, `-7` |
| `*` | multiply | `3 * 4` | `12` |
| `/` | divide | `10 / 4` | `2.5` |
| `**` | exponent | `2 ** 8`, `9 ** 0.5` | `256`, `3` |
| `< <= > >= = !=` | comparison | `5 < 10`, `3.0 = 3.00` | `true`, `true` |
| `between a and b` | inclusive range test | `5 between 1 and 10` | `true` |
| `in` | membership | `5 in [1..10]` | `true` |

Some behaviours that distinguish blkit from ordinary Go arithmetic:

- **Division by zero is `null`, not an error.** `5 / 0` evaluates to `null`, so
  a rule that hits a zero divisor degrades gracefully instead of failing.
- **`null` propagates.** `null + 1` is `null`; any arithmetic with a `null`
  operand yields `null`.
- **Equality ignores trailing zeros.** `3.0 = 3.00` is `true` — comparison is by
  numeric value.
- **A complex exponent result is `null`.** `(-2) ** 0.5` (a negative base with a
  fractional exponent) evaluates to `null` rather than an error.

```
// expression-language
5 / 0            // → null
null + 1         // → null
3.0 = 3.00       // → true
(-2) ** 0.5      // → null
```

## Built-in functions

blkit provides the DMN-standard numeric functions plus a set of practical
extensions. Extensions are flagged **ext** below (they have no DMN equivalent).
The `scale` argument, where it appears, is a count of decimal places.

### Rounding

There is a whole family of rounding functions because "round" means different
things in different business contexts. `round` is a strict alias of
`roundHalfUp`, matching Excel's `ROUND`.

| Function | Example | Result |
|---|---|---|
| `round(n, scale)` **ext** | `round(2.345, 2)` | `2.35` (alias of `roundHalfUp`; Excel `ROUND`) |
| `roundUp(n, scale)` | `roundUp(5.1, 0)` | `6` (always away from zero; Excel `ROUNDUP`) |
| `roundDown(n, scale)` | `roundDown(5.9, 0)` | `5` (always toward zero — truncation; Excel `ROUNDDOWN`) |
| `roundHalfUp(n, scale)` | `roundHalfUp(5.5, 0)` | `6` (halfway away from zero; `roundHalfUp(5.1, 0)` → `5`) |
| `roundHalfDown(n, scale)` | `roundHalfDown(5.5, 0)` | `5` (halfway toward zero; `roundHalfDown(5.9, 0)` → `6`) |
| `roundHalfEven(n, scale)` | `roundHalfEven(2.5, 0)` | `2` (ties round to the even neighbour — banker's rounding) |
| `floor(n[, scale])` | `floor(-1.56, 1)` | `-1.6` (always toward −∞) |
| `ceiling(n[, scale])` | `ceiling(-1.56, 1)` | `-1.5` (always toward +∞) |

`floor` and `ceiling` take an optional `scale`; called with one argument they
round to a whole number.

### Arithmetic and powers

| Function | Example | Result |
|---|---|---|
| `abs(n)` | `abs(-10)` | `10` |
| `modulo(dividend, divisor)` | `modulo(-10, 3)` | `2` (floor division; sign follows the divisor) |
| `sqrt(n)` | `sqrt(16)` | `4` (negative → `null`) |
| `exp(n)` | `exp(1)` | `≈2.718281828` (Euler's number) |
| `ln(n)` **ext** | `ln(2.718281828)` | `≈1` (natural log; 0 or negative → `null`) |
| `log(n[, base])` **ext** | `log(100)`, `log(8, 2)` | `2`, `3` (default base 10) |

### Predicates

These return a boolean — handy in conditions and as building blocks for guards.

| Function | Example | Result |
|---|---|---|
| `odd(n)` | `odd(5)` | `true` |
| `even(n)` | `even(2)` | `true` |
| `isPositive(n)` **ext** | `isPositive(5)` | `true` |
| `isNegative(n)` **ext** | `isNegative(-3)` | `true` |
| `isZero(n)` **ext** | `isZero(0)` | `true` |

### Bounding

| Function | Example | Result |
|---|---|---|
| `clamp(n, min, max)` **ext** | `clamp(150, 0, 100)` | `100` (`min > max` → `null`) |

`clamp` constrains `n` to the inclusive range `[min, max]`: values below `min`
become `min`, values above `max` become `max`.

### Aggregates over lists

The aggregate functions — `min`, `max`, `sum`, `mean`, `median`, `product`,
`stddev`, `mode` — operate on a list of numbers rather than a single number, so
they are documented on the [Lists](lists.md) page. The point-versus-interval
relations (`before`, `after`, `during`, `starts`, `finishes`, …) accept numbers
as points and live on the [Ranges](ranges.md) page.

## Null and edge-case behaviour

blkit prefers `null` over errors for the numeric operations that have no sensible
real-valued answer, so a single bad input doesn't blow up a whole rule:

- **Division and `modulo` by zero** → `null`, never an error.
- **`sqrt` of a negative number** → `null`. `sqrt(0)` is `0`.
- **`ln` or `log` of zero or a negative number** → `null`; `log` with
  `base <= 0` or `base = 1` → `null`.
- **A complex `**` result** (e.g. `(-2) ** 0.5`) → `null`.
- **`clamp` with `min > max`** → `null`.
- **Any arithmetic with a `null` operand** → `null`.
- **`NaN` / `Infinity`** as a source literal or host input → a type error (these
  values cannot exist inside the engine).

Beyond `null` handling, two precision facts are worth remembering:

- Arithmetic is exact decimal; a non-terminating division (such as `1 / 3`) is
  truncated at 34 significant digits.
- Equality is by numeric value, so trailing zeros never matter: `3.0 = 3.00`.

```
// expression-language
sqrt(-1)         // → null
ln(0)            // → null
modulo(-10, 3)   // → 2
clamp(150, 0, 100)   // → 100
```

## Constructing numbers from Go

To feed numbers into an expression you build a `bl.BlNumber` with the generic
constructor `bl.Number`. It accepts any Go numeric type, `bool`, a
`decimal.Decimal`, or a string:

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

// Integer and float inputs are infallible from the constraint's perspective;
// the error only fires for a NaN / Inf float.
var age,    _ = bl.Number(30)
var pi,     _ = bl.Number(3.14159)
var amount, _ = bl.Number(decimal.RequireFromString("1500.50"))

// Bool coerces to 0 / 1 — useful when wiring a flag into arithmetic.
var flag, _ = bl.Number(true)        // → bl.BlNumber(1)
```

The string form is the forgiving one: it parses a decimal and tolerates
thousands separators, currency symbols, and surrounding whitespace.

```go
// host-side (Go)
var price, _ = bl.Number("$1,234.56")   // → bl.BlNumber(1234.56)
var big,   _ = bl.Number("1,000.50")    // → bl.BlNumber(1000.5)
```

`bl.Number` returns `(bl.BlNumber, error)`. Integer types, `decimal.Decimal`,
and `bool` are infallible — the `_` slot just keeps the call site uniform. The
error only fires in two cases: a `float32`/`float64` holding `NaN` or `Inf`, and
a string that can't be parsed as a number after the format characters are
stripped.

Once you have a result back from `Evaluate`, recover the underlying decimal with
`Decimal()`, then use the `shopspring/decimal` API (`IntPart`, `Float64`,
`StringFixed`, …) for any further conversion:

```go
// host-side (Go)
type order struct {
    Price bl.BlNumber `expr:"price"`
    Qty   bl.BlNumber `expr:"qty"`
}

var total, _ = bl.Expr[order](`round(price * qty, 2)`)

var result, _ = total.Evaluate(order{Price: price, Qty: age})
var d = result.(bl.BlNumber).Decimal()   // shopspring/decimal value
```

### Parsing formatted text inside an expression

The string-parsing convenience is also available *inside* the language as the
`number` function. The one-argument form parses a plain decimal string; the
three-argument form lets you state the grouping and decimal separators
explicitly (useful for locales where the roles of `.` and `,` are swapped):

```
// expression-language
number("1500.50")              // → 1500.5
number("1.500,50", ".", ",")   // → 1500.5
```

The inverse, `string(n)`, renders a number as text and is documented on the
[Strings](strings.md) page. Note that a raw host-side string passed as an env
value must be a clean decimal — to accept a thousands-separated string from
within an expression, route it through `number(...)`.

## Where to go next

For the exhaustive, authoritative definition of every operator, function,
signature, and edge case, see the spec at `specs/expressions/number.spec.md` and
the generated [Reference](../reference/blkit.md). For how expressions compile
once and run many times, see [Architecture → Expressions](../architecture/expressions.md).
