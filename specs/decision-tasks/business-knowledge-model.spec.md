---
name: BusinessKnowledgeModel
description: A reusable function definition invoked by Invocation nodes — can be defined by an expression body, a PMML model, an ONNX model, a Go function reference, or a reference to another DecisionTask
targets:
  - ../decisions/bkm.go
---

# BusinessKnowledgeModel

A `BusinessKnowledgeModel` (BKM) is a reusable, parameterised function. It is not a node in the decision graph — instead, it is invoked by `Invocation` nodes. A BKM declares typed parameters via `Require*` methods and a body that produces a result from those parameters.

The body can be one of five kinds:

- **Expression** — a `BlExpr` evaluated against the parameters
- **PMML model** — a reference to a PMML (Predictive Model Markup Language) model
- **ONNX model** — a reference to an ONNX (Open Neural Network Exchange) model
- **Native function** — a Go function reference
- **Decision model** — a reference to an entire other `DecisionTask`, evaluated as a black box

```go
type BusinessKnowledgeModel struct {
    Id          string
    Name        *string
    Description *string

    // Body — exactly one of these must be set
    Body           BlExpr                   // expression body
    PMML           *PMMLReference           // PMML model reference
    ONNX           *ONNXReference           // ONNX model reference
    NativeFunction *NativeFunctionReference // Go function reference
    DecisionTask  *DecisionTaskReference  // reference to another DecisionTask

    // Internal: typed-parameter registry populated by Require*/Optional* calls.
}

// Per-type parameter declarations — record a parameter on the BKM AND return
// a typed Go ref that the body / native function can reference.
func (b *BusinessKnowledgeModel) RequireNumber(name string) BlNumber
func (b *BusinessKnowledgeModel) RequireString(name string) BlString
func (b *BusinessKnowledgeModel) RequireBoolean(name string) BlBoolean
func (b *BusinessKnowledgeModel) RequireDate(name string) BlDate
func (b *BusinessKnowledgeModel) RequireTime(name string) BlTime
func (b *BusinessKnowledgeModel) RequireDateTime(name string) BlDateTime
func (b *BusinessKnowledgeModel) RequireDaysTime(name string) BlDaysTimeDuration
func (b *BusinessKnowledgeModel) RequireYearsMonths(name string) BlYearsMonthsDuration
func (b *BusinessKnowledgeModel) RequireList(name string) BlList
func (b *BusinessKnowledgeModel) RequireContext(name string, schema *ContextContract) BlContext
// + Optional* variants

// Invoke the BKM with the given arguments
func (b *BusinessKnowledgeModel) Invoke(arguments map[string]BlValue) (BlValue, error)

// Render the BKM as a markdown string
func (b *BusinessKnowledgeModel) ToMarkdown() string
```

`BusinessKnowledgeModel` is instantiated via direct struct literal — no `New*` factory. Parameters are declared via `Require*` methods on the value (which lazy-initialize the internal registry).

---

## Expression Body

The simplest form — the BKM body is a `BlExpr` that uses the captured parameter refs:

```go
func paymentCalcBKM() *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:   "payment_calc",
        Name: "Payment Calculator",
    }
    principal := bkm.RequireNumber("principal")
    rate      := bkm.RequireNumber("rate")
    term      := bkm.RequireNumber("term")
    bkm.Body = principal.Multiply(rate).Divide(Bl.Number(12))
    _ = term  // declared but not used in this example body
    return bkm
}

result, err := paymentCalcBKM().Invoke(map[string]BlValue{
    "principal": Bl.Number(200000),
    "rate":      Bl.Number(0.05),
    "term":      Bl.Number(360),
})
// result is a BlNumber
```

---

## PMML Model Body

A `PMMLReference` points to a PMML model file or resource. The BKM maps its declared parameters to PMML model inputs and converts the PMML output to a `BlValue`.

```go
type PMMLReference struct {
    ModelName string  // name of the model within the PMML document
    Location  string  // file path or URI to the PMML document

    // Input mapping — maps BKM parameter names to PMML field names.
    // If not set, parameters are passed through by matching names.
    InputMapping map[string]string

    // Output field — the PMML output field whose value is returned.
    // If not set, the primary predicted value is returned.
    OutputField *string
}
```

```go
func creditScorerBKM() *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:   "credit_scorer",
        Name: "Credit Score Model",
        PMML: &PMMLReference{
            ModelName: "CreditScoreModel",
            Location:  "models/credit_score.pmml",
        },
    }
    bkm.RequireNumber("income")
    bkm.RequireNumber("debt")
    bkm.RequireNumber("employment_years")
    return bkm
}
```

---

## ONNX Model Body

An `ONNXReference` points to an ONNX model file. The BKM maps its parameters to ONNX model inputs and converts the output tensor to a `BlValue`.

