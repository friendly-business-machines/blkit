# Decision Tables

> Express decision logic as a table of rules — rows of input conditions mapped to
> output values — the spreadsheet-style core of DMN.

A **decision table** lays business rules out as a grid: each row is a rule that
matches a combination of input conditions and produces a set of outputs. It is the
form business analysts reach for first, because it reads like a spreadsheet while
staying exact and type-checked.

A `bl.DecisionTable[I, O]` is generic over a typed input struct `I` and output
struct `O`, both built from `bl.Handle` fields. `Evaluate(in I)` returns a typed
`O`.

## A first table

```go
type EligibilityInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Income bl.Handle[bl.BlNumber] `expr:"income"`
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
        {Label: "Income", Expr: `income`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        // Age     Income       eligibility
        {`>= 18`, `>= 30000`, `"eligible"`},
        {`< 18`,  `-`,        `"ineligible"`},
        {`-`,     `< 30000`,  `"ineligible"`},
    },
})

age, _ := bl.Number(30)
income, _ := bl.Number(50000)
out, _ := eligibility.Evaluate(EligibilityInputs{
    Age:    bl.NewHandle(age),
    Income: bl.NewHandle(income),
})
// out.Eligibility.Get() == "eligible"
```

## Columns and cells

A **column** turns an input into a comparable value: its `Expr` is an expression
over the input variables (often just the variable name), and its `Type` bounds the
test forms the column accepts.

Input cells are **unary tests** — compact conditions evaluated against the column
value:

| Form | Example | Matches |
|---|---|---|
| comparison | `>= 18`, `< 650` | ordered values |
| interval | `[650..749]`, `(5..20]` | a range |
| list | `"US", "CA"` | any of several values |
| equality | `"VIP"` | exactly that value |
| wildcard | `-` | anything (including null) |
| expression | `count(?) > 0` | the cell's own test, `?` = the column value |

Output cells are ordinary [expressions](../expressions/overview.md): `3.5`,
`"eligible"`, `"approved by " + reviewer`. An empty output cell yields null.

Every cell is compiled when the table is built, so a malformed test or a reference
to an unknown input fails fast at program start — never at evaluation.

## Hit policies

When more than one rule matches, the **hit policy** decides what happens:

| Policy | Result |
|---|---|
| `Unique` | exactly one rule may match (more than one is an error) |
| `First` | rules in order; the first match wins |
| `Priority` | the highest-priority match wins |
| `Any` | several may match, but they must agree |
| `Collect` | all matches, combined by an `Aggregation` (`Sum`/`Min`/`Max`/`Count`) or gathered into a list |
| `RuleOrder` / `OutputOrder` | all matches, as a list |

`Collect` with an aggregation reduces the matches to one value:

```go
sum := bl.AggregationSum
penalties := bl.NewDecisionTable[PenaltyInputs, PenaltyOutputs](bl.DecisionTableConfig{
    Id:          "penalties",
    HitPolicy:   bl.HitPolicyCollect,
    Aggregation: &sum,
    Columns: []bl.Column{
        {Label: "Speed", Expr: `speed`, Type: bl.TypeNumber},
        {Label: "Zone", Expr: `zone_type`, Type: bl.TypeString},
    },
    Rules: bl.Rules{
        {`> 30`, `"school"`, `200`},
        {`> 50`, `-`,        `150`},
        {`-`,    `"school"`, `50`},
    },
})
// speed 55 in a school zone matches all three → fine = 400
```

When a list-returning policy is used, every output must be a `bl.Handle[bl.BlList]`.

## Rule ids and descriptions

A rule may carry an optional leading **id** cell (all rows, or none) so you can
attach a human-readable description in `Descriptions`, keyed by id:

```go
Rules: bl.Rules{
    {`prime`,    `>= 750`,     `<= 500000`, `3.5`, `360`},
    {`standard`, `[650..749]`, `-`,         `5.0`, `240`},
},
```

## Rendering

`ToMarkdown(showRuleIDs, showRuleDescriptions, showInputMappings)` renders the
table as a GitHub-flavoured markdown table — handy for documenting or reviewing a
decision:

```text
| U | Age   | Income   |   | eligibility  |
|---|-------|----------|---|--------------|
| 1 | >= 18 | >= 30000 | █ | "eligible"   |
| 2 | < 18  | -        | █ | "ineligible" |
| 3 | -     | < 30000  | █ | "ineligible" |
```

## Inside a task

A table rarely stands alone. Inside a [decision task](decision-tasks.md), you
**wire** its inputs from upstream producers and its outputs onward, by connecting
handles — `bl.Edge(someProducer.Out.X, eligibility.In.Age)`. See
[Decision tasks](decision-tasks.md).
