---
name: BlList
description: blkit's list type (modelled on FEEL) — an ordered, immutable, heterogeneous collection; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/list.go
---

# BlList

`BlList` is blkit's ordered collection type, modelled on FEEL's list. It holds zero or more blkit values (mixed types allowed) and is **immutable**. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes. Call `.evaluate()` to materialise the result.

```go
type BlList struct { BlExpr }

// Construction is via Bl.List(items ...BlExpr) — see bl.spec.md.
// Bl.List() with no arguments yields the empty list.

// Properties — deferred
// Length BlNumber

func (l *BlList) IsEmpty() BlExpr { ... }   // evaluates to BlBoolean

// Access — deferred; 1-indexed; negative indices count from the end
func (l *BlList) Get(index BlExpr) BlExpr { ... }   // evaluates to BlNull if out of range
func (l *BlList) First() BlExpr { ... }              // evaluates to BlNull if empty
func (l *BlList) Last() BlExpr { ... }               // evaluates to BlNull if empty

// Immutable structural operations — deferred; evaluate to BlList
func (l *BlList) Append(items ...BlExpr) BlList { ... }
func (l *BlList) Prepend(items ...BlExpr) BlList { ... }
func (l *BlList) InsertBefore(position BlExpr, item BlExpr) BlList { ... }  // 1-indexed
func (l *BlList) Remove(position BlExpr) BlList { ... }     // removes item at 1-indexed position
func (l *BlList) Replace(position BlExpr, item BlExpr) BlList { ... }
func (l *BlList) Reverse() BlList { ... }
func (l *BlList) Flatten() BlList { ... }                      // recursively flattens nested lists
func (l *BlList) DistinctValues() BlList { ... }               // removes duplicates; preserving order
func (l *BlList) DuplicateValues() BlList { ... }              // items that appear more than once

// Slicing — deferred; evaluates to BlList
func (l *BlList) Sublist(startPosition BlExpr, length ...BlExpr) BlList { ... }

// Set-like operations — deferred; evaluate to BlList
func (l *BlList) Union(others ...BlExpr) BlList { ... }          // concatenate then deduplicate
func (l *BlList) Concatenate(others ...BlExpr) BlList { ... }    // concatenate without deduplication

// String join — deferred; evaluates to BlString
func (l *BlList) Join(separator BlExpr) BlString { ... }
// All elements must evaluate to BlString. The separator is inserted between
// elements (no leading or trailing separator). On an empty list evaluates to
// BlString(""). Non-string elements produce a BlTypeError at evaluation time.
// e.g. Bl.List(Bl.String("a"), Bl.String("b"), Bl.String("c")).Join(Bl.String(", "))
//      → BlString("a, b, c")

// Search — deferred
func (l *BlList) Contains(element BlExpr) BlExpr { ... }   // evaluates to BlBoolean
func (l *BlList) IndexOf(element BlExpr) BlList { ... }    // all 1-indexed positions

// Count — deferred; evaluates to BlNumber
func (l *BlList) Count() BlNumber { ... }

// Aggregation — deferred; evaluate to BlNumber or BlNull
func (l *BlList) Sum() BlExpr { ... }      // BlNull if empty or non-numeric
func (l *BlList) Product() BlExpr { ... }
func (l *BlList) Min() BlExpr { ... }      // BlNull if empty; works on any comparable type
func (l *BlList) Max() BlExpr { ... }
func (l *BlList) Mean() BlExpr { ... }     // arithmetic mean; BlNull if empty or non-numeric
func (l *BlList) Median() BlExpr { ... }
func (l *BlList) Stddev() BlExpr { ... }   // sample standard deviation

// Mode — deferred; evaluates to BlList of most-frequent values
func (l *BlList) Mode() BlList { ... }

// Boolean aggregation — deferred; evaluate to BlBoolean or BlNull (three-valued logic)
func (l *BlList) All() BlExpr { ... }      // true if all items are true; null if any item is null
func (l *BlList) Any() BlExpr { ... }      // true if any item is true; null if result is indeterminate

// Sorting — deferred; evaluates to BlList
func (l *BlList) Sort(precedes func(BlValue, BlValue) bool) BlList { ... }
// precedes is an eager host-language comparator applied at evaluation time

// Equality — deferred; evaluates to BlBoolean
func (l *BlList) Equals(other BlExpr) BlExpr { ... }   // element-wise, order-sensitive

// Conversion — deferred; evaluates to BlTable
func (l *BlList) AsTable() BlTable { ... }
// Validates that every element is a BlContext and that all rows share the same
// set of keys; raises BlTypeError at evaluation time on mismatch. See
// [table.spec.md](table.spec.md).

// Eager host-language utilities — only valid on a concrete BlList after .Evaluate()
func (l *BlList) ToArray() []BlValue { ... }
func (l *BlList) String() string { ... }   // FEEL-style notation: "[1, 2, 3]"
```

## Deferred semantics

`Bl.List(Bl.Number(1), Bl.Number(2))` is a literal leaf node. Chaining:

```go
expr := Bl.ListVar("scores").Sum().GreaterThan(Bl.Number(200))
result := expr.Evaluate(map[string]BlExpr{"scores": Bl.List(Bl.Number(90), Bl.Number(85), Bl.Number(78))})
// result == BlBoolean.TRUE
```

## Indexing

`BlList` uses **1-based indexing** (matching FEEL). The first element is at index `1`; the last is at index `length`. Negative indices count from the end: `-1` is the last element.

`get()` evaluates to `BlNull` for any out-of-range index.

## Immutability

All structural methods (`append`, `remove`, `reverse`, etc.) return new `BlExpr` nodes that evaluate to new `BlList` instances. The original is never modified.

## `all_()` and `any_()` Semantics

Follow three-valued logic (see [boolean.spec.md](./boolean.spec.md)):

- `all_([true, true])` → `true`
- `all_([true, false])` → `false`
- `all_([true, null])` → `null`
- `any_([false, false])` → `false`
- `any_([false, null])` → `null`
- `any_([true, null])` → `true`
- `all_([])` → `true` (vacuous truth)
- `any_([])` → `false`

## `flatten()`

Recursively flattens all nested `BlList` values into a single flat list. Non-list values are left in place. `[[1,[2]],3]` → `[1, 2, 3]`.

## `index_of()`

Evaluates to a `BlList` of all 1-indexed positions where the element appears. Evaluates to an empty list if not found.

## Aggregation and Null

`sum()`, `mean()`, etc. ignore `BlNull` elements in the list (modelled on the FEEL built-in behaviour). They evaluate to `BlNull` only if the list is empty or contains entirely non-numeric values (for numeric aggregations).

## Edge Cases

- `sublist(start, length)` with a negative `start` counts from the end.
- `sublist()` with a `length` of zero evaluates to an empty list.
- `remove(position)` on an out-of-range position evaluates to the original list unchanged.
- `sort()` with a `precedes` function that is not a strict weak ordering produces undefined behaviour.
