---
name: BusinessKnowledgeModel
description: A reusable function generic over a parameters struct and an output type — invoked by Invocation nodes; body can be an expression, a PMML/ONNX model, a native Go function, or a reference to another DecisionTask
targets:
  - ../decisions/bkm.go
---

# BusinessKnowledgeModel

A `BusinessKnowledgeModel` (BKM) is a reusable, parameterised function. It is not a node in the decision graph — it is referenced by `Invocation` nodes.

`BusinessKnowledgeModel` is generic over a parameters struct (whose exported fields declare the BKM's typed parameters) and an output type (the single typed return value). The body can be one of five forms:

- **Expression** — a `BlExpr` of the output type, evaluated against the parameter handles
- **PMML model** — a reference to a PMML (Predictive Model Markup Language) model
- **ONNX model** — a reference to an ONNX (Open Neural Network Exchange) model
- **Native function** — a Go function reference
- **Decision task** — a reference to an entire other `DecisionTask`, evaluated as a black box

```go
type BusinessKnowledgeModel[Parameters any, Output BlValue] struct {
    Id          string
    Name        string
    Description string

    // Body — exactly one of these must be set
    Body           BlExpr                  // expression body (typed Output)
    PMML           *PMMLReference          // PMML model reference
    ONNX           *ONNXReference          // ONNX model reference
    NativeFunction *NativeFunctionReference[Parameters, Output] // typed Go function reference
    DecisionTask   *DecisionTaskReference  // reference to another DecisionTask

    Parameters Parameters // typed parameter handles, populated by NewBusinessKnowledgeModel
}

func NewBusinessKnowledgeModel[Parameters any, Output BlValue](
    opts BKMOpts[Parameters, Output],
) *BusinessKnowledgeModel[Parameters, Output]

type BKMOpts[Parameters any, Output BlValue] struct {
    Id          string
    Name        string
    Description string

    // Exactly one of the body fields below must be set.
    Body           func(p *Parameters) Output // expression body
    PMML           *PMMLReference
    ONNX           *ONNXReference
    NativeFunction *NativeFunctionReference[Parameters, Output]
    DecisionTask   *DecisionTaskReference
}

// Invokable is the non-generic interface that an Invocation accepts as its BKM
// reference. Every BusinessKnowledgeModel[Parameters, Output] satisfies this
// interface regardless of its type parameters.
type Invokable interface {
    GetId() string
    GetName() string
    Invoke(arguments map[string]BlValue) (BlValue, error)
}

// Render the BKM as a markdown string
func (b *BusinessKnowledgeModel[Parameters, Output]) ToMarkdown() string
```

`NewBusinessKnowledgeModel` reflects on the `Parameters` type parameter and allocates a typed handle for each exported field. Field-name and tag rules match the outputs-struct contract on [decision-node.spec.md](decision-node.spec.md) (exported field names are lowercased by default; `bl:"name"` tags override). Each parameter handle carries its parameter name internally so that `Bind` can identify it at the call site.

---

## Expression Body

The simplest form — the body closure builds a typed expression of the BKM's output type from the parameter handles.

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

`paymentCalc.Parameters.Principal`, `.Rate`, `.Term` are typed `BlNumber` handles that `Invocation` nodes use as the source side of bindings.

---

## PMML Model Body

```go
type PMMLReference struct {
    ModelName string  // name of the model within the PMML document
    Location  string  // file path or URI

    // InputMapping maps BKM parameter names to PMML field names. Defaults to
    // passing through by matching name.
    InputMapping map[string]string

    // OutputField — the PMML output field whose value is returned. Defaults to
    // the primary predicted value.
    OutputField *string
}
```

```go
type CreditScorerParameters struct {
    Income          BlNumber
    Debt            BlNumber
    EmploymentYears BlNumber
}

var creditScorer = NewBusinessKnowledgeModel[CreditScorerParameters, BlNumber](BKMOpts[CreditScorerParameters, BlNumber]{
    Id:   "credit_scorer",
    Name: "Credit Score Model",
    PMML: &PMMLReference{
        ModelName: "CreditScoreModel",
        Location:  "models/credit_score.pmml",
    },
})
```

---

## ONNX Model Body

```go
type ONNXReference struct {
    Location string // file path or URI to the .onnx file

    // InputMapping maps BKM parameter names to ONNX input tensor names.
    InputMapping map[string]string

    // OutputTensor — the ONNX output tensor whose value is returned.
    OutputTensor *string
}
```

```go
type FraudDetectorParameters struct {
    TransactionAmount BlNumber
    MerchantCategory  BlString
    TimeOfDay         BlString
}

var fraudDetector = NewBusinessKnowledgeModel[FraudDetectorParameters, BlNumber](BKMOpts[FraudDetectorParameters, BlNumber]{
    Id:   "fraud_detector",
    Name: "Fraud Detection Model",
    ONNX: &ONNXReference{Location: "models/fraud_detection.onnx"},
})
```

---

## Native Function Body

A `NativeFunctionReference[Parameters, Output]` wraps a Go function. The function's typed signature mirrors the BKM's: it receives a `*Parameters` value with the parameter handles already populated (resolved against the invocation arguments at call time) and returns the typed `Output`.

```go
type NativeFunctionReference[Parameters any, Output BlValue] struct {
    Fn func(p *Parameters) (Output, error)
}
```

```go
type PremiumCalcParameters struct {
    Age            BlNumber
    CoverageAmount BlNumber
    RiskFactors    BlList
}

var premiumCalc = NewBusinessKnowledgeModel[PremiumCalcParameters, BlNumber](BKMOpts[PremiumCalcParameters, BlNumber]{
    Id:   "premium_calc",
    Name: "Insurance Premium Calculator",
    NativeFunction: &NativeFunctionReference[PremiumCalcParameters, BlNumber]{
        Fn: insurance.CalculatePremium, // signature: (p *PremiumCalcParameters) (BlNumber, error)
    },
})
```

---

## Decision Task Body

A `DecisionTaskReference` points to another `DecisionTask`. The BKM maps its parameters to the referenced task's inputs and evaluates it as a black box.

```go
type DecisionTaskReference struct {
    Task         DecisionTask     // the task to evaluate (interface, not the parameterised type)
    InputMapping map[string]BlExpr // referenced task's input name → expression over BKM parameters
}

func (d *DecisionTaskReference) MapInput(taskInput string, expression BlExpr) *DecisionTaskReference
```

When invoked:

1. Each entry in `InputMapping` is evaluated against the BKM's parameter handles to produce the referenced task's input map.
2. The `DecisionTask` is evaluated with those inputs.
3. If the referenced task has a single declared output, that output's `BlValue` is returned directly.
4. If the referenced task has multiple declared outputs, a `BlDictionary` of all outputs (keyed by output name) is returned.

```go
type CreditBKMParameters struct {
    Applicant       BlDictionary
    RequestedAmount BlNumber
}

var creditBKM = NewBusinessKnowledgeModel[CreditBKMParameters, BlDictionary](BKMOpts[CreditBKMParameters, BlDictionary]{
    Id:   "credit_bkm",
    Name: "Credit Risk Check",
    DecisionTask: (&DecisionTaskReference{Task: creditModel}).
        MapInput("applicant_income", credit_bkm_params.Applicant.Get("income")).
        MapInput("applicant_age",    credit_bkm_params.Applicant.Get("age")).
        MapInput("loan_amount",      credit_bkm_params.RequestedAmount),
})
```

The referenced task is evaluated fresh on each invocation.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string describing the BKM. The output includes the name, parameters (derived from the `Parameters` struct), and body.

### Expression Body

```go
fmt.Println(paymentCalc.ToMarkdown())
```

Output:

```text
### Payment Calculator

| Parameter | Type   |
|-----------|--------|
| principal | number |
| rate      | number |
| term      | number |

**Expression:** `principal * rate / 12`
```

### Native Function Body

```go
fmt.Println(premiumCalc.ToMarkdown())
```

Output:

```text
### Insurance Premium Calculator

| Parameter       | Type   |
|-----------------|--------|
| age             | number |
| coverage_amount | number |
| risk_factors    | list   |

**Native function:** `insurance.CalculatePremium`
```

(PMML, ONNX, and Decision Task bodies render analogously.)

---

## Edge Cases

- A BKM with no body field set is invalid; `NewBusinessKnowledgeModel` raises `DecisionDefinitionError`.
- A BKM with more than one body field set is invalid; raises `DecisionDefinitionError`.
- A BKM whose `Parameters` struct has a field with a non-`BlValue` type produces `DecisionDefinitionError`.
- A BKM invoked with an argument whose name is not present on its `Parameters` struct silently ignores that argument.
- A `Parameters` field with no corresponding argument in `Invoke()` receives `BlNull`.
- An argument whose runtime type disagrees with the declared `Parameters` field type produces `BlTypeError` at evaluation time.
- PMML and ONNX model loading failures produce a `DecisionEvaluationError` wrapping the cause.
- A native function that returns an error produces a `DecisionEvaluationError` wrapping it.
- A `DecisionTaskReference` whose `Task` is nil is invalid; raises `DecisionDefinitionError`.
- A `DecisionTaskReference` input-mapping entry whose key does not match any input on the referenced task is silently ignored.
- BKMs are not nodes in the decision graph — they have no inter-node dependencies. They are standalone reusable functions referenced by `Invocation` nodes.
- A BKM that references a `DecisionTask` which itself invokes (via another BKM) the first model creates a cycle. This is detected at evaluation time and produces `DecisionEvaluationError`.
