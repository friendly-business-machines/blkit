# Unversioned PostgreSQL baseline

Frozen from `PostgresStore::connect_with_pool_size` at commit
`e5b4430b27f3ed55b1269df7e06cc25b5c2f8deb`, before CLI-owned migrations.
These historical assets must not track later schema changes or serve as runtime
schema preparation.

Load `schema.sql` then `data.sql` into an empty disposable database, using any SQL
client (for example `psql -v ON_ERROR_STOP=1 -1 -f schema.sql`, then
`psql -v ON_ERROR_STOP=1 -f data.sql`). Neither asset needs the runtime constructor,
the CLI administration path, object storage, or a migration ledger.

`expected-data.json` independently records every persisted column of every row.
Compare timestamps in UTC. Input, checkpoint, policy, schema and reply values are
stored as TEXT, so their exact serialized bytes matter, not just JSON semantics.

Coverage:

- All seven tables, with the original column order, types, defaults, constraints,
  partial indexes, notification triggers and PL/pgSQL function.
- Eleven instances across three process identities and all ten current statuses,
  including an owned lease/generation, retry history/policy, queued/claimed
  deadlines, timer wake-up, input-resume flag and five resumable checkpoints.
- Outstanding, answered, consumed and retired input requests; persisted replies.
- Start/reply attachments, multiple ordinals, nullable media type and zero bytes.
- Live/expired interests, active/draining workers, capabilities and policies.

`tests/postgres_legacy.rs` loads the fixture standalone, checks all rows against
independent expectations, and deserializes checkpoints to verify request/activation
invariants. The temporary runtime-constructor catalog oracle was retired when
startup DDL was removed. `tests/postgres_preparation.rs` independently compares the
frozen application schema with canonical CLI preparation; the historical loader
never depends on that preparation path.
