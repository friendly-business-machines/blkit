---
name: DecisionTable
description: A DecisionNode defined by input/output clauses, rules, hit policies, and evaluation semantics — modelled on the DMN decision table
targets:
  - ../decisions/decision_table.go
---

# DecisionTable

A `DecisionTable` is a `DecisionNode` that defines decision logic as a table of input conditions and output values. Each row is a `Rule`; a rule matches when all of its input entries are satisfied by the input variables. The hit policy determines how matching rules are combined into the output.

```go
type DecisionTable struct {
    DecisionNode  // Id, Name, Description, OutputName, plus Require*/Optional* methods

    HitPolicy   HitPolicy    // default: HitPolicyUnique
    Aggregation *Aggregation // only valid with HitPolicyCollect

    Inputs  []InputColumn
    Outputs []OutputClause
    Rules   []Rule
}

// Per-type input column factories — register a column on the table AND return a
// typed ref bound to the column's label. The expression is evaluated against
// the rule-evaluation context to produce the column's value.
func (d *DecisionTable) NumberInput(label string, expression BlExpr) BlNumber
func (d *DecisionTable) StringInput(label string, expression BlExpr) BlString
func (d *DecisionTable) BooleanInput(label string, expression BlExpr) BlBoolean
func (d *DecisionTable) DateInput(label string, expression BlExpr) BlDate
func (d *DecisionTable) TimeInput(label string, expression BlExpr) BlTime
func (d *DecisionTable) DateTimeInput(label string, expression BlExpr) BlDateTime
func (d *DecisionTable) DaysTimeInput(label string, expression BlExpr) BlDaysTimeDuration
func (d *DecisionTable) YearsMonthsInput(label string, expression BlExpr) BlYearsMonthsDuration
func (d *DecisionTable) ListInput(label string, expression BlExpr) BlList
func (d *DecisionTable) ContextInput(label string, expression BlExpr, schema *ContextContract) BlContext

func (d *DecisionTable) AddOutput(clause OutputClause) *DecisionTable
func (d *DecisionTable) AddRule(rule Rule) *DecisionTable

// Evaluate the table against the input variables
func (d *DecisionTable) Evaluate(input map[string]any) (BlValue, error)

// Render the table as a markdown string
func (d *DecisionTable) ToMarkdown(
    showRuleIDs bool,
    showRuleDescriptions bool,
    showInputMappings bool,
) string
```

`DecisionTable` is instantiated via direct struct literal — no `New*` factory. See [decision-node.spec.md](decision-node.spec.md) for the constructor-function idiom and the inherited `Require*`/`Optional*` declaration surface.

---

## InputColumn

An input column — produced by one of the per-type input factory methods — pairs a label with the expression that produces the column's value at evaluation time:

```go
type InputColumn struct {
    Label      string  // column header AND local variable name in rule predicates
    Expression BlExpr  // evaluates against rule context to produce the column value
    TypeRef    string  // blkit type name; derived from the factory method
}
```

The factory call returns a typed `Bl*` ref that **is** the column's value at evaluation time — use it directly in rule predicates.

---

## OutputClause

Declares one column of outputs — the name and type of a value the table produces.

```go
type OutputClause struct {
    Name               string    // output variable name in the result
    OutputValues       []BlValue // allowed values in priority order (for PRIORITY/OUTPUT_ORDER hit policies); optional
    TypeRef            *string   // blkit type name
    DefaultOutputEntry BlExpr    // used when no rule matches (COLLECT/RULE_ORDER only); optional
}
```

- If the table has exactly one output clause, the result is a single value.
- If the table has multiple output clauses, the result is a context object keyed by output clause names.

---

## Rule

A single row in the decision table.

