use std::{sync::Arc, time::Duration};

use blkit_cli::postgres_admin::{Preparation, prepare_database};
use blkit_core::{
    Attachment, DeadlinePolicy, RetryPolicy,
    compiled_graph::{
        GraphDefinition, GraphLink, GraphNode, GraphNodeKind, RequestSchema, RequestType,
    },
    distributed::{DistributedControl, DistributedWorker},
    postgres_config::DatabaseRole,
    postgres_store::PostgresStore,
};
use tokio::time::{sleep, timeout};

#[path = "support/managed_postgres.rs"]
mod managed_postgres;
use managed_postgres::Database;

async fn stores(db: &Database) -> (PostgresStore, PostgresStore) {
    let settings = db.settings();
    prepare_database(&settings, Preparation::Init { role_prefix: None })
        .await
        .unwrap();
    let options = settings.connection_options().unwrap();
    let mut stores = Vec::new();
    for (role, expected) in [
        (DatabaseRole::Api, "blkit_api"),
        (DatabaseRole::Worker, "blkit_worker"),
    ] {
        let database = settings.resolve_role(role, &options).unwrap();
        let (client, driver) = database.connection().connect().await.unwrap();
        tokio::spawn(driver);
        assert_eq!(
            client
                .query_one("SELECT current_user", &[])
                .await
                .unwrap()
                .get::<_, String>(0),
            expected
        );
        assert!(
            !client
                .query_one(
                    "SELECT has_table_privilege(current_user, 'public.process_policies', 'UPDATE')",
                    &[]
                )
                .await
                .unwrap()
                .get::<_, bool>(0)
        );
        stores.push(database.connect_store(4).await.unwrap());
    }
    let worker = stores.pop().unwrap();
    (stores.pop().unwrap(), worker)
}

fn graph(name: &'static str, middle: GraphNodeKind) -> GraphDefinition {
    GraphDefinition {
        namespace: "roles",
        version: "1",
        name,
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(60),
            retry_delay: Duration::ZERO,
            backoff: "exponential",
        }),
        deadline: Some(DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_secs(300),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "action",
                kind: middle,
            },
            GraphNode {
                name: "end",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "action",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "action",
                target: "end",
                value: Some(Arc::new(|_, values| {
                    Ok(values
                        .get("action")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null))
                })),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

fn task_graph() -> GraphDefinition {
    graph(
        "task",
        GraphNodeKind::Task(Arc::new(|_, _| Ok(serde_json::json!(42)))),
    )
}

