---
name: BlDictionary
description: The dictionary type in the blkit expression language — an ordered key-value map. Covers dictionary literals, path access, the dictionary built-in library (incl. blkit extensions), and the Go layer (BlDictionary + expr registrations).
targets:
  - ../../expr/dictionary.go
---

# BlDictionary — the `dictionary` type

`dictionary` is an ordered map from string keys to values. The Go value type backing it is
`BlDictionary`. It is a pure value used within expressions — distinct from `ExecutionContext`
(the mutable process variable store, see [execution-context.spec.md](../data/execution-context.spec.md)).

See [bl-expr.spec.md](bl-expr.spec.md) for dictionary literals and path access, documented
there as language constructs; this spoke covers the function library and the Go layer.

---

## Literals & path access

A **dictionary literal** is the syntactic form used inside a blkit expression to write a
constant dictionary value — for example, the `{name: "Alice", age: 30}` in
`{name: "Alice", age: 30}.name` (which extracts `"Alice"`). Literals are delimited by braces
with comma-separated `key: value` entries.

```
// expression-language
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

`[@test] ../../expr/dictionary_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `.` / `[ ]` | member access (path / bracket lookup) | `d.name`, `d["my key"]` | the value, or `null` for missing |
| `=` `!=` | equality (order-insensitive structural) | `{a:1,b:2} = {b:2,a:1}` | `true` (same key/value pairs regardless of insertion order) |