```go
type Rule struct {
    Id          *string  // optional; if set, must be unique within the DecisionTable
    Description *string  // human-readable explanation of this rule

    InputEntries  []InputEntry
    OutputEntries map[string]BlExpr // keyed by OutputClause name
}

type InputEntry struct {
    Column    BlExpr // the column ref returned from a *Input factory
    Predicate BlExpr // boolean expression referencing the column
}

func NewRule(id ...string) *Rule

// AddInputEntry takes the column ref (returned from a *Input factory) plus
// the boolean predicate. Passing a column not registered on this table
// produces a DecisionDefinitionError at validation time.
func (r *Rule) AddInputEntry(column BlExpr, predicate BlExpr) *Rule

func (r *Rule) AddOutputEntry(outputName string, value BlExpr) *Rule
```

- `Id` is optional. When set, it must be unique within the `DecisionTable` — adding a rule with a duplicate id produces a `DecisionDefinitionError`. Ids should be meaningful identifiers that help describe or group logically related rules.
- `Description` is optional — a human-readable explanation of why this rule exists or when it applies.
- `InputEntries` are keyed by the column ref. Each predicate is a boolean `BlExpr` referencing the column ref directly (e.g. `age.GreaterThanOrEqual(Bl.Number(18))`).
- Not every input column needs an entry for every rule — omitted inputs match any value (wildcard, rendered as `"-"` in markdown).
- A rule matches when **all** specified input entries evaluate to `true` and omitted inputs are treated as matching.
- Output entries are keyed by `OutputClause` name. Not every output column needs a value for every rule — omitted outputs produce `BlNull` for that rule. An `outputName` that does not match any `OutputClause` produces a `DecisionDefinitionError`.

### Example — Single Output Column

```go
func eligibilityTable() *DecisionTable {
    elig := &DecisionTable{
        Id:   "eligibility",
        Name: "Eligibility Check",
    }
    applicant := elig.RequireContext("applicant", applicantSchema)

    age    := elig.NumberInput("Age",    applicant.Get("age"))
    income := elig.NumberInput("Income", applicant.Get("income"))

    elig.AddOutput(OutputClause{Name: "eligibility"})

    elig.AddRule(*NewRule().
        AddInputEntry(age,    age.GreaterThanOrEqual(Bl.Number(18))).
        AddInputEntry(income, income.GreaterThanOrEqual(Bl.Number(30000))).
        AddOutputEntry("eligibility", Bl.String("eligible")))

    elig.AddRule(*NewRule().
        AddInputEntry(age, age.LessThan(Bl.Number(18))).
        AddOutputEntry("eligibility", Bl.String("ineligible")))

    elig.AddRule(*NewRule().
        AddInputEntry(income, income.LessThan(Bl.Number(30000))).
        AddOutputEntry("eligibility", Bl.String("ineligible")))

    return elig
}

fmt.Println(eligibilityTable().ToMarkdown(false, false, false))
```

Output:

```text
| U | Age       | Income          |   | eligibility  |
|---|-----------|-----------------|---|--------------|
| 1 | Age >= 18 | Income >= 30000 | █ | "eligible"   |
| 2 | Age < 18  | -               | █ | "ineligible" |
| 3 | -         | Income < 30000  | █ | "ineligible" |
```

### Example — Multiple Output Columns

```go
func loanPricingTable() *DecisionTable {
    pricing := &DecisionTable{
        Id:   "pricing",
        Name: "Loan Pricing",
    }
    creditScore := pricing.RequireNumber("credit_score")
    loanAmount  := pricing.RequireNumber("loan_amount")

    score  := pricing.NumberInput("Score",  creditScore)
    amount := pricing.NumberInput("Amount", loanAmount)

    pricing.AddOutput(OutputClause{Name: "rate"})
    pricing.AddOutput(OutputClause{Name: "term"})

    pricing.AddRule(*NewRule("prime").
        AddInputEntry(score,  score.GreaterThanOrEqual(Bl.Number(750))).
        AddInputEntry(amount, amount.LessThanOrEqual(Bl.Number(500000))).
        AddOutputEntry("rate", Bl.Number(3.5)).
        AddOutputEntry("term", Bl.Number(360)))

    pricing.AddRule(*NewRule("standard").
        AddInputEntry(score, score.In(Bl.Range(Bl.Number(650), Bl.Number(749), true, true))).
        AddOutputEntry("rate", Bl.Number(5.0)).
        AddOutputEntry("term", Bl.Number(240)))

    pricing.AddRule(*NewRule("subprime").
        AddInputEntry(score, score.LessThan(Bl.Number(650))).
        AddOutputEntry("rate", Bl.Number(7.5)).
        AddOutputEntry("term", Bl.Number(180)))

    return pricing
}

fmt.Println(loanPricingTable().ToMarkdown(true, false, false))
```

