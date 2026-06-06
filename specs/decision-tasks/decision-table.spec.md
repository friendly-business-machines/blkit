---
name: DecisionTable
description: A DecisionNode generic over an outputs struct — defines decision logic as input columns, output columns (derived from the outputs struct), and rules with hit policies
targets:
  - ../decisions/decision_table.go
---

# DecisionTable

A `DecisionTable` is a `DecisionNode` that defines decision logic as a table of input conditions and output values. Each row is a `Rule`; a rule matches when all of its input entries are satisfied. The hit policy determines how matching rules are combined into the output.

`DecisionTable` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)). The output columns are inferred from the outputs struct's exported fields; the table does not carry a separate output-clause declaration.

```go
type DecisionTable[Outputs any] struct {
    Id          string
    Name        string
    Description string

    HitPolicy   HitPolicy
    Aggregation *Aggregation

    Inputs []TableInput
    Rules  []Rule

    Outputs Outputs // typed handles, populated by NewDecisionTable
}

func NewDecisionTable[Outputs any](opts DecisionTableOpts) *DecisionTable[Outputs]

type DecisionTableOpts struct {
    Id          string
    Name        string
    Description string
    HitPolicy   HitPolicy    // default: HitPolicyUnique
    Aggregation *Aggregation // only valid with HitPolicyCollect
    Inputs      []TableInput
    Rules       []Rule
}

// Evaluate the table against the input variables
func (d *DecisionTable[Outputs]) Evaluate(input map[string]any) (BlValue, error)

// Render the table as a markdown string
func (d *DecisionTable[Outputs]) ToMarkdown(
    showRuleIDs bool,
    showRuleDescriptions bool,
    showInputMappings bool,
) string
```

The output columns on a `DecisionTable[Outputs]` are inferred from `Outputs`'s exported fields (see [decision-node.spec.md](decision-node.spec.md) for the reflection contract and naming rules). `NewDecisionTable` validates that every output name referenced by a rule appears on `Outputs`, and that every declared output is set by at least one rule.

---

## TableInput

A `TableInput` is the label-plus-expression pairing that gives a rule predicate a column to read against. Each input column is built by a per-type free constructor that returns a value implementing **both** `TableInput` (for placement in `DecisionTableOpts.Inputs`) and the matching `Bl*` interface (for direct use in rule predicates).

```go
type TableInput interface {
    GetLabel() string
    GetExpression() BlExpr
    GetTypeRef() string
}

// Per-type input column constructors. The returned column value implements
// both TableInput and the matching Bl* interface — methods of the Bl* interface
// are usable directly on the column for building rule predicates.
func NumberInput(label string, expression BlNumber) NumberInputColumn
func StringInput(label string, expression BlString) StringInputColumn
func BooleanInput(label string, expression BlBoolean) BooleanInputColumn
func DateInput(label string, expression BlDate) DateInputColumn
func TimeInput(label string, expression BlTime) TimeInputColumn
func DateTimeInput(label string, expression BlDateTime) DateTimeInputColumn
func DaysTimeInput(label string, expression BlDaysTimeDuration) DaysTimeInputColumn
func YearsMonthsInput(label string, expression BlYearsMonthsDuration) YearsMonthsInputColumn
func ListInput(label string, expression BlList) ListInputColumn
func DictionaryInput(label string, expression BlDictionary, schema *DictionaryContract) DictionaryInputColumn
```

Each concrete `*InputColumn` type embeds the matching `Bl*` interface — `NumberInputColumn` embeds `bl.BlNumber`, `StringInputColumn` embeds `bl.BlString`, etc. — so the column value can be used directly inside rule predicates without an extra `.Ref` hop.

---

## Rule

A single row in the decision table.

