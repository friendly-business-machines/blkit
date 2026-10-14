# Proposal

## Why

`.bl` has typed lists and basic `for`/`every` iteration, but lacks most list expressions and operations needed to transform, search, sort, and summarize business data. `list-requirements.md` supplies concrete expected behavior; this change makes that behavior a validated language contract, subject to the explicit decision to keep lists single-typed and reject `null` list elements.

## What Changes

- Extend list expressions with one-based and negative indexing, predicate filtering, dictionary-field projection, `some` quantification, multiple Cartesian-product iteration bindings, list equality/membership, and inclusive decimal sequences (`seq` and `start:end`). Keep `List<T>` homogeneous; reject heterogeneous literals and `null` outside its existing range-bound role. Use `.item` to reference the current element only inside a bracket filter predicate; plain `item` remains an ordinary name.
- Add typed, non-mutating list inspection, updates, set-like operations, sorting, joins, and aggregates with the signatures and ordering rules in `list-requirements.md`; preserve existing string, dictionary, calendar, and range overloads where their argument types differ.
- **BREAKING** Change numeric `mode` from a single `Number` to `List<Number>` of all modes in ascending order (including a singleton for a unique mode); update its current tie behavior. Widen `sum`/`mean`/`median` to same-kind duration lists and `min`/`max` to comparable single-type lists while keeping supported numeric behavior and existing empty-list rules.
- Extend `stringJoin` to optional separator/prefix/suffix and add `zipStringJoin`, including shape/type errors; align decimal sequence generation and ordering with the existing precision and temporal comparison rules.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Extend typed list expressions and built-ins, and revise the existing numeric aggregate and string-join contracts.

## Impact

Expression parsing, type inference/validation, generated Rust code, shared runtime helpers, decision/process expression consumers, language documentation, and parser/generated-output tests. Existing `.bl` callers expecting scalar `mode` must migrate to a `List<Number>` output or explicitly select an element. The new filter syntax uses `.item` instead of implicit plain `item`; ordinary uses of `item` (including decision/context names, record fields and input names) remain valid. No project-code changes are made by this proposal.