Output:

```text
| U | rule-id  | Score               | Amount           |   | rate | term |
|---|----------|---------------------|------------------|---|------|------|
| 1 | prime    | Score >= 750        | Amount <= 500000 | █ | 3.5  | 360  |
| 2 | standard | Score in [650..749] | -                | █ | 5.0  | 240  |
| 3 | subprime | Score < 650         | -                | █ | 7.5  | 180  |
```

### Example — Range and List Membership

```go
func shippingTable() *DecisionTable {
    shipping := &DecisionTable{
        Id:        "shipping",
        Name:      "Shipping Cost",
        HitPolicy: HitPolicyFirst,
    }
    pkg          := shipping.RequireContext("package", packageSchema)
    destinationV := shipping.RequireString("destination")

    weight      := shipping.NumberInput("Weight",      pkg.Get("weight"))
    destination := shipping.StringInput("Destination", destinationV)

    shipping.AddOutput(OutputClause{Name: "cost"})

    shipping.AddRule(*NewRule("light-domestic").
        AddInputEntry(weight,      weight.LessThanOrEqual(Bl.Number(5))).
        AddInputEntry(destination, destination.In(Bl.List(Bl.String("US"), Bl.String("CA")))).
        AddOutputEntry("cost", Bl.Number(9.99)))

    shipping.AddRule(*NewRule("medium-domestic").
        AddInputEntry(weight,      weight.In(Bl.Range(Bl.Number(5), Bl.Number(20), false, true))).
        AddInputEntry(destination, destination.In(Bl.List(Bl.String("US"), Bl.String("CA")))).
        AddOutputEntry("cost", Bl.Number(19.99)))

    shipping.AddRule(*NewRule("fallback").
        AddOutputEntry("cost", Bl.Number(49.99)))

    return shipping
}
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
    HitPolicyPriority                     // Among matching rules, select by output priority (OutputValues order)
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
| `Priority` | single value | highest priority wins | Priority from `OutputValues` |
| `Any` | single value | allowed if outputs identical | Error if outputs differ |
| `Collect` + Aggregation | aggregated value | all matches aggregated | Numeric outputs only for Sum/Min/Max |
| `Collect` (no aggregation) | list of values | all match outputs as list | — |
| `RuleOrder` | list of values | all matches in rule order | — |
| `OutputOrder` | list of values | all matches sorted by priority | — |

When the hit policy returns a list and the table has multiple output clauses, the result is a list of context objects.

### Example — FIRST Hit Policy

```go
func discountTable() *DecisionTable {
    discount := &DecisionTable{
        Id:        "discount",
        Name:      "Discount Rules",
        HitPolicy: HitPolicyFirst,
    }
    customerTypeV := discount.RequireString("customer_type")
    orderTotalV   := discount.RequireNumber("order_total")

    customerType := discount.StringInput("Type",  customerTypeV)
    total        := discount.NumberInput("Total", orderTotalV)

    discount.AddOutput(OutputClause{Name: "discount"})

    discount.AddRule(*NewRule("vip").
        AddInputEntry(customerType, customerType.Equals(Bl.String("VIP"))).
        AddOutputEntry("discount", Bl.Number(0.20)))

    discount.AddRule(*NewRule("large-order").
        AddInputEntry(total, total.GreaterThan(Bl.Number(500))).
        AddOutputEntry("discount", Bl.Number(0.10)))

    discount.AddRule(*NewRule("default").
        AddOutputEntry("discount", Bl.Number(0.0)))

    return discount
}

