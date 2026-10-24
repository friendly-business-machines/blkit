## Review

**Scope:** Supplied owner-only patch: production **+5/−3 lines**, four semantic tests, one generated regression. No whole-branch re-review or edits.

### Addressed

- **Declared Field discovery: ADDRESSED.** `crates/blkit-transpiler/src/semantic/types.rs:273,877-894` discovers Field schemas using existing inference without an expected type, accepting only `Table<NamedRow>`. Record fields are cloned unchanged, preserving order, types and nullability. Name inference remains an environment lookup (`:1259-1262`).
- **Direct/multilevel records and DictionaryEntry.value: ADDRESSED.** `crates/blkit-transpiler/tests/table_rows.rs:712-740` covers all four receiver forms, ordered selection, nullable arithmetic, nested named cells and aggregation.
- **Static rejection guarantees: ADDRESSED.** `tests/table_rows.rs:744-832` checks unknown-column diagnostics, incompatible types, nullable-to-non-null rejection, reordered output rejection, and unchanged dynamic/nullable-table/unknown-length-path limits.
- **Inference recursion: ADDRESSED.** The new discovery entry is restricted to Name/Field (`semantic/types.rs:877`). Field inference resolves its base; table discovery there targets that smaller base (`:3515-3539`), not the original Field. No same-expression inference/discovery cycle was introduced.
- **Generated behavior: ADDRESSED.** `tests/generated.rs:5268-5297` directly authors the receiver expressions without typed intermediates. Assertions check chronological—not lexical—temporal ordering, selected column order, DateTime cells, nullable/non-null arithmetic, true-only predicate selection, mixed-zone errors and malformed non-nullable input rejection. The helper checks actual generated compilation/execution (`:16-39`).

### New Critical/Important fix breakage

No issues found.

### Saved verification checked

`E/` denotes `.superpowers/sdd/tasks/task-5.3-discovery-20261023T152921Z-evidence/`. Commands and saved exits were inspected; nothing was rerun.

| Evidence | Verified result |
|---|---|
| `E/semantic-red.log:53-63` | Valid RED: declared Holder field rejected with `sort requires statically known column types`; exit **101** |
| `E/generated-red.log:53-63` | Valid RED: same semantic failure before Rust emission; exit **101** |
| `E/semantic-green.log:51-56`, `E/generated-green.log:51-54` | Initial semantic **3 passed**, generated **1 passed**; both exit **0** |
| `E/semantic-focused.log:58,82-100` | Final nullable **5** + table_rows **36**, including all four new semantic tests; exit **0** |
| `E/generated-focused.log:87,115` | Final generated **62 passed**, including the new regression; exit **0** |

**Final focused total: 103 passed, zero failed.**

The saved preservation attestation records the supplied patch SHA256 and exactly three changed source/test files. Hashes were not independently recomputed.

### Unrelated observations

Inherited warnings and reported disk pressure are outside this fix; neither is a new finding. Earlier T1–T10 work was not reopened.

- **Fixed by reviewer:** None; read-only.
- **Merge verdict:** **OK**, bounded discovery gate only. Parent retains acceptance.

Skipped: test execution and whole-workspace review. Risk: parent-owned final workspace verification remains outstanding.

**SCOPED_GATE: PASS**