---
name: BlNumber
description: The number type in the blkit expression language — an arbitrary-precision decimal. Covers numeric literals, arithmetic/comparison operators, the numeric built-in functions, and the Go layer (BlNumber + expr operator/function registrations) that implements them on expr-lang/expr.
targets:
  - ../../expr/number.go
---

# BlNumber — the `number` type

`number` is blkit's numeric type: an **arbitrary-precision decimal**. There is no integer/float
distinction — every number is an exact decimal up to the implementation precision limit (34
significant digits, matching IEEE 754 decimal128). The Go value type backing it is `BlNumber`.

See [bl-expr.spec.md](bl-expr.spec.md) for the engine, operator precedence, and the cross-cutting
null/error semantics referenced below.

---

## Literals

A **number literal** is the syntactic form used inside a blkit expression to write a constant
numeric value — for example, the `42` in `count(items) = 42`. Decimal and scientific forms are
accepted; a leading `-` is the unary minus operator applied to a non-negative literal.

```
42            // → 42
3.14          // → 3.14
-5            // → -5
1500.50       // → 1500.5
1.5e3         // → 1500        (scientific notation)
```

- Decimals are exact: `0.1 + 0.2 // → 0.3` (not `0.30000000000000004`).
- Scientific notation is accepted. **Hexadecimal is not** (see
  [bl-expr.spec.md](bl-expr.spec.md#relationship-to-feel-and-future-direction)).
- `NaN` and `Infinity` are not representable — a source/host value of either is a `BlTypeError`.

`[@test] ../../expr/number_test.go`

---

## Operators

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

- **Division by zero → `null`** (not an error): `5 / 0 // → null`.
- `null` propagates: `null + 1 // → null`.
- Equality ignores trailing zeros: `3.0 = 3.00 // → true`.
- `**` with a result that would be complex (e.g. negative base, fractional exponent) → `null`:
  `(-2) ** 0.5 // → null`.

`[@test] ../../expr/number_operators_test.go`

---

## Built-in functions

Standard DMN functions plus blkit extensions (**ext**, flagged — no DMN equivalent). Signatures use
`name(arg: type): returnType`. `scale` is a decimal-place count.

| Function | Example | Result |
|---|---|---|
| `roundHalfEven(n, scale)` | `roundHalfEven(2.5, 0)` | `2` (ties round to the even neighbour; also known as banker's rounding) |
| `floor(n[, scale])` | `floor(-1.56, 1)` | `-1.6` (always toward −∞) |
| `ceiling(n[, scale])` | `ceiling(-1.56, 1)` | `-1.5` (always toward +∞) |
| `round(n, scale)` **ext** | `round(2.345, 2)` | `2.35` (alias of `roundHalfUp`; Excel `ROUND`) |
| `roundUp(n, scale)` | `roundUp(5.1, 0)` | `6` (always rounds away from zero; Excel `ROUNDUP`) |
| `roundDown(n, scale)` | `roundDown(5.9, 0)` | `5` (always toward zero — truncation; Excel `ROUNDDOWN`) |
| `roundHalfUp(n, scale)` | `roundHalfUp(5.5, 0)` | `6` (halfway away from zero; `roundHalfUp(5.1, 0)` → `5`; Excel `ROUND`) |
| `roundHalfDown(n, scale)` | `roundHalfDown(5.5, 0)` | `5` (halfway toward zero; `roundHalfDown(5.9, 0)` → `6`) |
| `abs(n)` | `abs(-10)` | `10` |
| `modulo(dividend, divisor)` | `modulo(-10, 3)` | `2` (floor division; sign follows divisor) |
| `sqrt(n)` | `sqrt(16)` | `4` (negative → `null`) |
| `exp(n)` | `exp(1)` | `≈2.718281828` (Euler's number) |
| `ln(n)` **ext** | `ln(2.718281828)` | `≈1` (natural log; 0/negative → `null`) |
| `log(n[, base])` **ext** | `log(100)`, `log(8, 2)` | `2`, `3` (default base 10) |
| `odd(n)` | `odd(5)` | `true` |
| `even(n)` | `even(2)` | `true` |
| `isPositive(n)` **ext** | `isPositive(5)` | `true` |
| `isNegative(n)` **ext** | `isNegative(-3)` | `true` |
| `isZero(n)` **ext** | `isZero(0)` | `true` |
| `clamp(n, min, max)` **ext** | `clamp(150, 0, 100)` | `100` (`min > max` → `null`) |

Aggregates over lists (`min`, `max`, `sum`, `mean`, `median`, `product`, `stddev`, `mode`) are in
[list.spec.md](list.spec.md). Conversion to/from text — `number(from, groupingSep, decimalSep)` and
`string(n)` — is documented under [§ Go implementation](#go-implementation-expr-extension) and
[string.spec.md](string.spec.md).

`[@test] ../../expr/number_functions_test.go`

### Interval algebra

A number is a *point*; the FEEL interval-algebra built-ins (`before`, `after`, `coincides`,
`during`, `starts`, `finishes`, …) accept points and ranges and are documented in
[range.spec.md](range.spec.md). Examples: `before(3, 5) // → true`; `during(5, [1..10]) // → true`.

---

## Semantics & behaviour

- **Arbitrary precision** — exact decimal arithmetic; non-terminating division is truncated at 34
  significant digits.
- **Division by zero** and **complex `**` results** evaluate to `null`.
- **`ln`/`log`** of zero or a negative number, and `log` with `base <= 0` or `base = 1`, → `null`.
- **`modulo`** uses floor division — the result's sign follows the divisor; it never raises on a zero
  divisor (→ `null`).
- **Equality** is by numeric value, ignoring trailing zeros.

`[@test] ../../expr/number_semantics_test.go`

---

---

## Go implementation (expr extension)

Lives in `expr/number.go`. Mechanics (calling convention, bridging, operator binding, the `…Options()`
assembly) are defined once in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go);
this section gives the concrete value type, host API, and registrations.

### Value type & host API (exported)

`BlNumber` is the immutable Go value type that represents a number inside the engine and at the
host-code boundary. Its only field is private (`d`) so callers cannot mutate the underlying
decimal — every operation in the library returns a fresh `BlNumber`.

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `isBlValue()` is the sealing mechanism: because it's unexported, no type outside
  this package can satisfy `BlValue`, preventing host code from inventing fake values that the
  engine would then have to defend against. `String()` doubles as the `fmt.Stringer`
  implementation, so `fmt.Println(n)` produces a readable form rather than the raw struct.
- **`Number[T NumberInput](v T)`** — the generic host constructor. The `NumberInput` constraint
  is the compile-time gate on what host code is allowed to pass in; anything outside the union
  is rejected by the compiler rather than failing at runtime. The `error` return only fires at
  runtime for the two cases the constraint cannot rule out: a `float32`/`float64` that holds
  `NaN`/`Inf`, and a `string` that cannot be parsed after format-stripping. All other input
  types (integers, `bool`, `decimal.Decimal`) are infallible — the conversion always succeeds.
- **`Decimal()` accessor** — hands the underlying `shopspring/decimal` value back to host code.
  This is the only conversion accessor needed because `shopspring/decimal` itself provides
  `IntPart()`, `Float64()`, `StringFixed()`, `BigInt()` and so on — there's no point blkit
  wrapping each of those.

```go
// BlNumber wraps an arbitrary-precision decimal (backing type: github.com/shopspring/decimal).
type BlNumber struct{ d decimal.Decimal }

// BlValue interface — required by all Bl* value types.
func (BlNumber) Type() BlType { return BlTypeNumber }
func (n BlNumber) Equal(other BlValue) BlValue   // three-valued (BlBoolean/BlNull)
func (n BlNumber) String() string
func (BlNumber) isBlValue() {}

// Host constructor — accepts any Go numeric type, bool (true→1, false→0),
// decimal string, or shopspring/decimal.
// float32/float64: returns error if v is NaN or Inf.
// string: accepts plain decimals ("3.14", "-5", "1.5e3"), thousands
//   separators ("1,000.50"), currency symbols ("$3.14", "£5.00",
//   "€1,234.56"), and leading/trailing whitespace. Returns error if
//   v cannot be parsed as a number after stripping these.
type NumberInput interface {
    int | int8 | int16 | int32 | int64 |
    uint | uint8 | uint16 | uint32 | uint64 |
    float32 | float64 |
    bool | string | decimal.Decimal
}
func Number[T NumberInput](v T) (BlNumber, error)

// Host accessor (consume an evaluated result).
func (n BlNumber) Decimal() decimal.Decimal         // underlying value; use shopspring/decimal API for further conversion
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlNumber` and cannot apply Go's native `+`/`-`/`<`/etc. to
blkit values. For every operator that should work on two numbers, blkit supplies a named Go
function that performs the operation in `shopspring/decimal` and returns the result wrapped as a
`BlValue`. The connection from operator token to function happens in two steps, neither of which
is unique to `BlNumber`:

1. The Registrations section below calls `expr.Function("addNumbers", typed2(addNumbers), …)`,
   which makes the engine aware of the function under that exact string name and records its type
   signature.
2. A central `operatorBindings()` in [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then
   calls `expr.Operator("+", "addNumbers", "concatStrings", "addDateYM", …)`, which tells the
   engine "when you see `+` at parse time, try each of these registered functions in turn and
   dispatch to whichever one's signature matches the operand types." This step is centralised in
   one place because a single operator spans many types — `+` covers number addition, string
   concatenation, date+duration, time+duration, and duration+duration — and `expr.Operator`
   needs the full list of candidates for each operator in a single call.

So when the parser encounters `a + b` and both operands type-check to `BlNumber`, the engine
finds `addNumbers` in the `"+"` binding list, sees its signature matches, and dispatches to it.

Each impl preserves decimal precision and propagates `BlNull`. The return type is `BlValue`
(rather than `BlNumber`) for every operation that may legitimately yield `null` — division by
zero, a complex `**` result, comparing against null. Unary negation cannot fail or produce null,
so `negNumber` returns the tighter `BlNumber` type. Equality (`eqNumbers`) is registered per-type
on the numeric path; the engine also has a cross-type fallback for `null = null` etc.

```go
func addNumbers(a, b BlNumber) BlValue   // "+"
func subNumbers(a, b BlNumber) BlValue   // "-"
func mulNumbers(a, b BlNumber) BlValue   // "*"
func divNumbers(a, b BlNumber) BlValue   // "/"  — divisor 0 → Null
func powNumber(a, b BlNumber) BlValue    // "**" — complex result → Null
func negNumber(n BlNumber) BlNumber      // unary "-" (patcher → negate)
func ltNumbers(a, b BlNumber) BlValue    // "<"
func leNumbers(a, b BlNumber) BlValue    // "<="
func gtNumbers(a, b BlNumber) BlValue    // ">"
func geNumbers(a, b BlNumber) BlValue    // ">="
func eqNumbers(a, b BlNumber) BlValue    // "="
func neNumbers(a, b BlNumber) BlValue    // "!="
```

These are written in clean typed form (`BlNumber → BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters in the registrations block below.

### Backing implementations (unexported, suffix `Fn`)

The library, round, and conversion functions are implemented as these typed Go functions. They
are wrapped by `typed1`/`typed2`/`typed3` when registered with the engine in the next section.
Variadic implementations (`floorFn`, `ceilingFn`, `logFn`, `numberFn`) instead implement the
engine's `func(...any) (any, error)` shape directly because they accept optional arguments and
cannot be expressed via a fixed-arity adapter.

```go
// Typed implementations — wrapped by typed1/typed2/typed3 at registration.
func roundFn(n, scale BlNumber) BlNumber          // strict alias of roundHalfUpFn
func roundUpFn(n, scale BlNumber) BlNumber
func roundDownFn(n, scale BlNumber) BlNumber
func roundHalfUpFn(n, scale BlNumber) BlNumber
func roundHalfDownFn(n, scale BlNumber) BlNumber
func roundHalfEvenFn(n, scale BlNumber) BlNumber
func absFn(n BlNumber) BlNumber
func moduloFn(a, b BlNumber) BlValue              // floor division; b==0 → Null
func sqrtFn(n BlNumber) BlValue                   // n<0 → Null
func expFn(n BlNumber) BlNumber
func lnFn(n BlNumber) BlValue                     // n≤0 → Null
func oddFn(n BlNumber) BlBoolean
func evenFn(n BlNumber) BlBoolean
func isPositiveFn(n BlNumber) BlBoolean
func isNegativeFn(n BlNumber) BlBoolean
func isZeroFn(n BlNumber) BlBoolean
func clampFn(n, min, max BlNumber) BlValue        // min>max → Null

// Variadic implementations — handle optional args themselves in expr's raw shape.
func floorFn(args ...any) (any, error)            // 1- or 2-arg
func ceilingFn(args ...any) (any, error)          // 1- or 2-arg
func logFn(args ...any) (any, error)              // 1- or 2-arg (default base 10)
func numberFn(args ...any) (any, error)           // 1- or 3-arg
```

### Registrations (`numberOptions`, unexported)

`numberOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every number-related operator impl and library function. Each entry
is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that `operatorBindings()`
  references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlNumber, BlNumber) BlValue` into that shape,
  type-asserting each argument and boxing the result. The variadic implementations declared
  above already satisfy the shape and are registered directly.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across arities (`floor(n)` and
  `floor(n, scale)` are both valid).

The registrations are grouped to reflect their role: operator impls (consumed by
`operatorBindings()`), library functions (called directly by name from expressions), and
conversions (`number`, `string`).

```go
func numberOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("addNumbers", typed2(addNumbers), new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("subNumbers", typed2(subNumbers), new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("mulNumbers", typed2(mulNumbers), new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("divNumbers", typed2(divNumbers), new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("powNumber",  typed2(powNumber),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("negNumber",  typed1(negNumber),  new(func(BlNumber) BlNumber)),
        expr.Function("ltNumbers",  typed2(ltNumbers),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("leNumbers",  typed2(leNumbers),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("gtNumbers",  typed2(gtNumbers),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("geNumbers",  typed2(geNumbers),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("eqNumbers",  typed2(eqNumbers),  new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("neNumbers",  typed2(neNumbers),  new(func(BlNumber, BlNumber) BlValue)),

        // library
        expr.Function("roundHalfEven", typed2(roundHalfEvenFn), new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("floor",         floorFn,                 new(func(BlNumber) BlNumber), new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("ceiling",       ceilingFn,               new(func(BlNumber) BlNumber), new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("round",         typed2(roundFn),         new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("roundUp",       typed2(roundUpFn),       new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("roundDown",     typed2(roundDownFn),     new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("roundHalfUp",   typed2(roundHalfUpFn),   new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("roundHalfDown", typed2(roundHalfDownFn), new(func(BlNumber, BlNumber) BlNumber)),
        expr.Function("abs",           typed1(absFn),           new(func(BlNumber) BlNumber)),
        expr.Function("modulo",        typed2(moduloFn),        new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("sqrt",          typed1(sqrtFn),          new(func(BlNumber) BlValue)),
        expr.Function("exp",           typed1(expFn),           new(func(BlNumber) BlNumber)),
        expr.Function("ln",            typed1(lnFn),            new(func(BlNumber) BlValue)),
        expr.Function("log",           logFn,                   new(func(BlNumber) BlValue), new(func(BlNumber, BlNumber) BlValue)),
        expr.Function("odd",           typed1(oddFn),           new(func(BlNumber) BlBoolean)),
        expr.Function("even",          typed1(evenFn),          new(func(BlNumber) BlBoolean)),
        expr.Function("isPositive",    typed1(isPositiveFn),    new(func(BlNumber) BlBoolean)),
        expr.Function("isNegative",    typed1(isNegativeFn),    new(func(BlNumber) BlBoolean)),
        expr.Function("isZero",        typed1(isZeroFn),        new(func(BlNumber) BlBoolean)),
        expr.Function("clamp",         typed3(clampFn),         new(func(BlNumber, BlNumber, BlNumber) BlValue)),

        // conversion
        expr.Function("number", numberFn, new(func(BlString, BlString, BlString) BlNumber)),
        expr.Function("string", stringFn, new(func(BlValue) BlString)), // shared registration; defined in string.spec.md
    }
}
```

`round` is a strict alias of `roundHalfUp`. `number(from, groupingSeparator, decimalSeparator)`
parses formatted text; `string(n)` renders. Native `int`/`float64`/decimal-`string` inputs are
wrapped to `BlNumber` by the engine bridge; a thousands-separated string is rejected — use
`number(...)`.

`[@test] ../../expr/number_test.go`

---

## Edge cases

- `NaN` / `Infinity` (source or host input) → `BlTypeError`.
- `sqrt` of `0` → `0`; of a negative → `null`.
- `(-2) ** 0.5` → `null` (complex).
- `clamp(n, min, max)` with `min > max` → `null`.
- `modulo` / `divide` with a zero divisor → `null`, never an error.
- `round` is a strict alias of `roundHalfUp`.
- `toNativeInt` truncates (does not round) and errors on overflow.
- `3.0 = 3.00` → `true` (trailing zeros ignored).