result, err := discountTable().Evaluate(map[string]any{
    "customer_type": Bl.String("VIP"),
    "order_total":   Bl.Number(1000),
})
// result is BlNumber(0.20) — first rule matched, second rule not considered
```

### Example — COLLECT with Aggregation

```go
func penaltiesTable() *DecisionTable {
    sumAgg := AggregationSum
    penalties := &DecisionTable{
        Id:          "penalties",
        Name:        "Penalty Assessment",
        HitPolicy:   HitPolicyCollect,
        Aggregation: &sumAgg,
    }
    speedV := penalties.RequireNumber("speed")
    zoneV  := penalties.RequireString("zone_type")

    speed := penalties.NumberInput("Speed", speedV)
    zone  := penalties.StringInput("Zone",  zoneV)

    penalties.AddOutput(OutputClause{Name: "fine"})

    penalties.AddRule(*NewRule("school-speeding").
        AddInputEntry(speed, speed.GreaterThan(Bl.Number(30))).
        AddInputEntry(zone,  zone.Equals(Bl.String("school"))).
        AddOutputEntry("fine", Bl.Number(200)))

    penalties.AddRule(*NewRule("excessive-speed").
        AddInputEntry(speed, speed.GreaterThan(Bl.Number(50))).
        AddOutputEntry("fine", Bl.Number(150)))

    penalties.AddRule(*NewRule("school-zone").
        AddInputEntry(zone, zone.Equals(Bl.String("school"))).
        AddOutputEntry("fine", Bl.Number(50)))

    return penalties
}

result, err := penaltiesTable().Evaluate(map[string]any{
    "speed":     Bl.Number(55),
    "zone_type": Bl.String("school"),
})
// All three rules match — result is BlNumber(400) (200 + 150 + 50)
```

---

## Evaluation

1. For each `InputColumn`, evaluate its `Expression` against the input variables and bind the result to the column's label in a local context.
2. For each `Rule`, evaluate each specified `InputEntry`'s predicate against the local context. An entry must evaluate to `BlBoolean.TRUE` to match. Input columns not referenced in the rule's `InputEntries` always match (wildcard).
3. A rule matches if **all** specified input entries match.
4. Apply the hit policy to the set of matching rules to produce the output.
5. If no rule matches:
   - For `Unique`, `First`, `Any`: return `BlNull` output (not an error, unless the decision has `required` semantics from the model).
   - For `Collect`, `RuleOrder`, `OutputOrder`: return an empty list.
   - If a `DefaultOutputEntry` is defined on an output clause, use that value.

---

## Markdown Rendering

`ToMarkdown()` returns the decision table as a GitHub-flavoured markdown table string.

### Format

- The first row is headers: hit policy indicator in the first cell, then input column labels, then output clause names.
- The hit policy indicator is the standard single-letter abbreviation (e.g. `U` for Unique, `F` for First, `C` for Collect). For `Collect` with an aggregation, the aggregation symbol is appended (e.g. `C+` for Sum, `C<` for Min, `C>` for Max, `C#` for Count).
- A visual separator column is placed between the last input column and the first output column to visually distinguish inputs from outputs. The header cell is empty, and every data row contains a `█` (Unicode full block, U+2588) character.
- Each subsequent row is a rule: rule index number in the first cell, then (if `showRuleIDs=true`) the rule id in a column headed `rule-id`, then input entries rendered via `ToMarkdown()` (with `"-"` for omitted/wildcard inputs), then the `█` separator cell, then output entries rendered via `ToMarkdown()` (with `""` for omitted outputs).
- When `showInputMappings=true`, a table mapping each input label to its expression is rendered above the decision table.
- When `showRuleDescriptions=true`, a numbered list of rule descriptions is appended below the table. Rules without descriptions are omitted from the list.

