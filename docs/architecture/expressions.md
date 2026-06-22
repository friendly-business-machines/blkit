# Expressions

> How blkit's expression engine turns a source string into type-checked,
> compiled bytecode that evaluates to a typed value — the pipeline, the value
> system, and the line between blkit and the host VM.

Expressions are the foundation blkit is built on. A decision rule, a unary test,
the condition that routes a process — all of them are blkit expressions. This
page explains how the engine compiles and evaluates them.

## What an expression is

A blkit expression is a string in blkit's expression language — a typed,
FEEL-flavoured language for business logic:

```text
age >= 18 and income > 25000
```

You compile that string **once** against a description of the variables it may
reference, and then **evaluate** it many times against different inputs. Every
expression produces a `BlValue` — a value in blkit's own type system — never a
raw Go `int`, `string`, or `bool`.

```go
import bl "github.com/friendly-business-machines/blkit/core"

// The variables the source may reference are the exported fields of an env
// struct; `expr:"name"` tags give them their source-level names.
type applicant struct {
    Age    bl.BlNumber `expr:"age"`
    Income bl.BlNumber `expr:"income"`
}

// Compile once. An undeclared name (or a syntax error) fails here, not at
// evaluation time.
var eligible, err = bl.Expr[applicant](`age >= 18 and income > 25000`)

// Evaluate many times. The Go compiler rejects any env that is not `applicant`.
var result, _ = eligible.Evaluate(applicant{Age: /* 20 */, Income: /* 30000 */})
```

## The compilation pipeline

