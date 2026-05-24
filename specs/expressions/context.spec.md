---
name: BlContext
description: The context type in the blkit expression language — an ordered key-value map. Covers context literals, path access, the context built-in library (incl. blkit extensions), and the Go layer (BlContext + expr registrations).
targets:
  - ../../expr/context.go
---

# BlContext — the `context` type

`context` is an ordered map from string keys to values. The Go value type backing it is `BlContext`.
It is a pure value used within expressions — distinct from `ExecutionContext` (the mutable process
variable store).

See [bl-expr.spec.md](bl-expr.spec.md) for context literals and path access, documented there as
language constructs; this spoke covers the function library and the Go layer.

---

## Literals & path access

A **context literal** is the syntactic form used inside a blkit expression to write a constant
context value — for example, the `{name: "Alice", age: 30}` in
`greet({name: "Alice", age: 30})`. Literals are delimited by braces with comma-separated
`key: value` entries.

```
{}                                 // empty
{name: "Alice", age: 30}           // unquoted keys
{"my key": 1}                      // quoted keys (special characters)
{a: 1, b: {c: 2}}                  // nested
{a: 2, b: a * 2}                   // later entries can reference earlier ones → {a: 2, b: 4}

{a: {b: 3}}.a.b                    // → 3        (path access)
{a: 1}.missing                     // → null     (missing key)
applicant["my key"]                // bracket access for special-character keys
```

Keys are non-empty, case-sensitive strings; insertion order is preserved.

`[@test] ../../expr/context_test.go`

---

## Built-in functions

Standard DMN functions plus blkit extensions (**ext**).

| Function | Example | Result |
|---|---|---|
| `getValue(c, key)` | `getValue({foo: 123}, "foo")` | `123` |
| `getValue(c, keys)` | `getValue({x:1, y:{z:0}}, ["y","z"])` | `0` (nested path) |
| `getEntries(c)` | `getEntries({foo: 123})` | `[{key: "foo", value: 123}]` |
| `contextPut(c, key, value)` | `contextPut({x:1}, "y", 2)` | `{x:1, y:2}` |
| `contextPut(c, keys, value)` | `contextPut({x:1, y:{z:0}}, ["y","z"], 2)` | `{x:1, y:{z:2}}` |
| `contextMerge(contexts)` | `contextMerge([{x:1},{y:2}])` | `{x:1, y:2}` (later wins) |
| `keys(c)` **ext** | `keys({a:1, b:2})` | `["a", "b"]` (insertion order) |
| `values(c)` **ext** | `values({a:1, b:2})` | `[1, 2]` |
| `has(c, key)` **ext** | `has({a:1}, "a")` | `true` (or `isDefined(c.a)`) |
| `size(c)` **ext** | `size({a:1, b:2})` | `2` |
| `isEmpty(c)` **ext** | `isEmpty({})` | `true` |
| `contextRemove(c, key)` **ext** | `contextRemove({a:1, b:2}, "a")` | `{b: 2}` |

`[@test] ../../expr/context_functions_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `.` / `[ ]` | member access | `c.name`, `c["my key"]` | the value, or `null` |
| `=` `!=` | equality (order-insensitive) | `{a:1,b:2} = {b:2,a:1}` | `true` |

`[@test] ../../expr/context_operators_test.go`

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Context(...)` | `{ … }` literal |
| `size` / `isEmpty` | `size(c)` **ext** / `isEmpty(c)` **ext** |
| `get(key)` | `c.key` / `c["key"]` / `getValue(c, key)` |
| `has(key)` | `has(c, key)` **ext** / `isDefined(c.key)` |
| `keys` / `values` | `keys(c)` **ext** / `values(c)` **ext** |
| `getEntries` | `getEntries(c)` |
| `put` / `putAll` / `merge` | `contextPut(c, key, value)` / `contextMerge([...])` |
| `remove` | `contextRemove(c, key)` **ext** |
| `equals` / `notEqual` | `=` / `!=` (order-insensitive) |
| `toRecord` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/context.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlContext struct{ keys []string; m map[string]BlValue } // insertion-ordered

func (BlContext) Type() BlType { return BlTypeContext }
func (c BlContext) Equal(other BlValue) BlValue // order-insensitive
func (c BlContext) ToMarkdown() string          // '{name: "Alice", age: 30}'
func (BlContext) isBlValue() {}

func Context(entries map[string]BlValue) BlContext // host constructor
func (c BlContext) ToRecord() map[string]BlValue
func (c BlContext) String() string
```

### Registrations (`contextOptions`, unexported)

```go
func contextOptions() []expr.Option {
    return []expr.Option{
        expr.Function("getValue", getValueFn, new(func(BlContext, BlString) BlValue), new(func(BlContext, BlList) BlValue)),
        expr.Function("getEntries", typed1(getEntriesFn), new(func(BlContext) BlList)),
        expr.Function("contextPut", contextPutFn, new(func(BlContext, BlString, BlValue) BlContext), new(func(BlContext, BlList, BlValue) BlContext)),
        expr.Function("contextMerge", typed1(contextMergeFn), new(func(BlList) BlContext)), // list of contexts
        // ext
        expr.Function("keys",    typed1(keysFn),    new(func(BlContext) BlList)),
        expr.Function("values",  typed1(valuesFn),  new(func(BlContext) BlList)),
        expr.Function("has",     typed2(hasFn),     new(func(BlContext, BlString) BlBoolean)),
        expr.Function("size",    typed1(ctxSizeFn), new(func(BlContext) BlNumber)),
        expr.Function("isEmpty", typed1(ctxIsEmptyFn), new(func(BlContext) BlBoolean)), // context overload
        expr.Function("contextRemove", typed2(contextRemoveFn), new(func(BlContext, BlString) BlContext)),
    }
}
```

**Operators.** Member access (`.`/`[]`) is lowered by the patcher to `getValue(ctx, "key")`,
returning `Null` for a missing key; `=`/`!=` are order-insensitive. Context literals with
forward-referencing entries (`{a: 2, b: a*2}`) compile to entries evaluated in order. Native Go maps
wrap to `BlContext`; the engine also models the evaluation scope as a `BlContext`.

`[@test] ../../expr/context_test.go`

---

## Edge cases

- Missing key (`.`/`[]`/`getValue`) → `null`.
- `contextPut` with an empty-string key → `BlTypeError`.
- `contextRemove` of an absent key → the context unchanged.
- `contextMerge` / `contextPut` with an empty context → no-op.
- Special-character keys require quoted/bracket syntax (`{"my key": 1}`, `c["my key"]`).
