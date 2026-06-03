---
name: BlList
description: The list type in the blkit expression language — an ordered, immutable, heterogeneous collection. Covers list literals, indexing/filter/projection, the list built-in library (incl. blkit extensions), and the Go layer (BlList + expr registrations).
targets:
  - ../../expr/list.go
---

# BlList — the `list` type

`list` is an ordered, immutable, heterogeneous collection. The Go value type backing it is `BlList`.
Indexing is **1-based**; negative indexes count from the end.

See [bl-expr.spec.md](bl-expr.spec.md) for list expressions (indexing, filter, projection, `for`,
`some`/`every`), which the hub documents as language constructs; this spoke covers literals, the
function library, and the Go layer.

---

## Literals, indexing, filter, projection

A **list literal** is the syntactic form used inside a blkit expression to write a constant list
value — for example, the `[1, 2, 3, 4]` in `count([1, 2, 3, 4])`. Literals are delimited by square
brackets with comma-separated elements; elements may be any expression (and may mix types).

```
// expression-language
[1, 2, 3, 4]                          // literal
[1, 2, 3, 4][1]                       // → 1     (1-based)
[1, 2, 3, 4][-1]                      // → 4     (from end; out of range → null)
[1, 2, 3, 4][item > 2]                // → [3, 4]   (filter — see note on `item` below)
[{name:"A"},{name:"B"}].name          // → ["A", "B"]   (projection)
for x in [1,2,3] return x * 2                          // → [2, 4, 6]
for x in [1,2], y in [3,4] return x * y                // → [3, 4, 6, 8]   (each x paired with each y)
some x in [1,2,3] satisfies x > 2                      // → true   (some element satisfies)
some x in [1,2], y in [3,4] satisfies x + y > 5        // → true   (some pair satisfies — here 2+4)
every x in [1,2,3] satisfies x > 0                     // → true   (every element satisfies)
every x in [1,2], y in [3,4] satisfies x + y >= 4      // → true   (every pair satisfies)
```

### The magic `item` variable in filter predicates

In the filter form `list[predicate]`, the variable name **`item`** is **magic** — the engine
implicitly binds it to the current element being tested, with no need for the caller to declare
or name it. So `[1, 2, 3, 4][item > 2]` reads as "for each element, bind it to `item`, evaluate
`item > 2`, keep the elements where it's true".

The other iteration forms (`for x in …`, `some x in …`, `every x in …`) use **explicit
binding** — the caller picks the variable name (`x` in those examples). `item` is *not* magic
inside `for`/`some`/`every` predicates; only inside the `[predicate]` filter shorthand.

If you need a variable name other than `item` in a filter — for example to avoid an `item`
collision when filtering over a list of lists — use the `for` form with a return that calls
`only`/`first` to extract the filtered subset.

### List projection (`.fieldName`)

When you apply field access (`.fieldName`) to a **list of contexts**, the engine projects the
field across every element and returns a list of the corresponding values:

```
// expression-language
[{name:"A"}, {name:"B"}].name         // → ["A", "B"]
orders.amount                         // → list of amounts, one per order
```

This is shorthand for `for x in list return x.fieldName`. The two forms are exactly equivalent
— projection is sugar that makes column-like access readable without writing an explicit loop.

Behaviour by input shape:

| Input | `.fieldName` returns |
|---|---|
| Single context with the field | the field's value (normal field access) |
| Single context without the field | `null` |
| List of contexts, every element has the field | list of values |
| List of contexts, some elements missing the field | list with `null` for missing fields, e.g. `[{name:"A"},{age:30}].name` → `["A", null]` |
| List containing non-context elements | `BlTypeError` (the projection is only defined for contexts) |
| Empty list | `[]` |

Projection composes naturally with the aggregates and the rest of the list library. Assume
the following data is in scope (`orders` and `customers` are each a `BlList` of `BlDictionary`,
equivalently a `BlTable`):

