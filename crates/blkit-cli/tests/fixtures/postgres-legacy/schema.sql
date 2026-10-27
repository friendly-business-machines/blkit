-- Frozen unversioned constructor DDL from e5b4430b27f3ed55b1269df7e06cc25b5c2f8deb.
-- Historical fixture, not a migration: never update it to follow new runtime schemas.
CREATE TABLE IF NOT EXISTS instances (
                id TEXT PRIMARY KEY, namespace TEXT NOT NULL, version TEXT NOT NULL,
                process TEXT NOT NULL, input TEXT NOT NULL, status TEXT NOT NULL,
                result TEXT, error TEXT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
                checkpoint TEXT, attempt BIGINT NOT NULL DEFAULT 0,
                first_failure_at BIGINT, next_eligible_at BIGINT, terminal_name TEXT,
                owner_id TEXT, lease_until BIGINT, generation BIGINT NOT NULL DEFAULT 0,
                retry_policy TEXT,
                queued_at_ms BIGINT, first_claim_at_ms BIGINT, wake_at_ms BIGINT,
                deadline_origin TEXT, deadline_duration_ms BIGINT, deadline_at_ms BIGINT
            );
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS input_resume BOOLEAN NOT NULL DEFAULT FALSE;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS retry_policy TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS queued_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS first_claim_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS wake_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_origin TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_duration_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_at_ms BIGINT;
            UPDATE instances SET queued_at_ms=created_at*1000 WHERE queued_at_ms IS NULL;
            CREATE TABLE IF NOT EXISTS input_requests (
                instance_id TEXT NOT NULL REFERENCES instances(id), request_id TEXT NOT NULL,
                prompt TEXT NOT NULL, schema TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'outstanding' CHECK (status IN ('outstanding','answered','consumed','retired')),
                response TEXT, PRIMARY KEY (instance_id, request_id)
            );
            CREATE INDEX IF NOT EXISTS input_requests_answered ON input_requests(instance_id) WHERE status='answered';
            CREATE TABLE IF NOT EXISTS instance_attachments (
                instance_id TEXT NOT NULL REFERENCES instances(id),
                request_id TEXT,
                ordinal BIGINT NOT NULL CHECK (ordinal >= 0),
                object_key TEXT NOT NULL,
                filename TEXT NOT NULL,
                media_type TEXT,
                byte_length BIGINT NOT NULL CHECK (byte_length >= 0),
                FOREIGN KEY (instance_id, request_id) REFERENCES input_requests(instance_id, request_id)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_start ON instance_attachments(instance_id, ordinal) WHERE request_id IS NULL;
            CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_reply ON instance_attachments(instance_id, request_id, ordinal) WHERE request_id IS NOT NULL;
            CREATE TABLE IF NOT EXISTS instance_interests (
                instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
                channel TEXT NOT NULL CHECK (octet_length(channel) BETWEEN 1 AND 63),
                expires_at TIMESTAMPTZ NOT NULL,
                PRIMARY KEY (instance_id, channel)
            );
            CREATE OR REPLACE FUNCTION notify_instance_interests() RETURNS trigger LANGUAGE plpgsql AS $$
            DECLARE changed_id TEXT;
            BEGIN
                IF TG_TABLE_NAME = 'instances' THEN
                    IF NEW.status NOT IN ('completed', 'business-error', 'cancelled', 'terminated', 'failed')
                        OR NEW.status = OLD.status THEN
                        RETURN NEW;
                    END IF;
                    changed_id := NEW.id;
                ELSE
                    IF TG_OP = 'INSERT' THEN
                        IF NEW.status <> 'outstanding' THEN RETURN NEW; END IF;
                    ELSIF OLD.status <> 'outstanding' OR NEW.status NOT IN ('answered', 'retired') THEN
                        RETURN NEW;
                    END IF;
                    changed_id := NEW.instance_id;
                END IF;
                PERFORM pg_notify(interest.channel, changed_id)
                    FROM instance_interests interest
                    WHERE interest.instance_id = changed_id AND interest.expires_at > clock_timestamp();
                RETURN NEW;
            END $$;
            DO $$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='input_requests'::regclass AND tgname='input_request_interests_notify') THEN
                    CREATE TRIGGER input_request_interests_notify
                        AFTER INSERT OR UPDATE OF status ON input_requests
                        FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
                END IF;
                IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='instances'::regclass AND tgname='instance_terminal_interests_notify') THEN
                    CREATE TRIGGER instance_terminal_interests_notify
                        AFTER UPDATE OF status ON instances
                        FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
                END IF;
            END $$;
            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY, heartbeat_at BIGINT NOT NULL, draining BOOLEAN NOT NULL DEFAULT FALSE
            );
            CREATE TABLE IF NOT EXISTS worker_capabilities (
                worker_id TEXT NOT NULL REFERENCES workers(id) ON DELETE CASCADE,
                namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL,
                PRIMARY KEY (worker_id, namespace, version, process)
            );
            CREATE TABLE IF NOT EXISTS process_policies (
                namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL,
                retry_policy TEXT, deadline_origin TEXT, deadline_duration_ms BIGINT,
                PRIMARY KEY (namespace, version, process)
            );
