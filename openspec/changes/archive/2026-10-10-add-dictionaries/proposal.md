# Proposal

## Why

`.bl` has typed lists and a separate colon-based record declaration, but no unified way to declare, construct, and transform dictionaries. `dictionaries-changes.md` calls for nested, queryable, updatable key-value values; named fixed-shape dictionaries should use the same model instead of a distinct record type.

## What Changes

- **BREAKING**: Replace `type Order:` record declarations with named dictionary schemas such as `Order = {total: Number, blocked: Bool};`. Reject the old declaration syntax; retain fixed-shape type checking and existing JSON object shapes for migrated declarations.
- Add dictionary literals with identifier or quoted string keys, including empty, heterogeneous, nested, and expression-valued entries. Named schemas and ad hoc dictionary values share dictionary access and operations.
- Add dot and bracket key access, nested path lookup, structural equality, ordered key/entry/value queries, membership and size checks, and non-mutating put, merge, and remove operations.
- Add list iteration expressions `every ... in ... satisfies ...` and `for ... in ... return ...` so dictionary values and entries can be filtered/tested or projected as in the source examples.
- Make dictionary values usable in typed input/output ports and list expressions while retaining compile-time checks where types are known and reporting missing paths or invalid runtime types as evaluation errors.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Replace record declarations with named dictionary schemas and extend expression grammar, typing, and evaluation with dictionaries and list iteration.
- `decision-models`: Treat multi-column decision-table results as named dictionaries rather than separate record values.

## Impact

Touches `.bl` declaration/expression parsing, type validation, Rust generation, runtime value representation/serialization, decision tables, gateway joins, examples/tests, and user-facing documentation. Existing `.bl` files using `type Name:` must migrate; enum declarations and JSON object payloads for fixed-shape types stay valid. No new external dependency is assumed; existing `isEmpty(String)` and `isEmpty(Calendar)` remain valid.
