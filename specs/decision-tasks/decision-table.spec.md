---
name: DecisionTable
description: A DecisionNode generic over an outputs struct — defines decision logic as input columns, output columns (derived from the outputs struct), and rules whose cells are text expressions (unary tests for inputs, expressions for outputs) compiled against the bl expression language.
targets:
  - ../decisions/decision_table.go
---

# DecisionTable

A `DecisionTable` is a `DecisionNode` that defines decision logic as a table of input conditions and output values. Each row is a `Rule`; a rule matches when all of its input cells are satisfied. The hit policy determines how matching rules are combined into the output.

Cells are **text expressions** in the blkit expression language (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). Input cells are **unary tests** (`>= 18`, `[650..749]`, `"US", "CA"`, `-`); output cells are **expressions** (`3.5`, `"eligible"`). Every cell is compiled once at construction — input cells via `bl.UnaryTest` against the column's type, output cells via `bl.Expr` — so malformed cells fail fast as a `DecisionDefinitionError`.

`DecisionTable` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)). The output columns are inferred from the outputs struct's exported fields, in declaration order; the table does not carry a separate output-clause declaration.

```go
type DecisionTable[Outputs any] struct {
    Id          string
    Name        string
    Description string

    HitPolicy   HitPolicy
    Aggregation *Aggregation

    Schema       BlSchema          // optional; type-checks input Exprs and output cells at construction
    Inputs       []Input
    Rules        Rules
    Descriptions map[string]string // optional; rule id -> human-readable description

    Outputs Outputs // typed handles, populated by NewDecisionTable
}

func NewDecisionTable[Outputs any](opts DecisionTableOpts) *DecisionTable[Outputs]

type DecisionTableOpts struct {
    Id           string
    Name         string
    Description  string
    HitPolicy    HitPolicy    // default: HitPolicyUnique
    Aggregation  *Aggregation // only valid with HitPolicyCollect
    Schema       BlSchema     // optional
    Inputs       []Input
    Rules        Rules
    Descriptions map[string]string
}

// Evaluate the table against the input variables. The map[string]any signature
// satisfies the DecisionNode interface (see decision-node.spec.md); values are
// wrapped to a bl.BlDictionary internally before the cells run.
func (d *DecisionTable[Outputs]) Evaluate(input map[string]any) (BlValue, error)

// Render the table as a markdown string
func (d *DecisionTable[Outputs]) ToMarkdown(
    showRuleIDs bool,
    showRuleDescriptions bool,
    showInputMappings bool,
) string
```

The output columns on a `DecisionTable[Outputs]` are inferred from `Outputs`'s exported fields (see [decision-node.spec.md](decision-node.spec.md) for the reflection contract and naming rules). `NewDecisionTable` validates that every declared output is set by at least one rule, and that every rule row is the expected width.

---

## Input

An `Input` is a labelled, typed column. `Expr` is a source expression in the blkit language, evaluated against the input variables to produce the column value; `Type` is the type that value holds and is used to compile each rule's unary-test cell for this column.

```go
type Input struct {
    Label string // column header
    Expr  string // source expression -> column value
    Type  Type   // the type the column holds; compiles each input cell via bl.UnaryTest
}
```