```go
type Rule struct {
    Id          *string  // optional; if set, must be unique within the DecisionTable
    Description string // human-readable explanation of this rule

    Conditions []InputEntry
    Results    map[string]BlExpr // keyed by output name (matching an Outputs-struct field)
}

type InputEntry struct {
    Column    TableInput // the column being matched
    Predicate BlExpr     // boolean expression referencing the column
}

func NewRule(idAndDescription ...string) *Rule

// AddInputEntry takes the column value (returned from a *Input factory) plus
// the boolean predicate. Passing a column not present in this table's Inputs
// produces a DecisionDefinitionError at construction.
func (r *Rule) AddInputEntry(column TableInput, predicate BlExpr) *Rule

// AddOutputEntry sets the value of an output. The outputName must match a
// field on the table's Outputs struct (lowercased name, or bl: tag).
func (r *Rule) AddOutputEntry(outputName string, value BlExpr) *Rule
```

- `Id` is optional. When set, it must be unique within the `DecisionTable`.
- `Description` is optional — a human-readable explanation of why this rule exists or when it applies.
- `Conditions` pair a column with a boolean `bl.BlExpr` predicate. The predicate references the column directly (e.g., `age.GreaterThanOrEqual(bl.Number(18))`).
- Not every input column needs an entry for every rule — omitted inputs match any value (wildcard, rendered as `"-"` in markdown).
- A rule matches when **all** specified conditions evaluate to `true`.
- `Results` are keyed by the output name as declared on the `Outputs` struct. An output that does not appear on the struct produces a `DecisionDefinitionError`. A rule that omits an output produces `bl.BlNull` for it.

### Example — Single Output Column

```go
type EligibilityOutputs struct {
    Eligibility BlString
}

// Input columns — package-scope, typed.
var (
    eligAgeCol    = NumberInput("Age",    applicant.Outputs.Age)
    eligIncomeCol = NumberInput("Income", applicant.Outputs.Income)
)

var eligibility = NewDecisionTable[EligibilityOutputs](DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: HitPolicyUnique,
    Inputs:    []TableInput{eligAgeCol, eligIncomeCol},
    Rules: []Rule{
        *bl.NewRule().
            AddInputEntry(eligAgeCol,    eligAgeCol.GreaterThanOrEqual(bl.Number(18))).
            AddInputEntry(eligIncomeCol, eligIncomeCol.GreaterThanOrEqual(bl.Number(30000))).
            AddOutputEntry("eligibility", bl.String("eligible")),
        *bl.NewRule().
            AddInputEntry(eligAgeCol, eligAgeCol.LessThan(bl.Number(18))).
            AddOutputEntry("eligibility", bl.String("ineligible")),
        *bl.NewRule().
            AddInputEntry(eligIncomeCol, eligIncomeCol.LessThan(bl.Number(30000))).
            AddOutputEntry("eligibility", bl.String("ineligible")),
    },
})
```

Output of `eligibility.ToMarkdown(false, false, false)`:

```text
| U | Age       | Income          |   | eligibility  |
|---|-----------|-----------------|---|--------------|
| 1 | Age >= 18 | Income >= 30000 | █ | "eligible"   |
| 2 | Age < 18  | -               | █ | "ineligible" |
| 3 | -         | Income < 30000  | █ | "ineligible" |
```

### Example — Multiple Output Columns

```go
type LoanPricingOutputs struct {
    Rate BlNumber
    Term BlNumber
}

var (
    pricingScoreCol  = NumberInput("Score",  applicant.Outputs.CreditScore)
    pricingAmountCol = NumberInput("Amount", applicant.Outputs.LoanAmount)
)

var loanPricing = NewDecisionTable[LoanPricingOutputs](DecisionTableOpts{
    Id:        "pricing",
    Name:      "Loan Pricing",
    HitPolicy: HitPolicyUnique,
    Inputs:    []TableInput{pricingScoreCol, pricingAmountCol},
    Rules: []Rule{
        *bl.NewRule("prime").
            AddInputEntry(pricingScoreCol,  pricingScoreCol.GreaterThanOrEqual(bl.Number(750))).
            AddInputEntry(pricingAmountCol, pricingAmountCol.LessThanOrEqual(bl.Number(500000))).
            AddOutputEntry("rate", bl.Number(3.5)).
            AddOutputEntry("term", bl.Number(360)),
        *bl.NewRule("standard").
            AddInputEntry(pricingScoreCol, pricingScoreCol.In(bl.Range(bl.Number(650), bl.Number(749), true, true))).
            AddOutputEntry("rate", bl.Number(5.0)).
            AddOutputEntry("term", bl.Number(240)),
        *bl.NewRule("subprime").
            AddInputEntry(pricingScoreCol, pricingScoreCol.LessThan(bl.Number(650))).
            AddOutputEntry("rate", bl.Number(7.5)).
            AddOutputEntry("term", bl.Number(180)),
    },
})
```

