# Decision Expressions

> Compute several named outputs from typed inputs, wiring outputs that depend on
> one another into a single evaluated unit.

A **decision expression** turns typed inputs into one or more named outputs, each
defined by a short [expression](../expressions/overview.md). Later outputs may
build on earlier ones, and blkit works out the order automatically. Reach for it
when a decision is a handful of formulas rather than a table of rules.

A `bl.DecisionExpression[I, O]` is generic over a typed input struct `I` and output
struct `O`. Each **entry** binds one output (by name) to an expression.

## A single output

```go
type PaymentInputs struct {
    LoanAmount bl.Handle[bl.BlNumber] `expr:"loan_amount"`
    Rate       bl.Handle[bl.BlNumber] `expr:"rate"`
}
type PaymentOutputs struct {
    Amount bl.Handle[bl.BlNumber] `expr:"amount"`
}

var monthlyPayment = bl.NewDecisionExpression[PaymentInputs, PaymentOutputs](bl.DecisionExpressionConfig{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    Entries: bl.Entries{
        "amount": `loan_amount * rate / 12`,
    },
})
```

The entry source is written in the [expression language](../expressions/overview.md)
and references the input variables by their `expr` names.

## Outputs that build on each other

An entry may reference another output by name. blkit discovers those dependencies,
orders the entries so each runs after the ones it reads, and rejects cycles — all
when the node is built:

```go
type Breakdown struct {
    Principal bl.Handle[bl.BlNumber] `expr:"principal"`
    Interest  bl.Handle[bl.BlNumber] `expr:"interest"`
    Total     bl.Handle[bl.BlNumber] `expr:"total"`
}

bl.Entries{
    "principal": `loan_amount / term`,
    "interest":  `loan_amount * rate / 12`,
    "total":     `principal + interest`, // reads the two outputs above
}
```

You never declare the order yourself — `total` simply runs last because it reads
`principal` and `interest`.

## Conditionals

Entries are full expressions, so conditional logic is `if … then … else …`:

```go
bl.Entries{
    "status": `if score >= 700 then "approved" else "review"`,
}
```

## Calling functions

Supply [user-defined functions](../expressions/user-defined-functions.md) in
`Funcs`, and any entry may call them by name with compile-time-checked arguments:

```go
addTax, _ := bl.Func[TaxParams, bl.BlNumber]("addTax", `amount * 1.2`)

bl.NewDecisionExpression[PriceInputs, PriceOutputs](bl.DecisionExpressionConfig{
    Entries: bl.Entries{
        "gross":         `addTax(base)`,
        "with_shipping": `gross + 5`,
    },
    Funcs: []bl.UDF{addTax},
})
```

## What's checked, and when

Everything outside the running values is checked when the node is built (at program
start): the contracts are well-formed, every entry compiles, every name it
references is a declared input or sibling output, and the dependency graph is
acyclic. Only a value whose computed type disagrees with its declared output
surfaces later, as an error from `Evaluate`.

## Inside a task

Like every node, a decision expression exposes `In`/`Out` handle surfaces and is
wired into a [decision task](decision-tasks.md) by connecting handles with
`bl.Edge`.
