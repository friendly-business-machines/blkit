---
name: BlNumber
description: blkit's number type (modelled on FEEL) — an arbitrary-precision decimal value; extends BlExpr so all operations are deferred and chainable, with a single terminal .evaluate() call
targets:
  - ../../expr/number.go
---

# BlNumber

`BlNumber` is blkit's numeric type, modelled on FEEL's number — an arbitrary-precision decimal. There is no integer/float distinction (matching FEEL); every number is a decimal with exact representation up to the implementation precision limit.

`BlNumber` extends `BlExpr`. Every `BlNumber` instance is a **literal leaf node** in a deferred expression tree. Chaining any method returns a new `BlExpr` without computing anything. Call `.evaluate(context?)` once at the end to materialise the result.

```go
type BlNumber struct { BlExpr }

// Construction is via Bl.Number(value) where value is int, float64, or
// a decimal string. See bl.spec.md.

// Arithmetic — return BlNumber for type-safe chaining
func (n *BlNumber) Add(other BlExpr) BlNumber { ... }
func (n *BlNumber) Subtract(other BlExpr) BlNumber { ... }
func (n *BlNumber) Multiply(other BlExpr) BlNumber { ... }
func (n *BlNumber) Divide(other BlExpr) BlNumber { ... }
func (n *BlNumber) Remainder(other BlExpr) BlNumber { ... }
func (n *BlNumber) Modulo(other BlExpr) BlNumber { ... }
func (n *BlNumber) Negate() BlNumber { ... }
func (n *BlNumber) Abs() BlNumber { ... }
func (n *BlNumber) Power(exponent BlExpr) BlNumber { ... }
func (n *BlNumber) Clamp(min BlExpr, max BlExpr) BlNumber { ... }

// Rounding — return BlNumber
func (n *BlNumber) Floor(scale *BlExpr) BlNumber { ... }
func (n *BlNumber) Ceiling(scale *BlExpr) BlNumber { ... }
func (n *BlNumber) Round(scale BlExpr) BlNumber { ... }
func (n *BlNumber) RoundUp(scale BlExpr) BlNumber { ... }
func (n *BlNumber) RoundDown(scale BlExpr) BlNumber { ... }
func (n *BlNumber) RoundHalfUp(scale BlExpr) BlNumber { ... }
func (n *BlNumber) RoundHalfDown(scale BlExpr) BlNumber { ... }

// Mathematical functions — return BlNumber
func (n *BlNumber) Sqrt() BlNumber { ... }
func (n *BlNumber) Ln() BlNumber { ... }
func (n *BlNumber) Log(base *BlExpr) BlNumber { ... }
func (n *BlNumber) Exp() BlNumber { ... }

// Classification
func (n *BlNumber) IsOdd() BlExpr { ... }
func (n *BlNumber) IsEven() BlExpr { ... }
func (n *BlNumber) IsPositive() BlExpr { ... }
func (n *BlNumber) IsNegative() BlExpr { ... }
func (n *BlNumber) IsZero() BlExpr { ... }
func (n *BlNumber) IsInteger() BlExpr { ... }

// Range membership
func (n *BlNumber) Between(min BlExpr, max BlExpr) BlExpr { ... }
func (n *BlNumber) In(test BlExpr) BlExpr { ... }

// Range algebra
func (n *BlNumber) Before(other BlExpr) BlExpr { ... }
func (n *BlNumber) After(other BlExpr) BlExpr { ... }
func (n *BlNumber) Coincides(other BlExpr) BlExpr { ... }
func (n *BlNumber) During(other BlExpr) BlExpr { ... }
func (n *BlNumber) Starts(other BlExpr) BlExpr { ... }
func (n *BlNumber) Finishes(other BlExpr) BlExpr { ... }

// Comparison
func (n *BlNumber) Equals(other BlExpr) BlExpr { ... }
func (n *BlNumber) NotEqual(other BlExpr) BlExpr { ... }
func (n *BlNumber) LessThan(other BlExpr) BlExpr { ... }
func (n *BlNumber) LessThanOrEqual(other BlExpr) BlExpr { ... }
func (n *BlNumber) GreaterThan(other BlExpr) BlExpr { ... }
func (n *BlNumber) GreaterThanOrEqual(other BlExpr) BlExpr { ... }

// Eager host-language utilities (call only on a concrete value after .Evaluate())
func (n *BlNumber) CompareTo(other BlNumber) int { ... }
func (n *BlNumber) ToNativeInt() int { ... }
func (n *BlNumber) ToNativeFloat() float64 { ... }
func (n *BlNumber) ToDecimalString(scale *int) string { ... }
func (n *BlNumber) String() string { ... }
```

---

## Construction

