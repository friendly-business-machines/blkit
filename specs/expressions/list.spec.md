---
name: bl.BlList
description: The list type in the blkit expression language — an ordered, immutable, heterogeneous collection. Covers list literals, indexing/filter/projection, the list built-in library (incl. blkit extensions), and the Go layer (bl.BlList + expr registrations).
targets:
  - ../../core/list.go
---

# bl.BlList — the `list` type

`list` is an ordered, immutable, heterogeneous collection. The Go value type backing it is `bl.BlList`.
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

When you apply field access (`.fieldName`) to a **list of dictionaries**, the engine projects
the field across every element and returns a list of the corresponding values:

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
| Single dictionary with the field | the field's value (normal field access) |
| Single dictionary without the field | `null` |
| List of dictionaries, every element has the field | list of values |
| List of dictionaries, some elements missing the field | list with `null` for missing fields, e.g. `[{name:"A"},{age:30}].name` → `["A", null]` |
| List containing non-dictionary elements | `bl.TypeError` (the projection is only defined for dictionaries) |
| Empty list | `[]` |

Projection composes naturally with the aggregates and the rest of the list library. Assume
the following data is in scope (`orders` and `customers` are each a `bl.BlList` of `bl.BlDictionary`,
equivalently a `bl.BlTable`):

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

`[@test] ../../core/list_test.go`

---

## Construction (host-side)

