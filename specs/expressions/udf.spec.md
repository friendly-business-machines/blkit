---
name: bl.BlUDF
description: Named, host-defined user functions compiled from expression strings. Func[P, R] defines a typed UDF (parameter struct P, return type R) by calling expr.Compile once on the body and registering it via expr.Function; passing it to bl.Expr makes it callable by name from other expressions with compile-time-checked arguments and return type. UDFs compose — a UDF body may call other UDFs.
targets:
  - ../../core/udf.go
---

# bl.BlUDF — named user-defined functions

A **UDF** is a named function defined host-side from an [expression-language](bl-expr.spec.md) string
and made callable *by name* from other expressions. It is the host-defined, typed, named sibling of
the language's inline anonymous functions (`function(x) x + 1`, see
[bl-expr.spec.md § User-defined functions](bl-expr.spec.md#user-defined-functions)): a UDF has a
Go-visible name, typed parameters, and a typed return, and its body is compiled **once** and reused.

```go
// host-side (Go)
// Func compiles a UDF body once (via expr.Compile) and builds an expr.Function
// registration. P is the parameter struct — its exported fields, in order, are the
// positional parameters; each field's `expr:"name"` tag is the name the body uses.
// R is the return type. deps are other UDFs the body may call.
func Func[P any, R BlValue](name, body string, deps ...UDF) (*BlUDF[P, R], error)

// BlUDF is a compiled named function. UDF is the sealed interface it satisfies, so a
// heterogeneous set of UDFs can be passed to bl.Expr or to another Func.
type BlUDF[P any, R BlValue] struct { /* unexported: name, body, compiled program, … */ }
type UDF interface { /* sealed — only *BlUDF[P, R] implements it */ }

func (u *BlUDF[P, R]) Call(params P) (R, error) // host-side call; runs the body via expr.Run
func (u *BlUDF[P, R]) Name() string
func (u *BlUDF[P, R]) Source() string
```

Define a UDF, then pass it to `bl.Expr` so an expression can call it by name:

```go
// host-side (Go)
type TaxParams struct {
    Amount bl.BlNumber `expr:"amount"`
}
var addTax, _ = bl.Func[TaxParams, bl.BlNumber]("addTax", `amount * 1.2`)

type PriceEnv struct {
    Base bl.BlNumber `expr:"base"`
}
var withTax, _ = bl.Expr[PriceEnv](`addTax(base) + 5`, addTax)

var base, _ = bl.Number(100)
var out, _  = withTax.Evaluate(PriceEnv{Base: base}) // the bl.BlNumber 125
```

`addTax` is also callable directly from Go, with a typed parameter struct and a typed result:

```go
// host-side (Go)
var also, _ = addTax.Call(TaxParams{Amount: base}) // the bl.BlNumber 120
```

`[@test] ../../core/udf_test.go`

## Parameters and the call mapping

`P`'s **exported, `expr`-tagged fields, in declaration order, are the positional parameters.** A call
`addTax(base)` binds its first argument to the first field, its second to the second, and so on; each
field's `expr` tag is the name the body uses to read that parameter. Each parameter field's Go type
must implement `bl.BlValue` (`bl.BlNumber`, `bl.BlString`, …); `bl.BlValue` itself is allowed for a
parameter whose type isn't fixed. A field tagged `expr:"-"`, or an unexported field, is not a
parameter.

## Type safety

The parameter types and the return type `R` form the UDF's **call signature**, registered with
`expr.Function`. At a call site inside another expression:

- **Arguments are type-checked** against the parameter types — `addTax(name)` where `name` is a
  `bl.BlString` is a **compile error** (a `bl.ParseError` from `bl.Expr`). A numeric literal still
  matches a `bl.BlNumber` parameter (`addTax(5)` works), because the patcher wraps literals into
  `bl.BlNumber` before type-checking.
- **The call is typed by `R`**, so `addTax(base) + 5` type-checks and a mis-typed use of the result is
  caught at compile time.
- **A body that produces a value of the wrong type** for `R` is a runtime `bl.TypeError` — the same
  guarantee, via the same reflection helper, as a `bl.DecisionExpression` output field.

Passing the wrong parameter struct to `Call` is, like passing the wrong env to `bl.Expr`'s `Evaluate`,
a Go build error.

## Composition

A UDF body may call other UDFs by passing them as `deps`:

```go
// host-side (Go)
var withFee, _ = bl.Func[TaxParams, bl.BlNumber]("withFee", `addTax(amount) + 2`, addTax)
```

`deps` are registered (via their `expr.Function` options) when `withFee`'s body is compiled, exactly
as UDFs are registered for a top-level `bl.Expr`. A body that calls a UDF not in `deps` is a
`bl.ParseError` at construction. **Recursion is not supported** — a UDF body cannot reference its own
name (the registration does not exist yet when the body is compiled).

## How it works (`expr.Compile` / `expr.Function` / `expr.Run`)

- **`bl.Func` calls `expr.Compile` exactly once**, compiling the body against the parameter struct `P`
  (with `deps` registered) into a stored `*vm.Program`. It then builds an **`expr.Function`**
  registration whose impl runs that program and whose call signature is a `reflect.FuncOf(paramTypes, R)`
  prototype (this is what gives call sites their compile-time typing). The body is **never recompiled**
  — unlike the inline `bl.BlFunc`, which recompiles its body on every `Apply`.
- **`bl.Expr[E](source, udfs…)` applies each UDF's `expr.Function` registration to its own
  `expr.Compile`**, so the source may call the UDF by name.
- **A call to the UDF runs the stored program via `expr.Run`** — inside the registered impl (when
  called from an expression, at the caller's `Evaluate` time) or inside `Call` (host-side). No
  compilation happens at call time.

## Edge cases

- An empty `name` is an error from `bl.Func`.
- A non-struct `P`, or a parameter field whose type does not implement `bl.BlValue`, is an error from `bl.Func`.
- A body that does not compile (a syntax error, or a name that is neither a parameter nor a registered
  function) is a `bl.ParseError` from `bl.Func`.
- Passing two UDFs with the same name to one `bl.Expr` / `bl.Func` call is an error.
- A zero-parameter UDF (`P = struct{}`) is valid: it is called with no arguments.
- `bl.UnaryTest` and `bl.DecisionExpression` do not yet accept UDFs — a straightforward future
  extension via the same `udfs …UDF` threading.