```go
type ONNXReference struct {
    Location string // file path or URI to the .onnx file

    // Input mapping — maps BKM parameter names to ONNX input tensor names.
    // If not set, parameters are passed through by matching names.
    InputMapping map[string]string

    // Output tensor — the ONNX output tensor whose value is returned.
    // If not set, the first output tensor is used.
    OutputTensor *string
}
```

```go
func fraudDetectorBKM() *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:   "fraud_detector",
        Name: "Fraud Detection Model",
        ONNX: &ONNXReference{
            Location: "models/fraud_detection.onnx",
        },
    }
    bkm.RequireNumber("transaction_amount")
    bkm.RequireString("merchant_category")
    bkm.RequireString("time_of_day")
    return bkm
}
```

---

## Native Function Body

A `NativeFunctionReference` wraps a Go function, allowing BKMs to delegate to arbitrary code — third-party APIs, custom algorithms, or any logic not expressible in Bl.

```go
type NativeFunctionReference struct {
    Fn func(map[string]BlValue) (BlValue, error)
}
```

The function must accept a single `map[string]BlValue` argument (keyed by the BKM's declared parameter names) and return a `BlValue`.

```go
func premiumCalcBKM() *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:   "premium_calc",
        Name: "Insurance Premium Calculator",
        NativeFunction: &NativeFunctionReference{
            Fn: insurance.CalculatePremium,
        },
    }
    bkm.RequireNumber("age")
    bkm.RequireNumber("coverage_amount")
    bkm.RequireList("risk_factors")
    return bkm
}
```

---

## Decision Model Body

A `DecisionTaskReference` points to another `DecisionTask`. The BKM maps its parameters to the referenced model's expected inputs and evaluates the entire model as a black box. This allows composing models — a node in one model can delegate to an entire other model.

```go
type DecisionTaskReference struct {
    Model        *DecisionTask    // the model to evaluate
    InputMapping map[string]BlExpr // maps: referenced model's input name → expression over BKM parameters
}

func (d *DecisionTaskReference) MapInput(modelInput string, expression BlExpr) *DecisionTaskReference
```

When invoked:

1. Each entry in `InputMapping` is evaluated: the `BlExpr` is evaluated against the BKM's arguments to produce the value for the referenced model's input.
2. The referenced `DecisionTask` is evaluated with the mapped inputs.
3. If the referenced model has a single declared output, the BKM returns that output's `BlValue` directly.
4. If the referenced model has multiple declared outputs, the BKM returns a `BlContext` containing all output values keyed by their output names.

```go
func creditBKM(creditModel *DecisionTask) *BusinessKnowledgeModel {
    bkm := &BusinessKnowledgeModel{
        Id:            "credit_bkm",
        Name:          "Credit Risk Check",
        DecisionTask: &DecisionTaskReference{Model: creditModel},
    }
    applicant := bkm.RequireContext("applicant", applicantSchema)
    requestedAmount := bkm.RequireNumber("requested_amount")

    bkm.DecisionTask.
        MapInput("applicant_income", applicant.Get("income")).
        MapInput("applicant_age",    applicant.Get("age")).
        MapInput("loan_amount",      requestedAmount)

    return bkm
}
```

The referenced model is evaluated fresh on each invocation — it does not share state with previous evaluations.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string describing the BKM. The output includes the name, parameters (with their declared types from the `Require*` calls), and body.

### Expression Body

```go
fmt.Println(paymentCalcBKM().ToMarkdown())
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
fmt.Println(premiumCalcBKM().ToMarkdown())
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

(PMML, ONNX, and Decision Model bodies render analogously — the parameter table is identical and the body section names the source of the implementation.)

---

## Edge Cases

- A BKM with no body (all five body fields are `nil`) is invalid; `Invoke()` returns a `DecisionDefinitionError`.
- A BKM with more than one body set is invalid.
- A BKM invoked with an argument name not declared via `Require*` silently ignores the extra argument.
- A BKM parameter with no corresponding argument in the `Invoke()` call receives `BlNull` (or, for `Optional*` declarations, evaluates to `BlNull` cleanly inside the body).
- The argument is validated against the declared `Require*` type. A mismatch produces a `BlTypeError`.
- PMML and ONNX model loading failures produce a `DecisionEvaluationError` with the underlying cause.
- A native function that returns an error produces a `DecisionEvaluationError` wrapping it.
- A `DecisionTaskReference` with a `nil` model is invalid; `Invoke()` returns a `DecisionDefinitionError`.
- A `DecisionTaskReference` input mapping entry whose key does not match any input name in the referenced model is silently ignored.
- A referenced model input with no corresponding mapping receives `BlNull`.
- If the referenced model's evaluation faults, the BKM faults with a `DecisionEvaluationError` wrapping the cause.
- A BKM that references a model which itself invokes a BKM referencing the first model creates a circular dependency. This is detected at evaluation time (not by `DecisionTask.Validate()`, since the cycle spans models) and produces a `DecisionEvaluationError`.
- BKMs are not nodes in the decision graph — they have no `Requires`-style dependencies on sibling nodes. They are standalone reusable functions referenced by `Invocation` nodes.