Output of `loanPricing.ToMarkdown(true, false, false)`:

```text
| U | rule-id  | Score               | Amount           |   | rate | term |
|---|----------|---------------------|------------------|---|------|------|
| 1 | prime    | Score >= 750        | Amount <= 500000 | █ | 3.5  | 360  |
| 2 | standard | Score in [650..749] | -                | █ | 5.0  | 240  |
| 3 | subprime | Score < 650         | -                | █ | 7.5  | 180  |
```

### Example — Range and List Membership

```go
type ShippingOutputs struct {
    Cost BlNumber
}

var (
    shippingWeightCol      = NumberInput("Weight",      pkg.Outputs.Weight)
    shippingDestinationCol = StringInput("Destination", order.Outputs.Destination)
)

var shipping = NewDecisionTable[ShippingOutputs](DecisionTableOpts{
    Id:        "shipping",
    Name:      "Shipping Cost",
    HitPolicy: HitPolicyFirst,
    Inputs:    []TableInput{shippingWeightCol, shippingDestinationCol},
    Rules: []Rule{
        *bl.NewRule("light-domestic").
            AddInputEntry(shippingWeightCol,      shippingWeightCol.LessThanOrEqual(bl.Number(5))).
            AddInputEntry(shippingDestinationCol, shippingDestinationCol.In(bl.List(bl.String("US"), bl.String("CA")))).
            AddOutputEntry("cost", bl.Number(9.99)),
        *bl.NewRule("medium-domestic").
            AddInputEntry(shippingWeightCol,      shippingWeightCol.In(bl.Range(bl.Number(5), bl.Number(20), false, true))).
            AddInputEntry(shippingDestinationCol, shippingDestinationCol.In(bl.List(bl.String("US"), bl.String("CA")))).
            AddOutputEntry("cost", bl.Number(19.99)),
        *bl.NewRule("fallback").
            AddOutputEntry("cost", bl.Number(49.99)),
    },
})
```

Output:

```text
| F | rule-id         | Weight            | Destination                 |   | cost  |
|---|-----------------|-------------------|-----------------------------|---|-------|
| 1 | light-domestic  | Weight <= 5       | Destination in ["US", "CA"] | █ | 9.99  |
| 2 | medium-domestic | Weight in (5..20] | Destination in ["US", "CA"] | █ | 19.99 |
| 3 | fallback        | -                 | -                           | █ | 49.99 |
```

---

## Hit Policy

The hit policy determines how the engine handles multiple matching rules.

```go
type HitPolicy int

const (
    HitPolicyUnique      HitPolicy = iota // Exactly one rule may match. Multiple matches → EvaluationError
    HitPolicyFirst                        // Rules evaluated in order; first match wins
    HitPolicyPriority                     // Among matching rules, select by output priority
    HitPolicyAny                          // Multiple rules may match but must produce identical outputs
    HitPolicyCollect                      // All matching rules collected; combined via Aggregation
    HitPolicyRuleOrder                    // All matching rules returned in rule declaration order
    HitPolicyOutputOrder                  // All matching rules sorted by output priority
)

type Aggregation int

const (
    AggregationSum   Aggregation = iota // numeric sum of all matching output values
    AggregationMin                      // minimum numeric value
    AggregationMax                      // maximum numeric value
    AggregationCount                    // number of matching rules
)
```

### Hit Policy Semantics

