# Numbers

> Exact decimal arithmetic and the numeric functions — literals, operators,
> rounding, absolute value, powers, predicates, and more.

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
operators, and the built-in numeric functions.

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

## Operators

The arithmetic and comparison operators all work on numbers and return either a
number or a boolean.

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

- **`/` always yields a decimal.** `10 / 4` is `2.5`, not a truncated `2` —
  there is no integer division.
- **Equality ignores trailing zeros.** `3.0 = 3.00` is `true` — comparison is by
  numeric value.

```
// expression-language
10 / 4           // → 2.5
3.0 = 3.00       // → true
```

## Built-in functions

blkit provides a library of numeric functions. The `scale` argument, where it
appears, is a count of decimal places.

### Rounding

There is a whole family of rounding functions because "round" means different
things in different business contexts. `round` is a strict alias of
`roundHalfUp`, matching Excel's `ROUND`.

| Function | Example | Result |
|---|---|---|
| `round(n, scale)` | `round(2.345, 2)` | `2.35` (alias of `roundHalfUp`; Excel `ROUND`) |
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
| `sqrt(n)` | `sqrt(16)` | `4` |
| `exp(n)` | `exp(1)` | `≈2.718281828` (Euler's number) |
| `ln(n)` | `ln(2.718281828)` | `≈1` (natural log) |
| `log(n[, base])` | `log(100)`, `log(8, 2)` | `2`, `3` (default base 10) |

### Predicates

These return a boolean — handy in conditions and as building blocks for guards.

| Function | Example | Result |
|---|---|---|
| `odd(n)` | `odd(5)` | `true` |
| `even(n)` | `even(2)` | `true` |
| `isPositive(n)` | `isPositive(5)` | `true` |
| `isNegative(n)` | `isNegative(-3)` | `true` |
| `isZero(n)` | `isZero(0)` | `true` |

### Bounding

| Function | Example | Result |
|---|---|---|
| `clamp(n, min, max)` | `clamp(150, 0, 100)` | `100` |

`clamp` constrains `n` to the inclusive range `[min, max]`: values below `min`
become `min`, values above `max` become `max`.

### Aggregates over lists

The aggregate functions — `min`, `max`, `sum`, `mean`, `median`, `product`,
`stddev`, `mode` — operate on a list of numbers rather than a single number, so
they are documented on the [Lists](lists.md) page. The point-versus-interval
relations (`before`, `after`, `during`, `starts`, `finishes`, …) accept numbers
as points and live on the [Ranges](ranges.md) page.

## Parsing formatted text inside an expression

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
[Strings](strings.md) page.

## Numbers from Go

Host Go code builds `number` values with the `bl.Number` constructor (any Go
numeric type, `bool`, a `decimal.Decimal`, or a string) and reads a result back
with `Decimal()`. See [Values from Go](values-from-go.md) for the full host-side
story.
