## Review

- **Correct:** Table storage is private and exposed through immutable accessors (`crates/blkit-core/src/table.rs:279–283,1024–1032`). Typed JSON validation checks envelope shape, declared column order, required fields, and nullable cell types (`crates/blkit-core/src/compiled_graph.rs:182–211`). No changed `unsafe` code was found.
- **Fixed:** None; read-only review.

### Findings

#### 1. P0 — Recursive table schemas cause unbounded compiler recursion
**Location:** `crates/blkit-transpiler/src/codegen/graph.rs:20`

A declaration such as:

```bl
Node = {children: Table<Node>};
```

passes type resolution (`semantic/types.rs:4102–4107`), while `record_cycle` only follows directly named fields (`semantic/types.rs:17–22`). Generation constructs table validators for every record (`codegen/mod.rs:4055–4064`). `request_type(Table<Node>)` consequently expands `Node.children`, recursively invoking itself without a stopping condition.

**Reachable impact:** Calling public `transpile` with this declaration reaches unbounded recursion/stack exhaustion instead of returning a diagnostic—even when the record is unused.

**Smallest fix:** Detect unsupported recursive inline table-schema expansion during validation and return an error before generation. Preserve existing supported list recursion.

**Checked rule:** [C-VALIDATE][validate], advisory input-validation guidance. The finding is grounded in the complete recursion path, not a Rust language-rule claim.

#### 2. P1 — Nullable constructor cells generate an unsupported Rust conversion
**Location:** `crates/blkit-transpiler/src/codegen/mod.rs:1295`

For a declared `Rate = {price: Number?}`, a constructor such as `table(["price"], [price])`, with input `price: Number?`, passes constructor validation (`semantic/types.rs:1995–2026`). However, `emit_table_cell_type` has no nullable branch and emits `Cell::from(price.clone())`, where `price` is `Option<Number>`.

The complete `Cell` conversion implementations (`crates/blkit-core/src/table.rs:147–226`) contain no corresponding conversion. Constructors also call `Table::new`, which explicitly rejects null-bearing cells (`table.rs:352–354`).

**Reachable impact:** Valid nullable-source table construction produces Rust that cannot compile; merely adding an `Option` conversion would still leave runtime null rejection.

**Smallest fix:** Lower constructor cells to `Option<Cell>` and use `from_optional`; recursively handle nullable members inside composite cells.

**Checked rule:** [C-VALIDATE][validate], advisory. Approved table-construction and nullable-preservation requirements are normative.

#### 3. P1 — Explicit conversions discard known cell types and nullability
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:1540,1566`

`toList()` adopts the expected list type, and `toValue()` adopts the expected scalar type, without checking the known column’s type.

For `data: Table<Rate>` where `Rate = {price: Number?}`, these expressions therefore satisfy non-null declarations:

- `data[, price].toList()` → `List<Number>`
- `data[1, price].toValue()` → `Number`

Generation attempts non-null numeric decoding, so a permitted null input fails during evaluation instead of being rejected during transpilation. Without an expected type, `toValue()` instead returns `Value`, preventing inference of the valid nullable result for `coalesce`.

**Smallest fix:** Derive conversion results from `table_fields_known`, preserving nullability, then check compatibility with the expected type.

**Checked rule:** [C-VALIDATE][validate], advisory static-enforcement guidance. The approved prohibition on nullable-to-non-null assignments is normative.

#### 4. P1 — Table sorting does not reliably preserve checked comparison failures
**Location:** `crates/blkit-core/src/table.rs:856–888`

Two related paths bypass the intended fallible comparison boundary:

- Fully ranked values are compared solely by rank (`862–868`). A naive Date and a zoned Date both present in `InOrder` can therefore sort successfully without their incompatible zones being checked.
- Failed comparisons become `Ordering::Equal` (`883–885`). With naive dates `A < B` and a zoned date `Z`, this gives `A ~ Z`, `Z ~ B`, but `A < B`: the comparator is not a total order.

**Reachable impact:** Ranking can silently accept incompatible temporal inputs. For the second path, the checked standard-library contract explicitly permits `sort_by` to panic for a non-total comparator; checking `error` after sorting cannot guarantee a returned error.

**Smallest fix:** Validate key/ranking comparison compatibility before sorting with a total comparator, or use bounded fallible sorting that immediately propagates errors. Existing `list_ops::sort_checked` demonstrates the repository’s checked-comparison approach.

**Checked sources:** [slice::sort_by][sort] contract; [C-VALIDATE][validate], advisory. No panic reproduction was executed.

#### 5. P2 — Public construction admits two inconsistent representations of a null cell
**Location:** `crates/blkit-core/src/table.rs:379–391`

`from_optional` accepts and retains `Some(Cell::Null)`, although deserialization represents top-level null as `None`. Both serialize identically, but joins treat them differently: `None` keys never match, whereas two `Some(Cell::Null)` keys fall through to structural equality (`table.rs:249–269`).

**Reachable impact:** Safe Rust callers can construct a table whose null keys match in an inner join; serializing and deserializing the same table changes that behavior. This concerns the exported Rust API, not ordinary JSON input.

**Smallest fix:** Normalize top-level `Some(Cell::Null)` to `None` in `from_optional`, covering its other callers as well. Keep nested `Cell::Null` intact.

**Checked rule:** [C-VALIDATE][validate], advisory invariant-enforcement guidance.

[validate]: https://rust-lang.github.io/api-guidelines/dependability.html#c-validate
[sort]: https://doc.rust-lang.org/std/primitive.slice.html#method.sort_by

### Merge verdict: BLOCK

Finding 1 is merge-blocking; findings 2–4 need correction before release. Parent owns adjudication.

### Inspected areas and uncertainties

Inspected the supplied full-feature snapshot, connected public APIs, parser/scope forms, schema propagation, constructor/conversion lowering, typed ports, row operations, sorting, aggregates, Boolean lowering, and relevant tests—including new nullable/table-row tests. External guideline and standard-library sections were checked through retrieved source artifacts with provenance.

Skipped test execution and independent snapshot-hash computation. The reported 949 passing tests are parent-supplied evidence; findings above are source traces, not executed reproductions.