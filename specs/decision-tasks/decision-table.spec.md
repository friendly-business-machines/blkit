---
name: DecisionTable
description: A generic DecisionNode[I, O] that defines decision logic as input columns, output columns, and rules whose cells are text expressions (unary tests for inputs, expressions for outputs) over a concrete Go input struct I, producing a concrete output struct O. Cells compile against the bl expression language at construction; Evaluate(in I) returns O, type-checked at Go compile time.
targets:
  - ../../core/decision_table.go
---

# DecisionTable

A `DecisionTable[I, O]` is a [`DecisionNode[I, O]`](decision-node.spec.md) that defines decision logic as a table of input conditions and output values over two concrete Go structs: an **input** struct `I` and an **output** struct `O`. Each row is a `Rule`; a rule matches when all of its input cells are satisfied. The hit policy determines how matching rules are combined into the output.

Like every node, a `DecisionTable` declares its contracts as concrete Go structs whose every exported field is a `bl.Handle` variable (see [decision-node.spec.md § Contracts are concrete Go structs](decision-node.spec.md#contracts-are-concrete-go-structs)): the fields of `I` are the variables it consumes, and the fields of `O` are the values it produces — one per output column, in declaration order. Between the two sit the table's `Columns` — the input columns that turn consumed variables into comparable cell values.

Because the contracts are concrete Go structs, a caller that passes the wrong input shape or reads a non-existent output gets a **Go compile error**, and `Evaluate(in I) (O, error)` returns a typed `O`. Within a task, the same node is driven through the netlist (see [decision-task.spec.md § Wiring](decision-task.spec.md#wiring)).

Cells are **text expressions** in the blkit expression language (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). Input cells are **unary tests** (`>= 18`, `[650..749]`, `"US", "CA"`, `-`); output cells are **expressions** (`3.5`, `"eligible"`). Every cell is compiled once at construction into a `BlExpr` over the node's declared inputs — an input cell by inlining its column's `Expr` as the test subject (the `?`), an output cell directly via `bl.Expr` — so malformed cells fail fast as a `DecisionDefinitionError`. See [§ Compiling cells](#compiling-cells).

```go
type DecisionTable[I, O any] struct {
    In  I // input port surface — stamped handles for wiring (eligibility.In.Age)
    Out O // output port surface — stamped handles for wiring (eligibility.Out.Eligibility)
    // unexported: id, name, description, hit policy, aggregation, columns, rules,
    // descriptions, and the compiled rule predicates/output expressions.
}

func NewDecisionTable[I, O any](config DecisionTableConfig) *DecisionTable[I, O]

type DecisionTableConfig struct {
    Id          string
    Name        string
    Description string

    HitPolicy   HitPolicy    // default: HitPolicyUnique
    Aggregation *Aggregation // only valid with HitPolicyCollect

    // Columns are the input columns: each Expr is a source expression over the
    // variables of I, inlined as the `?` subject of that column's input cells.
    Columns []Column

    // Rules are the rows: input cells (unary tests) then output cells
    // (expressions), one output cell per field of O in declaration order.
    Rules        Rules
    Descriptions map[string]string // optional; rule id -> human-readable description
}

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionTable[I, O]) GetId() string
func (d *DecisionTable[I, O]) GetName() string
func (d *DecisionTable[I, O]) GetDescription() string
func (d *DecisionTable[I, O]) Inputs() []Field  // reflected from I
func (d *DecisionTable[I, O]) Outputs() []Field // reflected from O

// Evaluate the table against the typed input struct, returning a typed output
// struct. For single-hit policies each output handle holds a scalar; for
// list-returning policies each output handle holds a bl.BlList.
func (d *DecisionTable[I, O]) Evaluate(in I) (O, error)

// Render the table as a markdown string.
func (d *DecisionTable[I, O]) ToMarkdown(
    showRuleIDs bool,
    showRuleDescriptions bool,
    showInputMappings bool,
) string

// DecisionDefinitionError reports one or more construction problems (shared
// across the decision family).
type DecisionDefinitionError struct {
    Node     string
    Problems []string
}
```

`NewDecisionTable` reflects over `I` and `O` to validate the contracts (the shared reflection contract: every field an exported `bl.Handle[BlValue]`, valid identifier names, no duplicate name within a struct), compiles every column expression and output cell against a schema built from `I`, compiles every input cell into a boolean predicate over `I` (its column's `Expr` inlined as the test subject), and checks that every rule row is the expected width and that every field of `O` is set by at least one rule. See [§ Compiling cells](#compiling-cells).

The compiled predicates and output expressions live unexported; the original raw `Columns`/`Rules` sources are retained (for `ToMarkdown` and inspection). There is **no topological sort**: rules and columns keep declaration order (which the `First`, `Priority`, and `RuleOrder` hit policies depend on).

---

## Column

A `Column` is a labelled, typed input column. `Expr` is a source expression over the variables of `I`; at construction it is inlined as the subject (the `?`) of each of this column's input cells. `Type` is the type that subject holds and **bounds the valid unary-test forms** for the column. A column whose `Expr` is a bare input variable simply forwards it.

```go
type Column struct {
    Label string // column header
    Expr  string // source expression over I's variables; inlined as the `?` subject of each input cell
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

There is one output cell per field of `O`, in `O`'s declaration order. The id column is **all-or-nothing per table**: either every row carries a leading id cell or none do. The constructor infers which from the uniform row width — `len(Columns) + (number of O fields)` means no id column; one more means the first cell is the id. Rows of differing width, or a width matching neither, are a `DecisionDefinitionError`.

Cell conventions:

- **id cell** — empty (`` `` ``) means this rule has no id; a non-empty id must be unique within the table.
- **input cell** — a unary-test source (see [bl-expr.spec.md § Unary tests](../expressions/bl-expr.spec.md#unary-tests)). At construction the column's `Expr` is substituted for the test's `?` subject, producing a boolean `BlExpr` over `I` (compiled via `bl.Expr`). `-` is the wildcard → the constant-true predicate (matches any value, including null). An empty input cell is invalid — write `-` for "matches anything".
- **output cell** — an expression source compiled via `bl.Expr` against `I`, assigned to the matching field of `O`. An empty output cell (`` `` ``) means "no value" and yields `bl.BlNull` for that output (it is not compiled).

Use Go **raw string literals** (backticks) for cells, so expression-language string literals need no escaping: an output of the string `eligible` is the cell `` `"eligible"` ``.

A rule matches when **all** of its non-wildcard input cells evaluate to `bl.BlBoolean` true. Per-rule descriptions live in the table's optional `Descriptions` map, keyed by rule id (a key with no matching rule id is a `DecisionDefinitionError`).

### Example — Single Output Column

```go
type EligibilityInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Points bl.Handle[bl.BlNumber] `expr:"points"`
}
type EligibilityOutputs struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
}

var eligibility = bl.NewDecisionTable[EligibilityInputs, EligibilityOutputs](bl.DecisionTableConfig{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: bl.HitPolicyUnique,
    Columns: []bl.Column{
        {Label: "Age", Expr: `age`, Type: bl.TypeNumber},
        {Label: "Points", Expr: `points`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // Age    Points      eligibility
        {`>= 18`, `>= 30000`, `"eligible"`},
        {`< 18`, `-`, `"ineligible"`},
        {`-`, `< 30000`, `"ineligible"`},
    },
})

// Standalone evaluation builds a typed input struct with value-carrying handles.
var age, _    = bl.Number(30)
var points, _ = bl.Number(50000)
var out, _    = eligibility.Evaluate(EligibilityInputs{
    Age:    bl.NewHandle(age),
    Points: bl.NewHandle(points),
})
// out.Eligibility.Get() == bl.String("eligible")
```

Output of `eligibility.ToMarkdown(false, false, false)`:

```text
| U | Age   | Points   |   | eligibility  |
|---|-------|----------|---|--------------|
| 1 | >= 18 | >= 30000 | █ | "eligible"   |
| 2 | < 18  | -        | █ | "ineligible" |
| 3 | -     | < 30000  | █ | "ineligible" |
```

### Example — Multiple Output Columns

Output cells are positional, in `O`'s field-declaration order (`Rate`, then `Term`). This table uses the optional id column.

```go
type PlanInputs struct {
    Usage  bl.Handle[bl.BlNumber] `expr:"usage"`
    Volume bl.Handle[bl.BlNumber] `expr:"volume"`
}
type PlanPricing struct {
    Rate bl.Handle[bl.BlNumber] `expr:"rate"`
    Term bl.Handle[bl.BlNumber] `expr:"term"`
}

var planPricing = bl.NewDecisionTable[PlanInputs, PlanPricing](bl.DecisionTableConfig{
    Id:        "pricing",
    Name:      "Plan Pricing",
    HitPolicy: bl.HitPolicyUnique,
    Columns: []bl.Column{
        {Label: "Usage", Expr: `usage`, Type: bl.TypeNumber},
        {Label: "Volume", Expr: `volume`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // id          Usage         Volume       rate   term
        {`platinum`, `>= 750`, `<= 500000`, `3.5`, `360`},
        {`standard`, `[650..749]`, `-`, `5.0`, `240`},
        {`starter`, `< 650`, `-`, `7.5`, `180`},
    },
})
```

Output of `planPricing.ToMarkdown(true, false, false)`:

```text
| U | rule-id  | Usage      | Volume    |   | rate | term |
|---|----------|------------|-----------|---|------|------|
| 1 | platinum | >= 750     | <= 500000 | █ | 3.5  | 360  |
| 2 | standard | [650..749] | -         | █ | 5.0  | 240  |
| 3 | starter  | < 650      | -         | █ | 7.5  | 180  |
```

### Example — Range and List Membership

Range membership is the interval unary test (`<= 5`, `(5..20]`); list membership is the comma-disjunction form (`"US", "CA"` matches either value).

```go
type ShippingInputs struct {
    Weight      bl.Handle[bl.BlNumber] `expr:"weight"`
    Destination bl.Handle[bl.BlString] `expr:"destination"`
}
type ShippingOutputs struct {
    Cost bl.Handle[bl.BlNumber] `expr:"cost"`
}

var shipping = bl.NewDecisionTable[ShippingInputs, ShippingOutputs](bl.DecisionTableConfig{
    Id:        "shipping",
    Name:      "Shipping Cost",
    HitPolicy: bl.HitPolicyFirst,
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

### Output cells reference input variables

An output cell is a full `bl.Expr` over `I`, so it may reference any input variable by its `expr` name, not just compare it in a column:

```go
type ReviewInputs struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
    Reviewer    bl.Handle[bl.BlString] `expr:"reviewer"`
}
type ReviewOutputs struct {
    Decision bl.Handle[bl.BlString] `expr:"decision"`
}

// Columns over `eligibility`; the output cell reads `reviewer` directly.
Columns: []bl.Column{
    {Label: "Eligibility", Expr: `eligibility`, Type: bl.TypeString},
},
Rules: bl.Rules{
    // Eligibility      decision
    {`"eligible"`, `"approved by " + reviewer`},
    {`not("eligible")`, `"declined"`},
},
```

Whether `eligibility` and `reviewer` are fed from a task input, an upstream node's output, or reference data is decided when the node is wired into a task (see [decision-task.spec.md § Wiring](decision-task.spec.md#wiring)) — by connecting a producer's output handle to `node.In.Eligibility` / `node.In.Reviewer`. The input cell `"eligible"` is the equality unary test (`? = "eligible"`); `not("eligible")` negates it.

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

Every policy fills the fields of `O`. The shape each output field holds depends on the policy:

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

Each output column is collected independently. A **list-returning** policy (`Collect` with no aggregation, `RuleOrder`, `OutputOrder`) yields a `bl.BlList` per output, so **every field of `O` must be a `bl.Handle[bl.BlList]`** — declaring a scalar output under such a policy is a `DecisionDefinitionError` at construction. Single-hit policies and `Collect`+aggregation fill scalar output handles.

### Example — FIRST Hit Policy

```go
type DiscountInputs struct {
    CustomerType bl.Handle[bl.BlString] `expr:"customer_type"`
    OrderTotal   bl.Handle[bl.BlNumber] `expr:"order_total"`
}
type DiscountOutputs struct {
    Discount bl.Handle[bl.BlNumber] `expr:"discount"`
}

var discount = bl.NewDecisionTable[DiscountInputs, DiscountOutputs](bl.DecisionTableConfig{
    Id:        "discount",
    Name:      "Discount Rules",
    HitPolicy: bl.HitPolicyFirst,
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

var ct, _     = bl.String("VIP")
var ot, _     = bl.Number(1000)
var result, _ = discount.Evaluate(DiscountInputs{
    CustomerType: bl.NewHandle(ct),
    OrderTotal:   bl.NewHandle(ot),
})
// result.Discount.Get() == bl.Number(0.20) — first rule matched
```

### Example — COLLECT with Aggregation

```go
type PenaltyInputs struct {
    Speed    bl.Handle[bl.BlNumber] `expr:"speed"`
    ZoneType bl.Handle[bl.BlString] `expr:"zone_type"`
}
type PenaltyOutputs struct {
    Fine bl.Handle[bl.BlNumber] `expr:"fine"`
}

var sumAgg = bl.AggregationSum

var penalties = bl.NewDecisionTable[PenaltyInputs, PenaltyOutputs](bl.DecisionTableConfig{
    Id:          "penalties",
    Name:        "Penalty Assessment",
    HitPolicy:   bl.HitPolicyCollect,
    Aggregation: &sumAgg,
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

var sp, _     = bl.Number(55)
var zt, _     = bl.String("school")
var result, _ = penalties.Evaluate(PenaltyInputs{
    Speed:    bl.NewHandle(sp),
    ZoneType: bl.NewHandle(zt),
})
// All three rules match — result.Fine.Get() == bl.Number(400) (200 + 150 + 50)
```

---

## Compiling cells

`NewDecisionTable` validates the contracts and compiles every cell into a `BlExpr` over `I`. It proceeds in four steps:

1. **Validate the contracts.** Reflect over `I` and `O` and apply the shared reflection contract (see [decision-node.spec.md § Contracts are concrete Go structs](decision-node.spec.md#contracts-are-concrete-go-structs)): every field an exported `bl.Handle[BlValue]`, names valid identifiers, no duplicate within a struct, and `O` non-empty. Each failure is a `DecisionDefinitionError`.
2. **Compile each column `Expr`** via `bl.Expr` against a schema of `I`'s variables, validating it (well-formed, references only declared inputs). Doing this for every column — not only those reached by a non-wildcard cell — catches a malformed column expression even when all of its cells are wildcards.
3. **Compile each rule's cells**:
   - **input cell** — the column's `Expr` is substituted for the unary test's `?` subject and the resulting predicate is compiled via `bl.Expr` against `I`, giving a boolean `BlExpr`; `-` compiles to the constant-true predicate. The column `Type` bounds the legal forms (an ordering/interval form against a non-comparable column is rejected here).
   - **output cell** — compiled via `bl.Expr` against `I`; an empty cell is left uncompiled and yields `bl.BlNull`.
4. **Structural checks** — every rule row is the expected width; every field of `O` is set by at least one rule; rule ids are unique; every `Descriptions` key matches a rule id; a `Collect` `Sum`/`Min`/`Max` aggregation targets a numeric output; a list-returning policy's `O` fields are all `Handle[bl.BlList]`.

There is no topological sort: rules and columns keep declaration order. The compiled forms are stored unexported; the raw sources stay in the retained `Columns`/`Rules`.

---

## Validation and type safety

A `DecisionTable` gets its safety from the family's three moments (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)): Go compile time for the typed boundary and the wiring netlist, construction for everything outside the Go type system, and evaluation for runtime values.

Following the decision-family convention, `NewDecisionTable` does not return an error: it accumulates every construction problem and **panics once** with a `*DecisionDefinitionError`. Because a `DecisionTable` is typically a package-scope `var`, a malformed node aborts the program (or its test binary) at startup — deterministic load-time fail-fast.

What each moment catches, one per phase — compile, runtime init, and runtime:

| Phase | Moment | Trigger | What it catches | Raised as |
|-------|--------|---------|-----------------|-----------|
| **Compile** | Go compilation | `go build` | A caller passing an input value of the wrong type to `Evaluate`, or reading an undeclared output field; a `bl.Edge` connecting one of this node's handles to a handle of a different type, or naming a field it does not have. | Go type error |
| **Runtime init** | Node construction | `NewDecisionTable` | A non-struct `I`/`O`; an unexported field; a field that is not a `bl.Handle[BlValue]`; an invalid variable name; a duplicate name within a struct; an empty `O`; a column `Expr` or output cell that fails to compile; an input cell whose inlined predicate fails to compile (including an ordering/interval form against a non-comparable column); a name referenced but not declared in `I`; a wrong-width rule row; an empty input cell (`-` is required); a duplicate rule id; a `Descriptions` key matching no rule; an output set by no rule; a `Collect` `Sum`/`Min`/`Max` over a non-numeric output; a list-returning policy whose `O` fields are not all `Handle[bl.BlList]`. | `DecisionDefinitionError` |
| **Runtime** | Evaluation | `Evaluate` | A runtime type mismatch inside an input predicate or output expression — e.g. a column value that disagrees with the form it is tested against, or a produced value whose type disagrees with its declared output handle. | `bl.TypeError` |

---

## Evaluation

`Evaluate` is stateless: the `DecisionTable` is immutable after construction, and each call works against its own local scope, so concurrent calls do not interfere.

The scope is built from the supplied input struct `I`: each input handle's value is bound under its variable name, giving the `BlDictionary` a compiled `bl.BlExpr` spreads into named variables when evaluated (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). Every input predicate and output expression is a self-contained `BlExpr` evaluated against this scope (each column's `Expr` is already inlined into the predicates that use it).

1. **Match.** For each `Rule`, evaluate its input predicates against the scope. The rule matches when **all** of them yield `bl.BlBoolean` true; a wildcard (`-`) is the constant-true predicate.
2. **Combine.** Apply the hit policy to the set of matching rules, evaluating their output expressions against the scope; an empty output cell yields `bl.BlNull`.
3. **Project.** Write the combined results into a fresh `O` — a scalar per output field for single-hit policies, a `bl.BlList` per output field for list-returning policies — and return it.
4. If no rule matches:
   - For `Unique`, `First`, `Priority`, `Any`: every output field is `bl.BlNull`.
   - For `Collect`, `RuleOrder`, `OutputOrder`: every output field is an empty list.

### Standalone vs. within a task

`Evaluate` behaves identically however the node is driven; only the source of its inputs differs:

- **Standalone** — the caller builds a typed `I` with value-carrying handles (`bl.NewHandle(...)`) and passes it directly.
- **Within a `DecisionTask`** — the task populates this node's input handles from the producers wired to them at construction (an upstream node output, a task input, or reference data), runs the node through its internal run-thunk, and routes the output handles onward (see [decision-task.spec.md § Evaluation](decision-task.spec.md#evaluation)).

---

## Markdown Rendering

`ToMarkdown()` returns the decision table as a GitHub-flavoured markdown table string.

### Format

- The first row is headers: hit policy indicator in the first cell, then column labels, then output names (the `expr` names of `O`'s fields).
- The hit policy indicator is the standard single-letter abbreviation (`U`, `F`, `C`, …). For `Collect` with an aggregation, the aggregation symbol is appended (`C+`, `C<`, `C>`, `C#`).
- A visual separator column is placed between the last input column and the first output column. The header cell is empty, and every data row contains `█` (Unicode full block).
- Each subsequent row is a rule: rule index, then (if `showRuleIDs=true`) the rule id in a `rule-id` column, then input cells rendered as their unary-test source (`-` for wildcards), then the `█` separator, then output cells rendered as their expression source (empty for omitted outputs).
- When `showInputMappings=true`, a table mapping each column label to its `Expr` is rendered above the decision table.
- When `showRuleDescriptions=true`, a numbered list of the `Descriptions` entries is appended below the table.

### Example

```go
type RiskInputs struct {
    Age   bl.Handle[bl.BlNumber] `expr:"age"`
    Score bl.Handle[bl.BlNumber] `expr:"score"`
}
type RiskOutputs struct {
    Risk   bl.Handle[bl.BlString] `expr:"risk"`
    Reason bl.Handle[bl.BlString] `expr:"reason"`
}

var riskLevel = bl.NewDecisionTable[RiskInputs, RiskOutputs](bl.DecisionTableConfig{
    Id:        "risk",
    Name:      "Risk Level",
    HitPolicy: bl.HitPolicyUnique,
    Columns: []bl.Column{
        {Label: "Age", Expr: `age`, Type: bl.TypeNumber},
        {Label: "Score", Expr: `score`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // id                 Age      Score    risk        reason
        {`young-strong`, `< 25`, `> 700`, `"low"`, `"Young with good record"`},
        {`older-any`, `>= 25`, `-`, `"medium"`, ``},
    },
    Descriptions: map[string]string{
        `young-strong`: `Young applicant with a strong track record`,
        `older-any`:    `Standard risk for older applicants`,
    },
})

fmt.Println(riskLevel.ToMarkdown(false, false, false))
```

Output:

```text
| U | Age   | Score |   | risk     | reason                   |
|---|-------|-------|---|----------|--------------------------|
| 1 | < 25  | > 700 | █ | "low"    | "Young with good record" |
| 2 | >= 25 | -     | █ | "medium" |                          |
```

`[@test] ../../core/decision_table_test.go`

---

## Edge Cases

- A `DecisionTable` with no columns is valid; rows carry only output cells (and an optional id) and all rules match unconditionally.
- A `DecisionTable` whose `O` has no fields is invalid — `NewDecisionTable` raises `DecisionDefinitionError`.
- A `DecisionTable` with no `Rule` entries evaluates each output field to `bl.BlNull` (or an empty list for list-returning policies) without error.
- Rows of inconsistent width, or a width matching neither `len(Columns) + len(O fields)` nor one more (id column), are a `DecisionDefinitionError`.
- An `I`/`O` field that is not a `bl.Handle[BlValue]`, or an unexported field, is a `DecisionDefinitionError`.
- A duplicate variable name within `I` or within `O` is a `DecisionDefinitionError`. Names need not be unique across nodes.
- A non-empty rule id that duplicates another rule's id in the table is a `DecisionDefinitionError`.
- An empty input cell is a `DecisionDefinitionError` — wildcards must be written `-`.
- An input cell whose inlined predicate does not compile — including an ordering/interval form against an unordered column — is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An output cell, or a column expression, that does not compile via `bl.Expr` is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- A column or output expression that references a name not declared in `I` is a `DecisionDefinitionError`.
- A `Descriptions` key with no matching rule id is a `DecisionDefinitionError`.
- An output set by no rule (every rule's cell for it is empty) is a `DecisionDefinitionError` — every field of `O` must be reachable.
- `HitPolicyCollect` with `AggregationSum`, `AggregationMin`, or `AggregationMax` over a non-numeric output is a `DecisionDefinitionError`.
- A list-returning hit policy (`Collect` with no aggregation, `RuleOrder`, `OutputOrder`) whose `O` fields are not all `bl.Handle[bl.BlList]` is a `DecisionDefinitionError`.
- A runtime type mismatch inside an input predicate (the column value versus the form it is tested against) surfaces as a `bl.TypeError` at evaluation time.