`Bl.Number(value)` accepts a native integer, float, or decimal string. String input is preferred for values with many decimal places because float literals in host languages may already carry floating-point rounding error before `BlNumber` sees them.

Scientific notation strings (`"1.5e3"`) are accepted and normalised to plain decimal form. Strings with thousands separators (`"1,000"`) are rejected with a `BlTypeError` — use the `number(from, groupingSeparator, decimalSeparator)` built-in (modelled on FEEL's) to parse those.

`"NaN"`, `"Infinity"`, and `"-Infinity"` are invalid inputs and raise a `BlTypeError` immediately; `BlNumber` has no representation for non-finite values.

```go
Bl.Number(42).Evaluate()           // → BlNumber("42")
Bl.Number(3.14).Evaluate()         // → BlNumber("3.14")  (float; may carry host-precision loss)
Bl.Number("3.14").Evaluate()       // → BlNumber("3.14")  (exact)
Bl.Number("0.1").Add(Bl.Number("0.2")).Evaluate()
// → BlNumber("0.3")  (exact — not the 0.30000000000000004 of IEEE 754 floats)
Bl.Number("1.5e3").Evaluate()      // → BlNumber("1500")
Bl.Number("-0.005").Evaluate()     // → BlNumber("-0.005")
Bl.Number(0).IsZero().Evaluate()   // → BlBoolean.TRUE
```

---

## Arithmetic

All arithmetic methods are deferred: they return a new `BlExpr` node without computing anything. Operands are evaluated left-to-right when `.Evaluate()` is called.

### `add(other)`

Returns an expression that evaluates to the sum of `self` and `other`.

```go
Bl.Number(3).Add(Bl.Number(4)).Evaluate()
// → BlNumber("7")

Bl.Number("1.25").Add(Bl.Number("2.75")).Evaluate()
// → BlNumber("4.00")

Bl.NumberVar("base").Add(Bl.NumberVar("bonus")).Evaluate(
    map[string]BlExpr{"base": Bl.Number(50000), "bonus": Bl.Number(5000)},
)
// → BlNumber("55000")

// Chaining: (a + b) + c
Bl.Number(1).Add(Bl.Number(2)).Add(Bl.Number(3)).Evaluate()
// → BlNumber("6")
```

### `subtract(other)`

Returns an expression that evaluates to `self` minus `other`.

```go
Bl.Number(10).Subtract(Bl.Number(3)).Evaluate()
// → BlNumber("7")

Bl.Number("100.00").Subtract(Bl.Number("0.01")).Evaluate()
// → BlNumber("99.99")

Bl.NumberVar("price").Subtract(Bl.NumberVar("discount")).Evaluate(
    map[string]BlExpr{"price": Bl.Number("29.99"), "discount": Bl.Number("5.00")},
)
// → BlNumber("24.99")
```

### `multiply(other)`

Returns an expression that evaluates to the product of `self` and `other`.

```go
Bl.Number(6).Multiply(Bl.Number(7)).Evaluate()
// → BlNumber("42")

Bl.Number("1.5").Multiply(Bl.Number("2.0")).Evaluate()
// → BlNumber("3.0")

// Compound expression: price * quantity * (1 - discount_rate)
Bl.NumberVar("price").
    Multiply(Bl.NumberVar("qty")).
    Multiply(Bl.Number(1).Subtract(Bl.NumberVar("rate"))).
    Evaluate(map[string]BlExpr{"price": Bl.Number("9.99"), "qty": Bl.Number(3), "rate": Bl.Number("0.1")})
// → BlNumber("26.973")
```

### `divide(other)`

Returns an expression that evaluates to `self` divided by `other`. Evaluates to `BlNull` when the divisor is zero (matching FEEL semantics) — it is not an exception.

```go
Bl.Number(10).Divide(Bl.Number(4)).Evaluate()
// → BlNumber("2.5")

Bl.Number(1).Divide(Bl.Number(3)).Evaluate()
// → BlNumber("0.3333333333333333333333333333333333")  (34 significant digits)

Bl.Number(5).Divide(Bl.Number(0)).Evaluate()
// → BlNull.INSTANCE

Bl.NumberVar("total").Divide(Bl.NumberVar("count")).Evaluate(
    map[string]BlExpr{"total": Bl.Number(100), "count": Bl.Number(4)},
)
// → BlNumber("25")
```

### `remainder(other)` / `modulo(other)`

Both methods compute the floor-division remainder: the value `r` such that `self = other * floor(self / other) + r`. They are identical in behaviour; `modulo()` is the more intention-revealing name and aligns with the FEEL `modulo` built-in.

The sign of the result always matches the sign of `other` (floor division). This differs from truncation-based remainder (where the sign follows the dividend), which is what most host-language `%` operators provide for negative operands.

```go
Bl.Number(10).Modulo(Bl.Number(3)).Evaluate()
// → BlNumber("1")   (10 = 3*3 + 1)

Bl.Number(-10).Modulo(Bl.Number(3)).Evaluate()
// → BlNumber("2")   (-10 = 3*(-4) + 2; floor division)

Bl.Number(10).Modulo(Bl.Number(-3)).Evaluate()
// → BlNumber("-2")  (10 = (-3)*(-4) + (-2))

Bl.Number(7).Remainder(Bl.Number(2)).Evaluate()
// → BlNumber("1")

// Useful for cyclic calculations (e.g. day-of-week offset)
Bl.NumberVar("day_index").Modulo(Bl.Number(7)).Evaluate(
    map[string]BlExpr{"day_index": Bl.Number(10)},
)
// → BlNumber("3")
```

### `negate()`

Returns an expression that evaluates to the arithmetic negation of `self` (multiply by -1).

```go
Bl.Number(5).Negate().Evaluate()
// → BlNumber("-5")

Bl.Number(-3.5).Negate().Evaluate()
// → BlNumber("3.5")

Bl.Number(0).Negate().Evaluate()
// → BlNumber("0")   (negative zero is normalised to zero)

// Negate a variable
Bl.NumberVar("temperature").Negate().Evaluate(map[string]BlExpr{"temperature": Bl.Number(20)})
// → BlNumber("-20")
```

### `abs()`

Returns an expression that evaluates to the absolute (non-negative) value of `self`.

```go
Bl.Number(-7).Abs().Evaluate()
// → BlNumber("7")

Bl.Number(7).Abs().Evaluate()
// → BlNumber("7")

Bl.Number(0).Abs().Evaluate()
// → BlNumber("0")

// Distance between two values
Bl.NumberVar("a").Subtract(Bl.NumberVar("b")).Abs().Evaluate(
    map[string]BlExpr{"a": Bl.Number(3), "b": Bl.Number(8)},
)
// → BlNumber("5")
```

### `power(exponent)`

Returns an expression that evaluates to `self` raised to `exponent`. The exponent may be any `BlNumber` expression. Evaluates to `BlNull` if the result would be complex (e.g. a negative base with a non-integer exponent).

```go
Bl.Number(2).Power(Bl.Number(10)).Evaluate()
// → BlNumber("1024")

Bl.Number(9).Power(Bl.Number("0.5")).Evaluate()
// → BlNumber("3")   (square root via fractional exponent)

Bl.Number(2).Power(Bl.Number(-1)).Evaluate()
// → BlNumber("0.5")

Bl.Number(-2).Power(Bl.Number("0.5")).Evaluate()
// → BlNull.INSTANCE   (complex result)

Bl.Number(10).Power(Bl.Number(0)).Evaluate()
// → BlNumber("1")   (anything to the power of zero is 1)
```

### `clamp(min, max)`

Returns an expression that evaluates to `min` if `self < min`, `max` if `self > max`, and `self` otherwise. The result is always within the closed interval `[min, max]`.

Evaluates to `BlNull` if `min > max`.

```go
Bl.Number(5).Clamp(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlNumber("5")   (within range; unchanged)

Bl.Number(-3).Clamp(Bl.Number(0), Bl.Number(100)).Evaluate()
// → BlNumber("0")   (below min; clamped to min)

Bl.Number(150).Clamp(Bl.Number(0), Bl.Number(100)).Evaluate()
// → BlNumber("100")  (above max; clamped to max)

Bl.Number(5).Clamp(Bl.Number(10), Bl.Number(1)).Evaluate()
// → BlNull.INSTANCE  (min > max)

// Clamping a score to a 0–100 range
Bl.NumberVar("raw_score").Clamp(Bl.Number(0), Bl.Number(100)).Evaluate(
    map[string]BlExpr{"raw_score": Bl.Number(112)},
)
// → BlNumber("100")
```

---

## Rounding

All rounding methods take `scale` — the number of decimal places to keep in the result. `scale` must be a non-negative integer value. `floor()` and `ceiling()` accept an optional scale; when omitted they round to the nearest integer (scale = 0).

### `floor(scale?)`

Rounds toward negative infinity (always rounds down, regardless of sign). With no `scale`, returns the largest integer less than or equal to `self`.

```go
Bl.Number("3.7").Floor(nil).Evaluate()
// → BlNumber("3")

Bl.Number("-3.2").Floor(nil).Evaluate()
// → BlNumber("-4")   (toward negative infinity, not toward zero)

Bl.Number("3.456").Floor(Bl.Number(2)).Evaluate()
// → BlNumber("3.45")

Bl.Number("-3.451").Floor(Bl.Number(2)).Evaluate()
// → BlNumber("-3.46")  (floor at 2 dp: rounds down)

Bl.Number("3.000").Floor(Bl.Number(2)).Evaluate()
// → BlNumber("3.00")   (trailing zeros preserved at the given scale)
```

### `ceiling(scale?)`

Rounds toward positive infinity (always rounds up, regardless of sign). With no `scale`, returns the smallest integer greater than or equal to `self`.

```go
Bl.Number("3.2").Ceiling(nil).Evaluate()
// → BlNumber("4")

Bl.Number("-3.7").Ceiling(nil).Evaluate()
// → BlNumber("-3")   (toward positive infinity)

Bl.Number("3.451").Ceiling(Bl.Number(2)).Evaluate()
// → BlNumber("3.46")

Bl.Number("-3.451").Ceiling(Bl.Number(2)).Evaluate()
// → BlNumber("-3.45")  (ceiling at 2 dp: rounds up toward positive infinity)

Bl.Number("2.5").Ceiling(Bl.Number(0)).Evaluate()
// → BlNumber("3")
```

### `round(scale)` / `round_half_up(scale)`

`round()` is a convenience alias for `round_half_up()`. Both round to `scale` decimal places using the "round half up" (commercial rounding) rule: values exactly halfway between two representable numbers round away from zero.

```go
Bl.Number("3.455").Round(Bl.Number(2)).Evaluate()
// → BlNumber("3.46")   (0.005 rounds up)

Bl.Number("3.445").Round(Bl.Number(2)).Evaluate()
// → BlNumber("3.45")   (0.005 rounds up from 3.445 → 3.45)

Bl.Number("-3.455").RoundHalfUp(Bl.Number(2)).Evaluate()
// → BlNumber("-3.46")  (away from zero for negatives too)

Bl.Number("2.5").Round(Bl.Number(0)).Evaluate()
// → BlNumber("3")

Bl.Number("-2.5").RoundHalfUp(Bl.Number(0)).Evaluate()
// → BlNumber("-3")     (away from zero)

// Rounding a currency value to 2 decimal places
Bl.NumberVar("amount").Round(Bl.Number(2)).Evaluate(
    map[string]BlExpr{"amount": Bl.Number("12.345")},
)
// → BlNumber("12.35")
```

### `round_half_down(scale)`

Rounds to `scale` decimal places, with the halfway case rounding toward zero (the opposite of `round_half_up`).

```go
Bl.Number("3.455").RoundHalfDown(Bl.Number(2)).Evaluate()
// → BlNumber("3.45")   (halfway rounds toward zero / down)

Bl.Number("3.445").RoundHalfDown(Bl.Number(2)).Evaluate()
// → BlNumber("3.44")

Bl.Number("-3.455").RoundHalfDown(Bl.Number(2)).Evaluate()
// → BlNumber("-3.45")  (toward zero for negatives)

Bl.Number("2.5").RoundHalfDown(Bl.Number(0)).Evaluate()
// → BlNumber("2")
```

### `round_up(scale)`

Rounds away from zero regardless of the fractional part. Any non-zero fraction causes a magnitude increase.

```go
Bl.Number("3.001").RoundUp(Bl.Number(2)).Evaluate()
// → BlNumber("3.01")   (non-zero at third dp → round up)

Bl.Number("3.400").RoundUp(Bl.Number(2)).Evaluate()
// → BlNumber("3.40")   (already exact at 2 dp; no rounding needed)

Bl.Number("-3.001").RoundUp(Bl.Number(2)).Evaluate()
// → BlNumber("-3.01")  (away from zero for negatives)

Bl.Number("2.1").RoundUp(Bl.Number(0)).Evaluate()
// → BlNumber("3")
```

### `round_down(scale)`

Truncates toward zero: discards digits beyond `scale` without rounding. Equivalent to truncation.

```go
Bl.Number("3.999").RoundDown(Bl.Number(2)).Evaluate()
// → BlNumber("3.99")   (third dp discarded, no rounding)

Bl.Number("-3.999").RoundDown(Bl.Number(2)).Evaluate()
// → BlNumber("-3.99")  (toward zero for negatives)

Bl.Number("2.9").RoundDown(Bl.Number(0)).Evaluate()
// → BlNumber("2")

// Truncating cents to whole dollars
Bl.NumberVar("amount").RoundDown(Bl.Number(0)).Evaluate(
    map[string]BlExpr{"amount": Bl.Number("47.89")},
)
// → BlNumber("47")
```

---

## Mathematical Functions

### `sqrt()`

Returns an expression that evaluates to the non-negative square root of `self`. Evaluates to `BlNull` for negative input — blkit does not support complex numbers (matching FEEL). Evaluates to `BlNumber.zero()` for zero input.

```go
Bl.Number(9).Sqrt().Evaluate()
// → BlNumber("3")

Bl.Number(2).Sqrt().Evaluate()
// → BlNumber("1.4142135623730950488...")  (to implementation precision)

Bl.Number(0).Sqrt().Evaluate()
// → BlNumber("0")

Bl.Number(-1).Sqrt().Evaluate()
// → BlNull.INSTANCE

// Euclidean distance: sqrt(dx² + dy²)
Bl.NumberVar("dx").Power(Bl.Number(2)).
    Add(Bl.NumberVar("dy").Power(Bl.Number(2))).
    Sqrt().
    Evaluate(map[string]BlExpr{"dx": Bl.Number(3), "dy": Bl.Number(4)})
// → BlNumber("5")
```

### `ln()`

Returns an expression that evaluates to the natural logarithm (base *e*) of `self`. Evaluates to `BlNull` for zero or negative input.

```go
Bl.Number(1).Ln().Evaluate()
// → BlNumber("0")   (ln(1) = 0)

Bl.Number("2.718281828459045").Ln().Evaluate()
// → BlNumber("1")   (ln(e) ≈ 1, to precision)

Bl.Number(10).Ln().Evaluate()
// → BlNumber("2.302585092994045684...")

Bl.Number(0).Ln().Evaluate()
// → BlNull.INSTANCE

Bl.Number(-5).Ln().Evaluate()
// → BlNull.INSTANCE

// Continuous compounding: P * e^(r*t) → solve for t: ln(A/P) / r
Bl.NumberVar("ratio").Ln().Divide(Bl.NumberVar("rate")).Evaluate(
    map[string]BlExpr{"ratio": Bl.Number(2), "rate": Bl.Number("0.07")},
)
// → BlNumber("9.902...")  (years to double at 7% continuous compounding)
```

### `log(base?)`

Returns an expression that evaluates to the base-N logarithm of `self`. When `base` is omitted or evaluates to `BlNull`, base 10 is used. Evaluates to `BlNull` for zero or negative input, and for `base <= 0` or `base == 1`.

Use `ln()` for the natural logarithm — `log()` is exclusively for configurable-base logarithms.

```go
Bl.Number(100).Log(nil).Evaluate()
// → BlNumber("2")   (log₁₀(100) = 2)

Bl.Number(1000).Log(nil).Evaluate()
// → BlNumber("3")

Bl.Number(8).Log(Bl.Number(2)).Evaluate()
// → BlNumber("3")   (log₂(8) = 3)

Bl.Number(81).Log(Bl.Number(3)).Evaluate()
// → BlNumber("4")   (log₃(81) = 4)

Bl.Number(1).Log(nil).Evaluate()
// → BlNumber("0")   (log of 1 in any base is 0)

Bl.Number(0).Log(nil).Evaluate()
// → BlNull.INSTANCE

Bl.Number(10).Log(Bl.Number(1)).Evaluate()
// → BlNull.INSTANCE   (base 1 is undefined)

Bl.Number(10).Log(Bl.Number(-2)).Evaluate()
// → BlNull.INSTANCE   (negative base is undefined)
```

### `exp()`

Returns an expression that evaluates to *e* raised to the power of `self` (the inverse of `ln()`). Equivalent to `power(Bl.NumberVar("e"))` but computed with full precision using the host's exponential function.

```go
Bl.Number(0).Exp().Evaluate()
// → BlNumber("1")   (e⁰ = 1)

Bl.Number(1).Exp().Evaluate()
// → BlNumber("2.718281828459045235...")   (e¹ = e)

Bl.Number(2).Exp().Evaluate()
// → BlNumber("7.389056098930650227...")   (e²)

Bl.Number(-1).Exp().Evaluate()
// → BlNumber("0.367879441171442321...")   (e⁻¹ = 1/e)

// Continuous compounding: P * e^(r*t)
Bl.NumberVar("principal").
    Multiply(Bl.NumberVar("rate").Multiply(Bl.NumberVar("years")).Exp()).
    Evaluate(map[string]BlExpr{"principal": Bl.Number(1000), "rate": Bl.Number("0.05"), "years": Bl.Number(10)})
// → BlNumber("1648.72...")
```

---

## Classification

Classification methods return a deferred `BlExpr` that evaluates to `BlBoolean`. They are most useful when the number comes from a variable or expression rather than a literal.

### `is_odd()` / `is_even()`

`is_odd()` evaluates to `BlBoolean.TRUE` if `self` is an odd integer. `is_even()` evaluates to `BlBoolean.TRUE` if `self` is an even integer (including zero). Both evaluate to `BlBoolean.FALSE` for non-integer values.

```go
Bl.Number(3).IsOdd().Evaluate()
// → BlBoolean.TRUE

Bl.Number(4).IsOdd().Evaluate()
// → BlBoolean.FALSE

Bl.Number(0).IsEven().Evaluate()
// → BlBoolean.TRUE

Bl.Number(2).IsEven().Evaluate()
// → BlBoolean.TRUE

Bl.Number("3.5").IsOdd().Evaluate()
// → BlBoolean.FALSE   (not an integer)

Bl.Number(-7).IsOdd().Evaluate()
// → BlBoolean.TRUE

// Guard: only process even-numbered items
Bl.NumberVar("item_index").IsEven().Evaluate(map[string]BlExpr{"item_index": Bl.Number(4)})
// → BlBoolean.TRUE
```

### `is_positive()` / `is_negative()` / `is_zero()`

`is_positive()` — evaluates to `TRUE` if `self > 0`.
`is_negative()` — evaluates to `TRUE` if `self < 0`.
`is_zero()` — evaluates to `TRUE` if `self == 0`. Zero is neither positive nor negative.

```go
Bl.Number(5).IsPositive().Evaluate()
// → BlBoolean.TRUE

Bl.Number(-5).IsPositive().Evaluate()
// → BlBoolean.FALSE

Bl.Number(0).IsPositive().Evaluate()
// → BlBoolean.FALSE

Bl.Number(-3).IsNegative().Evaluate()
// → BlBoolean.TRUE

Bl.Number(0).IsNegative().Evaluate()
// → BlBoolean.FALSE

Bl.Number(0).IsZero().Evaluate()
// → BlBoolean.TRUE

Bl.Number("0.001").IsZero().Evaluate()
// → BlBoolean.FALSE

// Validate that a balance hasn't gone negative
Bl.NumberVar("balance").IsNegative().Evaluate(map[string]BlExpr{"balance": Bl.Number(-50)})
// → BlBoolean.TRUE
```

### `is_integer()`

Evaluates to `BlBoolean.TRUE` if `self` has no fractional component (i.e. it is a whole number). The sign does not matter.

```go
Bl.Number(5).IsInteger().Evaluate()
// → BlBoolean.TRUE

Bl.Number("5.0").IsInteger().Evaluate()
// → BlBoolean.TRUE   (5.0 is mathematically an integer)

Bl.Number("5.1").IsInteger().Evaluate()
// → BlBoolean.FALSE

Bl.Number(-3).IsInteger().Evaluate()
// → BlBoolean.TRUE

Bl.Number(0).IsInteger().Evaluate()
// → BlBoolean.TRUE

// Require whole-number quantities
Bl.NumberVar("quantity").IsInteger().Evaluate(map[string]BlExpr{"quantity": Bl.Number("2.5")})
// → BlBoolean.FALSE
```

---

## Range Membership

### `between(min, max)`

Evaluates to `BlBoolean.TRUE` if `min <= self <= max` (both bounds inclusive). Equivalent to `self.GreaterThanOrEqual(min).And(self.LessThanOrEqual(max))` but expressed as a single readable method.

```go
Bl.Number(5).Between(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(1).Between(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlBoolean.TRUE   (inclusive lower bound)

Bl.Number(10).Between(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlBoolean.TRUE   (inclusive upper bound)

Bl.Number(0).Between(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlBoolean.FALSE

Bl.Number(11).Between(Bl.Number(1), Bl.Number(10)).Evaluate()
// → BlBoolean.FALSE

// Validate a percentage is in range
Bl.NumberVar("rate").Between(Bl.Number(0), Bl.Number(100)).Evaluate(
    map[string]BlExpr{"rate": Bl.Number(75)},
)
// → BlBoolean.TRUE
```

### `in_(test)`

Applies a membership check to `self`. Inherited from `BlExpr` and listed here for discoverability on numeric values.

`test` may be a `BlRange`, `BlList`, or `BlCalendar`.

```go
Bl.Number(150).In(Bl.Range(Bl.Number(100), nil, false, true)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(50).In(Bl.Range(Bl.Number(100), nil, false, true)).Evaluate()
// → BlBoolean.FALSE

Bl.Number(25).In(Bl.Range(Bl.Number(18), Bl.Number(65), true, true)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(17).In(Bl.Range(Bl.Number(18), Bl.Number(65), true, true)).Evaluate()
// → BlBoolean.FALSE

Bl.Number(25).In(Bl.Range(Bl.Number(18), Bl.Number(65))).Evaluate()
// → BlBoolean.TRUE
```

---

## Range Algebra

The six range-algebra methods are modelled on the point/range relationship functions described in DMN 1.4 §10.3.2. Each method's `other` argument may be either a **point** (another `BlNumber`) or a **range** (`BlRange`). The semantics differ depending on which is provided.

All methods evaluate to `BlBoolean`. When the combination of point/range arguments is semantically undefined (e.g. `coincides(range)` for a point), the result is `BlNull`.

### `before(other)`

Evaluates to `TRUE` if `self` is entirely to the left of `other` on the number line — i.e. every possible value of `self` is strictly less than every value in `other`.

```go
// Point vs point
Bl.Number(3).Before(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(5).Before(Bl.Number(5)).Evaluate()
// → BlBoolean.FALSE   (equal, not before)

// Point vs range (closed lower bound)
Bl.Number(3).Before(Bl.Range(Bl.Number(5), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (3 < 5)

Bl.Number(5).Before(Bl.Range(Bl.Number(5), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.FALSE   (5 is the start of the closed range, so it's inside)

Bl.Number(5).Before(Bl.Range(Bl.Number(5), Bl.Number(10), false, false)).Evaluate()
// → BlBoolean.TRUE    (5 is excluded by the open bound, so 5 is before the range)
```

### `after(other)`

Evaluates to `TRUE` if `self` is entirely to the right of `other` on the number line. Converse of `before`.

```go
// Point vs point
Bl.Number(7).After(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(5).After(Bl.Number(5)).Evaluate()
// → BlBoolean.FALSE

// Point vs range (closed upper bound)
Bl.Number(11).After(Bl.Range(Bl.Number(5), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (11 > 10)

Bl.Number(10).After(Bl.Range(Bl.Number(5), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.FALSE   (10 is inside the closed range)

Bl.Number(10).After(Bl.Range(Bl.Number(5), Bl.Number(10), false, false)).Evaluate()
// → BlBoolean.TRUE    (10 is excluded by the open bound)
```

### `coincides(other)`

Evaluates to `TRUE` if `self` is at the same point as `other`. Meaningful only when `other` is also a point; evaluates to `BlNull` when `other` is a range (a point cannot coincide with a range).

```go
Bl.Number(5).Coincides(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(5).Coincides(Bl.Number(6)).Evaluate()
// → BlBoolean.FALSE

Bl.Number("5.0").Coincides(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE   (5.0 == 5 numerically)

Bl.Number(5).Coincides(Bl.Range(Bl.Number(5), Bl.Number(5), true, true)).Evaluate()
// → BlNull.INSTANCE   (point cannot coincide with a range)
```

### `during(other)`

Evaluates to `TRUE` if `self` falls strictly within `other`, respecting the range's boundary inclusion flags. Meaningful only when `other` is a range; evaluates to `BlNull` when `other` is a point.

```go
Bl.Number(5).During(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (1 ≤ 5 ≤ 10)

Bl.Number(1).During(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (closed bound; 1 is included)

Bl.Number(1).During(Bl.Range(Bl.Number(1), Bl.Number(10), false, false)).Evaluate()
// → BlBoolean.FALSE   (open bound; 1 is excluded)

Bl.Number(10).During(Bl.Range(Bl.Number(1), Bl.Number(10), true, false)).Evaluate()
// → BlBoolean.FALSE   (open upper bound; 10 is excluded)

Bl.Number(0).During(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.FALSE   (below range)

Bl.Number(5).During(Bl.Number(5)).Evaluate()
// → BlNull.INSTANCE   (other is a point, not a range)
```

### `starts(other)`

Evaluates to `TRUE` if `self` equals the start point of `other` and that start boundary is closed (included). Meaningful only when `other` is a range; evaluates to `BlNull` when `other` is a point.

```go
Bl.Number(1).Starts(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (self == range.start and start is closed)

Bl.Number(1).Starts(Bl.Range(Bl.Number(1), Bl.Number(10), false, false)).Evaluate()
// → BlBoolean.FALSE   (open start; 1 is excluded, cannot start it)

Bl.Number(5).Starts(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.FALSE   (5 is not the start point)

Bl.Number(1).Starts(Bl.Number(1)).Evaluate()
// → BlNull.INSTANCE   (other is a point, not a range)
```

### `finishes(other)`

Evaluates to `TRUE` if `self` equals the end point of `other` and that end boundary is closed (included). Meaningful only when `other` is a range; evaluates to `BlNull` when `other` is a point.

```go
Bl.Number(10).Finishes(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.TRUE    (self == range.end and end is closed)

Bl.Number(10).Finishes(Bl.Range(Bl.Number(1), Bl.Number(10), false, false)).Evaluate()
// → BlBoolean.FALSE   (open end; 10 is excluded)

Bl.Number(10).Finishes(Bl.Range(Bl.Number(1), Bl.Number(10), true, false)).Evaluate()
// → BlBoolean.FALSE   (open upper bound)

Bl.Number(5).Finishes(Bl.Range(Bl.Number(1), Bl.Number(10), true, true)).Evaluate()
// → BlBoolean.FALSE   (5 is not the end point)
```

---

## Comparison

All comparison methods return a deferred `BlExpr` that evaluates to `BlBoolean`. Two `BlNumber`s are equal if and only if their numeric values are equal, regardless of trailing zeros (`3.0 == 3.00`).

```go
Bl.Number(5).Equals(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number("3.0").Equals(Bl.Number("3.00")).Evaluate()
// → BlBoolean.TRUE   (numerically identical)

Bl.Number(5).NotEqual(Bl.Number(6)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(3).LessThan(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(5).LessThan(Bl.Number(5)).Evaluate()
// → BlBoolean.FALSE

Bl.Number(5).LessThanOrEqual(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(7).GreaterThan(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

Bl.Number(5).GreaterThanOrEqual(Bl.Number(5)).Evaluate()
// → BlBoolean.TRUE

// Comparison with a variable
Bl.NumberVar("score").GreaterThanOrEqual(Bl.Number(60)).Evaluate(
    map[string]BlExpr{"score": Bl.Number(72)},
)
// → BlBoolean.TRUE
```

---

## Eager Host-Language Utilities

These methods operate eagerly on a **concrete** `BlNumber` value — they must only be called on the result of `.Evaluate()`, never on an unevaluated expression node.

### `compare_to(other)`

Returns `-1` if `self < other`, `0` if equal, `1` if `self > other`. Useful for implementing native sort comparators.

```go
result, err := Bl.Number(3).Evaluate()
result.CompareTo(Bl.Number(5))   // → -1
result.CompareTo(Bl.Number(3))   // → 0
result.CompareTo(Bl.Number(1))   // → 1
```

### `to_native_int()`

Truncates the decimal and returns a host-language integer. Raises `OverflowError` if the value exceeds the host language's integer range. Does not round — use a rounding method first if precision matters.

```go
Bl.Number("7.9").Evaluate().ToNativeInt()   // → 7  (truncated, not rounded)
Bl.Number("-3.2").Evaluate().ToNativeInt()  // → -3
Bl.Number(42).Evaluate().ToNativeInt()      // → 42
```

### `to_native_float()`

Returns the nearest host-language float. Precision may be lost for values with more significant digits than the host float type supports (typically 15–17 decimal digits for IEEE 754 double).

```go
Bl.Number("3.14159265358979323846").Evaluate().ToNativeFloat()
// → 3.141592653589793  (IEEE 754 double; last digits lost)
```

### `to_decimal_string(scale?)`

Returns a decimal string representation. When `scale` is provided, the result has exactly that many decimal places (trailing zeros included). When omitted, returns the minimal string that uniquely identifies the value.

```go
Bl.Number("3.14").Evaluate().ToDecimalString(nil)      // → "3.14"
Bl.Number(3).Evaluate().ToDecimalString(nil)           // → "3"
Bl.Number(3).Evaluate().ToDecimalString(2)             // → "3.00"
Bl.Number("3.14159").Evaluate().ToDecimalString(2)     // → "3.14"  (truncated, not rounded)
```

---

## Precision

`BlNumber` must maintain arbitrary precision through all arithmetic operations. The recommended backing type is `github.com/shopspring/decimal` or `math/big.Float` with sufficient precision.

Division that produces a non-terminating decimal is truncated at a configurable precision limit (default: 34 significant digits, matching IEEE 754 decimal128).

---

## Edge Cases

- `Bl.Number("NaN")` and `Bl.Number("Infinity")` raise a `BlTypeError` at construction time.
- `power()` with a non-integer or negative exponent producing a complex result evaluates to `BlNull`.
- `sqrt()` of zero evaluates to `BlNumber.zero()`.
- `ln()` and `log()` of zero or a negative number evaluate to `BlNull`.
- `log()` with `base == 1` or `base <= 0` evaluates to `BlNull`.
- `clamp(min, max)` with `min > max` evaluates to `BlNull`.
- `round()` is a strict alias for `round_half_up()`.
- `modulo()` and `remainder()` are identical; neither raises on a zero divisor — the result is `BlNull`.
- `to_native_int()` truncates (does not round) and raises `OverflowError` if out of range.
- Numeric equality ignores trailing zeros: `Bl.Number("3.0").Equals(Bl.Number("3.00"))` evaluates to `TRUE`.