```
// expression-language
orders = [
    {id: 1, amount: 100, quantity: 2, region: "NA"},
    {id: 2, amount: 150, quantity: 1, region: "EU"},
    {id: 3, amount: 75,  quantity: 3, region: "NA"}
]
customers = [
    {id: 1, name: "Alice", active: true},
    {id: 2, name: "Bob",   active: false},
    {id: 3, name: "Carol", active: true}
]
```

Then:

```
// expression-language
sum(orders.amount)                                       // → 325       (total revenue)
count(customers[item.active = true])                     // → 2         (active customer count — filter, then count)
distinct(orders.region)                                  // → ["NA","EU"]   (unique regions)
sum(for o in orders return o.amount * o.quantity)        // → 575       (per-row arithmetic first, then sum)
```

`[@test] ../../expr/list_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `=` `!=` | element-wise, order-sensitive equality | `[1,2] = [1,2]` | `true` |
| `in` | element membership of a value | `2 in [1,2,3]` | `true` (lowered to `listContains(l, x)`) |

Lists have no arithmetic operators (`+`/`-`/etc.) and no ordering operators (`<`/`<=`/etc.) —
"which list is less" has no meaningful definition. The indexing, filter, and projection forms
(`l[i]`, `l[predicate]`, `l.field`) are documented above under [§ Literals, indexing, filter,
projection](#literals-indexing-filter-projection) — they're lowered by the engine per the hub
(see [bl-expr.spec.md](bl-expr.spec.md)).

`[@test] ../../expr/list_operators_test.go`

---

## Built-in functions

DMN-inspired functions plus blkit extensions (**ext**). Positions are 1-based.

| Function | Example | Result |
|---|---|---|
| `count(l)` | `count([1,2,3])` | `3` |
| `isEmpty(l)` | `isEmpty([])` | `true` |
| `listContains(l, e)` | `listContains([1,2,3], 2)` | `true` |
| `indexOf(l, match)` | `indexOf([1,2,3,2], 2)` | `[2, 4]` (all positions) |
| `sublist(l, start[, length])` | `sublist([1,2,3], 2)` | `[2, 3]` (negative `start` from end) |
| `append(l, items…)` | `append([1], 2, 3)` | `[1, 2, 3]` |
| `prepend(l, items…)` **ext** | `prepend([2,3], 1)` | `[1, 2, 3]` |
| `concatenate(lists…)` | `concatenate([1,2],[3])` | `[1, 2, 3]` |
| `union(lists…)` | `union([1,2],[2,3])` | `[1, 2, 3]` (dedup) |
| `insertBefore(l, position, item)` | `insertBefore([1,3], 1, 2)` | `[2, 1, 3]` (single item) |
| `insertBefore(l, position, items)` **ext** | `insertBefore([1,5], 2, [2,3,4])` | `[1, 2, 3, 4, 5]` (spread a list of items) |
| `insertAfter(l, position, item)` **ext** | `insertAfter([1,3], 1, 2)` | `[1, 2, 3]` (single item; `position = count(l)` appends) |
| `insertAfter(l, position, items)` **ext** | `insertAfter([1,5], 1, [2,3,4])` | `[1, 2, 3, 4, 5]` (spread a list of items) |
| `remove(l, position)` | `remove([1,2,3], 2)` | `[1, 3]` |
| `remove(l, match)` **ext** | `remove([1,2,3], function(i) i=2)` | `[1, 3]` |
| `listReplace(l, position, item)` | `listReplace([1,2,3], 2, 9)` | `[1, 9, 3]` |
| `listReplace(l, match, item)` **ext** | `listReplace([2,4,7], function(i) i<5, 5)` | `[5, 5, 7]` |
| `reverse(l)` | `reverse([1,2,3])` | `[3, 2, 1]` |
| `flatten(l)` | `flatten([[1,2],[[3]],4])` | `[1, 2, 3, 4]` |
| `distinct(l)` | `distinct([1,2,2,1])` | `[1, 2]` |
| `duplicateValues(l)` **ext** | `duplicateValues([1,2,2,1])` | `[1, 2]` |
| `intersection(lists…)` **ext** | `intersection([1,2],[2,3])` | `[2]` |
| `sort(l, precedes)` | `sort([3,1,2], function(x,y) x < y)` | `[1, 2, 3]` |
| `stringJoin(l[, sep[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` |
| `zipStringJoin(lists[, delim[, prefix, suffix]])` **ext** | `zipStringJoin([["a","b","c"],["1","2","3"]], "-")` | `["a-1","b-2","c-3"]` |

### Inserting a single list as one element

`insertBefore` and `insertAfter` dispatch on the third argument's type:

- Any non-list value → insert as a **single element**.
- A `BlList` → **spread** the list's contents into the result.

This means there's no direct way to insert a single list as one element — `insertBefore([1,4], 2, [2,3])` will spread, giving `[1,2,3,4]`, not `[1, [2,3], 4]`. To insert a list as one element, wrap it in another list so the engine spreads the outer list:

```
// expression-language
insertBefore([1, 4], 2, [[2, 3]])    // → [1, [2, 3], 4]   (outer list spread, inner kept as one element)
```

### `zipStringJoin` details (**ext**)

`zipStringJoin` takes `N` lists of equal length and produces a single list of strings, where
each element is the concatenation of the corresponding-position elements from each input list.

```
// expression-language
zipStringJoin([["a","b","c"], ["1","2","3"]])           // → ["a1", "b2", "c3"]
zipStringJoin([["a","b","c"], ["1","2","3"]], "-")      // → ["a-1", "b-2", "c-3"]
```

**Input constraints**:

- The outer `lists` argument is a `BlList` of `BlList` (a list of lists).
- All inner lists must have the **same length** — mismatched lengths → `BlTypeError`.
- All inner-list elements must be `BlString` — non-string elements → `BlTypeError`. Use
  `string(x)` to convert explicitly upstream if needed.

**Delimiter** (`delim`) accepts two forms:

- A single `BlString` — applied between every adjacent pair (so for `N` lists, the same
  delimiter appears `N−1` times per element).
- A `BlList` of `BlString` of length **`N−1`** — each delimiter applied at the corresponding
  gap. Wrong length → `BlTypeError`.

```
// expression-language
zipStringJoin(["a", "b", "c"], "-")                     // single delim → "a-b-c"
zipStringJoin(["a", "b", "c"], ["-", ":"])              // per-gap delims → "a-b:c"
```

**Prefix and suffix** each accept two forms:

- A single `BlString` — applied **once at the very start (prefix) or end (suffix)** of each
  output element.
- A `BlList` of `BlString` of length **`N`** — each prefix wraps its corresponding inner list's
  element on the leading side; each suffix wraps its corresponding inner list's element on the
  trailing side. Wrong length → `BlTypeError`.

```
// expression-language
// Single prefix/suffix: overall wrap
zipStringJoin(["a", "b", "c"], "-", "[", "]")
// → "[a-b-c]"

// Per-list prefix/suffix: each list's element is wrapped individually
zipStringJoin(["a", "b", "c"], "-", ["<", "(", "{"], [">", ")", "}"])
// → "<a>-(b)-{c}"
```

You can mix forms (e.g. single delim with per-list prefix/suffix, or vice versa) — `delim`,
`prefix`, and `suffix` are independent.

**Edge cases**:

- `zipStringJoin([])` (no inner lists) → `[]`
- `zipStringJoin([[]])` (one empty inner list) → `[]`
- `zipStringJoin([list])` (one inner list, no concat happens) → `list` itself

### Aggregation

| Function | Supported types | Example | Result |
|---|---|---|---|
| `sum(l)` | `number` \| `duration` | `sum([dtDuration("PT1H"), dtDuration("PT2H")])` | `dtDuration("PT3H")` |
| `product(l)` | `number` | `product([2,3,4])` | `24` |
| `min(l)` | comparable (see note) | `min([3,1,2])` | `1` |
| `max(l)` | comparable (see note) | `max([1,2,3])` | `3` |
| `mean(l)` | `number` \| `duration` | `mean([1,2,3])` | `2` (arithmetic mean) |
| `median(l)` | `number` \| `duration` | `median([6,1,2,3])` | `2.5` (average of middle two) |
| `stddev(l)` | `number` | `stddev([2,4,7,5])` | `≈2.08` (sample stddev) |
| `mode(l)` | any | `mode([6,1,6,1])` | `[1, 6]` (multimodal; returns a list) |
| `all(l)` | `boolean` | `all([true,true,false])` | `false` (three-valued; `all([]) = true`) |
| `any(l)` | `boolean` | `any([false,false,true])` | `true` (three-valued; `any([]) = false`) |

Aggregates ignore `null` elements; they return `null` only when the list is empty or has no
elements of the expected type. `all`/`any` follow three-valued logic ([boolean.spec.md](boolean.spec.md)):
`all([]) = true` (vacuous), `any([]) = false`, `all([true, null]) = null`.

`sum`, `mean`, and `median` accept lists of either **numbers** or a single **duration kind**
(all `BlDaysTimeDuration` or all `BlYearsMonthsDuration`) — mixing the two duration kinds, or
mixing numbers with durations, → `BlTypeError`. `product` and `stddev` are number-only because
they require multiplication or squaring, which isn't defined for durations.

**Comparable types.** `min(l)` and `max(l)` accept any list whose elements all share one of
the orderable types: `number`, `string`, `date`, `time`, `datetime`,
`duration` (days-time), or `duration` (years-months). `boolean`, `null`, `list`, `context`,
`table`, `range`, and `calendar` are **not** comparable (they support only `=`/`!=`, not
`<`/`<=`/`>`/`>=`).

A list whose elements mix incompatible comparable types (e.g. `min([1, "a"])` —
number+string) → `BlTypeError`. Same-type comparisons across the temporal sub-distinctions
follow the per-type rules: cross-kind datetime comparison (naive vs zoned) within a list still
yields the type's normal cross-kind `BlNull` semantics, which then propagates through the
aggregate.

### Ordering used by `min` / `max` / `sort`

The ordering applied to each comparable type is the same as the `<`/`<=`/`>`/`>=` operators on
that type. Summary:

| Element type | Ordering | Documented in |
|---|---|---|
| `number` | numeric; precision-preserving (`3.0 = 3.00`) | [number.spec.md § Operators](number.spec.md#operators) |
| `string` | **code-point order** — case-sensitive, not locale-aware | [string.spec.md § Operators](string.spec.md#operators) |
| `date` | chronological | [date.spec.md § Operators](date.spec.md#operators) |
| `time` | chronological; wall-clock for naive, UTC-instant for zoned | [time.spec.md § Comparison semantics](time.spec.md#comparison-semantics) |
| `datetime` | chronological; wall-clock for naive, UTC-instant for zoned | [datetime.spec.md § Comparison semantics](datetime.spec.md#comparison-semantics) |
| `duration` (days-time) | total seconds | [days_time_duration.spec.md](days_time_duration.spec.md) |
| `duration` (years-months) | total months | [years_months_duration.spec.md](years_months_duration.spec.md) |

**Worked string example** — code-point order has some characteristics worth knowing:

```
// expression-language
min(["banana", "apple", "cherry"])   // → "apple"
min(["banana", "Apple"])             // → "Apple"   (uppercase A = U+0041 < lowercase b = U+0062)
min(["app", "apple"])                // → "app"     (shorter wins when it's a prefix)
min(["cafz", "café"])                // → "cafz"    (z = U+007A < é = U+00E9, not what Spanish/French locale would expect)
```

`sort(l, precedes)` doesn't use these defaults — the caller supplies the comparison function
explicitly. Use the per-type operators inside the `precedes` function to match the same
ordering: `sort(items, function(a, b) a < b)` gives the same order as the type's natural
ordering would produce in `min`/`max`.

`[@test] ../../expr/list_functions_test.go`

---

## Go implementation (expr extension)

Lives in `expr/list.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlList` is the immutable Go value type that represents a list inside the engine and at the
host-code boundary. It wraps a Go slice of `BlValue`. The single field is private so callers
cannot mutate the underlying slice — every operation in the library returns a fresh `BlList`,
and the `Native()` accessor returns a defensive copy so host code can mutate freely without
affecting the source.

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` is **element-wise and order-sensitive** — `[1, 2]` and `[2, 1]` are not
  equal, and two lists of different length are never equal. `String()` doubles as the
  `fmt.Stringer` implementation, producing the canonical literal form (e.g. `"[1, 2, 3]"`).
- **`List(items ...BlValue)`** — the host constructor. Variadic so callers can write
  `List(v1, v2, v3)` directly or `List(slice...)` to spread an existing slice. Infallible:
  there's no input shape that can fail at construction. Native Go slices passed in via the
  engine bridge are also wrapped to `BlList`.
- **`Native()` accessor** — returns a defensive copy of the underlying `[]BlValue`. Callers
  may mutate the returned slice without affecting the `BlList`. From there, normal Go slice
  operations are available.

```go
// host-side (Go)
type BlList struct{ items []BlValue }   // immutable; items is private and never mutated

// BlValue interface — required by all Bl* value types.
func (BlList) Type() BlType { return BlTypeList }
func (l BlList) Equal(other BlValue) BlValue   // element-wise, order-sensitive; mismatched lengths → false
func (l BlList) String() string                // canonical literal form, e.g. "[1, 2, 3]"
func (BlList) isBlValue() {}

// Host constructor — variadic; spread an existing slice with List(slice...).
func List(items ...BlValue) BlList

// Host accessor (consume an evaluated result).
func (l BlList) Native() []BlValue              // defensive copy; callers may mutate freely
```

### Backing implementations (unexported, suffix `Fn`)

List has **no per-type operator implementation functions**. Equality (`=` / `!=`) dispatches
through the `BlValue.Equal()` interface method (see [§ Value type & host API](#value-type--host-api-exported)),
and the `in` operator is patcher-lowered to a call to `listContains(l, x)`. The indexing,
filter, and projection bracket/dot syntax (`l[i]`, `l[predicate]`, `l.field`) is lowered by
the engine per [bl-expr.spec.md](bl-expr.spec.md). List has no arithmetic or ordering
operators.

The library functions are implemented as these typed/variadic Go functions, wrapped by
`typed1`/`typed2`/`typed3`/`variadic` at registration time:

```go
// host-side (Go)
// Typed implementations — wrapped by typed1/typed2/typed3 at registration.
func countFn(l BlList) BlNumber
func listIsEmptyFn(l BlList) BlBoolean                            // overloads string/context/range
func listContainsFn(l BlList, e BlValue) BlBoolean
func listIndexOfFn(l BlList, match BlValue) BlList                // list overload of indexOf (string overload in string.spec.md)
func listReverseFn(l BlList) BlList                               // list overload of reverse
func flattenFn(l BlList) BlList                                   // recursive
func distinctFn(l BlList) BlList                            // preserves first-occurrence order
func duplicateValuesFn(l BlList) BlList                           // ext; values appearing more than once
func sortFn(l BlList, precedes BlFunc) BlList

// Aggregation impls — return BlValue because empty / wrong-type / mixed-type inputs yield BlNull or BlTypeError.
func sumFn(l BlList) BlValue        // accepts number list or single-kind duration list; mixed → BlTypeError
func productFn(l BlList) BlValue    // number only
func minFn(l BlList) BlValue        // any comparable element type (uniform within list); mixed → BlTypeError
func maxFn(l BlList) BlValue
func meanFn(l BlList) BlValue       // number or single-kind duration list
func medianFn(l BlList) BlValue     // number or single-kind duration list; even-length averages middle two
func stddevFn(l BlList) BlValue     // sample stddev; number only
func modeFn(l BlList) BlList        // any element type (just counts occurrences); multimodal returns list
func allFn(l BlList) BlValue        // three-valued: all([]) = true, all([true, null]) = null
func anyFn(l BlList) BlValue        // three-valued: any([]) = false, any([false, null]) = null

// Variadic implementations — handle multiple input shapes in expr's raw shape.
func sublistFn(args ...any) (any, error)        // (l, start) | (l, start, length)
func appendFn(args ...any) (any, error)         // (l, items…)
func prependFn(args ...any) (any, error)        // (l, items…)
func concatenateFn(args ...any) (any, error)    // (lists…)
func unionFn(args ...any) (any, error)          // (lists…) — dedup across all
func intersectionFn(args ...any) (any, error)   // (lists…) — ext
func stringJoinFn(args ...any) (any, error)     // (l) | (l, sep) | (l, sep, prefix, suffix); reduces a list of strings to a single string via strings.Join
func zipStringJoinFn(args ...any) (any, error)  // (lists) | (lists, delim) | (lists, delim, prefix, suffix); delim/prefix/suffix each BlString or BlList of BlString — ext
func removeFn(args ...any) (any, error)         // (l, pos BlNumber) | (l, pred BlFunc) — predicate form is ext
func listReplaceFn(args ...any) (any, error)    // (l, pos BlNumber, item) | (l, pred BlFunc, item) — predicate form is ext
func insertBeforeFn(args ...any) (any, error)   // (l, pos, item) inserts single; (l, pos, items BlList) spreads — 1-based; out-of-range → BlTypeError
func insertAfterFn(args ...any) (any, error)    // ext; (l, pos, item) inserts single; (l, pos, items BlList) spreads — 1-based; pos = count(l) appends; out-of-range → BlTypeError
```

`BlFunc` is the engine's value type for an inline `function(...) …` comparator/predicate (see
[§ Function invocation](bl-expr.spec.md#function-invocation)). Out-of-range positions for
`insertBefore`, `remove`, and `listReplace` follow the rules documented in [§ Edge cases](#edge-cases).

`stringJoin` lives here (its first argument is a `BlList` and it reduces the list to a single
string, matching the pattern of every other list-reducing function — `sum`, `min`, `max`,
`mode`, `zipStringJoin`). The string spec carries only a brief reference back to this section.
The backing impl wraps Go's [`strings.Join`](https://pkg.go.dev/strings#Join).

### Registrations (`listOptions`, unexported)

`listOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the list library. Each entry is built with
`expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions.
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlList) BlNumber` into that shape; the variadic
  impls are registered directly because their optional-arg shapes can't be expressed as a
  fixed-arity adapter.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures (e.g.
  `sublist` accepts both `(l, start)` and `(l, start, length)` forms).

The registrations are grouped by role: the core list operations, aggregation, and ext
additions.

```go
// host-side (Go)
func listOptions() []expr.Option {
    return []expr.Option{
        // core list operations
        expr.Function("count",         typed1(countFn),         new(func(BlList) BlNumber)),
        expr.Function("isEmpty",       typed1(listIsEmptyFn),   new(func(BlList) BlBoolean)),       // overloads string/context/range
        expr.Function("listContains",  typed2(listContainsFn),  new(func(BlList, BlValue) BlBoolean)),
        expr.Function("indexOf",       typed2(listIndexOfFn),   new(func(BlList, BlValue) BlList)), // list overload; string overload in string.spec.md
        expr.Function("sublist",       sublistFn,
            new(func(BlList, BlNumber) BlList),
            new(func(BlList, BlNumber, BlNumber) BlList)),
        expr.Function("append",        appendFn,                new(func(BlList, ...BlValue) BlList)),
        expr.Function("concatenate",   concatenateFn,           new(func(...BlList) BlList)),
        expr.Function("union",         unionFn,                 new(func(...BlList) BlList)),
        expr.Function("insertBefore",  insertBeforeFn,
            new(func(BlList, BlNumber, BlValue) BlList),                     // single item
            new(func(BlList, BlNumber, BlList) BlList)),                     // ext — spread a list of items
        expr.Function("insertAfter",   insertAfterFn,                        // ext (both forms)
            new(func(BlList, BlNumber, BlValue) BlList),                     // single item
            new(func(BlList, BlNumber, BlList) BlList)),                     // spread a list of items
        expr.Function("remove",        removeFn,
            new(func(BlList, BlNumber) BlList),
            new(func(BlList, BlFunc) BlList)),                  // predicate form is ext
        expr.Function("listReplace",   listReplaceFn,
            new(func(BlList, BlNumber, BlValue) BlList),
            new(func(BlList, BlFunc, BlValue) BlList)),         // predicate form is ext
        expr.Function("reverse",       typed1(listReverseFn),   new(func(BlList) BlList)),          // list overload; string overload in string.spec.md
        expr.Function("flatten",       typed1(flattenFn),       new(func(BlList) BlList)),
        expr.Function("distinct",typed1(distinctFn),new(func(BlList) BlList)),
        expr.Function("sort",          typed2(sortFn),          new(func(BlList, BlFunc) BlList)),

        // aggregation
        expr.Function("sum",     typed1(sumFn),     new(func(BlList) BlValue)),
        expr.Function("product", typed1(productFn), new(func(BlList) BlValue)),
        expr.Function("min",     typed1(minFn),     new(func(BlList) BlValue)),
        expr.Function("max",     typed1(maxFn),     new(func(BlList) BlValue)),
        expr.Function("mean",    typed1(meanFn),    new(func(BlList) BlValue)),
        expr.Function("median",  typed1(medianFn),  new(func(BlList) BlValue)),
        expr.Function("stddev",  typed1(stddevFn),  new(func(BlList) BlValue)),
        expr.Function("mode",    typed1(modeFn),    new(func(BlList) BlList)),
        expr.Function("all",     typed1(allFn),     new(func(BlList) BlValue)),  // three-valued
        expr.Function("any",     typed1(anyFn),     new(func(BlList) BlValue)),  // three-valued

        // string-producing reducers (input is a list; output is a string or list of strings)
        expr.Function("stringJoin",      stringJoinFn,
            new(func(BlList) BlString),
            new(func(BlList, BlString) BlString),
            new(func(BlList, BlString, BlString, BlString) BlString)),

        // ext
        expr.Function("prepend",         prependFn,               new(func(BlList, ...BlValue) BlList)),
        expr.Function("duplicateValues", typed1(duplicateValuesFn), new(func(BlList) BlList)),
        expr.Function("intersection",    intersectionFn,          new(func(...BlList) BlList)),
        expr.Function("zipStringJoin",   zipStringJoinFn,
            new(func(BlList) BlList),                                                              // no delim/prefix/suffix
            new(func(BlList, BlValue) BlList),                                                     // delim only (BlString or BlList)
            new(func(BlList, BlValue, BlValue, BlValue) BlList)),                                  // delim + prefix + suffix (each BlString or BlList)
    }
}
```

Native Go slices wrap to `BlList` via the engine's input bridge.

`[@test] ../../expr/list_test.go`

---

## Edge cases

- Out-of-range index → `null`.
- `sublist` with `length = 0` → `[]`; negative `start` counts from the end.
- `remove` at an out-of-range position → the list unchanged.
- `sort` with a non-strict-weak-ordering comparator → undefined order.
- numeric aggregates over an empty/non-numeric list → `null`.
