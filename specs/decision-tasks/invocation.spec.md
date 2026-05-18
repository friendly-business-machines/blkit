---
name: Invocation
description: A DecisionNode generic over a single-field outputs struct — calls a BusinessKnowledgeModel with typed parameter bindings and returns the BKM's result
targets:
  - ../decisions/invocation.go
---

# Invocation

An `Invocation` is a `DecisionNode` that calls a `BusinessKnowledgeModel` (BKM) with explicit parameter bindings. The BKM defines a reusable function; the Invocation supplies the arguments and exposes the BKM's result as its single output.

`Invocation` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)) that must have exactly one exported field. The field's type must match the referenced BKM's `Output` type.

```go
type Invocation[Outputs any] struct {
    Id          string
    Name        string
    Description string

    BKM      Invokable
    Bindings []ParameterBinding

    Outputs Outputs // single-field typed handle, populated by NewInvocation
}

func NewInvocation[Outputs any](opts InvocationOpts) *Invocation[Outputs]

type InvocationOpts struct {
    Id          string
    Name        string
    Description string
    BKM         Invokable
    Bindings    []ParameterBinding
}

type ParameterBinding struct {
    Parameter BlExpr // the BKM-side typed parameter handle (e.g. paymentCalc.Parameters.Principal)
    Argument  BlExpr // the typed argument expression
}

// Bind pairs a BKM parameter handle with an argument expression. The two
// arguments must have the same Bl* type — a mismatch is a compile error.
func Bind[T BlValue](parameter T, argument T) ParameterBinding

// Evaluate calls the BKM with the bound parameters, returns its result.
func (i *Invocation[Outputs]) Evaluate(input map[string]any) (BlValue, error)
```

`NewInvocation` validates:

- The `Outputs` struct has exactly one exported field.
- Every `ParameterBinding`'s `Parameter` handle originates from `BKM`'s parameter struct.
- Every BKM parameter has at most one binding (duplicate bindings are an error).
- A BKM parameter with no binding will receive `BlNull` at invocation time.

The constructor's `Bind[T]` helper enforces type matching between the parameter handle and the argument at the call site through Go's type parameter inference.

---

## Building an Invocation

Given a BKM declared elsewhere (see [business-knowledge-model.spec.md](business-knowledge-model.spec.md)):

```go
type PaymentCalcParameters struct {
    Principal BlNumber
    Rate      BlNumber
    Term      BlNumber
}

var paymentCalc = NewBusinessKnowledgeModel[PaymentCalcParameters, BlNumber](BKMOpts[PaymentCalcParameters, BlNumber]{
    Id:   "payment_calc",
    Name: "Payment Calculator",
    Body: func(p *PaymentCalcParameters) BlNumber {
        return p.Principal.Multiply(p.Rate).Divide(Bl.Number(12))
    },
})
```

An Invocation node binds the BKM's parameters to typed argument expressions:

```go
type MonthlyPaymentOutputs struct {
    Amount BlNumber
}

var monthlyPayment = NewInvocation[MonthlyPaymentOutputs](InvocationOpts{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    BKM:  paymentCalc,
    Bindings: []ParameterBinding{
        Bind(paymentCalc.Parameters.Principal, loanAmount),
        Bind(paymentCalc.Parameters.Rate,      annualRate),
        Bind(paymentCalc.Parameters.Term,      termMonths),
    },
})

// monthlyPayment.Outputs.Amount — BlNumber handle for downstream nodes
```

Here `loanAmount`, `annualRate`, and `termMonths` are typed `BlNumber` handles drawn from upstream nodes' `.Outputs.X` fields or DecisionTask-level inputs.

---

## Parameter Bindings

Each binding pairs a BKM-side parameter handle with an argument expression. The `Bind[T BlValue]` helper enforces that both have the same `Bl*` type:

```go
// Compile-time error: cannot pass BlString where BlNumber is expected
Bind(paymentCalc.Parameters.Principal, someString)
```

The argument may be any typed expression, not just a raw handle:

```go
Bind(paymentCalc.Parameters.Principal, loanAmount.Multiply(Bl.Number(1.1)))
```

A parameter without a binding receives `BlNull` at invocation time. The BKM body is responsible for tolerating `BlNull` (or the parameter being optional). The Invocation does not enforce that every BKM parameter has a binding — though future revisions may add an opt-in strictness flag.

The same BKM can be invoked by multiple `Invocation` nodes with different bindings.

---

## Evaluation

When an `Invocation` evaluates:

1. Each `ParameterBinding`'s `Argument` expression is evaluated against the current input context.
2. The resulting values are passed to the BKM via `Invoke`, keyed by the BKM's parameter names (resolved from each `Parameter` handle).
3. The BKM's result is returned as the Invocation's output and stored in `Outputs`'s single field.

---

## Edge Cases

- An `Invocation` with a `nil` `BKM` is invalid; `NewInvocation` raises `DecisionDefinitionError`.
- An `Outputs` struct with zero or more than one exported field is invalid.
- An `Outputs`-field type that does not match the BKM's declared `Output` type is invalid.
- A `ParameterBinding` whose `Parameter` handle does not belong to `BKM`'s parameters is invalid.
- Two bindings targeting the same BKM parameter is invalid.
- An argument whose evaluation produces a runtime type incompatible with the BKM parameter type produces a `BlTypeError` at evaluation time (caught after the compile-time `Bind[T]` constraint).
- An `Invocation` that has no bindings is valid; all BKM parameters receive `BlNull`.
- If the BKM faults during invocation, the Invocation faults with the same error.