Compilation is a pipeline of five stages. Two of them are blkit's own; three are
provided by the [`expr-lang/expr`](https://github.com/expr-lang/expr) library
that blkit builds on (see [Where blkit ends and expr-lang/expr
begins](#where-blkit-ends-and-expr-langexpr-begins)).

```text
 source string
      │
      ▼
 ┌──────────────┐
 │ normalise    │  blkit      — source-level rewrites
 └──────────────┘
      │  expr-compatible source
      ▼
 ┌──────────────┐
 │ parse        │  expr-lang  — recursive-descent parser → AST
 └──────────────┘
      │  AST
      ▼
 ┌──────────────┐
 │ patch        │  blkit      — feelPatcher lowers the AST (post-order)
 └──────────────┘
      │  lowered AST
      ▼
 ┌──────────────┐
 │ compile      │  expr-lang  — type-check + emit bytecode
 └──────────────┘
      │  *vm.Program
      ▼
 ┌──────────────┐
 │ run          │  expr-lang  — stack VM executes against env → BlValue
 └──────────────┘
```

The whole pipeline lives behind one function, `compileWithEnv`, which backs
every public constructor:

```go
func compileWithEnv(source string, env any, declared map[string]bool, extra ...expr.Option) (*vm.Program, error) {
    src, err := normalise(source)            // stage 1
    if err != nil {
        return nil, err
    }
    if name, bad := firstUndefined(src, declared); bad { // undeclared-name check
        return nil, fmt.Errorf("unknown name %s", name)
    }
    opts := buildOptionsEnv(env)             // env + patcher + registrations
    opts = append(opts, extra...)
    return expr.Compile(src, opts...)        // stages 2–4
}
```

### 1. Normalisation

`expr-lang/expr` has a fixed lexer and parser; it cannot be taught new syntax.
blkit's expression language has constructs that lexer would reject or
misinterpret — FEEL's single-`=` equality, `if/then/else`, ranges, `between`,
comprehensions, exact decimal literals. **Normalisation** rewrites these into
expr-compatible source *as text*, before a parser ever sees it.

`normalise` is a fixed sequence of small, independent string rewrites, each
handling one construct:

```go
func normalise(source string) (string, error) {
    s := eqNorm(source)            // FEEL `=` → `==`
    s = lowerNamedArgs(s)          // substring(string: x, ...) → positional
    s = lowerInlinePredicates(s)
    s = lowerInlineFunctions(s)
    s = lowerSequence(s)
    s = lowerTableIndex(s)
    s = lowerInstanceOf(s)
    s = lowerIsDefined(s)
    s = rewriteIdentifiers(s)
    s = lowerRanges(s)             // [a..b] → newRange(...)
    s = lowerBetween(s)            // x between a and b → x >= a and x <= b
    s = lowerComprehensions(s)     // for / some / every → map / filter
    s = convertConditionals(s)     // if C then A else B → expr block form
    s = captureDecimals(s)         // exact decimal-literal capture
    return s, nil
}
```

The output is still recognisably the same expression — it is just written in the
subset of syntax `expr-lang/expr` can parse.

### 2. Parsing

The normalised source is handed to `expr.Compile`, which parses it with
`expr-lang/expr`'s recursive-descent parser into an AST. blkit does not own a
lexer or parser; operator precedence, associativity, and the grammar are the
library's. (blkit does call the parser directly in one place — to walk the AST
for the undeclared-name check, see below.)

### 3. Patching (lowering)

This is where blkit imposes its **semantics**. A parsed `1 + 2` means whatever
`expr-lang/expr` decides `+` means — integer addition on Go values. blkit needs
`+` to mean *blkit number addition* with decimal precision and three-valued null
handling, operating on `BlValue`s. The **patcher** rewrites the AST to make that
so.

`feelPatcher` walks the tree **post-order** — every operand is already lowered by
the time its parent operator is visited — and replaces nodes with calls into
blkit's runtime:

```go
func (p *feelPatcher) Visit(node *ast.Node) {
    switch n := (*node).(type) {
    case *ast.IntegerNode:
        ast.Patch(node, constNode(BlNumber{decimal.NewFromInt(int64(n.Value))}))
    case *ast.StringNode:
        ast.Patch(node, constNode(BlString{n.Value}))
    case *ast.BinaryNode:
        p.patchBinary(node, n)   // + → __add(l, r), < → __lt(l, r), ...
    case *ast.ArrayNode:
        ast.Patch(node, call("__mklist", n.Nodes...)) // [a,b] → BlList, not []any
    // ... unary ops, conditionals, member access, table operations
    }
}
```

The lowering does several distinct jobs:

- **Literals → typed constants.** `42` becomes a `BlNumber` backed by a
  `shopspring/decimal`; `"hi"` a `BlString`; `true` a `BlBoolean`; `null` a
  `BlNull`.
- **Operators → dispatch calls.** Every arithmetic and comparison operator maps
  to a named runtime function: `+ → __add`, `- → __sub`, `< → __lt`, `== → __eq`,
  and so on. Each is registered (in `baseOptions`) with a single signature over
  `BlValue`, so type behaviour lives in one place per operator.
- **Short-circuit `and`/`or`** are lowered into let-bound guards that implement
  three-valued (true/false/null) logic, rather than Go's two-valued `&&`/`||`.
- **`if/then/else`** conditions are wrapped in `__truthy` so a `BlValue`
  condition reduces to a Go `bool` for expr's native conditional, with null and
  non-boolean conditions treated as falsy.
- **Composite literals and table operations** (`[...]`, `{...}`, filters,
  `groupBy`, `withColumn`) are lowered to the calls that build the corresponding
  `BlList`, `BlDictionary`, and `BlTable` values.

The patcher is registered as a compile option, `expr.Patch(newFeelPatcher())`,
alongside the per-type and operator registrations in `baseOptions`.

### 4. Compilation to bytecode

With the AST lowered, `expr.Compile` type-checks it against the env and emits a
`*vm.Program` — `expr-lang/expr`'s compiled bytecode form. blkit treats the
program as an opaque, reusable unit; it stores it and never inspects the
instructions.

Before compiling, blkit applies its own discipline on top of expr's type
checker: `firstUndefined` parses the normalised source, collects every free
identifier, and rejects the first one that is not a declared variable. This is
what turns a typo'd variable name into a **compile-time** error.

The compiled program is wrapped in the public handle:

```go
type BlExpr[E any] struct {
    source  string      // original text, for Source()
    program *vm.Program // compiled bytecode
}
```

### 5. Evaluation

`Evaluate` runs the stored program against an env value on `expr-lang/expr`'s
stack-based VM, then wraps the result back into blkit's type system with `asBl`:

```go
func (e *BlExpr[E]) Evaluate(env E) (BlValue, error) {
    out, err := expr.Run(e.program, env)
    if err != nil {
        return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
    }
    return asBl(out), nil
}
```

Because the program is compiled once and the VM is stateless between runs, the
same `BlExpr` can be evaluated concurrently against different inputs — the
expensive work (normalise, parse, patch, compile) happens exactly once.

## The value system

Every value the engine produces or consumes implements the `BlValue` interface.
This is the type system the patcher lowers literals into and the operator
dispatch functions operate over:

```go
type BlValue interface {
    Type() Type                  // language type tag (number, string, ...)
    Equal(other BlValue) BlValue // three-valued equality
    String() string              // canonical literal rendering
    IsNull() bool
    isBlValue()                  // sealed: only blkit can implement it
}
```

Implementations include `BlNumber` (decimal-backed, not float), `BlString`,
`BlBoolean`, the temporal types (`BlDate`, `BlTime`, `BlDateTime`, and the two
duration types), and the collection types (`BlList`, `BlDictionary`, `BlRange`,
`BlTable`). Numbers use `shopspring/decimal` so business arithmetic is exact, and
the interface is **sealed** (`isBlValue`) so the set of language types is closed
and exhaustive `switch`es over them are safe.

## Where blkit ends and expr-lang/expr begins

blkit does **not** ship its own parser, bytecode format, or virtual machine. It
builds on the [`expr-lang/expr`](https://github.com/expr-lang/expr) library and
contributes the layers that make the language *blkit's*:

| Concern | Owner |
|---|---|
| Lexer, parser, grammar, operator precedence | `expr-lang/expr` |
| Bytecode format (`*vm.Program`) and the VM that runs it | `expr-lang/expr` |
| Type-checking and compilation driver (`expr.Compile`) | `expr-lang/expr` |
| **Source normalisation** (the blkit→expr syntax bridge) | **blkit** |
| **AST patching / lowering** (blkit semantics on expr syntax) | **blkit** |
| **The value system** (`BlValue` and its implementations) | **blkit** |
| **Operator and function semantics** (`__add`, decimals, 3-valued logic) | **blkit** |
| **The typed, generic public API** (`Expr[E]`, env-struct binding) | **blkit** |

The short version: `expr-lang/expr` provides a fast, embeddable evaluation
*substrate*; blkit provides the *language* — its syntax, its types, and its
semantics — by rewriting source on the way in and lowering the AST before it is
compiled.

## Typed environments

The variables an expression may use are not a loose `map[string]any` — they are
the exported fields of a Go struct, the type parameter `E` in `Expr[E]`. This
makes the env part of the compile-time contract:

- An `expr:"name"` tag sets the source-level name of a field; an untagged field
  uses its Go name; `expr:"-"` hides it.
- Passing the wrong struct type to `Evaluate` is a **Go compile error**, not a
  runtime surprise.
- A source reference to a name no field declares is a blkit **compile error**
  (the `firstUndefined` check), surfaced when you call `Expr`.

For variable-free expressions (`1 + 1`, `date("2025-01-01")`) there is `NoEnv`
(an alias for `struct{}`) and the `ExprNoEnv` shorthand.

## Beyond single expressions

The same pipeline backs blkit's higher-level constructs — they differ only in how
their env is built and how their result is shaped, then reuse `compileWithEnv`:

| Construct | Adds |
|---|---|
| **`UnaryTest[T]`** | A single-value predicate. A bare `> 5` or `[1..10]` is rewritten against an implicit input placeholder, so the test reads as a condition on one value. |
| **`DecisionExpression[I, O]`** | Many named sub-expressions with a typed input `I` and output `O`. Entries are compiled, their inter-dependencies found by walking each AST, and evaluated in topological order (self-references and cycles are rejected). |
| **`Func` / `BlUDF[P, R]`** | A named, host-defined function callable by name from other expressions, with compile-time-checked arguments. |

Each of these is, underneath, one or more `BlExpr`-style compiled programs run on
the same VM over the same value system.

## Source map

| Concept | File |
|---|---|
| Public API, pipeline driver, `BlExpr[E]` | `core/engine.go` |
| Source normalisation (the 14 rewrites) | `core/normalise.go` |
| Undeclared-name check | `core/check.go` |
| AST patching / lowering | `core/patch.go` |
| Operator dispatch functions (`__add`, …) | `core/operators.go` |
| The value system (`BlValue` and implementations) | `core/value.go` |
| Unary tests | `core/unarytest.go` |
| Decision expressions | `core/decision_expression.go` |
| User-defined functions | `core/udf.go` |
| Comprehension lowering | `core/comprehensions.go` |

For the public API itself, see the [Reference](../reference/blkit.md) section.