`Type` is one of the comparable scalars (`TypeNumber`, `TypeString`, `TypeDate`, `TypeTime`, `TypeDateTime`, `TypeDaysTimeDuration`, `TypeYearsMonthsDuration`) or an unordered type (`TypeBoolean`, `TypeList`, `TypeDictionary`, `TypeTable`, `TypeRange`). The ordering and interval unary-test forms (`<`, `<=`, `>`, `>=`, `[a..b]`) require a comparable scalar; using them against an unordered column is a `DecisionDefinitionError` at construction (see [bl-expr.spec.md § Unary tests](../expressions/bl-expr.spec.md#unary-tests)). For structured columns, use the `?`-expression form (e.g. `count(?) > 0`, `listContains(?, "urgent")`).

---

## Rule

A single row in the decision table. `Rule` is a slice of raw-string cells: input cells (unary tests) followed by output cells (expressions), in column order. An **optional** leading id cell may precede the inputs.

```go
type Rule []string
type Rules []Rule
```

Row layout is one of:

```text
[ inputCells…, outputCells… ]            // no id column
[ id, inputCells…, outputCells… ]        // id column present
```

The id column is **all-or-nothing per table**: either every row carries a leading id cell or none do. The constructor infers which from the uniform row width — `len(Inputs) + nOutputs` means no id column; one more means the first cell is the id. Rows of differing width, or a width matching neither, are a `DecisionDefinitionError`.

Cell conventions:

- **id cell** — empty (`` `` ``) means this rule has no id; a non-empty id must be unique within the table.
- **input cell** — a unary-test source compiled via `bl.UnaryTest(cell, column.Type)`. `-` is the wildcard (matches any value, including null). An empty input cell is invalid — write `-` for "matches anything".
- **output cell** — an expression source compiled via `bl.Expr(cell, Schema)`, in `Outputs`-struct field order. An empty output cell (`` `` ``) means "no value" and yields `bl.BlNull` for that output (it is not compiled).

Use Go **raw string literals** (backticks) for cells, so expression-language string literals need no escaping: an output of the string `eligible` is the cell `` `"eligible"` ``.

A rule matches when **all** of its non-wildcard input cells evaluate to `bl.BlBoolean` true. Per-rule descriptions live in the table's optional `Descriptions` map, keyed by rule id (a key with no matching rule id is a `DecisionDefinitionError`).

### Example — Single Output Column

```go
type EligibilityOutputs struct {
    Eligibility BlString
}

var eligibility = NewDecisionTable[EligibilityOutputs](DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: HitPolicyUnique,
    Inputs: []Input{
        {`Age`,    `applicant.age`,    TypeNumber},
        {`Income`, `applicant.income`, TypeNumber},
    },
    Rules: Rules{
        // Age    Income      eligibility
        {`>= 18`, `>= 30000`, `"eligible"`  },
        {`< 18` , `-`       , `"ineligible"`},
        {`-`    , `< 30000` , `"ineligible"`},
    },
})
```

Output of `eligibility.ToMarkdown(false, false, false)`:

```text
| U | Age   | Income   |   | eligibility  |
|---|-------|----------|---|--------------|
| 1 | >= 18 | >= 30000 | █ | "eligible"   |
| 2 | < 18  | -        | █ | "ineligible" |
| 3 | -     | < 30000  | █ | "ineligible" |
```

### Example — Multiple Output Columns

Output cells are positional, in `Outputs`-struct field order (`Rate`, then `Term`). This table uses the optional id column.

```go
type LoanPricingOutputs struct {
    Rate BlNumber
    Term BlNumber
}

var loanPricing = NewDecisionTable[LoanPricingOutputs](DecisionTableOpts{
    Id:        "pricing",
    Name:      "Loan Pricing",
    HitPolicy: HitPolicyUnique,
    Inputs: []Input{
        {`Score`,  `applicant.creditScore`, TypeNumber},
        {`Amount`, `applicant.loanAmount`,  TypeNumber},
    },
    Rules: Rules{
        // id        Score         Amount       rate   term
        {`prime`   , `>= 750`    , `<= 500000`, `3.5`, `360`},
        {`standard`, `[650..749]`, `-`        , `5.0`, `240`},
        {`subprime`, `< 650`     , `-`        , `7.5`, `180`},
    },
})
```

Output of `loanPricing.ToMarkdown(true, false, false)`:

```text
| U | rule-id  | Score      | Amount    |   | rate | term |
|---|----------|------------|-----------|---|------|------|
| 1 | prime    | >= 750     | <= 500000 | █ | 3.5  | 360  |
| 2 | standard | [650..749] | -         | █ | 5.0  | 240  |
| 3 | subprime | < 650      | -         | █ | 7.5  | 180  |
```

### Example — Range and List Membership

Range membership is the interval unary test (`<= 5`, `(5..20]`); list membership is the comma-disjunction form (`"US", "CA"` matches either value).

```go
type ShippingOutputs struct {
    Cost BlNumber
}

var shipping = NewDecisionTable[ShippingOutputs](DecisionTableOpts{
    Id:        "shipping",
    Name:      "Shipping Cost",
    HitPolicy: HitPolicyFirst,
    Inputs: []Input{
        {`Weight`,      `pkg.weight`,        TypeNumber},
        {`Destination`, `order.destination`, TypeString},
    },
    Rules: Rules{
        // id               Weight     Destination   cost
        {`light-domestic` , `<= 5`   , `"US", "CA"`, `9.99` },
        {`medium-domestic`, `(5..20]`, `"US", "CA"`, `19.99`},
        {`fallback`       , `-`      , `-`         , `49.99`},
    },
})
```

Output:

```text
| F | rule-id         | Weight  | Destination |   | cost  |
|---|-----------------|---------|-------------|---|-------|
| 1 | light-domestic  | <= 5    | "US", "CA"  | █ | 9.99  |
| 2 | medium-domestic | (5..20] | "US", "CA"  | █ | 19.99 |
| 3 | fallback        | -       | -           | █ | 49.99 |
```

### Example — Referencing upstream nodes

Within a `DecisionTask`, each node's result is bound in the evaluation context under the node's id (see [decision-node.spec.md § Evaluation](decision-node.spec.md#evaluation)). An **input column** reads an upstream output by naming it in the column's `Expr`; the cell then tests the resulting column value with the implicit-input forms. An **output cell**, being a full `bl.Expr` over the input context, may reference upstream outputs directly.

```go
Inputs: []Input{
    {`Eligibility`, `eligibility`, TypeString}, // column value = upstream `eligibility` output
},
Rules: Rules{
    // Eligibility      decision
    {`"eligible"`     , `"approved by " + reviewer`},
    {`not("eligible")`, `"declined"`               },
},
```

The input cell `"eligible"` is the equality unary test (`? = "eligible"`, where `?` is the column value); `not("eligible")` negates it. The output cell `"approved by " + reviewer` references another upstream output, `reviewer`, directly.

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

var discount = NewDecisionTable[DiscountOutputs](DecisionTableOpts{
    Id:        "discount",
    Name:      "Discount Rules",
    HitPolicy: HitPolicyFirst,
    Inputs: []Input{
        {`Type`,  `order.customerType`, TypeString},
        {`Total`, `order.total`,        TypeNumber},
    },
    Rules: Rules{
        // id           Type     Total    discount
        {`vip`        , `"VIP"`, `-`    , `0.20`},
        {`large-order`, `-`    , `> 500`, `0.10`},
        {`default`    , `-`    , `-`    , `0.0` },
    },
})

var result, _ = discount.Evaluate(map[string]any{
    "order": map[string]any{"customerType": "VIP", "total": 1000},
})
// result is the bl.BlNumber 0.20 — first rule matched, others not considered
```

### Example — COLLECT with Aggregation

```go
type PenaltyOutputs struct {
    Fine BlNumber
}

var sumAgg = AggregationSum

var penalties = NewDecisionTable[PenaltyOutputs](DecisionTableOpts{
    Id:          "penalties",
    Name:        "Penalty Assessment",
    HitPolicy:   HitPolicyCollect,
    Aggregation: &sumAgg,
    Inputs: []Input{
        {`Speed`, `incident.speed`,    TypeNumber},
        {`Zone`,  `incident.zoneType`, TypeString},
    },
    Rules: Rules{
        // id               Speed   Zone        fine
        {`school-speeding`, `> 30`, `"school"`, `200`},
        {`excessive-speed`, `> 50`, `-`       , `150`},
        {`school-zone`    , `-`   , `"school"`, `50` },
    },
})

var result, _ = penalties.Evaluate(map[string]any{
    "incident": map[string]any{"speed": 55, "zoneType": "school"},
})
// All three rules match — result is the bl.BlNumber 400 (200 + 150 + 50)
```

---

## Evaluation

1. For each `Input`, evaluate its compiled `Expr` against the input variables and bind the result to the column.
2. For each `Rule`, feed each non-wildcard input cell's column value through that cell's compiled `bl.UnaryTest`. An input cell must yield `bl.BlBoolean` true to match; `-` (and an absent cell when the table has no column for it) always matches.
3. A rule matches when **all** of its input cells match.
4. Apply the hit policy to the set of matching rules. Output cells are evaluated via their compiled `bl.Expr` against the input variables; an empty output cell yields `bl.BlNull`.
5. If no rule matches:
   - For `Unique`, `First`, `Priority`, `Any`: return `bl.BlNull` (not an error, unless surrounding semantics treat the output as required).
   - For `Collect`, `RuleOrder`, `OutputOrder`: return an empty list.

---

## Markdown Rendering

`ToMarkdown()` returns the decision table as a GitHub-flavoured markdown table string.

### Format

- The first row is headers: hit policy indicator in the first cell, then input column labels, then output names.
- The hit policy indicator is the standard single-letter abbreviation (`U`, `F`, `C`, …). For `Collect` with an aggregation, the aggregation symbol is appended (`C+`, `C<`, `C>`, `C#`).
- A visual separator column is placed between the last input column and the first output column. The header cell is empty, and every data row contains `█` (Unicode full block).
- Each subsequent row is a rule: rule index, then (if `showRuleIDs=true`) the rule id in a `rule-id` column, then input cells rendered as their unary-test source (`-` for wildcards), then the `█` separator, then output cells rendered as their expression source (empty for omitted outputs).
- When `showInputMappings=true`, a table mapping each input label to its `Expr` is rendered above the decision table.
- When `showRuleDescriptions=true`, a numbered list of the `Descriptions` entries is appended below the table.

### Example

```go
type RiskOutputs struct {
    Risk   BlString
    Reason BlString
}

var riskLevel = NewDecisionTable[RiskOutputs](DecisionTableOpts{
    Id:        "risk",
    Name:      "Risk Level",
    HitPolicy: HitPolicyUnique,
    Inputs: []Input{
        {`Age`,   `applicant.age`,         TypeNumber},
        {`Score`, `applicant.creditScore`, TypeNumber},
    },
    Rules: Rules{
        // id                 Age      Score    risk        reason
        {`young-good-credit`, `< 25` , `> 700`, `"low"`   , `"Young with good credit"`},
        {`older-any-credit` , `>= 25`, `-`    , `"medium"`, ``                        },
    },
    Descriptions: map[string]string{
        `young-good-credit`: `Young applicant with strong credit history`,
        `older-any-credit`:  `Standard risk for older applicants`,
    },
})

fmt.Println(riskLevel.ToMarkdown(false, false, false))
```

Output:

```text
| U | Age   | Score |   | risk     | reason                   |
|---|-------|-------|---|----------|--------------------------|
| 1 | < 25  | > 700 | █ | "low"    | "Young with good credit" |
| 2 | >= 25 | -     | █ | "medium" |                          |
```

---

## Edge Cases

- A `DecisionTable` with no input columns is valid; rows carry only output cells (and an optional id) and all rules match unconditionally.
- A `DecisionTable` whose `Outputs` struct has no exported fields is invalid — `NewDecisionTable` raises `DecisionDefinitionError` (see [decision-node.spec.md](decision-node.spec.md)).
- A `DecisionTable` with no `Rule` entries evaluates to `bl.BlNull` (or an empty list for multi-result policies) without error.
- Rows of inconsistent width, or a width matching neither `len(Inputs) + nOutputs` nor one more (id column), are a `DecisionDefinitionError`.
- A non-empty rule id that duplicates another rule's id in the table is a `DecisionDefinitionError`.
- An empty input cell is a `DecisionDefinitionError` — wildcards must be written `-`.
- An input cell that does not compile as a unary test for its column type — including an ordering/interval form against an unordered column — is a `DecisionDefinitionError` (wrapping the `bl.ParseError` from `bl.UnaryTest`).
- An output cell that does not compile via `bl.Expr` is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- A `Descriptions` key with no matching rule id is a `DecisionDefinitionError`.
- An output declared on the outputs struct that is set by no rule (every rule's cell for it is empty) is a `DecisionDefinitionError` — every declared output must be reachable.
- `HitPolicyCollect` with `AggregationSum`, `AggregationMin`, or `AggregationMax` over a non-numeric output is a `DecisionDefinitionError`.
- An input cell whose runtime column value disagrees with the declared column `Type` produces a `bl.TypeError` at evaluation time.
