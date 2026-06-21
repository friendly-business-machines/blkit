---
name: DecisionTable
description: A DecisionNode that defines decision logic as input columns, output columns, and rules whose cells are text expressions (unary tests for inputs, expressions for outputs). Declares its input and output contracts as plain []Field data; cells compile against the bl expression language at construction.
targets:
  - ../../core/decision_table.go
---

# DecisionTable

A `DecisionTable` is a [`DecisionNode`](decision-node.spec.md) that defines decision logic as a table of input conditions and output values. Each row is a `Rule`; a rule matches when all of its input cells are satisfied. The hit policy determines how matching rules are combined into the output.

Like every node, a `DecisionTable` declares its contracts as plain data (see [decision-node.spec.md § Contracts are plain data](decision-node.spec.md#contracts-are-plain-data-not-go-generics)): `Inputs` is the list of named, typed variables it consumes from outside, and `Outputs` is the list of named, typed values it produces. Between them sit the table's `Columns` — the input columns that turn consumed variables into comparable cell values. There is no type parameter and no reflected outputs struct.

Cells are **text expressions** in the blkit expression language (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). Input cells are **unary tests** (`>= 18`, `[650..749]`, `"US", "CA"`, `-`); output cells are **expressions** (`3.5`, `"eligible"`). Every cell is compiled once at construction into a `BlExpr` over the node's declared inputs — an input cell by inlining its column's `Expr` as the test subject (the `?`), an output cell directly via `bl.Expr` — so malformed cells fail fast as a `DecisionDefinitionError`. See [§ Compiling cells](#compiling-cells).

```go
type DecisionTable struct {
    Id          string
    Name        string
    Description string

    HitPolicy   HitPolicy
    Aggregation *Aggregation

    Columns      []Column
    Rules        Rules
    Descriptions map[string]string // optional; rule id -> human-readable description

    // inputs / outputs hold the declared contracts; exposed via the
    // DecisionNode interface methods Inputs() / Outputs().
    inputs  []Field
    outputs []Field

    // compiledRules parallels Rules in declaration order; each holds the rule's
    // inlined input predicates and output expressions, all BlExpr over the
    // inputs. Built by NewDecisionTable and walked by Evaluate.
    compiledRules []compiledRule
}

type compiledRule struct {
    inputs  []BlExpr // one per Column; a boolean predicate (the column Expr inlined as the `?` subject); the wildcard `-` is the constant-true predicate
    outputs []BlExpr // one per Output; nil for an omitted (empty) cell, which yields bl.BlNull
}

func NewDecisionTable(opts DecisionTableOpts) *DecisionTable

type DecisionTableOpts struct {
    Id          string
    Name        string
    Description string
    HitPolicy   HitPolicy    // default: HitPolicyUnique
    Aggregation *Aggregation // only valid with HitPolicyCollect

    // Inputs declares the named, typed variables the column and output
    // expressions consume from outside this node (task inputs, upstream node
    // outputs, or reference data).
    Inputs []Field

    // Outputs declares the named, typed values this node produces, one per
    // output column, in declaration (column) order.
    Outputs []Field

    Columns      []Column
    Rules        Rules
    Descriptions map[string]string
}

// DecisionNode interface satisfaction.
func (d *DecisionTable) GetId() string
func (d *DecisionTable) GetName() string
func (d *DecisionTable) GetDescription() string
func (d *DecisionTable) Inputs() []Field
func (d *DecisionTable) Outputs() []Field

// Evaluate the table against the input variables, returning a map keyed by this
// node's output names (see decision-node.spec.md). Single-hit policies map each
// output name to a scalar; multi-hit policies map each output name to a list.
func (d *DecisionTable) Evaluate(input map[string]BlValue) (map[string]BlValue, error)

// Render the table as a markdown string
func (d *DecisionTable) ToMarkdown(
    showRuleIDs bool,
    showRuleDescriptions bool,
    showInputMappings bool,
) string
```

`NewDecisionTable` validates the input and output contracts (well-formed names and types, no duplicates), compiles every column expression and output cell against a schema built from the declared inputs, compiles every input cell into a boolean predicate over those inputs (its column's `Expr` inlined as the test subject), and checks that every rule row is the expected width and that every declared output is set by at least one rule. See [§ Compiling cells](#compiling-cells) for the full pipeline.

The exported `Columns` and `Rules` retain their original raw sources (used by `ToMarkdown` and for inspection). The compiled predicates and output expressions live in the unexported `compiledRules`, populated by `NewDecisionTable`. There is **no separate column storage** — each column's `Expr` is folded into the input predicates that reference it — and **no topological sort**: rules and columns keep declaration order (which the `First`, `Priority`, and `RuleOrder` hit policies depend on), so there is no `evalPlan`-style structure as in [`DecisionExpression`](decision-expression.spec.md).

---

## Column

A `Column` is a labelled, typed input column. `Expr` is a source expression over the node's declared inputs; at construction it is inlined as the subject (the `?`) of each of this column's input cells. `Type` is the type that subject holds and **bounds the valid unary-test forms** for the column. A column whose `Expr` is a bare input name simply forwards that input.

```go
type Column struct {
    Label string // column header
    Expr  string // source expression over declared inputs; inlined as the `?` subject of each input cell
    Type  Type   // the type the column subject holds; bounds the valid unary-test forms
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

The id column is **all-or-nothing per table**: either every row carries a leading id cell or none do. The constructor infers which from the uniform row width — `len(Columns) + len(Outputs)` means no id column; one more means the first cell is the id. Rows of differing width, or a width matching neither, are a `DecisionDefinitionError`.

Cell conventions:

- **id cell** — empty (`` `` ``) means this rule has no id; a non-empty id must be unique within the table.
- **input cell** — a unary-test source (see [bl-expr.spec.md § Unary tests](../expressions/bl-expr.spec.md#unary-tests)). At construction the column's `Expr` is substituted for the test's `?` subject, producing a boolean `BlExpr` over the inputs (compiled via `bl.Expr`). `-` is the wildcard → the constant-true predicate (matches any value, including null). An empty input cell is invalid — write `-` for "matches anything".
- **output cell** — an expression source compiled via `bl.Expr` against the input contract, in `Outputs` order. An empty output cell (`` `` ``) means "no value" and yields `bl.BlNull` for that output (it is not compiled).

Use Go **raw string literals** (backticks) for cells, so expression-language string literals need no escaping: an output of the string `eligible` is the cell `` `"eligible"` ``.

A rule matches when **all** of its non-wildcard input cells evaluate to `bl.BlBoolean` true. Per-rule descriptions live in the table's optional `Descriptions` map, keyed by rule id (a key with no matching rule id is a `DecisionDefinitionError`).

### Example — Single Output Column

```go
var eligibility = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: bl.HitPolicyUnique,
    Inputs: []bl.Field{
        {Name: "age", Type: bl.TypeNumber},
        {Name: "income", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "eligibility", Type: bl.TypeString},
    },
    Columns: []bl.Column{
        {Label: "Age", Expr: `age`, Type: bl.TypeNumber},
        {Label: "Income", Expr: `income`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // Age    Income      eligibility
        {`>= 18`, `>= 30000`, `"eligible"`},
        {`< 18`, `-`, `"ineligible"`},
        {`-`, `< 30000`, `"ineligible"`},
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

Output cells are positional, in `Outputs` order (`rate`, then `term`). This table uses the optional id column.

```go
var loanPricing = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "pricing",
    Name:      "Loan Pricing",
    HitPolicy: bl.HitPolicyUnique,
    Inputs: []bl.Field{
        {Name: "credit_score", Type: bl.TypeNumber},
        {Name: "loan_amount", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "rate", Type: bl.TypeNumber},
        {Name: "term", Type: bl.TypeNumber},
    },
    Columns: []bl.Column{
        {Label: "Score", Expr: `credit_score`, Type: bl.TypeNumber},
        {Label: "Amount", Expr: `loan_amount`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // id        Score         Amount       rate   term
        {`prime`, `>= 750`, `<= 500000`, `3.5`, `360`},
        {`standard`, `[650..749]`, `-`, `5.0`, `240`},
        {`subprime`, `< 650`, `-`, `7.5`, `180`},
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
var shipping = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "shipping",
    Name:      "Shipping Cost",
    HitPolicy: bl.HitPolicyFirst,
    Inputs: []bl.Field{
        {Name: "weight", Type: bl.TypeNumber},
        {Name: "destination", Type: bl.TypeString},
    },
    Outputs: []bl.Field{
        {Name: "cost", Type: bl.TypeNumber},
    },
    Columns: []bl.Column{
        {Label: "Weight", Expr: `weight`, Type: bl.TypeNumber},
        {Label: "Destination", Expr: `destination`, Type: bl.TypeString},
    },
    Rules: bl.Rules{
        // id               Weight     Destination   cost
        {`light-domestic`, `<= 5`, `"US", "CA"`, `9.99`},
        {`medium-domestic`, `(5..20]`, `"US", "CA"`, `19.99`},
        {`fallback`, `-`, `-`, `49.99`},
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

A node consumes an upstream node's output by declaring an input of that output's name (see [decision-task.spec.md § Wiring](decision-task.spec.md#wiring)). An **input column** turns a consumed value into a comparable column by naming it in the column's `Expr`; the cell then tests the resulting column value with the implicit-input forms. An **output cell**, being a full `bl.Expr` over the input contract, may reference consumed inputs directly.

```go
Inputs: []bl.Field{
    {Name: "eligibility", Type: bl.TypeString}, // an upstream node's output
    {Name: "reviewer", Type: bl.TypeString},    // an upstream node's output
},
Outputs: []bl.Field{
    {Name: "decision", Type: bl.TypeString},
},
Columns: []bl.Column{
    {Label: "Eligibility", Expr: `eligibility`, Type: bl.TypeString},
},
Rules: bl.Rules{
    // Eligibility      decision
    {`"eligible"`, `"approved by " + reviewer`},
    {`not("eligible")`, `"declined"`},
},
```

The input cell `"eligible"` is the equality unary test (`? = "eligible"`, where `?` is the column value); `not("eligible")` negates it. The output cell `"approved by " + reviewer` references the consumed input `reviewer` directly.

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

Every policy returns a `map[string]BlValue` keyed by output name. The shape of each output's value depends on the policy:

| Policy | Per-output value | Multiple matches | Notes |
|---|---|---|---|
| `Unique` | scalar | error | Default |
| `First` | scalar | first match wins | Rule order matters |
| `Priority` | scalar | highest priority wins | Priority by declared output-value order |
| `Any` | scalar | allowed if outputs identical | Error if outputs differ |
| `Collect` + Aggregation | scalar | all matches aggregated | Numeric outputs only for Sum/Min/Max |
| `Collect` (no aggregation) | list | all match outputs as a list | — |
| `RuleOrder` | list | all matches in rule order | — |
| `OutputOrder` | list | all matches sorted by priority | — |

Each output column is collected independently: under a list-returning policy, every output name maps to its own `bl.BlList` of values across the matching rules.

### Example — FIRST Hit Policy

```go
var discount = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "discount",
    Name:      "Discount Rules",
    HitPolicy: bl.HitPolicyFirst,
    Inputs: []bl.Field{
        {Name: "customer_type", Type: bl.TypeString},
        {Name: "order_total", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "discount", Type: bl.TypeNumber},
    },
    Columns: []bl.Column{
        {Label: "Type", Expr: `customer_type`, Type: bl.TypeString},
        {Label: "Total", Expr: `order_total`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // id           Type     Total    discount
        {`vip`, `"VIP"`, `-`, `0.20`},
        {`large-order`, `-`, `> 500`, `0.10`},
        {`default`, `-`, `-`, `0.0`},
    },
})

var result, _ = discount.Evaluate(map[string]bl.BlValue{
    "customer_type": bl.String("VIP"),
    "order_total":   bl.Number(1000),
})
// result is map[string]bl.BlValue{"discount": 0.20} — first rule matched
```

### Example — COLLECT with Aggregation

```go
var sumAgg = bl.AggregationSum

var penalties = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:          "penalties",
    Name:        "Penalty Assessment",
    HitPolicy:   bl.HitPolicyCollect,
    Aggregation: &sumAgg,
    Inputs: []bl.Field{
        {Name: "speed", Type: bl.TypeNumber},
        {Name: "zone_type", Type: bl.TypeString},
    },
    Outputs: []bl.Field{
        {Name: "fine", Type: bl.TypeNumber},
    },
    Columns: []bl.Column{
        {Label: "Speed", Expr: `speed`, Type: bl.TypeNumber},
        {Label: "Zone", Expr: `zone_type`, Type: bl.TypeString},
    },
    Rules: bl.Rules{
        // id               Speed   Zone        fine
        {`school-speeding`, `> 30`, `"school"`, `200`},
        {`excessive-speed`, `> 50`, `-`, `150`},
        {`school-zone`, `-`, `"school"`, `50`},
    },
})

var result, _ = penalties.Evaluate(map[string]bl.BlValue{
    "speed":     bl.Number(55),
    "zone_type": bl.String("school"),
})
// All three rules match — result is map[string]bl.BlValue{"fine": 400} (200 + 150 + 50)
```

---

## Compiling cells

`NewDecisionTable` validates the contracts and compiles every cell into a `BlExpr` over the declared inputs. It proceeds in four steps:

1. **Validate the contracts.** Each failure is a `DecisionDefinitionError`:
   - `Inputs` and `Outputs` are each well-formed: every `Field` has a valid name and type;
   - no name is duplicated within `Inputs`, nor within `Outputs`;
   - `Outputs` is non-empty.
2. **Compile each column `Expr`** via `bl.Expr` against a schema of the declared `Inputs`, validating it (well-formed, references only declared inputs). Doing this for every column — not only those reached by a non-wildcard cell — catches a malformed column expression even when all of its cells are wildcards.
3. **Compile each rule's cells** into `compiledRule`:
   - **input cell** — the column's `Expr` is substituted for the unary test's `?` subject and the resulting predicate is compiled via `bl.Expr` against the input schema, giving a boolean `BlExpr` over the inputs; `-` compiles to the constant-true predicate. The column `Type` bounds the legal forms (an ordering/interval form against a non-comparable column is rejected here).
   - **output cell** — compiled via `bl.Expr` against the input schema; an empty cell is left `nil` and yields `bl.BlNull`.
4. **Structural checks** — every rule row is the expected width; every declared output is set by at least one rule; rule ids are unique; every `Descriptions` key matches a rule id; a `Collect` `Sum`/`Min`/`Max` aggregation targets a numeric output.

There is no topological sort: rules and columns keep declaration order. The compiled forms are stored in `compiledRules`; the raw sources stay in the exported `Columns`/`Rules`.

---

## Validation and type safety

A `DecisionTable` carries its own contracts as plain data, so safety is contract-matching, not name-inference. Following the family rule (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)), the checks are concentrated at **construction** — the mental model is *if construction does not complain, the node is well-formed* — with only value-versus-declaration correctness left to evaluation.

These are **runtime** checks, not Go compile-time ones. "Construction" means the moment the `NewDecisionTable` (or `NewDecisionTask`) constructor executes; a `DecisionTable`'s contracts are `[]Field` data and its cells are raw strings, all outside the Go type system, so a malformed node is rejected when its constructor runs, never by `go build`. Likewise "a cell compiles" means `bl.Expr` parses its expression-language source — also at construction — not Go compilation.

`NewDecisionTable` does not return an error: following the decision-family convention, it accumulates every construction-time problem and **panics once** with a `*DecisionDefinitionError` (see [decision-task.spec.md](decision-task.spec.md), which documents the same convention). Because a `DecisionTable` is typically declared as a package-scope `var` — including inside a package the application author writes — its construction runs during that package's **initialisation**, when the program (or its test binary) starts, before `main`. A malformed node therefore aborts the program at startup: the panic lists each problem and the stack trace pins the offending declaration. This is not compile-time safety, but it is deterministic **load-time fail-fast** — any run of the program, or any test that merely imports the declaring package, surfaces every construction error, regardless of whether a code path later evaluates the node.

Three moments catch three distinct classes of problem:

| Moment | Trigger | What it catches | Raised as |
|--------|---------|-----------------|-----------|
| **Node construction** | `NewDecisionTable` | A malformed contract (an ill-formed `Inputs`/`Outputs` name or type, a duplicate name within either list, or an empty `Outputs`); a column `Expr` or output cell that fails to compile; an input cell whose inlined predicate fails to compile (including an ordering/interval form against a non-comparable column type); a name referenced but not declared in `Inputs`; a wrong-width rule row; an empty input cell (`-` is required); a duplicate rule id; a `Descriptions` key matching no rule; a declared output set by no rule; a `Collect` `Sum`/`Min`/`Max` over a non-numeric output. | `DecisionDefinitionError` |
| **Task construction** | `NewDecisionTask` | A declared input with no producer of matching name **and** declared type; an output name or `Id` that collides with another node in the task; a cross-node cycle. | `DecisionDefinitionError` |
| **Evaluation** | `Evaluate` | A runtime type mismatch inside an input predicate or output expression — e.g. a column value that disagrees with the form it is tested against. | `bl.TypeError` |

**Node construction** is detailed in [§ Compiling cells](#compiling-cells): it checks the one node in isolation. **Task construction** checks the whole graph and is detailed in [decision-task.spec.md § Wiring](decision-task.spec.md#wiring); a standalone node — evaluated with no containing task — skips this moment entirely, and the caller is then responsible for supplying inputs of the right type. **Evaluation** is the only moment that inspects *values*: the expression engine is runtime-typed, so a value that disagrees with a declared type cannot be caught earlier and surfaces here as a `bl.TypeError`.

---

## Evaluation

`Evaluate` is stateless: the `DecisionTable` is immutable after construction, and each call works against its own local scope, so concurrent calls do not interfere.

The scope is a `BlDictionary` of the supplied input variables — the one `BlValue` shape a compiled `bl.BlExpr` spreads into named variables when evaluated (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). The `map[string]BlValue` of the `Evaluate` signature is just the API boundary; internally it is carried as this dictionary. Unlike [`DecisionExpression`](decision-expression.spec.md), there is **no column-binding step and no inter-rule accumulation**: every input predicate and output expression is a self-contained `BlExpr` evaluated against this same input scope (each column's `Expr` is already inlined into the predicates that use it).

1. **Match.** For each `Rule`, evaluate its input predicates against the scope. The rule matches when **all** of them yield `bl.BlBoolean` true; a wildcard (`-`) is the constant-true predicate.
2. **Combine.** Apply the hit policy to the set of matching rules, evaluating their output expressions against the scope; an empty output cell yields `bl.BlNull`.
3. **Project.** The result is a fresh `map[string]BlValue` keyed by the declared output names — a scalar per output for single-hit policies, a `bl.BlList` per output for list-returning policies. The input variables are never copied into the result.
4. If no rule matches:
   - For `Unique`, `First`, `Priority`, `Any`: every output maps to `bl.BlNull`.
   - For `Collect`, `RuleOrder`, `OutputOrder`: every output maps to an empty list.

### Standalone vs. within a task

The input map handed to `Evaluate` is the same map of `Inputs()` values regardless of how the node is driven; only its source differs:

- **Standalone** — with no containing task, the caller supplies every value the node's `Inputs()` declare directly to `Evaluate`.
- **Within a `DecisionTask`** — the task resolves each declared input from the producer it was wired to at task construction (an upstream node output, a task input, or reference data) and passes the assembled map to `Evaluate`.

Either way `Evaluate` behaves identically: it builds the scope from the supplied inputs and evaluates the rules as above.

---

## Markdown Rendering

`ToMarkdown()` returns the decision table as a GitHub-flavoured markdown table string.

### Format

- The first row is headers: hit policy indicator in the first cell, then column labels, then output names.
- The hit policy indicator is the standard single-letter abbreviation (`U`, `F`, `C`, …). For `Collect` with an aggregation, the aggregation symbol is appended (`C+`, `C<`, `C>`, `C#`).
- A visual separator column is placed between the last input column and the first output column. The header cell is empty, and every data row contains `█` (Unicode full block).
- Each subsequent row is a rule: rule index, then (if `showRuleIDs=true`) the rule id in a `rule-id` column, then input cells rendered as their unary-test source (`-` for wildcards), then the `█` separator, then output cells rendered as their expression source (empty for omitted outputs).
- When `showInputMappings=true`, a table mapping each column label to its `Expr` is rendered above the decision table.
- When `showRuleDescriptions=true`, a numbered list of the `Descriptions` entries is appended below the table.

### Example

```go
var riskLevel = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "risk",
    Name:      "Risk Level",
    HitPolicy: bl.HitPolicyUnique,
    Inputs: []bl.Field{
        {Name: "age", Type: bl.TypeNumber},
        {Name: "credit_score", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "risk", Type: bl.TypeString},
        {Name: "reason", Type: bl.TypeString},
    },
    Columns: []bl.Column{
        {Label: "Age", Expr: `age`, Type: bl.TypeNumber},
        {Label: "Score", Expr: `credit_score`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // id                 Age      Score    risk        reason
        {`young-good-credit`, `< 25`, `> 700`, `"low"`, `"Young with good credit"`},
        {`older-any-credit`, `>= 25`, `-`, `"medium"`, ``},
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

- A `DecisionTable` with no columns is valid; rows carry only output cells (and an optional id) and all rules match unconditionally.
- A `DecisionTable` whose `Outputs` is empty is invalid — `NewDecisionTable` raises `DecisionDefinitionError`.
- A `DecisionTable` with no `Rule` entries evaluates each output to `bl.BlNull` (or an empty list for multi-result policies) without error.
- Rows of inconsistent width, or a width matching neither `len(Columns) + len(Outputs)` nor one more (id column), are a `DecisionDefinitionError`.
- A duplicate name within `Inputs` or within `Outputs` is a `DecisionDefinitionError`.
- A non-empty rule id that duplicates another rule's id in the table is a `DecisionDefinitionError`.
- An empty input cell is a `DecisionDefinitionError` — wildcards must be written `-`.
- An input cell whose inlined predicate does not compile — including an ordering/interval form against an unordered column — is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An output cell, or a column expression, that does not compile via `bl.Expr` is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- A column or output expression that references a name not declared in `Inputs` is a `DecisionDefinitionError`.
- A `Descriptions` key with no matching rule id is a `DecisionDefinitionError`.
- An output set by no rule (every rule's cell for it is empty) is a `DecisionDefinitionError` — every declared output must be reachable.
- `HitPolicyCollect` with `AggregationSum`, `AggregationMin`, or `AggregationMax` over a non-numeric output is a `DecisionDefinitionError`.
- A runtime type mismatch inside an input predicate (the column value versus the form it is tested against) surfaces as a `bl.TypeError` at evaluation time.
