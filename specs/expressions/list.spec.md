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
[1, 2, 3, 4]                       // literal
[1, 2, 3, 4][1]                    // → 1     (1-based)
[1, 2, 3, 4][-1]                   // → 4     (from end; out of range → null)
[1, 2, 3, 4][item > 2]             // → [3, 4]   (filter; `item` is the element)
[{name:"A"},{name:"B"}].name       // → ["A", "B"]   (projection)
for x in [1,2,3] return x * 2      // → [2, 4, 6]
some x in [1,2,3] satisfies x > 2  // → true
```

`[@test] ../../expr/list_test.go`

---

## Built-in functions

Standard DMN functions plus blkit extensions (**ext**). Positions are 1-based.

| Function | Example | Result |
|---|---|---|
| `count(l)` | `count([1,2,3])` | `3` |
| `isEmpty(l)` | `isEmpty([])` | `true` |
| `listContains(l, e)` | `listContains([1,2,3], 2)` | `true` |
| `indexOf(l, match)` | `indexOf([1,2,3,2], 2)` | `[2, 4]` (all positions) |
| `first(l)` **ext** / `last(l)` **ext** | `first([1,2,3])` / `last([1,2,3])` | `1` / `3` (or `l[1]` / `l[-1]`) |
| `sublist(l, start[, length])` | `sublist([1,2,3], 2)` | `[2, 3]` (negative `start` from end) |
| `append(l, items…)` | `append([1], 2, 3)` | `[1, 2, 3]` |
| `prepend(l, items…)` **ext** | `prepend([2,3], 1)` | `[1, 2, 3]` |
| `concatenate(lists…)` | `concatenate([1,2],[3])` | `[1, 2, 3]` |
| `union(lists…)` | `union([1,2],[2,3])` | `[1, 2, 3]` (dedup) |
| `insertBefore(l, position, item)` | `insertBefore([1,3], 1, 2)` | `[2, 1, 3]` |
| `remove(l, position)` | `remove([1,2,3], 2)` | `[1, 3]` |
| `remove(l, match)` **ext** | `remove([1,2,3], function(i) i=2)` | `[1, 3]` |
| `listReplace(l, position, item)` | `listReplace([1,2,3], 2, 9)` | `[1, 9, 3]` |
| `listReplace(l, match, item)` **ext** | `listReplace([2,4,7], function(i) i<5, 5)` | `[5, 5, 7]` |
| `reverse(l)` | `reverse([1,2,3])` | `[3, 2, 1]` |
| `flatten(l)` | `flatten([[1,2],[[3]],4])` | `[1, 2, 3, 4]` |
| `distinctValues(l)` | `distinctValues([1,2,2,1])` | `[1, 2]` |
| `duplicateValues(l)` **ext** | `duplicateValues([1,2,2,1])` | `[1, 2]` |
| `intersection(lists…)` **ext** | `intersection([1,2],[2,3])` | `[2]` |
| `sort(l, precedes)` | `sort([3,1,2], function(x,y) x < y)` | `[1, 2, 3]` |
| `stringJoin(l[, sep[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` |

### Aggregation

| Function | Example | Result |
|---|---|---|
| `sum(l)` / `product(l)` | `sum([1,2,3])` | `6` |
| `min(l)` / `max(l)` | `max([1,2,3])` | `3` (any comparable type) |
| `mean(l)` / `median(l)` | `median([6,1,2,3])` | `2.5` |
| `stddev(l)` | `stddev([2,4,7,5])` | `≈2.08` (sample) |
| `mode(l)` | `mode([6,1,6,1])` | `[1, 6]` |
| `all(l)` / `any(l)` | `all([true,false])` | `false` (three-valued) |

Numeric aggregates ignore `null` elements; they return `null` only when the list is empty or has no
numeric values. `all`/`any` follow three-valued logic ([boolean.spec.md](boolean.spec.md)):
`all([]) = true` (vacuous), `any([]) = false`, `all([true, null]) = null`.

`[@test] ../../expr/list_functions_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `=` `!=` | element-wise, order-sensitive equality | `[1,2] = [1,2]` | `true` |
| `in` | element membership of a value | `2 in [1,2,3]` | `true` |

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.List(...)` | `[ … ]` literal |
| `length` / `count` | `count(l)` |
| `isEmpty` | `isEmpty(l)` |
| `get(i)` / `first` / `last` | `l[i]` / `first(l)` **ext** (`l[1]`) / `last(l)` **ext** (`l[-1]`) |
| `append` / `prepend` | `append(l, …)` / `prepend(l, …)` **ext** |
| `insertBefore` / `remove` / `replace` | `insertBefore` / `remove` / `listReplace` |
| `reverse` / `flatten` / `distinctValues` / `duplicateValues` | same-named built-ins |
| `sublist` | `sublist(l, start[, length])` |
| `union` / `concatenate` | `union` / `concatenate` |
| `join` | `stringJoin(l, sep)` ([string.spec.md](string.spec.md)) |
| `contains` / `indexOf` | `listContains(l, e)` / `indexOf(l, match)` |
| `sum`/`product`/`min`/`max`/`mean`/`median`/`stddev`/`mode` | same-named built-ins |
| `all` / `any` | `all(l)` / `any(l)` |
| `sort` | `sort(l, precedes)` |
| `equals` | `=` / `!=` |
| `asTable` | `table(l)` ([table.spec.md](table.spec.md)) |
| `toArray` / `String` | Go host accessors (below) |
| (filter / projection / for / some / every) | language constructs — see [bl-expr.spec.md](bl-expr.spec.md) |

---

## Go implementation (expr extension)

Lives in `expr/list.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
type BlList struct{ items []BlValue } // immutable

func (BlList) Type() BlType { return BlTypeList }
func (l BlList) Equal(other BlValue) BlValue  // element-wise, order-sensitive
func (l BlList) ToMarkdown() string           // "[1, 2, 3]"
func (BlList) isBlValue() {}

func List(items ...BlValue) BlList // host constructor
func (l BlList) ToArray() []BlValue
func (l BlList) String() string
```

### Registrations (`listOptions`, unexported)

```go
func listOptions() []expr.Option {
    return []expr.Option{
        expr.Function("count",   typed1(countFn),   new(func(BlList) BlNumber)),
        expr.Function("isEmpty", typed1(listIsEmptyFn), new(func(BlList) BlBoolean)),     // overloads string/context/range
        expr.Function("listContains", typed2(listContainsFn), new(func(BlList, BlValue) BlBoolean)),
        expr.Function("indexOf", typed2(listIndexOfFn), new(func(BlList, BlValue) BlList)), // list overload (string overload in string.go)
        expr.Function("sublist", sublistFn, new(func(BlList, BlNumber) BlList), new(func(BlList, BlNumber, BlNumber) BlList)),
        expr.Function("append",  variadic(appendFn), new(func(BlList, ...BlValue) BlList)),
        expr.Function("concatenate", variadic(concatenateFn), new(func(...BlList) BlList)),
        expr.Function("union",   variadic(unionFn), new(func(...BlList) BlList)),
        expr.Function("insertBefore", typed3(insertBeforeFn), new(func(BlList, BlNumber, BlValue) BlList)),
        expr.Function("remove",  removeFn, new(func(BlList, BlNumber) BlList), new(func(BlList, BlFunc) BlList)), // position | predicate(ext)
        expr.Function("listReplace", listReplaceFn, new(func(BlList, BlNumber, BlValue) BlList), new(func(BlList, BlFunc, BlValue) BlList)),
        expr.Function("reverse", typed1(listReverseFn), new(func(BlList) BlList)),         // list overload (string overload in string.go)
        expr.Function("flatten", typed1(flattenFn), new(func(BlList) BlList)),
        expr.Function("distinctValues",  typed1(distinctValuesFn),  new(func(BlList) BlList)),
        expr.Function("sort",    typed2(sortFn), new(func(BlList, BlFunc) BlList)),

        // aggregation
        expr.Function("sum",  typed1(sumFn),  new(func(BlList) BlValue)),
        expr.Function("min",  typed1(minFn),  new(func(BlList) BlValue)),
        expr.Function("max",  typed1(maxFn),  new(func(BlList) BlValue)),
        expr.Function("mean", typed1(meanFn), new(func(BlList) BlValue)),
        expr.Function("all",  typed1(allFn),  new(func(BlList) BlValue)),  // three-valued
        expr.Function("any",  typed1(anyFn),  new(func(BlList) BlValue)),
        // … product, median, stddev, mode

        // ext
        expr.Function("prepend",    variadic(prependFn),    new(func(BlList, ...BlValue) BlList)),
        expr.Function("first",      typed1(firstFn),        new(func(BlList) BlValue)),
        expr.Function("last",       typed1(lastFn),         new(func(BlList) BlValue)),
        expr.Function("duplicateValues", typed1(duplicateValuesFn), new(func(BlList) BlList)),
        expr.Function("intersection", variadic(intersectionFn), new(func(...BlList) BlList)),
        // stringJoin is registered in string.go (BlList → BlString)
    }
}
```

`BlFunc` is the engine's value type for an inline `function(...) …` comparator/predicate (see
[§ Function invocation](bl-expr.spec.md#function-invocation); subject to the user-defined-function
scope decision). **Operators.** `=`/`!=` element-wise; `in` membership and **indexing / filter /
projection** (`l[i]`, `l[pred]`, `l.field`, 1-based, `item` binding) are lowered by the engine per the
hub. Native Go slices wrap to `BlList`.

`[@test] ../../expr/list_test.go`

---

## Edge cases

- Out-of-range index → `null`.
- `sublist` with `length = 0` → `[]`; negative `start` counts from the end.
- `remove` at an out-of-range position → the list unchanged.
- `sort` with a non-strict-weak-ordering comparator → undefined order.
- numeric aggregates over an empty/non-numeric list → `null`.