Dictionaries have no arithmetic operators (`+`/`-`/etc.), no ordering operators
(`<`/`<=`/`>`/`>=`), and no `in` operator — dictionary membership uses `has(d, key)` or
`isDefined(d.key)` (see [§ Built-in functions](#built-in-functions)).

Member access is **patcher-lowered** to a call to `getValue(d, key)`. So `d.name` and
`d["name"]` are both equivalent to `getValue(d, "name")`.

`[@test] ../../expr/dictionary_operators_test.go`

---

## Built-in functions

DMN-inspired functions plus blkit extensions (**ext**).

| Function | Example | Result |
|---|---|---|
| `getValue(d, key)` | `getValue({foo: 123}, "foo")` | `123` |
| `getValue(d, keys)` | `getValue({x:1, y:{z:0}}, ["y","z"])` | `0` (nested path) |
| `getEntries(d)` | `getEntries({foo: 123})` | `[{key: "foo", value: 123}]` |
| `dictionaryPut(d, key, value)` | `dictionaryPut({x:1}, "y", 2)` | `{x:1, y:2}` |
| `dictionaryPut(d, keys, value)` | `dictionaryPut({x:1, y:{z:0}}, ["y","z"], 2)` | `{x:1, y:{z:2}}` |
| `dictionaryMerge(dicts)` | `dictionaryMerge([{x:1},{y:2}])` | `{x:1, y:2}` (later wins) |
| `keys(d)` **ext** | `keys({a:1, b:2})` | `["a", "b"]` (insertion order) |
| `values(d)` **ext** | `values({a:1, b:2})` | `[1, 2]` |
| `has(d, key)` **ext** | `has({a:1}, "a")` | `true` (or `isDefined(d.a)`) |
| `size(d)` **ext** | `size({a:1, b:2})` | `2` |
| `isEmpty(d)` **ext** | `isEmpty({})` | `true` |
| `dictionaryRemove(d, key)` **ext** | `dictionaryRemove({a:1, b:2}, "a")` | `{b: 2}` |

`[@test] ../../expr/dictionary_functions_test.go`

---

## Go implementation (expr extension)

Lives in `expr/dictionary.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlDictionary` is the immutable Go value type that represents a dictionary inside the engine and at
the host-code boundary. It maintains **insertion order** for keys via a parallel `keys` slice
alongside the underlying `map[string]BlValue`. Both fields are private so callers cannot
mutate the underlying value — every operation in the library returns a fresh `BlDictionary`.

The exported surface has four parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` is **order-insensitive structural** — `{a:1, b:2}` equals `{b:2, a:1}`
  because they hold the same key/value pairs regardless of insertion order. `String()` doubles
  as the `fmt.Stringer` implementation, producing the canonical literal form (e.g.
  `'{name: "Alice", age: 30}'`).
- **`Dictionary(entries)`** — the host constructor, accepting a Go `map[string]BlValue`. Because
  Go maps don't carry insertion order, the resulting `BlDictionary`'s key order is unspecified
  when constructed from a map. Callers needing a specific order should use the expression
  literal syntax `{a: 1, b: 2}` (which preserves the source order) or construct via the
  expression engine.
- **`Native()` accessor** — returns a defensive copy of the underlying `map[string]BlValue`.
  Callers may mutate the returned map without affecting the `BlDictionary`. Key insertion order
  is NOT preserved through this accessor (Go maps don't carry it); use `Keys()` separately if
  you need the ordered key list.
- **`Keys()` accessor** — returns the insertion-ordered list of keys, so callers can iterate
  the dictionary in order via `for _, k := range d.Keys() { v := d.Native()[k]; ... }`.

```go
// host-side (Go)
type BlDictionary struct {
    keys []string             // insertion-ordered key list
    m    map[string]BlValue   // value lookup
}

// BlValue interface — required by all Bl* value types.
func (BlDictionary) Type() BlType { return BlTypeDictionary }
func (d BlDictionary) Equal(other BlValue) BlValue   // order-insensitive structural
func (d BlDictionary) String() string                // canonical literal form
func (BlDictionary) isBlValue() {}

// Host constructor.
func Dictionary(entries map[string]BlValue) BlDictionary

// Host accessors (consume an evaluated result).
func (d BlDictionary) Native() map[string]BlValue    // defensive copy; key order NOT preserved through Go's map
func (d BlDictionary) Keys() []string                // insertion-ordered key list
```

### Backing implementations (unexported, suffix `Fn`)

Dictionary has **no per-type operator implementation functions**. Equality (`=` / `!=`)
dispatches through the `BlValue.Equal()` interface method (see [§ Value type & host
API](#value-type--host-api-exported)), and member access (`.field`, `[key]`) is patcher-lowered
to a call to `getValue(d, key)`. Dictionary has no arithmetic operators (`+`/`-`/etc.), no
ordering operators (`<`/`<=`/`>`/`>=`), and no `in` operator.

The library functions are implemented as these typed/variadic Go functions, wrapped by
`typed1`/`typed2`/`typed3` at registration time:

```go
// host-side (Go)
// Typed implementations — wrapped by typed1/typed2 at registration.
func getEntriesFn(d BlDictionary) BlList                            // → list of {key, value} dictionaries
func dictionaryMergeFn(l BlList) BlDictionary                       // accepts a BlList of BlDictionary; later dictionaries win on key collision
func keysFn(d BlDictionary) BlList                                  // ext; insertion-ordered
func valuesFn(d BlDictionary) BlList                                // ext; insertion-ordered
func hasFn(d BlDictionary, key BlString) BlBoolean                  // ext
func dictSizeFn(d BlDictionary) BlNumber                            // ext; number of entries
func dictIsEmptyFn(d BlDictionary) BlBoolean                        // ext; dictionary overload (string/list/range overloads in their own specs)
func dictionaryRemoveFn(d BlDictionary, key BlString) BlDictionary  // ext; absent key → unchanged

// Variadic implementations — handle multiple input shapes in expr's raw shape.
func getValueFn(args ...any) (any, error)                           // (d, BlString) | (d, BlList) — path access via key string or list of keys
func dictionaryPutFn(args ...any) (any, error)                      // (d, BlString, BlValue) | (d, BlList, BlValue) — single-key or nested-path put
```

Member-access patcher dispatch is documented in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).
The engine also models the evaluation scope as a `BlDictionary`. Dictionary literals with
forward-referencing entries (`{a: 2, b: a*2}`) compile to entries evaluated in order, where
each entry adds its key to the scope before the next entry evaluates. Native Go
`map[string]any` inputs wrap to `BlDictionary` via the engine's input bridge.

### Registrations (`dictionaryOptions`, unexported)

`dictionaryOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the dictionary library. Each entry is built with
`expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that the patcher
  emits when lowering member-access syntax).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2`
  adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlDictionary) BlNumber` into that shape; the
  variadic impls are registered directly because their multi-shape dispatch can't be expressed
  as a fixed-arity adapter.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures (e.g.
  `getValue` accepts both `(d, "key")` and `(d, ["nested", "path"])` forms).

The registrations are grouped by role: the core dictionary operations and ext additions.

```go
// host-side (Go)
func dictionaryOptions() []expr.Option {
    return []expr.Option{
        // core dictionary operations
        expr.Function("getValue",    getValueFn,
            new(func(BlDictionary, BlString) BlValue),
            new(func(BlDictionary, BlList) BlValue)),
        expr.Function("getEntries",  typed1(getEntriesFn),  new(func(BlDictionary) BlList)),
        expr.Function("dictionaryPut",  dictionaryPutFn,
            new(func(BlDictionary, BlString, BlValue) BlDictionary),
            new(func(BlDictionary, BlList, BlValue) BlDictionary)),
        expr.Function("dictionaryMerge", typed1(dictionaryMergeFn), new(func(BlList) BlDictionary)),  // list of dictionaries; later wins

        // ext
        expr.Function("keys",          typed1(keysFn),          new(func(BlDictionary) BlList)),
        expr.Function("values",        typed1(valuesFn),        new(func(BlDictionary) BlList)),
        expr.Function("has",           typed2(hasFn),           new(func(BlDictionary, BlString) BlBoolean)),
        expr.Function("size",          typed1(dictSizeFn),       new(func(BlDictionary) BlNumber)),
        expr.Function("isEmpty",       typed1(dictIsEmptyFn),    new(func(BlDictionary) BlBoolean)),       // dictionary overload (other type overloads in their own specs)
        expr.Function("dictionaryRemove", typed2(dictionaryRemoveFn), new(func(BlDictionary, BlString) BlDictionary)),
    }
}
```

`[@test] ../../expr/dictionary_test.go`

---

## Edge cases

- Missing key (`.`/`[]`/`getValue`) → `null`.
- `dictionaryPut` with an empty-string key → `BlTypeError`.
- `dictionaryRemove` of an absent key → the dictionary unchanged.
- `dictionaryMerge` / `dictionaryPut` with an empty dictionary → no-op.
- Special-character keys require quoted/bracket syntax (`{"my key": 1}`, `d["my key"]`).