| Policy | Returns | Multiple matches | Notes |
|---|---|---|---|
| `Unique` | single value | error | Default |
| `First` | single value | first match wins | Rule order matters |
| `Priority` | single value | highest priority wins | Priority from output struct's declaration order or tag |
| `Any` | single value | allowed if outputs identical | Error if outputs differ |
| `Collect` + Aggregation | aggregated value | all matches aggregated | Numeric outputs only for Sum/Min/Max |
| `Collect` (no aggregation) | list of values | all match outputs as list | — |
| `RuleOrder` | list of values | all matches in rule order | — |
| `OutputOrder` | list of values | all matches sorted by priority | — |

When the hit policy returns a list and the table has multiple output columns, the result is a list of context objects.

### Example — FIRST Hit Policy

```go
type DiscountOutputs struct {
    Discount BlNumber
}

var (
    discountTypeCol  = StringInput("Type",  order.Outputs.CustomerType)
    discountTotalCol = NumberInput("Total", order.Outputs.OrderTotal)
)

var discount = NewDecisionTable[DiscountOutputs](DecisionTableOpts{
    Id:        "discount",
    Name:      "Discount Rules",
    HitPolicy: HitPolicyFirst,
    Inputs:    []TableInput{discountTypeCol, discountTotalCol},
    Rules: []Rule{
        *bl.NewRule("vip").
            AddInputEntry(discountTypeCol, discountTypeCol.Equals(bl.String("VIP"))).
            AddOutputEntry("discount", bl.Number(0.20)),
        *bl.NewRule("large-order").
            AddInputEntry(discountTotalCol, discountTotalCol.GreaterThan(bl.Number(500))).
            AddOutputEntry("discount", bl.Number(0.10)),
        *bl.NewRule("default").
            AddOutputEntry("discount", bl.Number(0.0)),
    },
})

result, err := discount.Evaluate(map[string]any{
    "customer_type": bl.String("VIP"),
    "order_total":   bl.Number(1000),
})
// result is bl.BlNumber(0.20) — first rule matched, others not considered
```

### Example — COLLECT with Aggregation

```go
type PenaltyOutputs struct {
    Fine BlNumber
}

var (
    penaltySpeedCol = NumberInput("Speed", incident.Outputs.Speed)
    penaltyZoneCol  = StringInput("Zone",  incident.Outputs.ZoneType)
)

var sumAgg = AggregationSum

var penalties = NewDecisionTable[PenaltyOutputs](DecisionTableOpts{
    Id:          "penalties",
    Name:        "Penalty Assessment",
    HitPolicy:   HitPolicyCollect,
    Aggregation: &sumAgg,
    Inputs:      []TableInput{penaltySpeedCol, penaltyZoneCol},
    Rules: []Rule{
        *bl.NewRule("school-speeding").
            AddInputEntry(penaltySpeedCol, penaltySpeedCol.GreaterThan(bl.Number(30))).
            AddInputEntry(penaltyZoneCol,  penaltyZoneCol.Equals(bl.String("school"))).
            AddOutputEntry("fine", bl.Number(200)),
        *bl.NewRule("excessive-speed").
            AddInputEntry(penaltySpeedCol, penaltySpeedCol.GreaterThan(bl.Number(50))).
            AddOutputEntry("fine", bl.Number(150)),
        *bl.NewRule("school-zone").
            AddInputEntry(penaltyZoneCol, penaltyZoneCol.Equals(bl.String("school"))).
            AddOutputEntry("fine", bl.Number(50)),
    },
})

result, err := penalties.Evaluate(map[string]any{
    "speed":     bl.Number(55),
    "zone_type": bl.String("school"),
})
// All three rules match — result is bl.BlNumber(400) (200 + 150 + 50)
```

---

## Evaluation

1. For each `TableInput`, evaluate its `Expression` against the input variables and bind the result to the column's label in a local context.
2. For each `Rule`, evaluate each specified `InputEntry`'s predicate against the local context. An entry must evaluate to `bl.BlBoolean.TRUE` to match. Input columns not referenced in the rule's `Conditions` always match (wildcard).
3. A rule matches if **all** specified conditions match.
4. Apply the hit policy to the set of matching rules to produce the output.
5. If no rule matches:
   - For `Unique`, `First`, `Any`: return `bl.BlNull` output (not an error, unless surrounding semantics treat the output as required).
   - For `Collect`, `RuleOrder`, `OutputOrder`: return an empty list.

