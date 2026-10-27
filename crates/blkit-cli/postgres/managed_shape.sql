-- Read-only v1/legacy logical shape; ownership and ACLs are validated separately.
WITH managed AS (SELECT c.* FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname IN ('instances','input_requests','instance_attachments','instance_interests','workers','worker_capabilities','process_policies','blkit_deployment','blkit_schema_migrations')), objects AS (
    SELECT 'table' AS kind, c.relname AS name,
           jsonb_build_object('kind', c.relkind, 'persistence', c.relpersistence,
                              'row_security', c.relrowsecurity, 'force_row_security', c.relforcerowsecurity,
                              'replica_identity', c.relreplident, 'options', c.reloptions,
                              'access_method', (SELECT am.amname FROM pg_am am WHERE am.oid=c.relam)) AS definition
    FROM managed c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
    UNION ALL
    SELECT 'column', c.relname || '.' || a.attname,
           jsonb_build_object('position', a.attnum, 'type', format_type(a.atttypid, a.atttypmod),
                              'not_null', a.attnotnull, 'default', pg_get_expr(d.adbin, d.adrelid),
                              'identity', a.attidentity, 'generated', a.attgenerated,
                              'collation', co.collname)
    FROM pg_attribute a JOIN managed c ON c.oid = a.attrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum
    LEFT JOIN pg_collation co ON co.oid = a.attcollation
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND a.attnum > 0 AND NOT a.attisdropped
    UNION ALL
    SELECT 'constraint', c.relname || '.' || con.conname,
           jsonb_build_object('definition', pg_get_constraintdef(con.oid),
                              'validated', con.convalidated, 'deferrable', con.condeferrable,
                              'initially_deferred', con.condeferred)
    FROM pg_constraint con JOIN managed c ON c.oid = con.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public'
    UNION ALL
    SELECT 'index', c.relname,
           jsonb_build_object('definition', pg_get_indexdef(c.oid),
                              'valid', i.indisvalid, 'ready', i.indisready,
                              'replica_identity', i.indisreplident, 'clustered', i.indisclustered)
    FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND i.indrelid IN (SELECT oid FROM managed)
    UNION ALL
    SELECT 'trigger', c.relname || '.' || t.tgname,
           jsonb_build_object('definition', pg_get_triggerdef(t.oid), 'enabled', t.tgenabled)
    FROM pg_trigger t JOIN managed c ON c.oid = t.tgrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public' AND NOT t.tgisinternal
    UNION ALL
    SELECT 'rule', c.relname || '.' || r.rulename,
           jsonb_build_object('definition', pg_get_ruledef(r.oid))
    FROM pg_rewrite r JOIN managed c ON c.oid = r.ev_class
    UNION ALL
    -- Canonical v1 has no inheritance or partition edges, in either direction.
    SELECT 'inheritance', i.inhrelid::regclass::text || '->' || i.inhparent::regclass::text,
           jsonb_build_object('sequence', i.inhseqno)
    FROM pg_inherits i
    WHERE i.inhrelid IN (SELECT oid FROM managed) OR i.inhparent IN (SELECT oid FROM managed)
    UNION ALL
    SELECT 'function', p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')',
           jsonb_build_object('definition', pg_get_functiondef(p.oid))
    FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public' AND p.proname='notify_instance_interests'
)
SELECT coalesce(string_agg(jsonb_build_object('kind', kind, 'name', name, 'definition', definition)::text, E'\n' ORDER BY kind, name), '') FROM objects;