async fn wait_for_policy_inserts(db: &Database) {
    timeout(Duration::from_secs(5), async {
        loop {
            db.client.simple_query("SELECT pg_catalog.pg_stat_clear_snapshot()").await.unwrap();
            let blocked: i64 = db.client.query_one("SELECT count(*) FROM pg_stat_activity WHERE usename='blkit_worker' AND wait_event_type='Lock' AND query LIKE 'INSERT INTO process_policies%'", &[]).await.unwrap().get(0);
            if blocked == 2 { break; }
            sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("both real worker advertisements must reach the policy insert concurrently");
}

#[tokio::test]
async fn concurrent_identical_advertisements_need_no_policy_update_permission() {
    let db = Database::start().await;
    let (_, store) = stores(&db).await;
    let first =
        DistributedWorker::new(store.clone(), "first", vec![task_graph()], 1, 30_000).unwrap();
    let second = DistributedWorker::new(store, "second", vec![task_graph()], 1, 30_000).unwrap();
    db.client
        .batch_execute("BEGIN; LOCK TABLE public.process_policies IN SHARE MODE")
        .await
        .unwrap();
    let a = tokio::spawn(async move { first.advertise().await });
    let b = tokio::spawn(async move { second.advertise().await });
    wait_for_policy_inserts(&db).await;
    db.client.batch_execute("COMMIT").await.unwrap();
    assert_eq!(
        a.await.unwrap(),
        Ok(()),
        "immutable policy read must not require UPDATE"
    );
    assert_eq!(b.await.unwrap(), Ok(()));
    let row = db.client.query_one("SELECT (SELECT count(*) FROM workers), (SELECT count(*) FROM worker_capabilities), (SELECT count(*) FROM process_policies)", &[]).await.unwrap();
    assert_eq!(
        (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2)
        ),
        (2, 2, 1)
    );
}

#[tokio::test]
async fn concurrent_conflicting_advertisements_roll_back_worker_and_capabilities() {
    let db = Database::start().await;
    let (_, store) = stores(&db).await;
    let first =
        DistributedWorker::new(store.clone(), "first", vec![task_graph()], 1, 30_000).unwrap();
    let mut conflicting = task_graph();
    conflicting.deadline.as_mut().unwrap().duration = Duration::from_secs(301);
    let second =
        DistributedWorker::new(store.clone(), "second", vec![conflicting], 1, 30_000).unwrap();
    db.client
        .batch_execute("BEGIN; LOCK TABLE public.process_policies IN SHARE MODE")
        .await
        .unwrap();
    let a = tokio::spawn(async move { first.advertise().await });
    let b = tokio::spawn(async move { second.advertise().await });
    wait_for_policy_inserts(&db).await;
    db.client.batch_execute("COMMIT").await.unwrap();
    let outcomes = [a.await.unwrap(), b.await.unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap(),
        "conflicting policy for roles/1/task"
    );
    let winner = if outcomes[0].is_ok() {
        "first"
    } else {
        "second"
    };
    let loser = if winner == "first" { "second" } else { "first" };
    assert!(store.get_worker(loser).await.unwrap().is_none());
    assert_eq!(
        store
            .get_worker(winner)
            .await
            .unwrap()
            .unwrap()
            .identities
            .len(),
        1
    );
    let row = db.client.query_one("SELECT (SELECT count(*) FROM workers), (SELECT count(*) FROM worker_capabilities), (SELECT count(*) FROM process_policies)", &[]).await.unwrap();
    assert_eq!(
        (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2)
        ),
        (1, 1, 1)
    );
}

fn review_graph() -> GraphDefinition {
    graph(
        "review",
        GraphNodeKind::InputRequest {
            prompt: "Approve?",
            schema: RequestSchema {
                ports: [("approved".into(), RequestType::Named("Bool".into()))].into(),
                records: Default::default(),
                enums: Default::default(),
            },
            decode_response: Box::new(Ok),
        },
    )
}

fn attachment(id: &str, request: Option<&str>, ordinal: i64) -> Attachment {
    Attachment {
        instance_id: id.into(),
        request_id: request.map(str::to_owned),
        ordinal,
        object_key: format!("objects/{id}/{ordinal}"),
        filename: "proof.txt".into(),
        media_type: Some("text/plain".into()),
        byte_length: 5,
    }
}

async fn run(worker: &DistributedWorker) -> usize {
    timeout(Duration::from_secs(10), worker.run_once())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn provisioned_roles_execute_reply_resume_and_attachment_metadata_atomically() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let control = DistributedControl::new(api.clone());
    let first =
        DistributedWorker::new(store.clone(), "first", vec![review_graph()], 1, 30_000).unwrap();
    first.advertise().await.unwrap();
    assert!(control.can_start("roles", "1", "review").await.unwrap());
    assert!(!control.can_start("roles", "1", "absent").await.unwrap());
    assert!(
        control
            .start("roles", "1", "absent", serde_json::Value::Null)
            .await
            .is_err()
    );
    let start_file = attachment("review", None, 0);
    let id = control
        .start_with_attachments(
            "review".into(),
            "roles",
            "1",
            "review",
            serde_json::Value::Null,
            std::slice::from_ref(&start_file),
        )
        .await
        .unwrap();
    api.insert_attachment(&attachment(&id, None, 1))
        .await
        .unwrap();
    assert_eq!(
        store.attachments(&id, None).await.unwrap(),
        control.start_attachments(&id).await.unwrap()
    );
    assert_eq!(store.attachments(&id, None).await.unwrap().len(), 2);
    let (_, mut notified, _registration) = control.subscribe(&id).await.unwrap();
    assert_eq!(run(&first).await, 1);
    assert_eq!(
        timeout(Duration::from_secs(5), notified.recv())
            .await
            .unwrap()
            .unwrap(),
        serde_json::json!({"id": id}),
        "worker request publication must notify the API listener"
    );
    let waiting = control.status(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting-for-input");
    assert!(waiting.owner_id.is_none() && waiting.lease_until.is_none());
    assert_eq!(waiting.instance.attempt, 1);
    let request = &waiting.requests[0].id;
    let response = serde_json::json!({"approved": true});
    assert!(
        control
            .validate_reply(&id, request, &serde_json::json!({"approved": 1}))
            .await
            .is_err()
    );
    control
        .validate_reply(&id, request, &response)
        .await
        .unwrap();
    let file = attachment(&id, Some(request), 0);
    // Duplicate metadata fails after the response UPDATE: the entire reply must roll back.
    assert!(
        control
            .respond_with_attachments(
                &id,
                request,
                response.clone(),
                &[file.clone(), file.clone()]
            )
            .await
            .is_err()
    );
    assert_eq!(
        api.get(&id).await.unwrap().unwrap().instance.status,
        "waiting-for-input"
    );
    assert!(store.answers(&id).await.unwrap().is_empty());
    assert!(
        store
            .attachments(&id, Some(request))
            .await
            .unwrap()
            .is_empty()
    );
    control
        .validate_reply(&id, request, &response)
        .await
        .unwrap();
    control
        .respond_with_attachments(&id, request, response.clone(), std::slice::from_ref(&file))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(5), notified.recv())
            .await
            .unwrap()
            .unwrap(),
        serde_json::json!({"id": id}),
        "API response must notify through the invoker trigger"
    );
    assert_eq!(
        store.answers(&id).await.unwrap(),
        vec![(request.clone(), response.clone())]
    );
    assert_eq!(
        control.reply_attachments(&id, request).await.unwrap(),
        vec![file.clone()]
    );
    // A fresh worker identity/store resumes the persisted request checkpoint.
    let settings = db.settings();
    let options = settings.connection_options().unwrap();
    let reconnected = settings
        .resolve_role(DatabaseRole::Worker, &options)
        .unwrap()
        .connect_store(4)
        .await
        .unwrap();
    let second = DistributedWorker::new(
        reconnected.clone(),
        "second",
        vec![review_graph()],
        1,
        30_000,
    )
    .unwrap();
    second.advertise().await.unwrap();
    assert_eq!(run(&second).await, 1);
    let completed = control.status(&id).await.unwrap().unwrap();
    assert_eq!(completed.instance.status, "completed");
    assert_eq!(completed.instance.result, Some(response));
    assert_eq!(
        completed.instance.attempt, 1,
        "reply resumption is not a retry"
    );
    assert_eq!(completed.generation, waiting.generation + 1);
    assert!(store.answers(&id).await.unwrap().is_empty());
    assert_eq!(
        reconnected.attachments(&id, Some(request)).await.unwrap(),
        vec![file]
    );
    assert_eq!(
        db.client
            .query_one(
                "SELECT status FROM input_requests WHERE instance_id=$1 AND request_id=$2",
                &[&id, request]
            )
            .await
            .unwrap()
            .get::<_, String>(0),
        "consumed"
    );
    assert!(
        control
            .respond(&id, request, serde_json::json!({"approved": false}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn worker_request_checkpoint_failure_rolls_back_and_stale_generation_cannot_publish() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![review_graph()], 1, 30_000).unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(api)
        .start("roles", "1", "review", serde_json::Value::Null)
        .await
        .unwrap();
    let claimed = store
        .claim("worker", 1, 30_000)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let checkpoint = review_graph().checkpoint(&serde_json::Value::Null).unwrap();
    assert_eq!(checkpoint.requests().len(), 1);
    // Admin-owned fault injection only: reject request publication after checkpoint UPDATE.
    db.client.batch_execute("CREATE FUNCTION public.reject_runtime_request() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected request failure'; END $$; CREATE TRIGGER reject_runtime_request BEFORE INSERT ON public.input_requests FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_request()").await.unwrap();
    assert!(
        store
            .commit_initial_checkpoint(
                &id,
                "worker",
                claimed.generation,
                &serde_json::json!({"typed": true}),
                &checkpoint
            )
            .await
            .is_err()
    );
    let unchanged = store.get(&id).await.unwrap().unwrap();
    assert!(unchanged.instance.checkpoint.is_none());
    assert_eq!(unchanged.instance.input, serde_json::Value::Null);
    assert!(store.requests(&id).await.unwrap().is_empty());
    db.client.batch_execute("DROP TRIGGER reject_runtime_request ON public.input_requests; DROP FUNCTION public.reject_runtime_request()").await.unwrap();
    assert!(
        !store
            .commit_initial_checkpoint(
                &id,
                "worker",
                claimed.generation + 1,
                &serde_json::Value::Null,
                &checkpoint
            )
            .await
            .unwrap()
    );
    assert!(store.requests(&id).await.unwrap().is_empty());
    assert!(
        store
            .commit_initial_checkpoint(
                &id,
                "worker",
                claimed.generation,
                &serde_json::Value::Null,
                &checkpoint
            )
            .await
            .unwrap()
    );
    assert_eq!(store.requests(&id).await.unwrap().len(), 1);
    assert!(
        store
            .commit_checkpoint(&id, "worker", claimed.generation, &checkpoint)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn concurrent_claims_renew_checkpoint_finish_and_cancellation_keep_fencing() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let control = DistributedControl::new(api);
    let first =
        DistributedWorker::new(store.clone(), "first", vec![task_graph()], 1, 30_000).unwrap();
    let second =
        DistributedWorker::new(store.clone(), "second", vec![task_graph()], 1, 30_000).unwrap();
    first.advertise().await.unwrap();
    second.advertise().await.unwrap();
    let mut ids = Vec::new();
    for _ in 0..8 {
        ids.push(
            control
                .start("roles", "1", "task", serde_json::Value::Null)
                .await
                .unwrap(),
        );
    }
    let (a, b) = tokio::join!(
        store.claim("first", 8, 30_000),
        store.claim("second", 8, 30_000)
    );
    let claims: Vec<_> = a.unwrap().into_iter().chain(b.unwrap()).collect();
    let mut actual: Vec<_> = claims
        .iter()
        .map(|claim| claim.instance.id.clone())
        .collect();
    ids.sort();
    actual.sort();
    assert_eq!(
        actual, ids,
        "SKIP LOCKED must neither duplicate nor omit claims"
    );
    let checkpoint = task_graph().checkpoint(&serde_json::Value::Null).unwrap();
    for claim in claims {
        let id = &claim.instance.id;
        let owner = claim.owner_id.as_deref().unwrap();
        let generation = claim.generation;
        assert!(
            !store
                .renew_claim(id, owner, generation + 1, 30_000)
                .await
                .unwrap()
        );
        assert!(
            store
                .renew_claim(id, owner, generation, 60_000)
                .await
                .unwrap()
        );
        assert!(store.get(id).await.unwrap().unwrap().lease_until > claim.lease_until);
        assert!(
            store
                .commit_initial_checkpoint(
                    id,
                    owner,
                    generation,
                    &serde_json::Value::Null,
                    &checkpoint
                )
                .await
                .unwrap()
        );
        assert!(
            !store
                .commit_checkpoint(id, owner, generation + 1, &checkpoint)
                .await
                .unwrap()
        );
        assert!(
            store
                .commit_checkpoint(id, owner, generation, &checkpoint)
                .await
                .unwrap()
        );
        if id == &ids[0] {
            control.cancel(id).await.unwrap();
            control.cancel(id).await.unwrap();
            assert!(
                !store
                    .renew_claim(id, owner, generation, 30_000)
                    .await
                    .unwrap()
            );
            assert!(
                !store
                    .commit_checkpoint(id, owner, generation, &checkpoint)
                    .await
                    .unwrap()
            );
            assert!(
                !store
                    .finish_owned(
                        id,
                        owner,
                        generation,
                        "completed",
                        Some(serde_json::json!(42)),
                        None,
                        None
                    )
                    .await
                    .unwrap()
            );
            assert_eq!(
                control.status(id).await.unwrap().unwrap().instance.status,
                "cancelled"
            );
        } else {
            assert!(
                !store
                    .finish_owned(id, owner, generation + 1, "completed", None, None, None)
                    .await
                    .unwrap()
            );
            assert!(
                store
                    .finish_owned(
                        id,
                        owner,
                        generation,
                        "completed",
                        Some(serde_json::json!(42)),
                        None,
                        None
                    )
                    .await
                    .unwrap()
            );
            assert_eq!(
                control.status(id).await.unwrap().unwrap().instance.result,
                Some(serde_json::json!(42))
            );
        }
    }
}

#[tokio::test]
async fn api_cancellation_retires_worker_requests_and_prevents_reply_resume() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let control = DistributedControl::new(api);
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![review_graph()], 1, 30_000).unwrap();
    worker.advertise().await.unwrap();
    let id = control
        .start("roles", "1", "review", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(run(&worker).await, 1);
    let request = store.requests(&id).await.unwrap().pop().unwrap().id;
    control.cancel(&id).await.unwrap();
    assert!(store.requests(&id).await.unwrap().is_empty());
    assert!(
        control
            .respond(&id, &request, serde_json::json!({"approved": true}))
            .await
            .is_err()
    );
    assert_eq!(run(&worker).await, 0);
    assert_eq!(
        control.status(&id).await.unwrap().unwrap().instance.status,
        "cancelled"
    );
}

#[tokio::test]
async fn both_roles_reconcile_expired_leases_retry_exhaustion_and_deadlines() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let admission = DistributedControl::new(api.clone());
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![review_graph()], 1, 30_000).unwrap();
    worker.advertise().await.unwrap();
    for reconciler in [
        DistributedControl::new(api),
        DistributedControl::new(store.clone()),
    ] {
        let id = admission
            .start("roles", "1", "review", serde_json::Value::Null)
            .await
            .unwrap();
        let checkpoint = review_graph().checkpoint(&serde_json::Value::Null).unwrap();
        let first = store
            .claim("worker", 1, 30_000)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            store
                .commit_initial_checkpoint(
                    &id,
                    "worker",
                    first.generation,
                    &serde_json::Value::Null,
                    &checkpoint
                )
                .await
                .unwrap()
        );
        db.client
            .execute("UPDATE instances SET lease_until=0 WHERE id=$1", &[&id])
            .await
            .unwrap();
        assert_eq!(reconciler.reconcile_once().await.unwrap(), 1);
        let retry = store.get(&id).await.unwrap().unwrap();
        assert_eq!(retry.instance.status, "retry-waiting");
        assert!(retry.owner_id.is_none() && retry.lease_until.is_none());
        assert_eq!(store.requests(&id).await.unwrap().len(), 1);
        let second = store
            .claim("worker", 1, 30_000)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(second.generation, first.generation + 1);
        assert_eq!(second.instance.attempt, 2);
        assert!(
            !store
                .commit_checkpoint(&id, "worker", first.generation, &checkpoint)
                .await
                .unwrap()
        );
        db.client
            .execute("UPDATE instances SET lease_until=0 WHERE id=$1", &[&id])
            .await
            .unwrap();
        assert_eq!(reconciler.reconcile_once().await.unwrap(), 1);
        let failed = store.get(&id).await.unwrap().unwrap();
        assert_eq!(failed.instance.status, "failed");
        assert_eq!(
            failed.instance.error.as_deref(),
            Some("owner lease expired")
        );
        assert!(store.requests(&id).await.unwrap().is_empty());
        assert_eq!(reconciler.reconcile_once().await.unwrap(), 0);
        let deadline_id = admission
            .start("roles", "1", "review", serde_json::Value::Null)
            .await
            .unwrap();
        assert_eq!(run(&worker).await, 1);
        assert_eq!(store.requests(&deadline_id).await.unwrap().len(), 1);
        db.client
            .execute(
                "UPDATE instances SET deadline_at_ms=0 WHERE id=$1",
                &[&deadline_id],
            )
            .await
            .unwrap();
        assert_eq!(reconciler.reconcile_once().await.unwrap(), 1);
        let timed_out = store.get(&deadline_id).await.unwrap().unwrap();
        assert_eq!(timed_out.instance.status, "business-error");
        assert_eq!(timed_out.instance.terminal_name.as_deref(), Some("timeout"));
        assert!(store.requests(&deadline_id).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn both_roles_resume_durable_waits_without_incrementing_attempt() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let admission = DistributedControl::new(api.clone());
    let worker = DistributedWorker::new(
        store.clone(),
        "worker",
        vec![graph(
            "wait",
            GraphNodeKind::PauseFor(Duration::from_millis(500)),
        )],
        1,
        30_000,
    )
    .unwrap();
    worker.advertise().await.unwrap();
    for reconciler in [
        DistributedControl::new(api),
        DistributedControl::new(store.clone()),
    ] {
        let id = admission
            .start("roles", "1", "wait", serde_json::Value::Null)
            .await
            .unwrap();
        assert_eq!(run(&worker).await, 1);
        let waiting = store.get(&id).await.unwrap().unwrap();
        assert_eq!(waiting.instance.status, "waiting");
        assert!(waiting.owner_id.is_none() && waiting.lease_until.is_none());
        timeout(Duration::from_secs(5), async {
            while !db
                .client
                .query_one(
                    "SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT >= $1",
                    &[&waiting.instance.wake_at_ms.unwrap()],
                )
                .await
                .unwrap()
                .get::<_, bool>(0)
            {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(store.due_waits(i64::MAX).await.unwrap().len(), 1);
        assert_eq!(reconciler.reconcile_once().await.unwrap(), 1);
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().instance.status,
            "pending"
        );
        assert_eq!(run(&worker).await, 1);
        let completed = store.get(&id).await.unwrap().unwrap();
        assert_eq!(completed.instance.status, "completed");
        assert_eq!(completed.instance.attempt, 1);
        assert_eq!(completed.generation, waiting.generation + 1);
    }
}

#[tokio::test]
async fn worker_failure_retries_then_commits_failed_outcome_under_worker_login() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let worker = DistributedWorker::new(
        store.clone(),
        "worker",
        vec![graph(
            "failure",
            GraphNodeKind::Task(Arc::new(|_, _| Err("task failed".into()))),
        )],
        1,
        30_000,
    )
    .unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(api)
        .start("roles", "1", "failure", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(run(&worker).await, 2);
    let failed = store.get(&id).await.unwrap().unwrap();
    assert_eq!(failed.instance.status, "failed");
    assert_eq!(failed.instance.attempt, 2);
    assert_eq!(failed.instance.error.as_deref(), Some("task failed"));
    assert!(failed.owner_id.is_none() && failed.lease_until.is_none());
}

async fn wait_for_blocked_worker(db: &Database, pattern: &str) {
    timeout(Duration::from_secs(5), async {
        loop {
            db.client.simple_query("SELECT pg_catalog.pg_stat_clear_snapshot()").await.unwrap();
            let count: i64 = db.client.query_one("SELECT count(*) FROM pg_stat_activity WHERE usename='blkit_worker' AND wait_event_type='Lock' AND query LIKE $1", &[&pattern]).await.unwrap().get(0);
            if count == 1 { break; }
            sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("worker operation must reach the intended lock boundary");
}

#[tokio::test]
async fn drain_waits_for_inflight_claim_and_unregister_waits_for_owned_work() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let control = DistributedControl::new(api);
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![task_graph()], 1, 30_000).unwrap();
    worker.advertise().await.unwrap();
    let id = control
        .start("roles", "1", "task", serde_json::Value::Null)
        .await
        .unwrap();
    assert!(!worker.drain_if_requested().await.unwrap());
    assert!(!store.unregister_drained("worker").await.unwrap());
    // Hold claim after its worker SHARE lock, before its instance locks.
    db.client
        .batch_execute("BEGIN; LOCK TABLE public.instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let claimant = store.clone();
    let claim = tokio::spawn(async move { claimant.claim("worker", 1, 30_000).await });
    wait_for_blocked_worker(&db, "SELECT i.id, i.checkpoint FROM instances i%").await;
    let draining = store.clone();
    let drain = tokio::spawn(async move { draining.drain("worker").await });
    wait_for_blocked_worker(&db, "UPDATE workers SET draining=TRUE WHERE id=$1").await;
    assert!(
        !drain.is_finished(),
        "drain cannot acknowledge an in-flight claim"
    );
    db.client.batch_execute("COMMIT").await.unwrap();
    let claimed = claim.await.unwrap().unwrap().pop().unwrap();
    assert!(drain.await.unwrap().unwrap());
    assert!(store.claim("worker", 1, 30_000).await.unwrap().is_empty());
    assert!(!control.can_start("roles", "1", "task").await.unwrap());
    assert!(
        control
            .start("roles", "1", "task", serde_json::Value::Null)
            .await
            .is_err()
    );
    assert!(!store.unregister_drained("worker").await.unwrap());
    assert!(worker.drain_if_requested().await.is_err());
    assert!(
        store
            .finish_owned(
                &id,
                "worker",
                claimed.generation,
                "completed",
                None,
                None,
                None
            )
            .await
            .unwrap()
    );
    assert!(worker.drain_if_requested().await.unwrap());
    assert!(store.get_worker("worker").await.unwrap().is_none());
    assert_eq!(
        db.client
            .query_one(
                "SELECT count(*) FROM worker_capabilities WHERE worker_id='worker'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, i64>(0),
        0
    );
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM process_policies", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1,
        "unregister must not delete immutable policy"
    );
}

async fn listener_pid(db: &Database, previous: Option<i32>) -> i32 {
    timeout(Duration::from_secs(5), async {
        loop {
            db.client.simple_query("SELECT pg_catalog.pg_stat_clear_snapshot()").await.unwrap();
            if let Some(row) = db.client.query_opt("SELECT pid FROM pg_stat_activity WHERE usename='blkit_api' AND application_name LIKE 'blkit_%' AND ($1::int IS NULL OR pid <> $1)", &[&previous]).await.unwrap() {
                break row.get(0);
            }
            sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("dedicated listener must reconnect as the provisioned API login")
}

#[tokio::test]
async fn api_sse_interest_renewal_worker_notification_and_listener_reconnect_resync() {
    let db = Database::start().await;
    let (api, store) = stores(&db).await;
    let control = DistributedControl::new(api);
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![task_graph()], 1, 30_000).unwrap();
    worker.advertise().await.unwrap();
    let id = control
        .start("roles", "1", "task", serde_json::Value::Null)
        .await
        .unwrap();
    let (snapshot, mut receiver, registration) = control.subscribe(&id).await.unwrap();
    assert_eq!(snapshot["status"], "pending");
    assert!(snapshot.get("checkpoint").is_none());
    let pid = listener_pid(&db, None).await;
    // Force expiry; the API listener must renew the interest before worker notification.
    db.client.execute("UPDATE instance_interests SET expires_at=clock_timestamp()-interval '1 second' WHERE instance_id=$1", &[&id]).await.unwrap();
    timeout(Duration::from_secs(6), async {
        while !db
            .client
            .query_one(
                "SELECT expires_at>clock_timestamp() FROM instance_interests WHERE instance_id=$1",
                &[&id],
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
        {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("API listener must renew expired interests using operational grants");
    // Renewal can send a persisted snapshot; isolate the subsequent terminal hint.
    while receiver.try_recv().is_ok() {}
    assert_eq!(run(&worker).await, 1);
    timeout(Duration::from_secs(5), async {
        loop {
            let value = receiver.recv().await.unwrap();
            if value == serde_json::json!({"id": id}) {
                break;
            }
        }
    })
    .await
    .expect("worker invoker trigger must SELECT API interests and notify API listener");
    assert_eq!(
        control.status(&id).await.unwrap().unwrap().instance.result,
        Some(serde_json::json!(42))
    );
    // Block reconnect resynchronization and prove readiness is not announced early.
    db.client.batch_execute("BEGIN").await.unwrap();
    db.client
        .query_one("SELECT id FROM instances WHERE id=$1 FOR UPDATE", &[&id])
        .await
        .unwrap();
    assert!(
        db.client
            .query_one("SELECT pg_catalog.pg_terminate_backend($1)", &[&pid])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let replacement = listener_pid(&db, Some(pid)).await;
    assert_ne!(replacement, pid);
    assert!(
        control.subscribe(&id).await.is_err(),
        "locked resync must not publish readiness"
    );
    db.client.batch_execute("COMMIT").await.unwrap();
    let resync = timeout(Duration::from_secs(6), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resync["status"], "completed");
    assert_eq!(resync["result"], serde_json::json!(42));
    assert!(resync.get("checkpoint").is_none());
    let (snapshot, reconnected, second_registration) = control.subscribe(&id).await.unwrap();
    assert_eq!(snapshot["status"], "completed");
    drop((receiver, reconnected, registration, second_registration));
    timeout(Duration::from_secs(5), async {
        while db
            .client
            .query_one(
                "SELECT count(*) FROM instance_interests WHERE instance_id=$1",
                &[&id],
            )
            .await
            .unwrap()
            .get::<_, i64>(0)
            != 0
        {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("last subscriber must remove its API-owned interest");
}
