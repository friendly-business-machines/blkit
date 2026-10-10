# Review adjudication

Four independent read-only reviews were received (Rust-project, Microsoft, Google, and rust-skills). Every reported issue was reproduced against the worktree before correction. Findings are grouped by behavior below; all are **accepted**, with regression coverage in `tests/language.rs` or `tests/generated.rs`.

| Finding (reviewers) | Disposition and evidence |
| --- | --- |
| Nested dynamic numeric values become JSON strings (all four) | Accepted. Recursively convert literal dictionary and list values, including dependent keys; generated tests `nested_numbers_keep_json_kind_in_dynamic_dictionary`, `dependent_dynamic_dictionary_numbers_remain_numbers`, and `typed_nested_collections_keep_numeric_json_at_dynamic_boundary`. |
| Named numeric field via runtime-key lookup becomes a string (Rust-project, Google) | Accepted. Convert named values at the dynamic lookup boundary; `dictionary_values_entries_and_named_lookup_work` tests string and list-valued runtime keys. |
| Variable list paths emit `Vec<Vec<String>>` (rust-skills) | Accepted. Mark list-valued paths in the typed expression pass; `variable_dictionary_paths_are_typed_from_the_port`. |
| Quoted schema keys containing commas/colons are split (all four) | Accepted. Split only outside quotes/nested delimiters; `named_schema_keys_preserve_commas_and_colons_inside_quotes`. |
| Empty `for` source fails type inference (Google) | Accepted. Provide contextual list type to an empty source and emit a typed empty list; `empty_for_iteration_uses_contextual_result_type`. |
| Null accepted at dynamic input ports (rust-skills) | Accepted. Validate dynamic/list/record inputs recursively; `dynamic_ports_reject_json_null_values`. |
| Earlier literal keys become decision-node dependencies (Rust-project) | Accepted. Walk dictionary entries with lexical scope; `earlier_dictionary_key_does_not_create_a_decision_dependency`. |
| Merge of known non-dictionary list passes validation (Rust-project, rust-skills) | Accepted. Check element types; `merge_rejects_a_known_non_dictionary_list_variable`. |
| Nested named literals in typed lists/knowledge emit maps (Microsoft) | Accepted. Preserve contextual type when emitting list elements and knowledge bodies; `nested_named_dictionary_literals_are_emitted_in_typed_contexts`. |
| Synthetic literal binding can shadow another key (Microsoft) | Accepted. Allocate collision-free names; `generated_dictionary_bindings_cannot_shadow_source_keys`. |
| Synthetic named field collides with source field (rust-skills) | Accepted. Allocate collision-free Rust fields; `named_dictionary_fields_cannot_collide_after_lowering`. |
| Comma in a decision-table dictionary output is split (Microsoft) | Accepted. Reuse top-level separator parsing; `table_outputs_accept_dictionary_literals_with_commas`. |

Reviewers explicitly declined speculative dependencies, alternative map crates, unmeasured clone optimizations, escaped-quote syntax beyond the defined grammar, and treating the approved `type Name:` migration as a defect. These are not acceptance blockers.

Ponytail complexity pass on the revised diff: reused the compiler's top-level delimiter splitter for decision-table expressions instead of maintaining a second scanner (about 20 lines removed). Other generated wrappers and runtime helpers correspond to distinct approved dictionary and list operations, with no unused dependency or speculative abstraction identified.
