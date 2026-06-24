# User-Defined Functions

> Named, reusable functions you define once and call by name from inside any
> expression — with typed parameters and a typed result.

Sometimes the same piece of logic — a tax calculation, an eligibility check, a
formatting rule — shows up in expression after expression. A **user-defined
function** (UDF) lets you name that logic once and then call it by name from any
other expression, the way you'd call a built-in like `round` or `upper`.

A UDF is the named sibling of the language's inline anonymous functions (the
`function(x) x + 1` form used by list operations like `remove` and `sort`). The
difference is that a UDF has a name, fixed typed parameters, and a typed result,
and its body is written once and reused everywhere.

## Calling a UDF in an expression

Once a function named `addTax` is available, an expression calls it exactly like
a built-in — by name, with positional arguments:

```
// expression-language
addTax(base) + 5
addTax(100)
```

The call is checked at compile time: the arguments must match the function's
parameter types, and the result carries the function's return type, so
`addTax(base) + 5` only compiles when `addTax` returns a number. A UDF can be
called anywhere a value of its return type is valid — in arithmetic, in a
condition, as an argument to another function.

## Defining a UDF

A UDF is defined in host Go code with `bl.Func`. You give it a name, the
**parameter struct** that names and types its parameters, and the body — an
expression-language string. The parameter struct's exported fields, in order, are
the positional parameters; each field's `expr:"..."` tag is the name the body
uses to read that parameter.

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

type TaxParams struct {
    Amount bl.BlNumber `expr:"amount"`
}

var addTax, _ = bl.Func[TaxParams, bl.BlNumber]("addTax", `amount * 1.2`)
```

`bl.Func[P, R]` is generic over the parameter struct `P` and the return type `R`.
Here the single parameter is `amount` (a number) and the function returns a
number. The body `amount * 1.2` is compiled once, when the function is defined,
and reused on every call.

## Making it callable from an expression

A UDF isn't global — you make it available to an expression by passing it to
`bl.Expr` alongside the source. The expression may then call it by name:

```go
// host-side (Go)
type PriceEnv struct {
    Base bl.BlNumber `expr:"base"`
}

var withTax, _ = bl.Expr[PriceEnv](`addTax(base) + 5`, addTax)

var base, _ = bl.Number(100)
var out, _  = withTax.Evaluate(PriceEnv{Base: base})   // the bl.BlNumber 125
```

Pass as many UDFs as the expression needs; each becomes callable by its name.

## Calling a UDF directly from Go

The same function is also callable straight from host Go code with `Call`, using
a typed parameter struct and getting a typed result back:

```go
// host-side (Go)
var also, _ = addTax.Call(TaxParams{Amount: base})   // the bl.BlNumber 120
```

`Name()` and `Source()` return the function's name and its original body text.

## Composing UDFs

A UDF body may call other UDFs. List the functions it depends on after the body,
and they become callable from inside it:

```go
// host-side (Go)
var withFee, _ = bl.Func[TaxParams, bl.BlNumber]("withFee", `addTax(amount) + 2`, addTax)
```

The dependencies are wired in when `withFee` is compiled, exactly as UDFs are
wired into a top-level `bl.Expr`. This lets you build small, named building
blocks and assemble them into larger logic.

## Type safety

The parameter types and the return type form the function's call signature, and
it is enforced on both sides of the boundary:

- **Arguments are checked at compile time.** Calling `addTax` with a string where
  it expects a number is a compile error from `bl.Expr`, not a surprise at
  evaluation time. A bare numeric literal still matches a number parameter, so
  `addTax(5)` works.
- **The call result is typed.** Because `addTax(base)` is known to be a number,
  using its result in the wrong place is caught when the expression compiles.
- **Passing the wrong parameter struct to `Call` is a Go build error**, the same
  way passing the wrong environment to `Evaluate` is.

## Rules and limits

- **Parameters come from the struct fields.** Each must be a `bl.BlValue` type
  (`bl.BlNumber`, `bl.BlString`, …); use `bl.BlValue` itself for a parameter
  whose type isn't fixed. A field tagged `expr:"-"`, or an unexported field, is
  not a parameter.
- **A zero-parameter UDF is fine.** Define it with an empty parameter struct and
  call it with no arguments.
- **Names must be unique** within a single expression: passing two functions with
  the same name to one `bl.Expr` (or one `bl.Func`) is an error.
- **A body must reference only its parameters and the functions it depends on.**
  A name that is neither is a compile error when the function is defined.
- **Recursion is not supported** — a UDF body cannot call itself by name.