---

## Markdown Rendering

`ToMarkdown()` returns the decision table as a GitHub-flavoured markdown table string.

### Format

- The first row is headers: hit policy indicator in the first cell, then input column labels, then output names.
- The hit policy indicator is the standard single-letter abbreviation (`U`, `F`, `C`, …). For `Collect` with an aggregation, the aggregation symbol is appended (`C+`, `C<`, `C>`, `C#`).
- A visual separator column is placed between the last input column and the first output column. The header cell is empty, and every data row contains `█` (Unicode full block).
- Each subsequent row is a rule: rule index, then (if `showRuleIDs=true`) the rule id in a `rule-id` column, then input entries rendered via `bl.BlUnaryTest.Source()` (with `"-"` for omitted/wildcard inputs), then the `█` separator, then output values rendered via `bl.BlValue.String()` (with `""` for omitted outputs).
- When `showInputMappings=true`, a table mapping each input label to its expression is rendered above the decision table.
- When `showRuleDescriptions=true`, a numbered list of rule descriptions is appended below the table.

### Example

```go
type RiskOutputs struct {
    Risk   BlString
    Reason BlString
}

var (
    riskAgeCol   = NumberInput("Age",   applicant.Outputs.Age)
    riskScoreCol = NumberInput("Score", applicant.Outputs.CreditScore)
)

var riskLevel = NewDecisionTable[RiskOutputs](DecisionTableOpts{
    Id:        "risk",
    Name:      "Risk Level",
    HitPolicy: HitPolicyUnique,
    Inputs:    []TableInput{riskAgeCol, riskScoreCol},
    Rules: []Rule{
        *bl.NewRule("young-good-credit", "Young applicant with strong credit history").
            AddInputEntry(riskAgeCol,   riskAgeCol.LessThan(bl.Number(25))).
            AddInputEntry(riskScoreCol, riskScoreCol.GreaterThan(bl.Number(700))).
            AddOutputEntry("risk",   bl.String("low")).
            AddOutputEntry("reason", bl.String("Young with good credit")),
        *bl.NewRule("older-any-credit", "Standard risk for older applicants").
            AddInputEntry(riskAgeCol, riskAgeCol.GreaterThanOrEqual(bl.Number(25))).
            AddOutputEntry("risk", bl.String("medium")),
    },
})

fmt.Println(riskLevel.ToMarkdown(false, false, false))
```

Output:

```text
| U | Age       | Score       |   | risk     | reason                   |
|---|-----------|-------------|---|----------|--------------------------|
| 1 | Age < 25  | Score > 700 | █ | "low"    | "Young with good credit" |
| 2 | Age >= 25 | -           | █ | "medium" |                          |
```

---

## Edge Cases

- A `DecisionTable` with no input columns is valid; all rules match unconditionally.
- A `DecisionTable` whose `Outputs` struct has no exported fields is invalid — `NewDecisionTable` raises `DecisionDefinitionError`.
- A `DecisionTable` with no `Rule` entries evaluates to `bl.BlNull` (or empty list for multi-result policies) without error.
- A `Rule` with an empty string `Id` is invalid — `bl.NewRule("")` produces `DecisionDefinitionError`.
- A `Rule` with no `Id` (`bl.NewRule()`) is valid.
- A `Rule` whose `Id` duplicates an existing rule's id in the table produces `DecisionDefinitionError`.
- An `AddOutputEntry` call referencing an output name not declared on the outputs struct produces `DecisionDefinitionError`.
- An `AddInputEntry` call whose column was not added to `DecisionTableOpts.Inputs` produces `DecisionDefinitionError`.
- A rule with no input entries (all inputs are wildcards) matches unconditionally.
- Predicates must be `bl.BlExpr` instances evaluating to `bl.BlBoolean`. A predicate that evaluates to a non-boolean type produces a `bl.TypeError` at evaluation time.
- `HitPolicyCollect` with `AggregationSum` on a non-numeric output is a `DecisionDefinitionError`.
- An output declared on the outputs struct that is set by no rule is a `DecisionDefinitionError` (every declared output must be reachable).
