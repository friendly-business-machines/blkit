# Task 10 / OpenSpec 3.3 report

Changed `README.md` only for deployment documentation: required S3/GCS/Azure backend and provider-native environment per remote role (including JSON-only and split/combined targets), local persistent default and override, matching database/object backup and safe orphan audit, and removed obsolete warnings claiming production multipart is unavailable. Existing multipart examples and file limits remain consistent with the production routes and parser.

Validation: `openspec validate attach-files-to-requests --strict` passed; `git diff --check` passed. A focused Python source/document assertion checked environment variable names, generated target feature/source mappings, production multipart handlers and byte/count limits. Inspected `object_store` 0.14.2 builder environment documentation for AWS, GCS and Azure names. No tests added (documentation-only); no builds or full suite run. Disk check: `/tmp` 434G available; inherited `/tmp/blkit-attach-files-cargo-cOIVhQ` 146G. No caches cleaned.

Self-review: no blocker in README diff. Residuals: remote provider examples need operator-supplied secrets, cloud permissions and a private provisioned bucket/container; no live cloud credentials or backup/restore exercise performed. OpenSpec 2.1 and 3.3 checkboxes remain unchanged. Commit: see commit containing this report and README.

## Reviewer follow-up: S3 abort permissions

Accepted P2: explicitly documented `s3:AbortMultipartUpload` for API roles on attachment objects and noted that startup's put/get/delete probe does not exercise multipart abort. Confirmed `crates/blkit-core/src/multipart.rs` calls `WriteMultipart::abort()` on rejected uploads; `object_store` 0.14.2 `upload.rs` delegates to the S3 upload's `abort()` in `aws/mod.rs`, which sends DELETE with `uploadId`. No other policy actions or unrelated code/tests changed; the separate task 2.1 HTTP coverage finding belongs to the parent. Follow-up validation: `openspec validate attach-files-to-requests --strict` passed (`Change 'attach-files-to-requests' is valid`); `git diff --check` passed. Self-review found no blocker. Residual: no live AWS IAM test performed.