### Example

```go
func riskLevelTable() *DecisionTable {
    table := &DecisionTable{
        Id:        "risk",
        Name:      "Risk Level",
        HitPolicy: HitPolicyUnique,
    }
    ageV         := table.RequireNumber("age")
    creditScoreV := table.RequireNumber("credit_score")

    age   := table.NumberInput("Age",   ageV)
    score := table.NumberInput("Score", creditScoreV)

    table.AddOutput(OutputClause{Name: "risk"})
    table.AddOutput(OutputClause{Name: "reason"})

    table.AddRule(*NewRule("young-good-credit", "Young applicant with strong credit history").
        AddInputEntry(age,   age.LessThan(Bl.Number(25))).
        AddInputEntry(score, score.GreaterThan(Bl.Number(700))).
        AddOutputEntry("risk",   Bl.String("low")).
        AddOutputEntry("reason", Bl.String("Young with good credit")))

    table.AddRule(*NewRule("older-any-credit", "Standard risk for older applicants").
        AddInputEntry(age, age.GreaterThanOrEqual(Bl.Number(25))).
        AddOutputEntry("risk", Bl.String("medium")))

    return table
}

fmt.Println(riskLevelTable().ToMarkdown(false, false, false))
```

Output:

```text
| U | Age       | Score       |   | risk     | reason                   |
|---|-----------|-------------|---|----------|--------------------------|
| 1 | Age < 25  | Score > 700 | █ | "low"    | "Young with good credit" |
| 2 | Age >= 25 | -           | █ | "medium" |                          |
```

### Example — with Rule IDs, Descriptions, and Input Mappings

```go
fmt.Println(riskLevelTable().ToMarkdown(true, true, true))
```

Output:

```text
**Inputs:**

| Label | Expression   |
|-------|--------------|
| Age   | age          |
| Score | credit_score |

| U | rule-id           | Age       | Score       |   | risk     | reason                   |
|---|-------------------|-----------|-------------|---|----------|--------------------------|
| 1 | young-good-credit | Age < 25  | Score > 700 | █ | "low"    | "Young with good credit" |
| 2 | older-any-credit  | Age >= 25 | -           | █ | "medium" |                          |

1. **young-good-credit** — Young applicant with strong credit history
2. **older-any-credit** — Standard risk for older applicants
```

---

## Edge Cases

- A `DecisionTable` with no input columns is valid; all rules match unconditionally (the table selects purely on output priority or returns all rules).
- A `DecisionTable` with no `OutputClause` entries is invalid — a `ValidationError` is raised before evaluation.
- A `DecisionTable` with no `Rule` entries evaluates to a `BlNull` output (or empty list for multi-result policies) without error.
- A `Rule` with an empty string `Id` is invalid — `NewRule("")` produces a `DecisionDefinitionError`.
- A `Rule` with no `Id` (`NewRule()`) is valid.
- Adding a `Rule` whose `Id` duplicates an existing rule in the table produces a `DecisionDefinitionError`.
- An `AddOutputEntry` call with an `outputName` that does not match any `OutputClause` produces a `DecisionDefinitionError`.
- A rule that omits an output column produces `BlNull` for that column in the result.
- An `AddInputEntry` call whose column ref was not produced by one of this table's `*Input` factory methods produces a `DecisionDefinitionError`.
- A rule with no input entries (all inputs are wildcards) matches unconditionally.
- Predicates must be `BlExpr` instances evaluating to `BlBoolean`. A predicate that evaluates to a non-boolean type produces a `BlTypeError` at evaluation time.
- `HitPolicyCollect` with `AggregationSum` on a non-numeric output clause is a `ValidationError`.
- `HitPolicyPriority` or `HitPolicyOutputOrder` without `OutputValues` defined on at least one output clause is a `ValidationError`.
