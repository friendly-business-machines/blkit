-- Fixed historical rows; timestamps are seconds except fields explicitly storing milliseconds.
BEGIN;
INSERT INTO instances (id, namespace, version, process, input, status, created_at, updated_at, queued_at_ms) VALUES
    ('queued', 'orders', '1', 'route', '{"amount":"12.50"}', 'pending', 1700000000, 1700000001, 1700000000000),
    ('running', 'orders', '1', 'route', '{"amount":"20"}', 'running', 1700000000, 1700000002, 1700000000000),
    ('retry', 'orders', '2', 'review', '{"amount":"30"}', 'retry-waiting', 1700000000, 1700000003, 1700000000000),
    ('timer', 'orders', '1', 'route', '{}', 'waiting', 1700000000, 1700000004, 1700000000000),
    ('input', 'orders', '2', 'review', '{"customer":"café"}', 'waiting-for-input', 1700000000, 1700000005, 1700000000000),
    ('reply', 'orders', '2', 'review', '{}', 'pending', 1700000000, 1700000006, 1700000000000),
    ('done', 'invoices', '1', 'settle', '{}', 'completed', 1700000000, 1700000007, 1700000000000),
    ('business-error', 'invoices', '1', 'settle', '{}', 'business-error', 1700000000, 1700000008, 1700000000000),
    ('cancelled', 'orders', '1', 'route', '{}', 'cancelled', 1700000000, 1700000009, 1700000000000),
    ('terminated', 'orders', '1', 'route', '{}', 'terminated', 1700000000, 1700000010, 1700000000000),
    ('failed', 'orders', '1', 'route', '{}', 'failed', 1700000000, 1700000011, 1700000000000);

UPDATE instances SET deadline_origin='queued', deadline_duration_ms=60000, deadline_at_ms=1700000060000 WHERE id='queued';
UPDATE instances SET attempt=1, owner_id='worker-active', lease_until=1700000030000, generation=3,
    first_claim_at_ms=1700000001000, deadline_origin='claimed', deadline_duration_ms=120000, deadline_at_ms=1700000121000,
    checkpoint='{"version":2,"next_activation_id":2,"ready":[{"node":"charge","path":[],"id":1,"values":{"amount":"20"}}],"completed":{"start":{"amount":"20"}},"selected":{},"outcome":null,"terminal":null}'
    WHERE id='running';
UPDATE instances SET attempt=2, generation=2, first_failure_at=1700000002000, next_eligible_at=1700000010000,
    error='temporary upstream failure', first_claim_at_ms=1700000001000,
    retry_policy='{"max_retries":4,"retry_for_ms":300000,"retry_delay_ms":1000}',
    deadline_origin='queued', deadline_duration_ms=300000, deadline_at_ms=1700000300000,
    checkpoint='{"version":2,"next_activation_id":3,"ready":[{"node":"review","path":[],"id":2}],"completed":{"start":{"amount":"30"}},"selected":{},"outcome":null,"terminal":null}'
    WHERE id='retry';
UPDATE instances SET attempt=1, generation=1, first_claim_at_ms=1700000001000, wake_at_ms=1700000050000,
    checkpoint='{"version":2,"next_activation_id":2,"ready":[],"waiting":[{"token":{"node":"pause","path":[],"id":1},"at_ms":1700000050000}],"completed":{},"selected":{},"outcome":null,"terminal":null}'
    WHERE id='timer';
UPDATE instances SET attempt=1, generation=1, first_claim_at_ms=1700000001000,
    checkpoint='{"version":2,"next_activation_id":2,"ready":[{"node":"approve","path":[],"id":1,"values":{"start":{"customer":"café"}},"generations":[],"iterations":0,"started_at_ms":0,"batch_id":null,"item_index":null,"item":null}],"completed":{"start":{"customer":"café"}},"selected":{},"requests":{"1":{"prompt":"Approve café order?","schema":{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}}},"outcome":null,"terminal":null}'
    WHERE id='input';
UPDATE instances SET attempt=1, generation=1, first_claim_at_ms=1700000001000, input_resume=TRUE,
    checkpoint='{"version":2,"next_activation_id":2,"ready":[{"node":"approve","path":[],"id":1,"values":{},"generations":[],"iterations":0,"started_at_ms":0,"batch_id":null,"item_index":null,"item":null}],"completed":{},"selected":{},"requests":{"1":{"prompt":"Approve reply?","schema":{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}}},"outcome":null,"terminal":null}'
    WHERE id='reply';
UPDATE instances SET result='{"receipt":"paid-42"}', terminal_name='paid', attempt=1, generation=1 WHERE id='done';
UPDATE instances SET error='credit limit exceeded', terminal_name='declined', attempt=1, generation=1 WHERE id='business-error';
UPDATE instances SET terminal_name='customer-cancelled' WHERE id='cancelled';
UPDATE instances SET terminal_name='operator-stop' WHERE id='terminated';
UPDATE instances SET error='retry budget exhausted', attempt=5, generation=5, first_failure_at=1700000001000,
    retry_policy='{"max_retries":4,"retry_for_ms":300000,"retry_delay_ms":1000}' WHERE id='failed';

INSERT INTO input_requests (instance_id, request_id, prompt, schema, status, response) VALUES
    ('input', '1', 'Approve café order?', '{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}', 'outstanding', NULL),
    ('reply', '1', 'Approve reply?', '{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}', 'answered', '{"approved":true}'),
    ('done', '1', 'Confirm payment?', '{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}', 'consumed', '{"approved":true}'),
    ('cancelled', '1', 'Continue?', '{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}', 'retired', NULL);
INSERT INTO instance_attachments (instance_id, request_id, ordinal, object_key, filename, media_type, byte_length) VALUES
    ('input', NULL, 0, 'legacy/input/start/0', 'order.pdf', 'application/pdf', 2048),
    ('input', NULL, 1, 'legacy/input/start/1', 'empty.bin', NULL, 0),
    ('reply', '1', 0, 'legacy/reply/1/0', 'approval.txt', 'text/plain', 17),
    ('done', '1', 0, 'legacy/done/1/0', 'receipt.pdf', 'application/pdf', 4096);
INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES
    ('input', 'legacy_input', '2030-01-01 00:00:00+00'),
    ('reply', 'legacy_reply', '2020-01-01 00:00:00+00');
INSERT INTO workers (id, heartbeat_at, draining) VALUES
    ('worker-active', 1700000005000, FALSE), ('worker-draining', 1700000004000, TRUE);
INSERT INTO worker_capabilities (worker_id, namespace, version, process) VALUES
    ('worker-active', 'orders', '1', 'route'), ('worker-active', 'orders', '2', 'review'),
    ('worker-draining', 'invoices', '1', 'settle');
INSERT INTO process_policies (namespace, version, process, retry_policy, deadline_origin, deadline_duration_ms) VALUES
    ('orders', '1', 'route', NULL, 'claimed', 120000),
    ('orders', '2', 'review', '{"max_retries":4,"retry_for_ms":300000,"retry_delay_ms":1000}', 'queued', 300000),
    ('invoices', '1', 'settle', NULL, NULL, NULL);
COMMIT;