Host Go code builds a list with the variadic `bl.List(items ...bl.BlValue)` constructor. It's
infallible — every input shape is valid; `bl.List()` with no arguments yields the empty list, and
existing Go slices spread in via `bl.List(slice...)`. Element order is preserved exactly (lists
are order-sensitive at the language level — see [§ Operators](#operators)).

```go
// host-side (Go)
// Build a list directly from bl.BlValue arguments.
var scores = bl.List(bl.Number(85), bl.Number(92), bl.Number(78))

// Spread an existing slice of BlValues.
var names  = []bl.BlValue{bl.String("Alice"), bl.String("Bob"), bl.String("Charlie")}
var roster = bl.List(names...)

// Empty list — degenerate but valid.
var empty  = bl.List()
```

`bl.List(...)` returns a `bl.BlList` directly (no error path). For the alternative of letting the
engine bridge wrap a native Go slice automatically when the list is supplied as an input
variable, see [bl-expr.spec.md § Bridging native ↔ Bl*](bl-expr.spec.md#bridging-native--bl-valuego);
when the host already holds `bl.BlValue`s, `bl.List(...)` is preferred since it avoids the
round-trip through the bridge.

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

`[@test] ../../core/list_test.go`

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
| `sort(l[, order])` | `sort([3,1,2], "desc")` | `[3, 2, 1]` — `order` is `"asc"` (default), `"desc"`, or an explicit-order list (see [§ Ordering](#ordering-used-by-min--max--sort)) |
| `stringJoin(l[, sep[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` |
| `zipStringJoin(lists[, delim[, prefix, suffix]])` **ext** | `zipStringJoin([["a","b","c"],["1","2","3"]], "-")` | `["a-1","b-2","c-3"]` |
| `seq(start, end[, step])` **ext** | `seq(5, 10)` | `[5, 6, 7, 8, 9, 10]` (materialised numeric sequence) |

### Inserting a single list as one element

`insertBefore` and `insertAfter` dispatch on the third argument's type:

- Any non-list value → insert as a **single element**.
- A `bl.BlList` → **spread** the list's contents into the result.

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

- The outer `lists` argument is a `bl.BlList` of `bl.BlList` (a list of lists).
- All inner lists must have the **same length** — mismatched lengths → `bl.TypeError`.
- All inner-list elements must be `bl.BlString` — non-string elements → `bl.TypeError`. Use
  `string(x)` to convert explicitly upstream if needed.

**Delimiter** (`delim`) accepts two forms:

- A single `bl.BlString` — applied between every adjacent pair (so for `N` lists, the same
  delimiter appears `N−1` times per element).
- A `bl.BlList` of `bl.BlString` of length **`N−1`** — each delimiter applied at the corresponding
  gap. Wrong length → `bl.TypeError`.

```
// expression-language
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-")        // single delim → ["a-1-x", "b-2-y"]
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], ["-", ":"]) // per-gap delims → ["a-1:x", "b-2:y"]
```

**Prefix and suffix** each accept two forms:

- A single `bl.BlString` — applied **once at the very start (prefix) or end (suffix)** of each
  output element.
- A `bl.BlList` of `bl.BlString` of length **`N`** — each prefix wraps its corresponding inner list's
  element on the leading side; each suffix wraps its corresponding inner list's element on the
  trailing side. Wrong length → `bl.TypeError`.

```
// expression-language
// Single prefix/suffix: wraps each output element once
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-", "[", "]")
// → ["[a-1-x]", "[b-2-y]"]

// Per-list prefix/suffix: each inner list's contribution is wrapped individually
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-", ["<", "(", "{"], [">", ")", "}"])
// → ["<a>-(1)-{x}", "<b>-(2)-{y}"]
```

You can mix forms (e.g. single delim with per-list prefix/suffix, or vice versa) — `delim`,
`prefix`, and `suffix` are independent.

**Edge cases**:

- `zipStringJoin([])` (no inner lists) → `[]`
- `zipStringJoin([[]])` (one empty inner list) → `[]`
- `zipStringJoin([list])` (one inner list, no concat happens) → `list` itself

### Sequence constructor: `seq` and the `:` operator

`seq(start, end[, step])` (**ext**) materialises a numeric sequence as a `bl.BlList`. Both `start`
and `end` are inclusive; `step` defaults to `1`. The shorthand `start:end` syntax (see
[bl-expr.spec.md § Sequences](bl-expr.spec.md#sequences-the--operator)) is parser-lowered to
`seq(start, end, 1)`, so the two forms are exactly equivalent for the default-step case.

```
// expression-language
seq(5, 10)                          // → [5, 6, 7, 8, 9, 10]
5:10                                // → [5, 6, 7, 8, 9, 10] (sugar for seq(5, 10, 1))
seq(0, 10, 2)                       // → [0, 2, 4, 6, 8, 10]
seq(1, 1)                           // → [1]                (single-element sequence)
seq(10, 5)                          // → [10, 9, 8, 7, 6, 5] (auto-reversed when start > end with positive step)
10:5                                // → [10, 9, 8, 7, 6, 5]
seq(1.5, 3.5)                       // → [1.5, 2.5, 3.5]    (fractional start/end with default step 1)
seq(0, 1, 0.25)                     // → [0, 0.25, 0.5, 0.75, 1]
```

**Auto-reverse.** When `start > end` and `step` is positive, `seq` auto-reverses internally —
the result steps downward from `start` to `end` by `step`. Explicitly passing a negative `step`
produces the same descending result for `start > end`. Reversing a sequence built this way
yields the ascending form (`reverse(10:5)` → `[5, 6, 7, 8, 9, 10]`).

**Step constraints.** `step` must be a non-zero `bl.BlNumber`. A `step` of `0` → `bl.TypeError` (no
progress would be made and the sequence would be infinite). A `step` with the wrong sign for a
non-reversed sequence (e.g. `seq(5, 10, -1)`) is treated as **auto-direction**: the function
inspects the relative ordering of `start` and `end` and uses `abs(step)` in the appropriate
direction. So `seq(5, 10, -1)` → `[5, 6, 7, 8, 9, 10]`, identical to `seq(5, 10, 1)`.

**Fractional steps** are supported and preserved exactly (no float rounding) — `seq(0, 1, 0.1)`
yields exactly 11 elements with no precision loss, matching blkit's arbitrary-precision
`bl.BlNumber` semantics. When `(end - start)` is not an exact multiple of `step`, the last element
is the largest value not exceeding `end` (for ascending) / not less than `end` (for descending);
`end` itself is included only when it falls exactly on the step grid (e.g. `seq(0, 0.95, 0.1)`
→ `[0, 0.1, 0.2, …, 0.9]`, stopping before `0.95`).

**Non-numeric arguments** → `bl.TypeError`. The function is integer-friendly but not
integer-only — any `bl.BlNumber` is acceptable.

`[@test] ../../core/list_test.go`

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
(all `bl.BlDaysTimeDuration` or all `bl.BlYearsMonthsDuration`) — mixing the two duration kinds, or
mixing numbers with durations, → `bl.TypeError`. `product` and `stddev` are number-only because
they require multiplication or squaring, which isn't defined for durations.

**Comparable types.** `min(l)` and `max(l)` accept any list whose elements all share one of
the orderable types: `number`, `string`, `date`, `time`, `datetime`,
`duration` (days-time), or `duration` (years-months). `boolean`, `null`, `list`, `dictionary`,
`table`, `range`, and `calendar` are **not** comparable (they support only `=`/`!=`, not
`<`/`<=`/`>`/`>=`).

A list whose elements mix incompatible comparable types (e.g. `min([1, "a"])` —
number+string) → `bl.TypeError`. Same-type comparisons across the temporal sub-distinctions
follow the per-type rules: cross-kind datetime comparison (naive vs zoned) within a list still
yields the type's normal cross-kind `bl.BlNull` semantics, which then propagates through the
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

**`sort(l[, order])`.** `sort` orders a list by the **element itself** — the mirror of the
table `t.sort(...)` method ([table.spec.md § Sort keys](table.spec.md#sort-keys)), but
without a column since a list element *is* the key. The optional `order` argument is:

| `order` | Meaning |
|---|---|
| omitted, or `"asc"` | ascending in the element type's natural ordering (the table above) |
| `"desc"` | descending |
| a `bl.BlList` | **explicit value order** — elements are ranked by their position in the list; any element not listed follows the listed ones in ascending order (the list analog of the table's `inOrder`) |

The sort is **stable**: equal-ranked elements keep their input order. Elements comparing as
`bl.Null` (e.g. a naive-vs-zoned datetime) sort to the **end** under `"asc"`/an explicit
order and **lead** under `"desc"`. A list whose elements aren't mutually comparable, or an
`order` string other than `"asc"`/`"desc"`, → `bl.TypeError`.

```
// expression-language
sort([3, 1, 2])                              // → [1, 2, 3]
sort([3, 1, 2], "desc")                      // → [3, 2, 1]
sort(["m", "s", "l"], ["s", "m", "l"])       // → ["s", "m", "l"]   (explicit order)
sort(["m", "s", "xl"], ["s", "m", "l"])      // → ["s", "m", "xl"]  ("xl" unlisted → trails, ascending)
```

This **diverges from DMN FEEL**, whose `sort(list, precedes)` takes a comparator function;
blkit's `sort` does not accept a comparator.

`[@test] ../../core/list_test.go`

---

## Go implementation (expr extension)

Lives in `expr/list.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlList` is the immutable Go value type that represents a list inside the engine and at the
host-code boundary. It wraps a Go slice of `bl.BlValue`. The single field is private so callers
cannot mutate the underlying slice — every operation in the library returns a fresh `bl.BlList`,
and the `Native()` accessor returns a defensive copy so host code can mutate freely without
affecting the source.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `Equal` is **element-wise and order-sensitive** — `[1, 2]` and `[2, 1]` are not
  equal, and two lists of different length are never equal. `bl.String()` doubles as the
  `fmt.Stringer` implementation, producing the canonical literal form (e.g. `"[1, 2, 3]"`).
- **`bl.List(items ...bl.BlValue)`** — the host constructor. Variadic so callers can write
  `bl.List(v1, v2, v3)` directly or `bl.List(slice...)` to spread an existing slice. Infallible:
  there's no input shape that can fail at construction. Native Go slices passed in via the
  engine bridge are also wrapped to `bl.BlList`. See [§ Construction
  (host-side)](#construction-host-side) for the worked example.
- **`Native()` accessor** — returns a defensive copy of the underlying `[]bl.BlValue`. Callers
  may mutate the returned slice without affecting the `bl.BlList`. From there, normal Go slice
  operations are available.

```go
// host-side (Go)
type BlList struct{ items []bl.BlValue }   // immutable; items is private and never mutated

// bl.BlValue interface — required by all Bl* value types.
func (BlList) Type() Type { return TypeList }
func (l BlList) Equal(other BlValue) BlValue   // element-wise, order-sensitive; mismatched lengths → false
func (l BlList) String() string                // canonical literal form, e.g. "[1, 2, 3]"
func (BlList) isBlValue() {}

// Host constructor — variadic; spread an existing slice with bl.List(slice...).
func List(items ...BlValue) BlList

// Host accessor (consume an evaluated result).
func (l BlList) Native() []BlValue              // defensive copy; callers may mutate freely
```

### Backing implementations (unexported, suffix `Fn`)

List has **no per-type operator implementation functions**. Equality (`=` / `!=`) dispatches
through the `bl.BlValue.Equal()` interface method (see [§ Value type & host API](#value-type--host-api-exported)),
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
func listIsEmptyFn(l BlList) BlBoolean                            // overloads string/dictionary/range
func listContainsFn(l BlList, e BlValue) BlBoolean
func listIndexOfFn(l BlList, match BlValue) BlList                // list overload of indexOf (string overload in string.spec.md)
func listReverseFn(l BlList) BlList                               // list overload of reverse
func flattenFn(l BlList) BlList                                   // recursive
func distinctFn(l BlList) BlList                            // preserves first-occurrence order
func duplicateValuesFn(l BlList) BlList                           // ext; values appearing more than once
// sort(l[, order]): ascending natural order by default. order is the BlString "asc"/"desc",
// or a BlList giving an explicit value order (elements ranked by list position; unlisted
// elements trail in ascending order). Stable. Non-comparable element types, or an unknown
// order string → TypeError; null elements sort to the end (asc/explicit) / lead (desc).
// Mirrors t.sort(...) for tables; no comparator form (a divergence from FEEL's sort).
func sortFn(l BlList, order ...BlValue) (BlList, error)

// Aggregation impls — return bl.BlValue because empty / wrong-type / mixed-type inputs yield bl.BlNull or TypeError.
func sumFn(l BlList) BlValue        // accepts number list or single-kind duration list; mixed → TypeError
func productFn(l BlList) BlValue    // number only
func minFn(l BlList) BlValue        // any comparable element type (uniform within list); mixed → TypeError
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
func insertBeforeFn(args ...any) (any, error)   // (l, pos, item) inserts single; (l, pos, items BlList) spreads — 1-based; out-of-range → TypeError
func insertAfterFn(args ...any) (any, error)    // ext; (l, pos, item) inserts single; (l, pos, items BlList) spreads — 1-based; pos = count(l) appends; out-of-range → TypeError
func seqFn(args ...any) (any, error)            // ext; (start, end) | (start, end, step) — materialises BlList[BlNumber]; auto-direction; zero step → TypeError
```

`BlFunc` is the engine's value type for an inline `function(...) …` comparator/predicate (see
[§ Function invocation](bl-expr.spec.md#function-invocation)). Out-of-range positions for
`insertBefore`, `remove`, and `listReplace` follow the rules documented in [§ Edge cases](#edge-cases).

`stringJoin` lives here (its first argument is a `bl.BlList` and it reduces the list to a single
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
  wrap a typed implementation such as `func(bl.BlList) bl.BlNumber` into that shape; the variadic
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
        expr.Function("count",         typed1(countFn),         new(func(bl.BlList) bl.BlNumber)),
        expr.Function("isEmpty",       typed1(listIsEmptyFn),   new(func(bl.BlList) bl.BlBoolean)),       // overloads string/dictionary/range
        expr.Function("listContains",  typed2(listContainsFn),  new(func(bl.BlList, bl.BlValue) bl.BlBoolean)),
        expr.Function("indexOf",       typed2(listIndexOfFn),   new(func(bl.BlList, bl.BlValue) bl.BlList)), // list overload; string overload in string.spec.md
        expr.Function("sublist",       sublistFn,
            new(func(bl.BlList, bl.BlNumber) bl.BlList),
            new(func(bl.BlList, bl.BlNumber, bl.BlNumber) bl.BlList)),
        expr.Function("append",        appendFn,                new(func(bl.BlList, ...BlValue) bl.BlList)),
        expr.Function("concatenate",   concatenateFn,           new(func(...BlList) bl.BlList)),
        expr.Function("union",         unionFn,                 new(func(...BlList) bl.BlList)),
        expr.Function("insertBefore",  insertBeforeFn,
            new(func(bl.BlList, bl.BlNumber, bl.BlValue) bl.BlList),                     // single item
            new(func(bl.BlList, bl.BlNumber, bl.BlList) bl.BlList)),                     // ext — spread a list of items
        expr.Function("insertAfter",   insertAfterFn,                        // ext (both forms)
            new(func(bl.BlList, bl.BlNumber, bl.BlValue) bl.BlList),                     // single item
            new(func(bl.BlList, bl.BlNumber, bl.BlList) bl.BlList)),                     // spread a list of items
        expr.Function("remove",        removeFn,
            new(func(bl.BlList, bl.BlNumber) bl.BlList),
            new(func(bl.BlList, BlFunc) bl.BlList)),                  // predicate form is ext
        expr.Function("listReplace",   listReplaceFn,
            new(func(bl.BlList, bl.BlNumber, bl.BlValue) bl.BlList),
            new(func(bl.BlList, BlFunc, bl.BlValue) bl.BlList)),         // predicate form is ext
        expr.Function("reverse",       typed1(listReverseFn),   new(func(bl.BlList) bl.BlList)),          // list overload; string overload in string.spec.md
        expr.Function("flatten",       typed1(flattenFn),       new(func(bl.BlList) bl.BlList)),
        expr.Function("distinct",typed1(distinctFn),new(func(bl.BlList) bl.BlList)),
        expr.Function("sort",          sortFn,
            new(func(bl.BlList) bl.BlList),
            new(func(bl.BlList, bl.BlString) bl.BlList),
            new(func(bl.BlList, bl.BlList) bl.BlList)),

        // aggregation
        expr.Function("sum",     typed1(sumFn),     new(func(bl.BlList) bl.BlValue)),
        expr.Function("product", typed1(productFn), new(func(bl.BlList) bl.BlValue)),
        expr.Function("min",     typed1(minFn),     new(func(bl.BlList) bl.BlValue)),
        expr.Function("max",     typed1(maxFn),     new(func(bl.BlList) bl.BlValue)),
        expr.Function("mean",    typed1(meanFn),    new(func(bl.BlList) bl.BlValue)),
        expr.Function("median",  typed1(medianFn),  new(func(bl.BlList) bl.BlValue)),
        expr.Function("stddev",  typed1(stddevFn),  new(func(bl.BlList) bl.BlValue)),
        expr.Function("mode",    typed1(modeFn),    new(func(bl.BlList) bl.BlList)),
        expr.Function("all",     typed1(allFn),     new(func(bl.BlList) bl.BlValue)),  // three-valued
        expr.Function("any",     typed1(anyFn),     new(func(bl.BlList) bl.BlValue)),  // three-valued

        // string-producing reducers (input is a list; output is a string or list of strings)
        expr.Function("stringJoin",      stringJoinFn,
            new(func(bl.BlList) bl.BlString),
            new(func(bl.BlList, bl.BlString) bl.BlString),
            new(func(bl.BlList, bl.BlString, bl.BlString, bl.BlString) bl.BlString)),

        // ext
        expr.Function("prepend",         prependFn,               new(func(bl.BlList, ...BlValue) bl.BlList)),
        expr.Function("duplicateValues", typed1(duplicateValuesFn), new(func(bl.BlList) bl.BlList)),
        expr.Function("intersection",    intersectionFn,          new(func(...BlList) bl.BlList)),
        expr.Function("zipStringJoin",   zipStringJoinFn,
            new(func(bl.BlList) bl.BlList),                                                              // no delim/prefix/suffix
            new(func(bl.BlList, bl.BlValue) bl.BlList),                                                     // delim only (bl.BlString or bl.BlList)
            new(func(bl.BlList, bl.BlValue, bl.BlValue, bl.BlValue) bl.BlList)),                                  // delim + prefix + suffix (each bl.BlString or bl.BlList)
        expr.Function("seq",             seqFn,
            new(func(bl.BlNumber, bl.BlNumber) bl.BlList),                                                  // default step = 1
            new(func(bl.BlNumber, bl.BlNumber, bl.BlNumber) bl.BlList)),                                       // ext; also the patcher's lowering target for `start:end`
    }
}
```

Native Go slices wrap to `bl.BlList` via the engine's input bridge.

`[@test] ../../core/list_test.go`

---

## Edge cases

- Out-of-range index → `null`.
- `sublist` with `length = 0` → `[]`; negative `start` counts from the end.
- `remove` at an out-of-range position → the list unchanged.
- `sort` with an `order` string other than `"asc"`/`"desc"`, or over a list of
  non-comparable element types → `bl.TypeError`.
- numeric aggregates over an empty/non-numeric list → `null`.
- `seq(start, end[, step])` with `step = 0` → `bl.TypeError`. `step` of the wrong sign for the
  start→end direction is treated as auto-direction (`abs(step)` applied in the correct
  direction). Non-numeric `start` / `end` / `step` → `bl.TypeError`. Fractional step preserves
  exact decimal precision (no float rounding); when `(end - start)` is not an exact multiple
  of `step`, `end` is excluded.
