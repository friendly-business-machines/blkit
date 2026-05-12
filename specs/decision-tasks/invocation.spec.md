---
name: Invocation
description: A DecisionNode that calls a BusinessKnowledgeModel with explicit parameter bindings — declares its own typed inputs and returns whatever the BKM returns
targets:
  - ../decisions/invocation.go
---

# Invocation

An `Invocation` is a `DecisionNode` that calls a `BusinessKnowledgeModel` (BKM) with explicit parameter bindings. The BKM defines a reusable function; the Invocation supplies the arguments and returns whatever the BKM produces.

```go
type Invocation struct {
    DecisionNode  // Id, Name, Description, OutputName, plus Require*/Optional* methods

    BKM      *BusinessKnowledgeModel // the BKM to invoke
    Bindings []ParameterBinding      // argument mappings
}

// Bind a BKM parameter to a BlExpr drawn from the invocation's typed inputs
// (or any other expression accessible in the invocation's evaluation context).
func (i *Invocation) Bind(parameter string, expression BlExpr) *Invocation

// Evaluate — calls the BKM with bound parameters, returns its result
func (i *Invocation) Evaluate(input map[string]any) (BlValue, error)


type ParameterBinding struct {
    Parameter  string // name of the BKM parameter
    Expression BlExpr // expression evaluated against input to produce the argument
}
```

`Invocation` is instantiated via direct struct literal — no `New*` factory.

---

## Building an Invocation — the constructor-function idiom

The Invocation declares its own typed inputs (which become the source side of bindings); the BKM's parameter names are the keys on the destination side.

```go
func paymentCalc() *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:   "payment_calc",
        Name: "Payment Calculator",
    }
    principal := bkm.RequireNumber("principal")
    rate      := bkm.RequireNumber("rate")
    bkm.RequireNumber("term") // declared so callers must supply it; not used in body
    bkm.Body = principal.Multiply(rate).Divide(Bl.Number(12))
    return bkm
}

func monthlyPaymentInvocation(bkm *BusinessKnowledgeModel) *Invocation {
    inv := &Invocation{
        Id:   "monthly_payment",
        Name: "Monthly Payment",
        BKM:  bkm,
    }
    loanAmount := inv.RequireNumber("loan_amount")
    annualRate := inv.RequireNumber("annual_rate")
    termMonths := inv.RequireNumber("loan_term_months")

    // Map invocation-side typed refs to the BKM's parameter names.
    inv.Bind("principal", loanAmount)
    inv.Bind("rate",      annualRate)
    inv.Bind("term",      termMonths)
    return inv
}

result, err := monthlyPaymentInvocation(paymentCalc()).Evaluate(map[string]any{
    "loan_amount":      Bl.Number(200000),
    "annual_rate":      Bl.Number(0.05),
    "loan_term_months": Bl.Number(360),
})
// result is a BlNumber
```

The Invocation's `Require*` calls declare what the surrounding model (or caller) must supply when this invocation is evaluated. `Bind` then maps each typed ref into a named BKM parameter slot.

---

## Parameter Bindings

Each binding maps a BKM parameter name to a `BlExpr`. The expression is evaluated against the invocation's input context (caller inputs + upstream node outputs) to produce the argument value. Typically the expression is a typed ref returned by one of the invocation's own `Require*` calls — but any `BlExpr` is valid (e.g. a literal, an arithmetic expression, a path into a context variable).

```go
// Pass an arithmetic expression, not just a typed ref:
inv.Bind("annual_total", monthly.Multiply(Bl.Number(12)))
```

---

## Evaluation

When an `Invocation` evaluates:

1. The invocation's declared inputs are resolved from the input context.
2. Each `ParameterBinding`'s expression is evaluated against the input context.
3. The resulting values are passed to the BKM as its parameter values, keyed by `ParameterBinding.Parameter`.
4. The BKM is invoked with those arguments.
5. The BKM's result is returned as the Invocation's result.

---

## Edge Cases

- An `Invocation` with no `BKM` set is invalid; `Evaluate()` returns a `DecisionDefinitionError`.
- A binding that references a BKM parameter name not declared on the BKM produces a `DecisionDefinitionError` at validation time.
- A BKM parameter with no binding receives `BlNull` as its argument (or, for `Optional*` declarations on the BKM, evaluates to `BlNull` cleanly inside the body).
- The same BKM can be invoked by multiple `Invocation` nodes in the same model with different bindings.
- An Invocation that declares an input via `Require*` but never uses it (neither in a `Bind` call nor as part of an expression bound to a parameter) is still recorded as a dependency of the node — the model evaluates the source before this node — even though the value is unused.
