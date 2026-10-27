#[path = "../../test-support/managed_runtime.rs"]
mod managed_runtime;
#[path = "../../../crates/blkit-cli/tests/support/postgres.rs"]
mod postgres_fixture;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
extern crate blkit_core as blkit;
use blkit::{
    RetryPolicy,
    compiled_graph::{GraphCheckpoint, GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
    distributed::{DistributedControl, DistributedWorker},
    postgres_store::PostgresStore,
    runtime::{Evaluate, Instance},
    server::router_distributed,
};
use serde_json::json;
use std::{
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};
use tokio_postgres::NoTls;
use tower::ServiceExt;

#[path = "../../test-support/seaweed.rs"]
mod seaweed;

#[tokio::test]
async fn postgres_pool_allows_independent_operation_while_another_session_is_blocked() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_pool_independent"),
        2,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let lock = gate.transaction().await.unwrap();
    lock.batch_execute("LOCK TABLE instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let blocked = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .create(&Instance::new("blocked", "test", "1", "pool", json!(null)))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: i64 = lock
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock'",
                    &[&"blkit_pool_independent"],
                )
                .await
                .unwrap()
                .get(0);
            if waiting == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first operation must be blocked inside PostgreSQL");
    assert!(
        tokio::time::timeout(Duration::from_millis(500), store.get_worker("absent"))
            .await
            .expect("independent operation must finish before the blocked SQL is released")
            .unwrap()
            .is_none()
    );
    assert!(!blocked.is_finished());
    lock.commit().await.unwrap();
    blocked.await.unwrap().unwrap();
    assert!(store.get("blocked").await.unwrap().is_some());
}

#[tokio::test]
async fn postgres_pool_replaces_terminated_backend_without_acknowledging_its_write() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_pool_replacement"),
        1,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let lock = gate.transaction().await.unwrap();
    lock.batch_execute("LOCK TABLE instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let blocked = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .create(&Instance::new(
                    "lost-write",
                    "test",
                    "1",
                    "pool",
                    json!(null),
                ))
                .await
        }
    });
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            lock.batch_execute("SELECT pg_stat_clear_snapshot()")
                .await
                .unwrap();
            let row = lock
                .query_opt(
                    "SELECT pid FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock'",
                    &[&"blkit_pool_replacement"],
                )
                .await
                .unwrap();
            if let Some(row) = row {
                break row.get::<_, i32>(0);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("write must reach the database before terminating its backend");
    assert!(
        lock.query_one("SELECT pg_terminate_backend($1)", &[&pid])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let error = tokio::time::timeout(Duration::from_secs(5), blocked)
        .await
        .expect("lost backend must not leave the operation waiting")
        .unwrap()
        .expect_err("lost write must not be acknowledged");
    assert!(!error.is_empty());
    lock.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        // A fast-recycled dead session may report one more error before replacement.
        if let Ok(row) = store.get("lost-write").await {
            assert!(row.is_none());
        }
        assert!(store.get("lost-write").await.unwrap().is_none());
        store
            .create(&Instance::new(
                "replacement",
                "test",
                "1",
                "pool",
                json!(null),
            ))
            .await
            .unwrap();
        assert!(store.get("replacement").await.unwrap().is_some());
        let replacement_pid: i32 = gate
            .query_one(
                "SELECT pid FROM pg_stat_activity WHERE application_name=$1",
                &[&"blkit_pool_replacement"],
            )
            .await
            .unwrap()
            .get(0);
        assert_ne!(
            replacement_pid, pid,
            "pool must replace the terminated backend"
        );
    })
    .await
    .expect("replacement backend must restore the only pool slot");
}

#[tokio::test]
async fn postgres_pool_limits_sessions_under_concurrent_load() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_pool_bounded"),
        2,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let lock = gate.transaction().await.unwrap();
    lock.batch_execute("LOCK TABLE instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let mut operations = Vec::new();
    for index in 0..2 {
        let store = store.clone();
        operations.push(tokio::spawn(async move {
            let id = format!("load-{index}");
            store
                .create(&Instance::new(&id, "test", "1", "pool", json!(null)))
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            lock.batch_execute("SELECT pg_stat_clear_snapshot()")
                .await
                .unwrap();
            let waiting: i64 = lock
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock'",
                    &[&"blkit_pool_bounded"],
                )
                .await
                .unwrap()
                .get(0);
            if waiting == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both allowed sessions must concurrently reach PostgreSQL");
    let (started, mut contending) = tokio::sync::mpsc::channel(6);
    for index in 2..8 {
        let store = store.clone();
        let started = started.clone();
        operations.push(tokio::spawn(async move {
            started.send(()).await.unwrap();
            let id = format!("load-{index}");
            store
                .create(&Instance::new(&id, "test", "1", "pool", json!(null)))
                .await
        }));
    }
    drop(started);
    for _ in 0..6 {
        contending.recv().await.expect("all contenders must start");
    }
    for _ in 0..6 {
        lock.batch_execute("SELECT pg_stat_clear_snapshot()")
            .await
            .unwrap();
        let sessions: i64 = lock
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1",
                &[&"blkit_pool_bounded"],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            sessions, 2,
            "contending operations must not open extra sessions"
        );
        assert!(operations.iter().all(|operation| !operation.is_finished()));
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    lock.commit().await.unwrap();
    for operation in operations {
        operation.await.unwrap().unwrap();
    }
    for index in 0..8 {
        assert!(store.get(&format!("load-{index}")).await.unwrap().is_some());
    }
}

#[tokio::test]
async fn postgres_pool_checkout_times_out_when_only_slot_is_held() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_pool_timeout"),
        1,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let lock = gate.transaction().await.unwrap();
    lock.batch_execute("LOCK TABLE instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let blocked = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .create(&Instance::new("blocked", "test", "1", "pool", json!(null)))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: i64 = lock
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock'",
                    &[&"blkit_pool_timeout"],
                )
                .await
                .unwrap()
                .get(0);
            if waiting == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the only slot must be occupied by a blocked operation");
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        store.create(&Instance::new(
            "timed-out",
            "test",
            "1",
            "pool",
            json!(null),
        )),
    )
    .await
    .expect("checkout must return before the outer deadline")
    .expect_err("a saturated pool must reject the write");
    assert!(
        error.to_lowercase().contains("timeout") || error.to_lowercase().contains("timed out"),
        "checkout must report a timeout, got: {error}"
    );
    assert!(!blocked.is_finished());
    lock.commit().await.unwrap();
    blocked.await.unwrap().unwrap();
    assert!(store.get("blocked").await.unwrap().is_some());
    assert!(store.get("timed-out").await.unwrap().is_none());
    store
        .create(&Instance::new(
            "recovered",
            "test",
            "1",
            "pool",
            json!(null),
        ))
        .await
        .unwrap();
    assert!(store.get("recovered").await.unwrap().is_some());
}

fn distributed_batch_graph(task: Evaluate) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "batch",
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(10),
            backoff: "exponential",
        }),
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "items",
                kind: GraphNodeKind::MultiInstance {
                    task,
                    items: Arc::new(|input, _| Ok(input.clone())),
                    parallel: false,
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "items",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "items",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["items"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn distributed_admission_requires_a_live_exact_capability_and_queues_after_worker_exit() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = || distributed_batch_graph(Arc::new(|value, _| Ok(value.clone())));
    let control = DistributedControl::new(store.clone());
    assert!(
        control
            .start("test", "1", "batch", json!([1]))
            .await
            .unwrap_err()
            .contains("unavailable")
    );

    let mut other = graph();
    other.version = "2";
    let wrong = DistributedWorker::new(store.clone(), "wrong", vec![other], 1, 1_000).unwrap();
    wrong.advertise().await.unwrap();
    assert!(
        control
            .start("test", "1", "batch", json!([1]))
            .await
            .unwrap_err()
            .contains("unavailable")
    );

    let matching =
        DistributedWorker::new(store.clone(), "matching", vec![graph()], 1, 1_000).unwrap();
    matching.advertise().await.unwrap();
    let id = control
        .start("test", "1", "batch", json!([1]))
        .await
        .unwrap();
    store.drain("matching").await.unwrap();
    assert!(
        control
            .start("test", "1", "batch", json!([2]))
            .await
            .unwrap_err()
            .contains("unavailable")
    );
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().instance.status,
        "pending"
    );
    store.unregister_drained("matching").await.unwrap();
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().instance.status,
        "pending"
    );

    let fresh = DistributedWorker::new(store.clone(), "fresh", vec![graph()], 1, 1_000).unwrap();
    fresh.advertise().await.unwrap();
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .execute("UPDATE workers SET heartbeat_at=0 WHERE id='fresh'", &[])
        .await
        .unwrap();
    assert!(
        control
            .start("test", "1", "batch", json!([3]))
            .await
            .unwrap_err()
            .contains("unavailable")
    );
}

#[tokio::test]
async fn worker_registration_rejects_conflicting_policy_and_queue_deadline_survives_exit() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = |ms| {
        let mut graph = distributed_batch_graph(Arc::new(|value, _| Ok(value.clone())));
        graph.deadline = Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(ms),
        });
        graph
    };
    let matching =
        DistributedWorker::new(store.clone(), "matching", vec![graph(50)], 1, 1_000).unwrap();
    matching.advertise().await.unwrap();
    let conflict =
        DistributedWorker::new(store.clone(), "conflict", vec![graph(500)], 1, 1_000).unwrap();
    assert!(conflict.advertise().await.is_err());
    assert!(store.get_worker("conflict").await.unwrap().is_none());
    let mut huge_graph = graph(i64::MAX as u64);
    huge_graph.version = "3";
    let huge = DistributedWorker::new(store.clone(), "huge", vec![huge_graph], 1, 1_000).unwrap();
    assert!(
        huge.advertise()
            .await
            .unwrap_err()
            .contains("deadline duration")
    );
    assert!(store.get_worker("huge").await.unwrap().is_none());
    let control = DistributedControl::new(store.clone());
    let id = control
        .start("test", "1", "batch", json!([1]))
        .await
        .unwrap();
    store.drain("matching").await.unwrap();
    store.unregister_drained("matching").await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    control.reconcile_once().await.unwrap();
    let result = store.get(&id).await.unwrap().unwrap().instance;
    assert_eq!(result.status, "business-error");
    assert_eq!(result.terminal_name.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn concurrent_workers_can_advertise_identical_process_policies() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let first_store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let second_store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (mut client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let guard = client.transaction().await.unwrap();
    guard
        .batch_execute("LOCK TABLE process_policies IN SHARE MODE")
        .await
        .unwrap();
    let (observer, observer_connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = observer_connection.await;
    });
    let graph = || distributed_batch_graph(Arc::new(|value, _| Ok(value.clone())));
    let first =
        DistributedWorker::new(first_store.clone(), "first", vec![graph()], 1, 1_000).unwrap();
    let second =
        DistributedWorker::new(second_store.clone(), "second", vec![graph()], 1, 1_000).unwrap();
    let first = tokio::spawn(async move { first.advertise().await });
    let second = tokio::spawn(async move { second.advertise().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: i64 = observer.query_one("SELECT COUNT(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE 'INSERT INTO process_policies%'", &[]).await.unwrap().get(0);
            if waiting == 2 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("both registration transactions should reach policy insert");
    guard.commit().await.unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert!(first_store.get_worker("first").await.unwrap().is_some());
    assert!(second_store.get_worker("second").await.unwrap().is_some());
}

#[tokio::test]
async fn distributed_worker_validates_admitted_input_on_claim() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut graph = distributed_batch_graph(Arc::new(|value, _| Ok(value.clone())));
    graph.decode_input = Box::new(|input| {
        if input.is_number() {
            Ok(input)
        } else {
            Err("expected number".into())
        }
    });
    let worker = DistributedWorker::new(store.clone(), "typed", vec![graph], 1, 1_000).unwrap();
    worker.advertise().await.unwrap();
    let api = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let request = |body| {
        Request::builder()
            .method("POST")
            .uri("/processes/test/1/batch/instances")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    assert_eq!(
        api.clone().oneshot(request("{")).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let accepted = api
        .clone()
        .oneshot(request(r#"{"invalid":true}"#))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let response: serde_json::Value =
        serde_json::from_slice(&to_bytes(accepted.into_body(), 4096).await.unwrap()).unwrap();
    let id = response["id"].as_str().unwrap().to_string();
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().instance.status,
        "pending"
    );
    worker.run_once().await.unwrap();
    let result = store.get(&id).await.unwrap().unwrap().instance;
    assert_eq!(result.status, "failed");
    assert!(result.error.unwrap().contains("expected number"));
    assert_eq!(
        worker.run_once().await.unwrap(),
        0,
        "typed failures must not retry"
    );
    let status = api
        .oneshot(
            Request::builder()
                .uri(format!("/instances/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
}

#[tokio::test]
async fn distributed_recovery_rejects_old_checkpoints_without_changing_rows() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = distributed_batch_graph(Arc::new(|input, _| Ok(input.clone())));
    let mut json = serde_json::to_value(graph.checkpoint(&json!([1])).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("version");
    let old: GraphCheckpoint = serde_json::from_value(json).unwrap();
    for (id, status) in [("old-pending", "pending"), ("old-waiting", "waiting")] {
        let mut item = Instance::new(id, "test", "1", "batch", json!([1]));
        item.status = status.into();
        item.wake_at_ms = (status == "waiting").then_some(0);
        item.checkpoint = Some(old.clone());
        store
            .create_with_policy(&item, graph.retry.as_ref())
            .await
            .unwrap();
    }
    let pending = serde_json::to_value(store.get("old-pending").await.unwrap()).unwrap();
    let waiting = serde_json::to_value(store.get("old-waiting").await.unwrap()).unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "legacy-worker", vec![graph], 1, 1_000).unwrap();
    worker.advertise().await.unwrap();
    assert!(
        store
            .claim("legacy-worker", 1, 1_000)
            .await
            .err()
            .unwrap()
            .contains("unsupported checkpoint version")
    );
    assert!(
        worker
            .run_once()
            .await
            .unwrap_err()
            .contains("unsupported checkpoint version")
    );
    let control = DistributedControl::new(store.clone());
    assert!(
        control
            .reconcile_once()
            .await
            .unwrap_err()
            .contains("unsupported checkpoint version")
    );
    assert_eq!(
        serde_json::to_value(store.get("old-pending").await.unwrap()).unwrap(),
        pending
    );
    assert_eq!(
        serde_json::to_value(store.get("old-waiting").await.unwrap()).unwrap(),
        waiting
    );
}

#[tokio::test]
async fn distributed_takeover_resumes_uncommitted_multi_instance_item() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let visits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let slow: Evaluate = {
        let (visits, entered, release) = (visits.clone(), entered.clone(), release.clone());
        Arc::new(move |item, _| {
            visits.lock().unwrap().push(item.as_i64().unwrap());
            if item == 2 {
                entered.store(true, Ordering::SeqCst);
                while !release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(item.clone())
        })
    };
    let fast: Evaluate = {
        let visits = visits.clone();
        Arc::new(move |item, _| {
            visits.lock().unwrap().push(item.as_i64().unwrap());
            Ok(item.clone())
        })
    };
    let control = DistributedControl::new(store.clone());
    let first = DistributedWorker::new(
        store.clone(),
        "batch-first",
        vec![distributed_batch_graph(slow)],
        1,
        120,
    )
    .unwrap();
    first.advertise().await.unwrap();
    let id = control
        .start("test", "1", "batch", json!([1, 2, 3]))
        .await
        .unwrap();
    let owner = tokio::spawn(async move { first.run_once().await });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = control.status(&id).await.unwrap().unwrap();
            let checkpoint = serde_json::to_value(item.instance.checkpoint).unwrap();
            if entered.load(Ordering::SeqCst)
                && checkpoint["multi"]
                    .as_object()
                    .is_some_and(|batches| batches.values().any(|batch| batch["results"][0] == 1))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    owner.abort();
    release.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(control.reconcile_once().await.unwrap(), 1);
    let second = DistributedWorker::new(
        store.clone(),
        "batch-second",
        vec![distributed_batch_graph(fast)],
        1,
        5000,
    )
    .unwrap();
    second.advertise().await.unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    assert_eq!(second.run_once().await.unwrap(), 1);
    let result = control.status(&id).await.unwrap().unwrap();
    assert_eq!(result.instance.status, "completed");
    assert_eq!(result.instance.result, Some(json!([1, 2, 3])));
    assert_eq!(*visits.lock().unwrap(), vec![1, 2, 2, 3]);
    drop(node);
}

fn distributed_cycle_graph(task: Evaluate) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "cycle",
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(10),
            backoff: "exponential",
        }),
        deadline: Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_secs(2),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "gate",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(task),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "xor",
                    split: "gate",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::Error,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "work",
                value: None,
                condition: Some(Arc::new(|_, values| {
                    Ok(json!(
                        values.get("joined").and_then(|v| v.as_i64()).unwrap_or(0) < 2
                    ))
                })),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "done",
                value: None,
                condition: None,
                fallback: true,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn distributed_takeover_keeps_prior_cycle_activation_committed() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let visited = Arc::new(std::sync::Mutex::new(Vec::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let slow: Evaluate = {
        let (visited, entered, release) = (visited.clone(), entered.clone(), release.clone());
        Arc::new(move |_, values| {
            let number = values.get("joined").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
            visited.lock().unwrap().push(number);
            if number == 2 {
                entered.store(true, Ordering::SeqCst);
                while !release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(json!(number))
        })
    };
    let fast: Evaluate = {
        let visited = visited.clone();
        Arc::new(move |_, values| {
            let number = values.get("joined").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
            visited.lock().unwrap().push(number);
            Ok(json!(number))
        })
    };
    let control = DistributedControl::new(store.clone());
    let first = DistributedWorker::new(
        store.clone(),
        "cycle-first",
        vec![distributed_cycle_graph(slow)],
        1,
        120,
    )
    .unwrap();
    first.advertise().await.unwrap();
    let id = control
        .start("test", "1", "cycle", json!(null))
        .await
        .unwrap();
    let owner = tokio::spawn(async move { first.run_once().await });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = control.status(&id).await.unwrap().unwrap();
            if entered.load(Ordering::SeqCst)
                && item
                    .instance
                    .checkpoint
                    .as_ref()
                    .is_some_and(|checkpoint| checkpoint.completed["work"] == json!(1))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    owner.abort();
    release.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(control.reconcile_once().await.unwrap(), 1);
    let second = DistributedWorker::new(
        store.clone(),
        "cycle-second",
        vec![distributed_cycle_graph(fast)],
        1,
        5000,
    )
    .unwrap();
    second.advertise().await.unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    assert_eq!(second.run_once().await.unwrap(), 1);
    let result = control.status(&id).await.unwrap().unwrap();
    assert_eq!(result.instance.status, "business-error");
    assert_eq!(result.instance.terminal_name.as_deref(), Some("done"));
    assert_eq!(*visited.lock().unwrap(), vec![1, 2, 2]);
    drop(node);
}

#[tokio::test]
async fn distributed_pending_branch_is_claimed_despite_parallel_wait() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let graph = || {
        let hits = hits.clone();
        let mut nodes = vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "pause",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(500)),
            },
            GraphNode {
                name: "task",
                kind: GraphNodeKind::Task(Arc::new(move |input, _| {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Ok(input.clone())
                })),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "fork",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ];
        let value: Evaluate = Arc::new(|input, _| Ok(input.clone()));
        let edge = |source, target, value, label| GraphLink {
            source,
            target,
            value,
            condition: None,
            fallback: false,
            label,
        };
        let mut links = vec![
            edge("start", "fork", None, None),
            edge("fork", "pause", None, Some("left")),
            edge("pause", "joined", Some(value.clone()), None),
            edge("fork", "split_0", None, Some("right")),
            edge("task", "joined", Some(value.clone()), None),
            edge(
                "joined",
                "done",
                Some(Arc::new(
                    |_: &serde_json::Value, values: &blkit::runtime::Values| {
                        Ok(values["joined"].clone())
                    },
                )),
                None,
            ),
        ];
        for i in 0..40 {
            let split: &'static str = Box::leak(format!("split_{i}").into_boxed_str());
            let join: &'static str = Box::leak(format!("join_{i}").into_boxed_str());
            let next: &'static str = if i == 39 {
                "task"
            } else {
                Box::leak(format!("split_{}", i + 1).into_boxed_str())
            };
            nodes.push(GraphNode {
                name: split,
                kind: GraphNodeKind::Split("and"),
            });
            nodes.push(GraphNode {
                name: join,
                kind: GraphNodeKind::Join { kind: "and", split },
            });
            links.push(edge(split, join, Some(value.clone()), Some("only")));
            links.push(edge(join, next, None, None));
        }
        GraphDefinition {
            namespace: "test",
            version: "1",
            name: "pending_wait",
            retry: None,
            deadline: Some(blkit::DeadlinePolicy {
                origin: "first_claimed",
                duration: Duration::from_secs(1),
            }),
            decode_input: Box::new(Ok),
            nodes,
            links,
        }
    };
    let worker =
        DistributedWorker::new(store.clone(), "pending-worker", vec![graph()], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let control = DistributedControl::new(store.clone());
    let id = control
        .start("test", "1", "pending_wait", json!(5))
        .await
        .unwrap();
    assert_eq!(
        control.status(&id).await.unwrap().unwrap().instance.status,
        "pending"
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), worker.run_once())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let waiting = control.status(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting");
    assert!(waiting.instance.first_claim_at_ms.is_some());
    control.cancel(&id).await.unwrap();
    drop(node);
}

#[tokio::test]
async fn distributed_task_free_cycle_starts_before_first_claim_deadline() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = || GraphDefinition {
        namespace: "test",
        version: "1",
        name: "task_free",
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin: "first_claimed",
            duration: Duration::from_millis(80),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "gate",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "xor",
                    split: "gate",
                },
            },
            GraphNode {
                name: "stop",
                kind: GraphNodeKind::Error,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "joined",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
                condition: Some(Arc::new(|input, _| Ok(json!(input.as_i64().unwrap() > 0)))),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "stop",
                value: None,
                condition: None,
                fallback: true,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let worker =
        DistributedWorker::new(store.clone(), "task-free-worker", vec![graph()], 1, 300).unwrap();
    worker.advertise().await.unwrap();
    let control = DistributedControl::new(store.clone());
    let id = control
        .start("test", "1", "task_free", json!(1))
        .await
        .unwrap();
    assert!(
        control
            .status(&id)
            .await
            .unwrap()
            .unwrap()
            .instance
            .deadline_at_ms
            .is_none()
    );
    let running = tokio::spawn(async move { worker.run_once().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while control
            .status(&id)
            .await
            .unwrap()
            .unwrap()
            .instance
            .first_claim_at_ms
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.reconcile_once().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let saved = control.status(&id).await.unwrap().unwrap().instance;
    assert_eq!(saved.status, "business-error");
    assert_eq!(saved.terminal_name.as_deref(), Some("timeout"));
    drop(node);
}

#[tokio::test]
async fn distributed_deadline_precedes_iteration_bound_and_late_claim_writes() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let task: Evaluate = {
        let (entered, release) = (entered.clone(), release.clone());
        Arc::new(move |_, _| {
            entered.store(true, Ordering::SeqCst);
            while !release.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(json!(1))
        })
    };
    let graph = |name: &'static str, duration: Duration| {
        let mut graph = distributed_cycle_graph(task.clone());
        graph.name = name;
        graph.deadline = Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration,
        });
        graph.nodes[2].kind = GraphNodeKind::TaskLoop(
            task.clone(),
            blkit::compiled_graph::LoopPolicy {
                condition: Arc::new(|_, _| Ok(json!(true))),
                initial: None,
                before: false,
                max_iterations: Some(1),
                max_duration: None,
            },
        );
        graph
    };
    let control = DistributedControl::new(store.clone());
    let worker = Arc::new(
        DistributedWorker::new(
            store.clone(),
            "bound-worker",
            vec![
                graph("short", Duration::from_millis(100)),
                graph("long", Duration::from_secs(2)),
            ],
            1,
            5000,
        )
        .unwrap(),
    );
    worker.advertise().await.unwrap();
    let short = control
        .start("test", "1", "short", json!(null))
        .await
        .unwrap();
    let running = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.run_once().await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(125)).await;
    control.reconcile_once().await.unwrap();
    release.store(true, Ordering::SeqCst);
    running.await.unwrap().unwrap();
    let result = control.status(&short).await.unwrap().unwrap();
    assert_eq!(result.instance.status, "business-error");
    assert_eq!(result.instance.terminal_name.as_deref(), Some("timeout"));
    assert!(
        !result
            .instance
            .checkpoint
            .unwrap()
            .completed
            .contains_key("work")
    );
    let long = control
        .start("test", "1", "long", json!(null))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let result = control.status(&long).await.unwrap().unwrap();
    assert_eq!(result.instance.status, "business-error");
    assert_eq!(
        result.instance.terminal_name.as_deref(),
        Some("task-iteration-limit")
    );
    assert_eq!(result.instance.attempt, 1);
    drop(node);
}

#[tokio::test]
async fn distributed_queued_and_first_claim_deadlines_expire_without_retrying_late_results() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = |origin: &'static str| GraphDefinition {
        namespace: "test",
        version: "1",
        name: origin,
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin,
            duration: Duration::from_millis(50),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "wait",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(80)),
            },
            GraphNode {
                name: "slow",
                kind: GraphNodeKind::Task(Arc::new(|input, _| {
                    std::thread::sleep(Duration::from_millis(180));
                    Ok(input.clone())
                })),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: [("start", "wait"), ("wait", "slow"), ("slow", "done")]
            .into_iter()
            .map(|(source, target)| GraphLink {
                source,
                target,
                value: if target == "done" {
                    Some(
                        Arc::new(|_: &serde_json::Value, values: &blkit::runtime::Values| {
                            Ok(values["slow"].clone())
                        }) as blkit::runtime::Evaluate,
                    )
                } else {
                    None
                },
                condition: None,
                fallback: false,
                label: None,
            })
            .collect(),
    };
    let worker = DistributedWorker::new(
        store.clone(),
        "deadline-worker",
        vec![graph("queued"), graph("first_claimed")],
        1,
        5000,
    )
    .unwrap();
    worker.advertise().await.unwrap();
    let control = DistributedControl::new(store.clone());
    let queued = control
        .start("test", "1", "queued", json!(1))
        .await
        .unwrap();
    let claimed = control
        .start("test", "1", "first_claimed", json!(2))
        .await
        .unwrap();
    assert!(
        control
            .status(&claimed)
            .await
            .unwrap()
            .unwrap()
            .instance
            .deadline_at_ms
            .is_none()
    );
    tokio::time::sleep(Duration::from_millis(95)).await;
    control.reconcile_once().await.unwrap();
    let timed_out = control.status(&queued).await.unwrap().unwrap();
    assert_eq!(timed_out.instance.status, "business-error");
    assert_eq!(timed_out.instance.terminal_name.as_deref(), Some("timeout"));
    let running = tokio::spawn(async move { worker.run_once().await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while control
            .status(&claimed)
            .await
            .unwrap()
            .unwrap()
            .instance
            .first_claim_at_ms
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("worker must claim before first-claim deadline begins");
    let before = control.status(&claimed).await.unwrap().unwrap().instance;
    assert!(before.first_claim_at_ms.is_some());
    assert_eq!(
        before.deadline_at_ms,
        Some(before.first_claim_at_ms.unwrap() + 50)
    );
    tokio::time::sleep(Duration::from_millis(65)).await;
    control.reconcile_once().await.unwrap();
    let result = control.status(&claimed).await.unwrap().unwrap().instance;
    assert_eq!(result.status, "business-error");
    assert_eq!(result.terminal_name.as_deref(), Some("timeout"));
    running.await.unwrap().unwrap();
    assert_eq!(
        control
            .status(&claimed)
            .await
            .unwrap()
            .unwrap()
            .instance
            .status,
        "business-error"
    );
    drop(node);
}

#[tokio::test]
async fn postgres_deadline_terminalizes_unclaimed_wait_and_fences_late_claim() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut item = Instance::new("timeout", "test", "1", "pause", json!(null));
    item.status = "waiting".into();
    item.wake_at_ms = Some(1);
    item.deadline_at_ms = Some(100);
    item.deadline_origin = Some("queued".into());
    item.deadline_duration_ms = Some(10);
    store.create(&item).await.unwrap();
    for (id, status) in [
        ("queued", "pending"),
        ("retry", "retry-waiting"),
        ("executing", "running"),
    ] {
        let mut additional = Instance::new(id, "test", "1", "pause", json!(null));
        additional.status = status.into();
        additional.deadline_origin = Some("queued".into());
        additional.deadline_duration_ms = Some(10);
        additional.deadline_at_ms = Some(100);
        store.create(&additional).await.unwrap();
    }
    let mut expired = store.expire_due().await.unwrap();
    expired.sort();
    assert_eq!(expired, vec!["executing", "queued", "retry", "timeout"]);
    assert!(store.expire_due().await.unwrap().is_empty());
    let saved = store.get("timeout").await.unwrap().unwrap();
    assert_eq!(saved.instance.status, "business-error");
    assert_eq!(saved.instance.terminal_name.as_deref(), Some("timeout"));
    assert_eq!(store.resume_due_waits().await.unwrap(), 0);
    assert_eq!(
        store.cancel("timeout").await.unwrap_err(),
        "instance already terminal"
    );
    drop(node);
}

#[tokio::test]
async fn distributed_wait_releases_claim_and_resumes_on_another_worker() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let graph = || {
        let hits = hits.clone();
        GraphDefinition {
            namespace: "test",
            version: "1",
            name: "pause",
            retry: None,
            deadline: None,
            decode_input: Box::new(Ok),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "before",
                    kind: GraphNodeKind::Task(Arc::new(move |input, _| {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Ok(input.clone())
                    })),
                },
                GraphNode {
                    name: "pause",
                    kind: GraphNodeKind::PauseFor(Duration::from_millis(150)),
                },
                GraphNode {
                    name: "after",
                    kind: GraphNodeKind::Task(Arc::new(|input, _| Ok(input.clone()))),
                },
                GraphNode {
                    name: "done",
                    kind: GraphNodeKind::End,
                },
            ],
            links: [
                ("start", "before"),
                ("before", "pause"),
                ("pause", "after"),
                ("after", "done"),
            ]
            .into_iter()
            .map(|(source, target)| GraphLink {
                source,
                target,
                value: if target == "done" {
                    Some(
                        Arc::new(|_: &serde_json::Value, values: &blkit::runtime::Values| {
                            Ok(values["after"].clone())
                        }) as blkit::runtime::Evaluate,
                    )
                } else {
                    None
                },
                condition: None,
                fallback: false,
                label: None,
            })
            .collect(),
        }
    };
    let control = DistributedControl::new(store.clone());
    let first = DistributedWorker::new(store.clone(), "w1", vec![graph()], 2, 5000).unwrap();
    first.advertise().await.unwrap();
    let id = control.start("test", "1", "pause", json!(9)).await.unwrap();
    assert_eq!(first.run_once().await.unwrap(), 1);
    let waiting = control.status(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting");
    assert!(waiting.owner_id.is_none());
    let wake = waiting.instance.wake_at_ms.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    drop(first);
    let second = DistributedWorker::new(store.clone(), "w2", vec![graph()], 2, 5000).unwrap();
    second.advertise().await.unwrap();
    assert_eq!(second.run_once().await.unwrap(), 0);
    tokio::time::sleep(Duration::from_millis(170)).await;
    assert_eq!(
        control
            .status(&id)
            .await
            .unwrap()
            .unwrap()
            .instance
            .wake_at_ms,
        Some(wake)
    );
    assert_eq!(second.run_once().await.unwrap(), 1);
    let result = control.status(&id).await.unwrap().unwrap();
    assert_eq!(result.instance.status, "completed");
    assert_eq!(result.instance.result, Some(json!(9)));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    drop(node);
}

#[tokio::test]
async fn postgres_preserves_wake_and_first_claim_deadline_across_reclaim() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut waiting = Instance::new("waiting", "orders", "1", "route", json!(null));
    waiting.status = "waiting".into();
    waiting.wake_at_ms = Some(100);
    store.create(&waiting).await.unwrap();
    assert!(store.due_waits(99).await.unwrap().is_empty());
    assert_eq!(
        store.due_waits(100).await.unwrap()[0].instance.id,
        "waiting"
    );
    let mut job = Instance::new("job", "orders", "1", "route", json!(null));
    job.deadline_origin = Some("first_claimed".into());
    job.deadline_duration_ms = Some(300_000);
    store.create(&job).await.unwrap();
    store
        .register_worker("worker", &[("orders", "1", "route")])
        .await
        .unwrap();
    let first = store
        .claim("worker", 1, 10_000)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let start = first.instance.first_claim_at_ms.unwrap();
    assert_eq!(first.instance.deadline_at_ms, Some(start + 300_000));
    let retry = RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(10),
        retry_delay: Duration::from_millis(1),
        backoff: "exponential",
    };
    store
        .fail_owned("job", "worker", first.generation, Some(&retry), "try again")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let second = store
        .claim("worker", 1, 10_000)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(second.instance.first_claim_at_ms, Some(start));
    assert_eq!(second.instance.deadline_at_ms, Some(start + 300_000));
    let mut takeover = Instance::new("takeover", "orders", "1", "route", json!(null));
    takeover.deadline_origin = Some("first_claimed".into());
    takeover.deadline_duration_ms = Some(300_000);
    store.create(&takeover).await.unwrap();
    let first_owner = store.claim("worker", 1, 40).await.unwrap().pop().unwrap();
    let started = first_owner.instance.first_claim_at_ms.unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        store
            .reconcile_expired("takeover", Some(&retry))
            .await
            .unwrap(),
        Some("retry-waiting")
    );
    store
        .register_worker("other", &[("orders", "1", "route")])
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let resumed = store
        .claim("other", 1, 10_000)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(resumed.instance.id, "takeover");
    assert_eq!(resumed.instance.first_claim_at_ms, Some(started));
    assert_eq!(resumed.instance.deadline_at_ms, Some(started + 300_000));
    drop(node);
}

#[tokio::test]
async fn postgres_store_reopens_queued_checkpoint_retry_and_worker_capabilities() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut item = Instance::new(
        "order-1",
        "orders",
        "1.0",
        "decide",
        json!({"total": "12.50"}),
    );
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("b".into(), json!(3));
    checkpoint.selected.insert("fork".into(), vec![1, 2]);
    item.checkpoint = Some(checkpoint);
    item.attempt = 1;
    item.first_failure_at = Some(1000);
    item.next_eligible_at = Some(2000);
    item.status = "retry-waiting".into();
    store.create(&item).await.unwrap();
    store
        .register_worker(
            "worker-1",
            &[("orders", "1.0", "decide"), ("orders", "2.0", "decide")],
        )
        .await
        .unwrap();
    drop(store);
    let reopened =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let row = reopened.get("order-1").await.unwrap().unwrap();
    assert_eq!(row.instance.status, "retry-waiting");
    assert_eq!(row.instance.input, json!({"total": "12.50"}));
    assert_eq!(row.instance.checkpoint.unwrap().completed["b"], json!(3));
    assert_eq!(row.instance.attempt, 1);
    assert_eq!(row.instance.first_failure_at, Some(1000));
    assert_eq!(row.instance.next_eligible_at, Some(2000));
    assert_eq!(row.owner_id, None);
    assert_eq!(row.generation, 0);
    let worker = reopened.get_worker("worker-1").await.unwrap().unwrap();
    assert!(!worker.draining);
    assert_eq!(
        worker.identities,
        vec![
            ("orders".into(), "1.0".into(), "decide".into()),
            ("orders".into(), "2.0".into(), "decide".into())
        ]
    );
    drop(reopened);
    drop(node);
}

#[tokio::test]
async fn distributed_worker_resumes_version_two_checkpoint_without_request_field() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = versioned_graph("1.0", Arc::new(|input, _| Ok(input.clone())), None);
    let input = json!("before-rollout");
    let mut checkpoint = serde_json::to_value(graph.checkpoint(&input).unwrap()).unwrap();
    checkpoint.as_object_mut().unwrap().remove("requests");
    let item = Instance::new("old-checkpoint", "orders", "1.0", "decide", input.clone());
    store.create(&item).await.unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.execute(
        "UPDATE instances SET checkpoint=$1 WHERE id='old-checkpoint'",
        &[&checkpoint.to_string()],
    )
    .await
    .unwrap();
    let stored: String = db
        .query_one(
            "SELECT checkpoint FROM instances WHERE id='old-checkpoint'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stored)
            .unwrap()
            .get("requests")
            .is_none(),
        "fixture must persist the pre-request checkpoint shape"
    );

    drop(db);
    drop(store);
    let reopened =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker = DistributedWorker::new(reopened.clone(), "capable", vec![graph], 1, 5000).unwrap();
    worker.advertise().await.unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let finished = reopened.get("old-checkpoint").await.unwrap().unwrap();
    assert_eq!(finished.instance.status, "completed");
    assert_eq!(finished.instance.result, Some(input));
    assert!(finished.requests.is_empty());
}

#[tokio::test]
async fn concurrent_claims_match_exact_identity_without_double_assignment() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let first =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let second =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    first
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    first
        .register_worker("w2", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    for (id, namespace, version, process) in [
        ("a", "orders", "1.0", "decide"),
        ("b", "orders", "1.0", "decide"),
        ("c", "orders", "1.0", "decide"),
        ("d", "orders", "1.0", "decide"),
        ("new-version", "orders", "2.0", "decide"),
        ("new-name", "orders", "1.0", "other"),
        ("new-namespace", "other", "1.0", "decide"),
    ] {
        first
            .create(&Instance::new(id, namespace, version, process, json!(null)))
            .await
            .unwrap();
    }
    let (one, two) = tokio::join!(first.claim("w1", 1, 1000), second.claim("w2", 1, 1000));
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(two.len(), 1);
    assert_ne!(one[0].instance.id, two[0].instance.id);
    assert_eq!(one[0].owner_id.as_deref(), Some("w1"));
    assert_eq!(two[0].owner_id.as_deref(), Some("w2"));
    assert_eq!(one[0].instance.attempt, 1);
    assert_eq!(one[0].generation, 1);
    let more = first.claim("w1", 2, 1000).await.unwrap();
    assert_eq!(more.len(), 2);
    assert!(
        more.iter()
            .all(|item| item.owner_id.as_deref() == Some("w1"))
    );
    assert!(first.claim("w1", 4, 1000).await.unwrap().is_empty());
    for id in ["new-version", "new-name", "new-namespace"] {
        let item = first.get(id).await.unwrap().unwrap();
        assert_eq!(item.instance.status, "pending");
        assert_eq!(item.owner_id, None);
    }
    drop(node);
}

#[tokio::test]
async fn pooled_cross_session_claim_and_drain_keep_owned_claim_and_gate_backlog() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let worker =
        PostgresStore::connect_with_options(&url, 2, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let control =
        PostgresStore::connect_with_options(&url, 2, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    worker
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    for (id, created_at) in [("job", 1), ("backlog", 2), ("after-drain", 3)] {
        let mut item = Instance::new(id, "orders", "1.0", "decide", json!(null));
        item.created_at = created_at;
        worker.create(&item).await.unwrap();
    }
    let peer = PostgresStore::connect_with_options(&url, 2, &postgres_fixture::plaintext_options())
        .await
        .unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    gate.batch_execute(
        "CREATE FUNCTION pause_claim_update() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock(987654321); RETURN NEW; END $$;
         CREATE TRIGGER pause_claim BEFORE UPDATE ON instances
         FOR EACH ROW WHEN (NEW.id='job' AND NEW.owner_id IS NOT NULL AND OLD.owner_id IS NULL)
         EXECUTE FUNCTION pause_claim_update()",
    )
    .await
    .unwrap();
    let barrier = gate.transaction().await.unwrap();
    barrier
        .query_one("SELECT pg_advisory_xact_lock(987654321)", &[])
        .await
        .unwrap();
    let claiming_store = worker.clone();
    let claim = tokio::spawn(async move { claiming_store.claim("w1", 1, 10_000).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = barrier
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND wait_event='advisory'",
                    &[],
                )
                .await
                .unwrap()
                .get(0);
            if blocked == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("claim must reach instance UPDATE while drain is still unset");
    let peer_claim = tokio::time::timeout(Duration::from_secs(5), peer.claim("w1", 1, 10_000))
        .await
        .expect("a second same-worker claimer must skip the locked instance")
        .unwrap();
    assert_eq!(peer_claim.len(), 1);
    assert_eq!(peer_claim[0].instance.id, "backlog");
    let draining_store = control.clone();
    let drain = tokio::spawn(async move { draining_store.drain("w1").await });
    let acknowledged_early = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if drain.is_finished() {
                break true;
            }
            let waiting: i64 = barrier
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND wait_event='transactionid'",
                    &[],
                )
                .await
                .unwrap()
                .get(0);
            if waiting == 1 {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("drain must acknowledge or block on the in-flight claim");
    barrier.commit().await.unwrap();
    let claimed = tokio::time::timeout(Duration::from_secs(5), claim)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].instance.id, "job");
    assert_eq!(claimed[0].owner_id.as_deref(), Some("w1"));
    assert!(drain.await.unwrap().unwrap());
    assert!(
        !acknowledged_early,
        "drain acknowledged before the already-selected claim was assigned"
    );
    assert!(
        worker
            .renew_claim("job", "w1", claimed[0].generation, 10_000)
            .await
            .unwrap(),
        "drain must not revoke an already owned claim"
    );
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("kept".into(), json!(1));
    assert!(
        worker
            .commit_checkpoint("job", "w1", claimed[0].generation, &checkpoint)
            .await
            .unwrap()
    );
    assert!(
        worker
            .finish_owned(
                "job",
                "w1",
                claimed[0].generation,
                "completed",
                Some(json!(1)),
                None,
                None
            )
            .await
            .unwrap()
    );
    let completed = control.get("job").await.unwrap().unwrap();
    assert_eq!(completed.instance.result, Some(json!(1)));
    assert_eq!(
        completed.instance.checkpoint.unwrap().completed["kept"],
        json!(1)
    );
    assert_eq!(peer_claim[0].generation, 1);
    assert_eq!(
        control
            .get("backlog")
            .await
            .unwrap()
            .unwrap()
            .owner_id
            .as_deref(),
        Some("w1")
    );
    let unclaimed = control.get("after-drain").await.unwrap().unwrap();
    assert_eq!(unclaimed.instance.status, "pending");
    assert!(unclaimed.owner_id.is_none());
    assert!(
        worker.claim("w1", 1, 10_000).await.unwrap().is_empty(),
        "a committed drain must gate later claims"
    );
    drop(node);
}

#[tokio::test]
async fn active_claim_renewal_reports_pool_contention_then_recovers_without_stale_checkpoint() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_active_claim"),
        1,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    store
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    store
        .create(&Instance::new(
            "job",
            "orders",
            "1.0",
            "decide",
            json!(null),
        ))
        .await
        .unwrap();
    let lease_ms = 30_000;
    let claimed = store.claim("w1", 1, lease_ms).await.unwrap().pop().unwrap();
    let (mut gate, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let lock = gate.transaction().await.unwrap();
    lock.batch_execute("LOCK TABLE instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("current".into(), json!(1));
    let writing = tokio::spawn({
        let store = store.clone();
        let checkpoint = checkpoint.clone();
        let generation = claimed.generation;
        async move {
            store
                .commit_checkpoint("job", "w1", generation, &checkpoint)
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            lock.batch_execute("SELECT pg_stat_clear_snapshot()").await.unwrap();
            let waiting: i64 = lock.query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock'",
                &[&"blkit_active_claim"],
            ).await.unwrap().get(0);
            if waiting == 1 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("checkpoint write must occupy the pool before renewal");
    let renewal = tokio::time::timeout(
        Duration::from_secs(3),
        store.renew_claim("job", "w1", claimed.generation, lease_ms),
    )
    .await
    .expect("maintenance must not wait indefinitely for the active pool slot")
    .expect_err("saturated pool must report an error, not an acknowledged renewal");
    assert!(
        renewal.to_lowercase().contains("timeout") || renewal.to_lowercase().contains("timed out"),
        "{renewal}"
    );
    assert!(!writing.is_finished());
    lock.commit().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), writing)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    assert!(
        store
            .renew_claim("job", "w1", claimed.generation, lease_ms)
            .await
            .unwrap()
    );
    let current = store.get("job").await.unwrap().unwrap();
    assert_eq!(
        current.instance.checkpoint.unwrap().completed["current"],
        json!(1)
    );
    assert_eq!(current.generation, claimed.generation);
    assert!(
        store
            .finish_owned(
                "job",
                "w1",
                claimed.generation,
                "completed",
                Some(json!(1)),
                None,
                None
            )
            .await
            .unwrap()
    );
    assert!(
        !store
            .commit_checkpoint("job", "w1", claimed.generation, &GraphCheckpoint::default())
            .await
            .unwrap()
    );
    let finished = store.get("job").await.unwrap().unwrap();
    assert_eq!(
        finished.instance.checkpoint.unwrap().completed["current"],
        json!(1)
    );
    assert_eq!(finished.instance.result, Some(json!(1)));
}

#[tokio::test]
async fn claim_lease_renewal_checks_owner_expiry_and_increments_generation_on_reclaim() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    store
        .register_worker("w2", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    store
        .create(&Instance::new(
            "job",
            "orders",
            "1.0",
            "decide",
            json!(null),
        ))
        .await
        .unwrap();
    let claim = store.claim("w1", 1, 250).await.unwrap().pop().unwrap();
    assert!(
        !store
            .renew_claim("job", "w2", claim.generation, 250)
            .await
            .unwrap()
    );
    assert!(
        !store
            .renew_claim("job", "w1", claim.generation + 1, 250)
            .await
            .unwrap()
    );
    assert!(
        store
            .renew_claim("job", "w1", claim.generation, 250)
            .await
            .unwrap()
    );
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    assert!(
        !store
            .renew_claim("job", "w1", claim.generation, 250)
            .await
            .unwrap()
    );
    // Requeue only as a test setup; lease reconciliation is implemented in task 4.3.
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client
        .execute(
            "UPDATE instances SET status='pending', owner_id=NULL, lease_until=NULL WHERE id='job'",
            &[],
        )
        .await
        .unwrap();
    let reclaimed = store.claim("w2", 1, 1000).await.unwrap().pop().unwrap();
    assert_eq!(reclaimed.generation, claim.generation + 1);
    assert_eq!(reclaimed.instance.attempt, 2);
    assert_eq!(reclaimed.owner_id.as_deref(), Some("w2"));
    drop(node);
}

#[tokio::test]
async fn expired_claim_reconciliation_is_idempotent_and_fences_old_owner_writes() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let first =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let second =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for worker in ["w1", "w2"] {
        first
            .register_worker(worker, &[("orders", "1.0", "decide")])
            .await
            .unwrap();
    }
    let mut item = Instance::new("job", "orders", "1.0", "decide", json!(null));
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("b".into(), json!(3));
    item.checkpoint = Some(checkpoint.clone());
    first.create(&item).await.unwrap();
    let owner = first.claim("w1", 1, 80).await.unwrap().pop().unwrap();
    tokio::time::sleep(Duration::from_millis(110)).await;
    let policy = RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(50),
        backoff: "exponential",
    };
    let (a, b) = tokio::join!(
        first.reconcile_expired("job", Some(&policy)),
        second.reconcile_expired("job", Some(&policy))
    );
    assert_eq!(
        [a.unwrap(), b.unwrap()]
            .into_iter()
            .filter(|result| result.is_some())
            .count(),
        1
    );
    let waiting = first.get("job").await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "retry-waiting");
    assert_eq!(waiting.instance.attempt, 1);
    assert_eq!(
        waiting.instance.error.as_deref(),
        Some("owner lease expired")
    );
    assert!(
        waiting.instance.next_eligible_at.unwrap()
            >= waiting.instance.first_failure_at.unwrap() + 50
    );
    assert_eq!(
        waiting.instance.checkpoint.unwrap().completed["b"],
        json!(3)
    );
    tokio::time::sleep(Duration::from_millis(60)).await;
    let new_owner = second.claim("w2", 1, 1000).await.unwrap().pop().unwrap();
    assert_eq!(new_owner.generation, owner.generation + 1);
    assert_eq!(new_owner.instance.attempt, 2);
    let mut old_checkpoint = checkpoint.clone();
    old_checkpoint.completed.insert("stale".into(), json!(99));
    assert!(
        !first
            .commit_checkpoint("job", "w1", owner.generation, &old_checkpoint)
            .await
            .unwrap()
    );
    assert!(
        !first
            .renew_claim("job", "w1", owner.generation, 1000)
            .await
            .unwrap()
    );
    checkpoint.completed.insert("c".into(), json!(5));
    assert!(
        second
            .commit_checkpoint("job", "w2", new_owner.generation, &checkpoint)
            .await
            .unwrap()
    );
    let saved = first
        .get("job")
        .await
        .unwrap()
        .unwrap()
        .instance
        .checkpoint
        .unwrap();
    assert_eq!(saved.completed["b"], json!(3));
    assert_eq!(saved.completed["c"], json!(5));
    assert!(!saved.completed.contains_key("stale"));
    assert!(
        !first
            .finish_owned(
                "job",
                "w1",
                owner.generation,
                "completed",
                Some(json!(99)),
                None,
                None
            )
            .await
            .unwrap()
    );
    assert!(
        second
            .finish_owned(
                "job",
                "w2",
                new_owner.generation,
                "completed",
                Some(json!(8)),
                None,
                None
            )
            .await
            .unwrap()
    );
    let finished = first.get("job").await.unwrap().unwrap();
    assert_eq!(finished.instance.result, Some(json!(8)));
    assert_eq!(finished.instance.status, "completed");
    assert_eq!(finished.owner_id, None);
    drop(node);
}

#[tokio::test]
async fn expired_claim_without_retry_fails_with_checkpoint_intact() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    let mut item = Instance::new("job", "orders", "1.0", "decide", json!(null));
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("b".into(), json!(3));
    item.checkpoint = Some(checkpoint);
    store.create(&item).await.unwrap();
    let claim = store.claim("w1", 1, 40).await.unwrap().pop().unwrap();
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        store.reconcile_expired("job", None).await.unwrap(),
        Some("failed")
    );
    assert_eq!(store.reconcile_expired("job", None).await.unwrap(), None);
    let failed = store.get("job").await.unwrap().unwrap();
    assert_eq!(failed.instance.status, "failed");
    assert_eq!(failed.instance.checkpoint.unwrap().completed["b"], json!(3));
    assert!(failed.instance.first_failure_at.is_some());
    assert!(failed.instance.next_eligible_at.is_none());
    assert!(
        !store
            .commit_checkpoint("job", "w1", claim.generation, &GraphCheckpoint::default())
            .await
            .unwrap()
    );
    drop(node);
}

#[tokio::test]
async fn worker_advertises_linked_identity_and_runs_multiple_instances_under_bound() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let task: blkit::runtime::Evaluate = Arc::new({
        let (entered, active, peak, release) = (
            entered.clone(),
            active.clone(),
            peak.clone(),
            release.clone(),
        );
        move |input, _| {
            entered.fetch_add(1, Ordering::SeqCst);
            let running = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(running, Ordering::SeqCst);
            for _ in 0..200 {
                if release.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(input.clone())
        }
    });
    let graph = GraphDefinition {
        namespace: "orders",
        version: "1.0",
        name: "decide",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<serde_json::Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(task),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "work",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    for value in 0..3 {
        let mut item = Instance::new(
            &format!("job-{value}"),
            "orders",
            "1.0",
            "decide",
            json!(value),
        );
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let worker = DistributedWorker::new(store.clone(), "worker-1", vec![graph], 2, 1000).unwrap();
    worker.advertise().await.unwrap();
    assert_eq!(
        store
            .get_worker("worker-1")
            .await
            .unwrap()
            .unwrap()
            .identities,
        vec![("orders".into(), "1.0".into(), "decide".into())]
    );
    let heartbeat = store
        .get_worker("worker-1")
        .await
        .unwrap()
        .unwrap()
        .heartbeat_at;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let execution = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while entered.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("two instances should run together");
    assert_eq!(entered.load(Ordering::SeqCst), 2);
    release.store(true, Ordering::SeqCst);
    assert_eq!(execution.await.unwrap(), 3);
    assert!(
        store
            .get_worker("worker-1")
            .await
            .unwrap()
            .unwrap()
            .heartbeat_at
            > heartbeat
    );
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.get("job-0").await.unwrap().unwrap().instance.status,
        "completed"
    );
    assert_eq!(
        store.get("job-1").await.unwrap().unwrap().instance.status,
        "completed"
    );
    assert_eq!(
        store.get("job-2").await.unwrap().unwrap().instance.status,
        "completed"
    );
    drop(node);
}

#[tokio::test]
async fn worker_execution_error_releases_claim_for_retry_without_losing_checkpoint() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = GraphDefinition {
        namespace: "orders",
        version: "1.0",
        name: "decide",
        deadline: None,
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(50),
            backoff: "exponential",
        }),
        decode_input: Box::new(Ok::<serde_json::Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(Arc::new(|_, _| Err("boom".into()))),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "work",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let mut item = Instance::new("job", "orders", "1.0", "decide", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let worker = DistributedWorker::new(store.clone(), "w1", vec![graph], 1, 1000).unwrap();
    worker.advertise().await.unwrap();
    worker.run_once().await.unwrap();
    let row = store.get("job").await.unwrap().unwrap();
    assert_eq!(row.instance.status, "retry-waiting");
    assert_eq!(row.instance.error.as_deref(), Some("boom"));
    assert_eq!(row.instance.attempt, 1);
    assert!(row.instance.checkpoint.unwrap().completed.is_empty());
    assert_eq!(row.owner_id, None);
    drop(node);
}

struct WorkerChild(Child);
impl Drop for WorkerChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Match the README's role-only env file; provider authentication stays ambient.
fn select_runtime_env_file(command: &mut Command, url: &str, role: &str) -> CrashDir {
    let root = std::env::temp_dir().join(format!("blkit-harness-{role}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let files = CrashDir(root);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&files.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let secret = files.0.join("url");
    std::fs::write(&secret, managed_runtime::role_url(url, role)).unwrap();
    let setting = format!("BLKIT_{}_DATABASE_URL", role.to_uppercase());
    let dotenv = files.0.join("role.env");
    std::fs::write(&dotenv, format!("{setting}_FILE={}\nBLKIT_POSTGRES_INSECURE_DEV=true\nBLKIT_ATTACHMENT_BACKEND=s3\nBLKIT_ATTACHMENT_BUCKET=my-bucket\n", secret.display())).unwrap();
    for name in [
        setting.as_str(),
        "BLKIT_POSTGRES_INSECURE_DEV",
        "BLKIT_ATTACHMENT_BACKEND",
        "BLKIT_ATTACHMENT_BUCKET",
    ] {
        command.env_remove(name);
    }
    command.arg("--env-file").arg(dotenv);
    files
}

// Generated Rust is checked by compilation and integration tests, not style lints.
#[allow(clippy::all)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::test]
async fn worker_binary_advertises_only_linked_identities_and_runs_without_source_files() {
    let (_cloud, endpoint) = seaweed::start().await;
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    managed_runtime::prepare(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blkit-worker"));
    seaweed::configure(&mut command, &endpoint);
    managed_runtime::configure(&mut command, &url, &["worker"]);
    let _files = select_runtime_env_file(&mut command, &url, "worker");
    let mut child = WorkerChild(
        command
            .args(["2", "1000"])
            .current_dir(std::env::temp_dir())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let worker_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
                .await
                .unwrap();
            tokio::spawn(async move {
                let _ = connection.await;
            });
            if let Some(row) = client
                .query_opt("SELECT id FROM workers LIMIT 1", &[])
                .await
                .unwrap()
            {
                break row.get::<_, String>(0);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("worker binary should register");
    managed_runtime::assert_child_configuration(&child.0, &["worker"]);
    managed_runtime::assert_sessions(&url, &["worker"]).await;
    let advertised = store.get_worker(&worker_id).await.unwrap().unwrap();
    assert_eq!(
        advertised.identities,
        vec![
            ("orders".into(), "1.0".into(), "decide".into()),
            ("orders".into(), "1.0".into(), "offers".into()),
            ("orders".into(), "1.0".into(), "parallel".into()),
        ]
    );
    let first_heartbeat = advertised.heartbeat_at;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        store
            .get_worker(&worker_id)
            .await
            .unwrap()
            .unwrap()
            .heartbeat_at
            > first_heartbeat
    );
    let graph = compiled::named_graph_definitions()
        .into_iter()
        .find(|g| g.name == "decide")
        .unwrap();
    let mut item = Instance::new(
        "binary-job",
        "orders",
        "1.0",
        "decide",
        json!({"total":"1200"}),
    );
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let finished = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = store.get("binary-job").await.unwrap().unwrap();
            if row.instance.status == "completed" {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("worker binary should run linked process without source file");
    assert_eq!(finished.instance.result, Some(json!("review")));
    assert!(store.drain(&worker_id).await.unwrap());
    let exit = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(exit) = child.0.try_wait().unwrap() {
                break exit;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("drained worker should exit when claims finish");
    assert!(exit.success());
    assert!(store.get_worker(&worker_id).await.unwrap().is_none());
    drop(child);
    drop(node);
}

fn subprocess_parent_graph() -> GraphDefinition {
    GraphDefinition {
        namespace: "orders",
        version: "1.0",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "called",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "called",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "called",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["called"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn distributed_subprocess_retry_and_wait_handoff_share_one_parent_claim() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let call: blkit::runtime::Evaluate = {
        let hits = hits.clone();
        Arc::new(move |input, _| {
            if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("retry child".into())
            } else {
                Ok(input.clone())
            }
        })
    };
    let child_graph = || {
        let mut graph = versioned_graph(
            "1.0",
            call.clone(),
            Some(RetryPolicy {
                max_retries: 1,
                retry_for: Duration::from_secs(2),
                retry_delay: Duration::from_millis(60),
                backoff: "exponential",
            }),
        );
        graph.name = "child";
        graph.nodes.push(GraphNode {
            name: "pause",
            kind: GraphNodeKind::PauseFor(Duration::from_millis(100)),
        });
        graph.links[1].source = "pause";
        graph.links.push(GraphLink {
            source: "work",
            target: "pause",
            value: None,
            condition: None,
            fallback: false,
            label: None,
        });
        graph
    };
    let parent = subprocess_parent_graph();
    let mut item = Instance::new("job", "orders", "1.0", "parent", json!(4));
    item.checkpoint = Some(parent.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let first =
        DistributedWorker::new(store.clone(), "first", vec![parent, child_graph()], 1, 5000)
            .unwrap();
    let second = DistributedWorker::new(
        store.clone(),
        "second",
        vec![subprocess_parent_graph(), child_graph()],
        1,
        5000,
    )
    .unwrap();
    first.advertise().await.unwrap();
    second.advertise().await.unwrap();
    assert_eq!(first.run_once().await.unwrap(), 1);
    assert_eq!(
        store.get("job").await.unwrap().unwrap().instance.status,
        "waiting"
    );
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(second.run_once().await.unwrap(), 1);
    assert_eq!(
        store.get("job").await.unwrap().unwrap().instance.status,
        "waiting"
    );
    tokio::time::sleep(Duration::from_millis(130)).await;
    assert_eq!(first.run_once().await.unwrap(), 1);
    let result = store.get("job").await.unwrap().unwrap();
    assert_eq!(
        result.instance.status, "completed",
        "{:?}",
        result.instance.error
    );
    assert_eq!(result.instance.result, Some(json!(4)));
    assert_eq!(result.instance.id, "job");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    drop(node);
}

#[tokio::test]
async fn distributed_parent_cancellation_signals_parallel_children() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicUsize::new(0));
    let hooks = Arc::new(AtomicUsize::new(0));
    let released = Arc::new(AtomicBool::new(false));
    let child = {
        let mut graph = versioned_graph(
            "1.0",
            {
                let entered = entered.clone();
                let released = released.clone();
                Arc::new(move |input, _| {
                    entered.fetch_add(1, Ordering::SeqCst);
                    for _ in 0..200 {
                        if released.load(Ordering::SeqCst) {
                            return Ok(input.clone());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err("child not signalled".into())
                })
            },
            None,
        );
        graph.name = "child";
        graph.nodes[1].kind = GraphNodeKind::TaskWithCancel(
            match &graph.nodes[1].kind {
                GraphNodeKind::Task(call) => call.clone(),
                _ => unreachable!(),
            },
            {
                let hooks = hooks.clone();
                let released = released.clone();
                Arc::new(move || {
                    hooks.fetch_add(1, Ordering::SeqCst);
                    released.store(true, Ordering::SeqCst);
                })
            },
        );
        graph
    };
    let link = |source, target, value, label| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label,
    };
    let parent = GraphDefinition {
        namespace: "orders",
        version: "1.0",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "split",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "left",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "right",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "split",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "split", None, None),
            link("split", "left", None, Some("left")),
            link("split", "right", None, Some("right")),
            link(
                "left",
                "joined",
                Some(Arc::new(|_, values| Ok(values["left"].clone()))),
                None,
            ),
            link(
                "right",
                "joined",
                Some(Arc::new(|_, values| Ok(values["right"].clone()))),
                None,
            ),
            link(
                "joined",
                "done",
                Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
                None,
            ),
        ],
    };
    let mut item = Instance::new("job", "orders", "1.0", "parent", json!(4));
    item.checkpoint = Some(parent.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "worker", vec![parent, child], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(4), async {
        while entered.load(Ordering::SeqCst) != 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    store.cancel("job").await.unwrap();
    running.await.unwrap();
    assert_eq!(hooks.load(Ordering::SeqCst), 2);
    let row = store.get("job").await.unwrap().unwrap();
    assert_eq!(row.instance.status, "cancelled");
    assert!(row.instance.result.is_none());
    drop(node);
}

fn versioned_graph(
    version: &'static str,
    task: blkit::runtime::Evaluate,
    retry: Option<RetryPolicy>,
) -> GraphDefinition {
    GraphDefinition {
        namespace: "orders",
        version,
        name: "decide",
        retry,
        deadline: None,
        decode_input: Box::new(Ok::<serde_json::Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(task),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "work",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

fn refill_graph(
    blocked: &'static [i64],
    release: Arc<AtomicBool>,
    release_first_fast: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
) -> GraphDefinition {
    versioned_graph(
        "1.0",
        Arc::new(move |input, _| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            let id = input.as_i64().unwrap();
            if blocked.contains(&id) {
                let gate = if id == 1 {
                    &release_first_fast
                } else {
                    &release
                };
                while !gate.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(input.clone())
        }),
        None,
    )
}

#[tokio::test]
async fn distributed_worker_refills_while_slow_claim_is_active() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release, &release);
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let graph = refill_graph(&[0], release.clone(), release.clone(), active, peak);
    for id in 0..3 {
        let mut item = Instance::new(&id.to_string(), "orders", "1.0", "decide", json!(id));
        item.created_at = id + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let worker = DistributedWorker::new(store.clone(), "refill", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    let replaced = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let slow = store.get("0").await.unwrap().unwrap();
            let replacement = store.get("2").await.unwrap().unwrap();
            if slow.instance.status == "running" && replacement.instance.status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    release.store(true, Ordering::SeqCst);
    let claimed = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("worker should finish after releasing blocked tasks")
        .unwrap();
    assert!(
        replaced,
        "replacement must complete while the slow claim is still running"
    );
    assert_eq!(claimed, 3, "run_once returns all claims, including refills");
    assert_eq!(
        store.get("0").await.unwrap().unwrap().instance.status,
        "completed"
    );
}

#[tokio::test]
async fn distributed_worker_claims_late_work_after_capacity_frees() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release, &release);
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let graph = refill_graph(&[0], release.clone(), release.clone(), active, peak);
    for id in 0..2 {
        let mut item = Instance::new(&id.to_string(), "orders", "1.0", "decide", json!(id));
        item.created_at = id + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let late_checkpoint = graph.checkpoint(&json!(2)).unwrap();
    let worker = DistributedWorker::new(store.clone(), "late", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    let freed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("0").await.unwrap().unwrap().instance.status == "running"
                && store.get("1").await.unwrap().unwrap().instance.status == "completed"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    let mut late = Instance::new("2", "orders", "1.0", "decide", json!(2));
    late.checkpoint = Some(late_checkpoint);
    store.create(&late).await.unwrap();
    let claimed_late = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("0").await.unwrap().unwrap().instance.status == "running"
                && store.get("2").await.unwrap().unwrap().instance.status == "completed"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    release.store(true, Ordering::SeqCst);
    let claimed = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("worker should finish after releasing blocked tasks")
        .unwrap();
    assert!(
        freed,
        "first fast claim must finish before late work arrives"
    );
    assert!(
        claimed_late,
        "late work must run while the slow claim is still active"
    );
    assert_eq!(claimed, 3);
}

#[tokio::test]
async fn distributed_worker_refill_respects_active_and_task_limit() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let release_first_fast = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release, &release_first_fast);
    let graph = refill_graph(
        &[0, 1, 2, 3],
        release.clone(),
        release_first_fast.clone(),
        active.clone(),
        peak.clone(),
    );
    for id in 0..4 {
        let mut item = Instance::new(&id.to_string(), "orders", "1.0", "decide", json!(id));
        item.created_at = id + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let worker = DistributedWorker::new(store.clone(), "limited", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    let initial_full = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("0").await.unwrap().unwrap().instance.status == "running"
                && store.get("1").await.unwrap().unwrap().instance.status == "running"
                && active.load(Ordering::SeqCst) == 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let third_before = store.get("2").await.unwrap().unwrap();
    let fourth_before = store.get("3").await.unwrap().unwrap();
    release_first_fast.store(true, Ordering::SeqCst);
    let refilled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("0").await.unwrap().unwrap().instance.status == "running"
                && store.get("2").await.unwrap().unwrap().instance.status == "running"
                && active.load(Ordering::SeqCst) == 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let fourth_after = store.get("3").await.unwrap().unwrap();
    release.store(true, Ordering::SeqCst);
    let claimed = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("worker should finish after releasing blocked tasks")
        .unwrap();
    let maximum = peak.load(Ordering::SeqCst);
    assert!(initial_full, "both initial claims should start");
    assert_eq!(
        third_before.instance.status, "pending",
        "no third active claim at full capacity"
    );
    assert!(third_before.owner_id.is_none());
    assert_eq!(
        fourth_before.instance.status, "pending",
        "no fourth active claim at full capacity"
    );
    assert!(fourth_before.owner_id.is_none());
    assert!(
        refilled,
        "a freed slot should refill while the slow claim runs"
    );
    assert_eq!(
        fourth_after.instance.status, "pending",
        "only one replacement claim may be active"
    );
    assert!(fourth_after.owner_id.is_none());
    assert!(
        maximum <= 2,
        "no more than two tasks may run concurrently: {maximum}"
    );
    assert_eq!(claimed, 4);
}

#[tokio::test]
async fn distributed_worker_defers_refill_error_until_active_claim_finishes() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let release = Arc::new(AtomicBool::new(false));
    let release_fast = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release, &release_fast);
    let graph = refill_graph(
        &[0, 1],
        release.clone(),
        release_fast.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    for (id, input) in [("slow", 0), ("fast", 1)] {
        let mut item = Instance::new(id, "orders", "1.0", "decide", json!(input));
        item.created_at = input + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let invalid_checkpoint = graph.checkpoint(&json!(1)).unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "refill-error", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await });
    let initial = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = store.get("slow").await.unwrap().unwrap();
            if row.instance.status == "running"
                && store.get("fast").await.unwrap().unwrap().instance.status == "running"
            {
                break row.lease_until;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both claims must start");
    let mut invalid = Instance::new("invalid", "orders", "1.0", "decide", json!(1));
    invalid.created_at = 2;
    invalid.checkpoint = Some(invalid_checkpoint);
    store.create(&invalid).await.unwrap();
    db.batch_execute(
        "CREATE FUNCTION reject_invalid_claim() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'injected claim failure'; END $$;
         CREATE TRIGGER reject_invalid_claim BEFORE UPDATE OF status ON instances
         FOR EACH ROW WHEN (NEW.id='invalid' AND OLD.status='pending' AND NEW.status='running')
         EXECUTE FUNCTION reject_invalid_claim()",
    )
    .await
    .unwrap();
    // Let periodic maintenance inspect the valid queued checkpoint before the claim fails.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    release_fast.store(true, Ordering::SeqCst);
    let fast_finished = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("fast").await.unwrap().unwrap().instance.status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let after_fast = store.get("slow").await.unwrap().unwrap().lease_until;
    let renewed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("slow").await.unwrap().unwrap().lease_until > after_fast {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let still_running = !running.is_finished();
    release.store(true, Ordering::SeqCst);
    let result = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("worker must finish after active claim")
        .unwrap();
    assert!(fast_finished.is_ok(), "fast claim must free a refill slot");
    assert!(after_fast >= initial);
    assert!(renewed.is_ok(), "worker must renew after refill fails");
    assert!(
        still_running,
        "refill failure must not abort the slow claim"
    );
    assert_eq!(
        store.get("slow").await.unwrap().unwrap().instance.status,
        "completed"
    );
    assert!(result.is_err(), "claim failure must propagate: {result:?}");
    let queued = store.get("invalid").await.unwrap().unwrap();
    assert_eq!(queued.instance.status, "pending");
    assert!(queued.owner_id.is_none());
}

#[tokio::test]
async fn distributed_worker_resumes_due_wait_during_sustained_refills() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release, &release);
    let graph = versioned_graph(
        "1.0",
        Arc::new({
            let release = release.clone();
            move |input, _| {
                if input == &json!("slow") {
                    while !release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(40));
                }
                Ok(input.clone())
            }
        }),
        None,
    );
    for id in 0..55 {
        let input = if id == 0 { json!("slow") } else { json!(id) };
        let mut item = Instance::new(&format!("job-{id:02}"), "orders", "1.0", "decide", input);
        item.created_at = id + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let due_checkpoint = graph.checkpoint(&json!("due")).unwrap();
    let worker = DistributedWorker::new(store.clone(), "maintain", vec![graph], 2, 1500).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("job-00").await.unwrap().unwrap().instance.status == "running"
                && store.get("job-01").await.unwrap().unwrap().instance.status == "completed"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("long claim and refills must start");
    let mut due = Instance::new("due", "orders", "1.0", "decide", json!("due"));
    due.created_at = 2;
    due.status = "waiting".into();
    due.wake_at_ms = Some(
        db.query_one(
            "SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT+150",
            &[],
        )
        .await
        .unwrap()
        .get(0),
    );
    due.checkpoint = Some(due_checkpoint);
    store.create(&due).await.unwrap();
    assert_eq!(
        store.get("due").await.unwrap().unwrap().instance.status,
        "waiting",
        "due wait must be inserted unclaimable"
    );
    let progressed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if store.get("due").await.unwrap().unwrap().instance.status == "completed" {
                break db
                    .query_one(
                        "SELECT count(*) FROM instances WHERE id LIKE 'job-%' AND status='pending'",
                        &[],
                    )
                    .await
                    .unwrap()
                    .get::<_, i64>(0);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let long_active = store.get("job-00").await.unwrap().unwrap().instance.status == "running";
    let still_running = !running.is_finished();
    release.store(true, Ordering::SeqCst);
    let total = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("worker should finish after long claim releases")
        .unwrap();
    assert!(
        progressed.is_ok(),
        "due wait must resume while run_once remains active"
    );
    assert!(
        long_active && still_running,
        "long claim must remain active through maintenance"
    );
    assert_eq!(total, 56);
    assert!(
        progressed.unwrap() > 0,
        "due wait must complete while backlog is pending"
    );
}

struct ReleaseOnDrop<'a>(&'a AtomicBool, &'a AtomicBool);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
        self.1.store(true, Ordering::SeqCst);
    }
}

#[test]
fn release_on_drop_opens_both_gates_after_panic() {
    let slow = AtomicBool::new(false);
    let fast = AtomicBool::new(false);
    let panic = std::panic::catch_unwind(|| {
        let _release = ReleaseOnDrop(&slow, &fast);
        panic!("early test failure");
    });
    assert!(panic.is_err());
    assert!(slow.load(Ordering::SeqCst));
    assert!(fast.load(Ordering::SeqCst));
}

#[tokio::test]
async fn distributed_worker_refill_does_not_burst_or_poll_on_heartbeat() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .with_cmd([
            "postgres",
            "-c",
            "shared_preload_libraries=pg_stat_statements",
        ])
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.batch_execute("CREATE EXTENSION pg_stat_statements")
        .await
        .unwrap();
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release_slow = Arc::new(AtomicBool::new(false));
    let release_fast = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release_slow, &release_fast);
    let graph = refill_graph(
        &[0, 1],
        release_slow.clone(),
        release_fast.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    for id in 0..2 {
        let mut item = Instance::new(&id.to_string(), "orders", "1.0", "decide", json!(id));
        item.created_at = id + 1;
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let worker = DistributedWorker::new(store.clone(), "cadence", vec![graph], 2, 180).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let slow = store.get("0").await.unwrap().unwrap();
            let fast = store.get("1").await.unwrap().unwrap();
            if slow.instance.status == "running" && fast.instance.status == "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both slots should be occupied");
    tokio::time::sleep(Duration::from_millis(1200)).await;
    db.query_one("SELECT pg_stat_statements_reset()", &[])
        .await
        .unwrap();
    release_fast.store(true, Ordering::SeqCst);
    let observed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("1").await.unwrap().unwrap().instance.status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let claims: i64 = db
        .query_one(
            "SELECT coalesce(sum(calls), 0)::bigint FROM pg_stat_statements WHERE query LIKE 'SELECT i.id, i.checkpoint FROM instances i%'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    release_slow.store(true, Ordering::SeqCst);
    let total = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("worker should finish after releasing slow task")
        .unwrap();
    assert!(
        observed.is_ok(),
        "fast claim must complete before the poll window"
    );
    assert_eq!(total, 2);
    assert!(
        claims > 0,
        "pg_stat_statements must observe empty claim queries"
    );
    assert!(
        claims <= 8,
        "unexpected empty claim burst or heartbeat polls: {claims}"
    );
}

#[tokio::test]
async fn draining_worker_finishes_owned_work_but_does_not_claim_old_version_backlog() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let old_task: blkit::runtime::Evaluate = Arc::new({
        let entered = entered.clone();
        let release = release.clone();
        move |_, _| {
            entered.store(true, Ordering::SeqCst);
            for _ in 0..200 {
                if release.load(Ordering::SeqCst) {
                    return Ok(json!("old"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("old not released".into())
        }
    });
    let new_task: blkit::runtime::Evaluate = Arc::new(|_, _| Ok(json!("new")));
    let old = versioned_graph("1.0", old_task, None);
    let new = versioned_graph("2.0", new_task, None);
    for (id, version, graph) in [
        ("old-running", "1.0", &old),
        ("old-backlog", "1.0", &old),
        ("new-job", "2.0", &new),
    ] {
        let mut item = Instance::new(id, "orders", version, "decide", json!(null));
        item.created_at = if id == "old-running" { 1 } else { 2 };
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let old_worker = DistributedWorker::new(store.clone(), "old", vec![old], 1, 180).unwrap();
    let new_worker = DistributedWorker::new(store.clone(), "new", vec![new], 1, 180).unwrap();
    old_worker.advertise().await.unwrap();
    new_worker.advertise().await.unwrap();
    let run_old = tokio::spawn(async move { old_worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(store.drain("old").await.unwrap());
    tokio::time::sleep(Duration::from_millis(240)).await;
    new_worker.run_once().await.unwrap();
    assert_eq!(
        store.get("new-job").await.unwrap().unwrap().instance.result,
        Some(json!("new"))
    );
    assert_eq!(
        store
            .get("old-backlog")
            .await
            .unwrap()
            .unwrap()
            .instance
            .status,
        "pending"
    );
    release.store(true, Ordering::SeqCst);
    run_old.await.unwrap();
    assert_eq!(
        store
            .get("old-running")
            .await
            .unwrap()
            .unwrap()
            .instance
            .result,
        Some(json!("old"))
    );
    assert_eq!(
        store
            .get("old-backlog")
            .await
            .unwrap()
            .unwrap()
            .instance
            .status,
        "pending"
    );
    drop(node);
}

#[tokio::test]
async fn draining_worker_does_not_refill_freed_slot_after_committed_drain() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release_slow = Arc::new(AtomicBool::new(false));
    let release_fast = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(&release_slow, &release_fast);
    let old_graph = refill_graph(
        &[0, 1],
        release_slow.clone(),
        release_fast.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let peer_graph = versioned_graph("1.0", Arc::new(|input, _| Ok(input.clone())), None);
    for id in 0..3 {
        let mut item = Instance::new(&id.to_string(), "orders", "1.0", "decide", json!(id));
        item.created_at = id + 1;
        item.checkpoint = Some(old_graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let old = DistributedWorker::new(store.clone(), "draining", vec![old_graph], 2, 900).unwrap();
    let peer = DistributedWorker::new(store.clone(), "peer", vec![peer_graph], 1, 900).unwrap();
    old.advertise().await.unwrap();
    peer.advertise().await.unwrap();
    let running = tokio::spawn(async move { old.run_once().await.unwrap() });
    let initial = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let slow = store.get("0").await.unwrap().unwrap();
            let fast = store.get("1").await.unwrap().unwrap();
            let backlog = store.get("2").await.unwrap().unwrap();
            if slow.owner_id.as_deref() == Some("draining")
                && fast.owner_id.as_deref() == Some("draining")
                && backlog.instance.status == "pending"
            {
                break slow;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let drained = store.drain("draining").await.unwrap();
    release_fast.store(true, Ordering::SeqCst);
    let freed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.get("1").await.unwrap().unwrap().instance.status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    // Allow a refill poll and more than one lease interval before checking the active claim.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let slow_while_draining = store.get("0").await.unwrap().unwrap();
    let backlog_before_peer = store.get("2").await.unwrap().unwrap();
    let peer_claimed = tokio::time::timeout(Duration::from_secs(5), peer.run_once()).await;
    let backlog_after_peer = store.get("2").await.unwrap().unwrap();
    release_slow.store(true, Ordering::SeqCst);
    let old_claimed = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("draining worker should finish after slow claim is released")
        .unwrap();
    assert!(
        initial.is_ok(),
        "both initial slots must be occupied with backlog queued"
    );
    assert!(drained, "drain request must commit before capacity frees");
    assert!(
        freed,
        "fast claim must complete while slow claim remains active"
    );
    assert_eq!(slow_while_draining.owner_id.as_deref(), Some("draining"));
    assert_eq!(slow_while_draining.instance.status, "running");
    assert!(
        slow_while_draining.lease_until > initial.unwrap().lease_until,
        "draining worker must renew its existing claim"
    );
    assert_eq!(backlog_before_peer.instance.status, "pending");
    assert!(backlog_before_peer.owner_id.is_none());
    assert_eq!(
        peer_claimed.expect("peer should finish promptly").unwrap(),
        1
    );
    assert_eq!(backlog_after_peer.instance.result, Some(json!(2)));
    assert_eq!(backlog_after_peer.generation, 1);
    assert_eq!(old_claimed, 2, "draining worker must not claim backlog");
    assert_eq!(
        store.get("0").await.unwrap().unwrap().instance.result,
        Some(json!(0))
    );
    drop(node);
}

#[tokio::test]
async fn retry_released_while_draining_goes_to_another_capable_worker() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicUsize::new(0));
    let task: blkit::runtime::Evaluate = Arc::new({
        let entered = entered.clone();
        let release = release.clone();
        let hits = hits.clone();
        move |_, _| {
            if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                entered.store(true, Ordering::SeqCst);
                for _ in 0..200 {
                    if release.load(Ordering::SeqCst) {
                        return Err("retry".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err("not released".into());
            }
            Ok(json!("recovered"))
        }
    });
    let policy = RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(3),
        retry_delay: Duration::from_millis(60),
        backoff: "exponential",
    };
    let graph1 = versioned_graph("1.0", task.clone(), Some(policy.clone()));
    let graph2 = versioned_graph("1.0", task, Some(policy));
    let mut item = Instance::new("job", "orders", "1.0", "decide", json!(null));
    item.checkpoint = Some(graph1.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let first = DistributedWorker::new(store.clone(), "draining", vec![graph1], 1, 500).unwrap();
    let second = DistributedWorker::new(store.clone(), "capable", vec![graph2], 1, 500).unwrap();
    first.advertise().await.unwrap();
    second.advertise().await.unwrap();
    let run = tokio::spawn(async move { first.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    store.drain("draining").await.unwrap();
    release.store(true, Ordering::SeqCst);
    run.await.unwrap();
    assert_eq!(
        store.get("job").await.unwrap().unwrap().instance.status,
        "retry-waiting"
    );
    tokio::time::sleep(Duration::from_millis(90)).await;
    assert_eq!(second.run_once().await.unwrap(), 1);
    assert_eq!(
        store.get("job").await.unwrap().unwrap().instance.result,
        Some(json!("recovered"))
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    drop(node);
}

#[tokio::test]
async fn distributed_rest_starts_reports_and_cancels_inflight_worker_without_late_commit() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let hooks = Arc::new(AtomicUsize::new(0));
    let task: blkit::runtime::Evaluate = Arc::new({
        let entered = entered.clone();
        let released = released.clone();
        move |input, _| {
            entered.store(true, Ordering::SeqCst);
            for _ in 0..200 {
                if released.load(Ordering::SeqCst) {
                    return Ok(input.clone());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("not signalled".into())
        }
    });
    let mut worker_graph = versioned_graph("1.0", task, None);
    worker_graph.nodes[1].kind = GraphNodeKind::TaskWithCancel(
        match &worker_graph.nodes[1].kind {
            GraphNodeKind::Task(call) => call.clone(),
            _ => unreachable!(),
        },
        Arc::new({
            let released = released.clone();
            let hooks = hooks.clone();
            move || {
                hooks.fetch_add(1, Ordering::SeqCst);
                released.store(true, Ordering::SeqCst);
            }
        }),
    );
    let api = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let worker = DistributedWorker::new(store.clone(), "w1", vec![worker_graph], 1, 500).unwrap();
    worker.advertise().await.unwrap();
    let start = api
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/processes/orders/1.0/decide/instances")
                .header("content-type", "application/json")
                .body(Body::from("7"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::ACCEPTED);
    let response: serde_json::Value =
        serde_json::from_slice(&to_bytes(start.into_body(), 4096).await.unwrap()).unwrap();
    let id = response["id"].as_str().unwrap().to_string();
    let run = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        store
            .get(&id)
            .await
            .unwrap()
            .unwrap()
            .instance
            .checkpoint
            .is_some(),
        "worker must persist the initial checkpoint before running tasks"
    );
    let cancel = api
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/instances/{id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancel.status(), StatusCode::ACCEPTED);
    let status = api
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/instances/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(status.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(body["status"], "cancelled");
    tokio::time::timeout(Duration::from_secs(3), async {
        while hooks.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("remote cancellation should signal the in-flight hook");
    run.await.unwrap();
    assert_eq!(hooks.load(Ordering::SeqCst), 1);
    let terminal = store.get(&id).await.unwrap().unwrap();
    assert_eq!(terminal.instance.status, "cancelled");
    assert!(
        !terminal
            .instance
            .checkpoint
            .unwrap()
            .completed
            .contains_key("work")
    );
    assert!(terminal.instance.result.is_none());
    drop(node);
}

#[tokio::test]
async fn cancellation_racing_checkpoint_or_lease_expiry_never_retries_or_overwrites_cancel() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("w1", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    let item = Instance::new("job", "orders", "1.0", "decide", json!(null));
    store.create(&item).await.unwrap();
    let claim = store.claim("w1", 1, 70).await.unwrap().pop().unwrap();
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("late".into(), json!(1));
    let (cancel, commit) = tokio::join!(
        store.cancel("job"),
        store.commit_checkpoint("job", "w1", claim.generation, &checkpoint)
    );
    cancel.unwrap();
    let committed = commit.unwrap();
    let row = store.get("job").await.unwrap().unwrap();
    assert_eq!(row.instance.status, "cancelled");
    assert_eq!(
        row.instance
            .checkpoint
            .as_ref()
            .is_some_and(|c| c.completed.contains_key("late")),
        committed
    );
    assert!(
        !store
            .commit_checkpoint("job", "w1", claim.generation, &checkpoint)
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(95)).await;
    assert_eq!(
        store
            .reconcile_expired(
                "job",
                Some(&RetryPolicy {
                    max_retries: 1,
                    retry_for: Duration::from_secs(2),
                    retry_delay: Duration::from_millis(1),
                    backoff: "exponential"
                })
            )
            .await
            .unwrap(),
        None
    );
    assert!(store.claim("w1", 1, 100).await.unwrap().is_empty());
    drop(node);
}

#[tokio::test]
async fn distributed_api_binary_serves_graph_free_process_control_on_loopback() {
    let (_cloud, endpoint) = seaweed::start().await;
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    managed_runtime::prepare(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("test-worker", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let mut command = Command::new(env!("CARGO_BIN_EXE_blkit-api"));
    seaweed::configure(&mut command, &endpoint);
    managed_runtime::configure(&mut command, &url, &["api"]);
    let _files = select_runtime_env_file(&mut command, &url, "api");
    let _api = WorkerChild(
        command
            .arg(format!("127.0.0.1:{port}"))
            .current_dir(std::env::temp_dir())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("API binary should bind loopback");
    managed_runtime::assert_child_configuration(&_api.0, &["api"]);
    managed_runtime::assert_sessions(&url, &["api"]).await;
    let output = std::process::Command::new("curl")
        .args([
            "-fsS",
            "-H",
            "content-type: application/json",
            "-d",
            "{\"total\":\"1200\"}",
            &format!("{base}/processes/orders/1.0/decide/instances"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let started: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = started["id"].as_str().unwrap();
    let output = std::process::Command::new("curl")
        .args([
            "-fsS",
            "-X",
            "POST",
            &format!("{base}/instances/{id}/cancel"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = std::process::Command::new("curl")
        .args(["-fsS", &format!("{base}/instances/{id}")])
        .output()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["status"],
        "cancelled"
    );
    drop(node);
}

fn crash_test_graph(
    b_log: std::path::PathBuf,
    c_started: std::path::PathBuf,
    hold: std::path::PathBuf,
) -> GraphDefinition {
    GraphDefinition {
        namespace: "orders",
        version: "1.0",
        name: "parallel",
        deadline: None,
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(5),
            retry_delay: Duration::from_millis(50),
            backoff: "exponential",
        }),
        decode_input: Box::new(Ok::<serde_json::Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "b",
                kind: GraphNodeKind::Task(Arc::new(move |_, _| {
                    use std::io::Write;
                    writeln!(
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&b_log)
                            .map_err(|e| e.to_string())?,
                        "b"
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(json!(3))
                })),
            },
            GraphNode {
                name: "c",
                kind: GraphNodeKind::Task(Arc::new(move |_, _| {
                    std::fs::write(&c_started, "started").map_err(|e| e.to_string())?;
                    for _ in 0..200 {
                        if !hold.exists() {
                            return Ok(json!(5));
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err("held too long".into())
                })),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "fork",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "fork",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "b",
                value: None,
                condition: None,
                fallback: false,
                label: Some("left"),
            },
            GraphLink {
                source: "fork",
                target: "c",
                value: None,
                condition: None,
                fallback: false,
                label: Some("right"),
            },
            GraphLink {
                source: "b",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["b"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "c",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["c"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[test]
fn process_worker_child_ignores_ambient_role_without_fixture_marker() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_worker_child", "--test-threads=1"])
        .env(
            "BLKIT_WORKER_DATABASE_URL",
            format!(
                "postgres://worker:private@{}/db",
                listener.local_addr().unwrap()
            ),
        )
        .env_remove("BLKIT_CRASH_CHILD_DIR")
        .env("TOKIO_WORKER_THREADS", "2")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child helper activated without its fixture marker"
    );
    assert!(
        listener.accept().is_err(),
        "child helper used ambient database configuration"
    );
}

#[tokio::test]
async fn process_worker_child() {
    let Ok(dir) = std::env::var("BLKIT_CRASH_CHILD_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let url = std::env::var("BLKIT_WORKER_DATABASE_URL").unwrap();
    let id = std::env::var("BLKIT_CRASH_CHILD_ID").unwrap();
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker = DistributedWorker::new(
        store,
        &id,
        vec![crash_test_graph(
            dir.join("b.log"),
            dir.join("c.started"),
            dir.join("hold"),
        )],
        2,
        200,
    )
    .unwrap();
    worker.advertise().await.unwrap();
    loop {
        worker.run_once().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct CrashDir(std::path::PathBuf);
impl Drop for CrashDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn second_worker_process_resumes_c_after_owner_killed_without_replaying_b() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    managed_runtime::prepare(&url).await;
    let dir = std::env::temp_dir().join(format!("blkit-crash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _cleanup = CrashDir(dir.clone());
    std::fs::write(dir.join("hold"), "").unwrap();
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = crash_test_graph(dir.join("b.log"), dir.join("c.started"), dir.join("hold"));
    let mut item = Instance::new("crash-job", "orders", "1.0", "parallel", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let spawn = |id: &str| {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "process_worker_child", "--nocapture"])
            .env("BLKIT_CRASH_CHILD_DIR", &dir)
            .env("BLKIT_CRASH_CHILD_ID", id)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        managed_runtime::configure(&mut command, &url, &["worker"]);
        WorkerChild(command.spawn().unwrap())
    };
    let mut owner = spawn("owner");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store
                .get("crash-job")
                .await
                .unwrap()
                .unwrap()
                .instance
                .checkpoint
                .as_ref()
                .is_some_and(|state| state.completed.contains_key("b"))
                && dir.join("c.started").exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("B must commit while C is in flight");
    managed_runtime::assert_child_configuration(&owner.0, &["worker"]);
    managed_runtime::assert_sessions(&url, &["worker"]).await;
    let first_generation = store.get("crash-job").await.unwrap().unwrap().generation;
    let survivor = spawn("survivor");
    managed_runtime::assert_child_configuration(&survivor.0, &["worker"]);
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    std::fs::remove_file(dir.join("hold")).unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let row = store.get("crash-job").await.unwrap().unwrap();
            if row.instance.status == "completed" {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("surviving worker should reconcile lease and resume C");
    assert_eq!(completed.instance.result, Some(json!({"left":3,"right":5})));
    assert_eq!(completed.generation, first_generation + 1);
    assert_eq!(completed.instance.attempt, 2);
    assert_eq!(
        std::fs::read_to_string(dir.join("b.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let mut stale = GraphCheckpoint::default();
    stale.completed.insert("stale".into(), json!(99));
    assert!(
        !store
            .commit_checkpoint("crash-job", "owner", first_generation, &stale)
            .await
            .unwrap()
    );
    assert!(
        !store
            .finish_owned(
                "crash-job",
                "owner",
                first_generation,
                "completed",
                Some(json!(99)),
                None,
                None
            )
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .get("crash-job")
            .await
            .unwrap()
            .unwrap()
            .instance
            .result,
        Some(json!({"left":3,"right":5}))
    );
    drop(survivor);
    drop(owner);
    drop(node);
}

#[tokio::test]
async fn distributed_api_reconciles_expired_claims_without_a_live_worker() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = versioned_graph(
        "1.0",
        Arc::new(|input, _| Ok(input.clone())),
        Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(50),
            backoff: "exponential",
        }),
    );
    store
        .register_worker("lost", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    let mut item = Instance::new("job", "orders", "1.0", "decide", json!(7));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store
        .create_with_policy(&item, graph.retry.as_ref())
        .await
        .unwrap();
    store.claim("lost", 1, 40).await.unwrap();
    tokio::time::sleep(Duration::from_millis(70)).await;
    let api = DistributedControl::new(store.clone());
    assert_eq!(api.reconcile_once().await.unwrap(), 1);
    assert_eq!(api.reconcile_once().await.unwrap(), 0);
    let row = store.get("job").await.unwrap().unwrap();
    assert_eq!(row.instance.status, "retry-waiting");
    assert_eq!(row.owner_id, None);
    drop(node);
}

#[tokio::test]
async fn malformed_claim_does_not_stop_worker_or_abandon_other_instances() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut graph = versioned_graph("1.0", Arc::new(|input, _| Ok(input.clone())), None);
    graph.decode_input = Box::new(|input| {
        if input == json!(1) {
            Err("malformed input".into())
        } else {
            Ok(input)
        }
    });
    let mut bad = Instance::new("a-bad", "orders", "1.0", "decide", json!(1));
    bad.created_at = 1;
    store.create(&bad).await.unwrap();
    let mut good = Instance::new("b-good", "orders", "1.0", "decide", json!(2));
    good.created_at = 2;
    good.checkpoint = Some(graph.checkpoint(&good.input).unwrap());
    store.create(&good).await.unwrap();
    let worker = DistributedWorker::new(store.clone(), "worker", vec![graph], 2, 300).unwrap();
    worker.advertise().await.unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 2);
    assert_eq!(
        store.get("b-good").await.unwrap().unwrap().instance.result,
        Some(json!(2))
    );
    assert_eq!(
        store.get("a-bad").await.unwrap().unwrap().instance.status,
        "failed"
    );
    drop(node);
}

#[tokio::test]
async fn expired_lease_signals_inflight_hook_without_stopping_other_instances() {
    let file = std::env::temp_dir().join(format!("blkit-claim-events-{}.log", std::process::id()));
    let writer = std::sync::Mutex::new(std::fs::File::create(&file).unwrap());
    tracing::subscriber::set_global_default(tracing_subscriber::fmt().with_writer(writer).finish())
        .unwrap();
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let hooks = Arc::new(AtomicUsize::new(0));
    let call: blkit::runtime::Evaluate = Arc::new({
        let entered = entered.clone();
        let released = released.clone();
        move |input, _| {
            if *input == json!(1) {
                entered.store(true, Ordering::SeqCst);
                for _ in 0..200 {
                    if released.load(Ordering::SeqCst) {
                        return Ok(input.clone());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                return Err("lease hook never signalled".into());
            }
            Ok(input.clone())
        }
    });
    let mut graph = versioned_graph("1.0", call, None);
    graph.nodes[1].kind = GraphNodeKind::TaskWithCancel(
        match &graph.nodes[1].kind {
            GraphNodeKind::Task(call) => call.clone(),
            _ => unreachable!(),
        },
        Arc::new({
            let hooks = hooks.clone();
            let released = released.clone();
            move || {
                hooks.fetch_add(1, Ordering::SeqCst);
                released.store(true, Ordering::SeqCst);
            }
        }),
    );
    for (id, input) in [("lost", json!(1)), ("healthy", json!(2))] {
        let mut item = Instance::new(id, "orders", "1.0", "decide", input);
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let worker = DistributedWorker::new(store.clone(), "worker", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .execute("UPDATE instances SET lease_until=0 WHERE id='lost'", &[])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while hooks.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("lease loss should signal the running task, not wait for it to finish");
    running.await.unwrap();
    assert_eq!(hooks.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.get("healthy").await.unwrap().unwrap().instance.result,
        Some(json!(2))
    );
    assert_eq!(
        store.get("lost").await.unwrap().unwrap().instance.status,
        "running"
    );
    let logs = std::fs::read_to_string(&file).unwrap();
    assert!(logs.contains("lost") && logs.contains("claim"), "{logs}");
    assert!(logs.contains("claim lost during execution"), "{logs}");
    assert!(!logs.contains("postgres:postgres"), "{logs}");
    std::fs::remove_file(file).unwrap();
    drop(node);
}

#[tokio::test]
async fn api_reconciles_expired_old_version_after_linked_graph_rolls_forward() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let old = versioned_graph(
        "1.0",
        Arc::new(|input, _| Ok(input.clone())),
        Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(40),
            backoff: "exponential",
        }),
    );
    let old_worker = DistributedWorker::new(store.clone(), "old", vec![old], 1, 40).unwrap();
    old_worker.advertise().await.unwrap();
    let old_api = DistributedControl::new(store.clone());
    let id = old_api
        .start("orders", "1.0", "decide", json!(7))
        .await
        .unwrap();
    store.claim("old", 1, 40).await.unwrap();
    tokio::time::sleep(Duration::from_millis(70)).await;
    drop(old_api);
    drop(store);
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let new_api = DistributedControl::new(store.clone());
    assert_eq!(new_api.reconcile_once().await.unwrap(), 1);
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "retry-waiting");
    assert_eq!(waiting.instance.attempt, 1);
    assert!(
        waiting.instance.next_eligible_at.unwrap()
            >= waiting.instance.first_failure_at.unwrap() + 40
    );
    assert_eq!(waiting.owner_id, None);
    drop(node);
}

#[tokio::test]
async fn postgres_module_launches_isolated_database_on_podman_socket() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let version: i32 = client
        .query_one("SELECT current_setting('server_version_num')::INTEGER", &[])
        .await
        .unwrap()
        .get(0);
    assert!((170_000..180_000).contains(&version));
    drop(client);
    drop(node);
}

async fn remote_sse_event(body: &mut Body) -> (String, serde_json::Value) {
    use http_body_util::BodyExt;
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("SSE event timed out")
        .expect("stream closed")
        .unwrap();
    let text = std::str::from_utf8(frame.data_ref().unwrap()).unwrap();
    (
        text.lines()
            .find_map(|line| line.strip_prefix("event: "))
            .unwrap()
            .to_string(),
        serde_json::from_str(
            text.lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap(),
    )
}

async fn remote_sse_body(app: &axum::Router, id: &str) -> Body {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/instances/{id}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body()
}

async fn listen_hints(
    url: &str,
    channel: &str,
) -> (tokio_postgres::Client, tokio::sync::mpsc::Receiver<String>) {
    let (client, mut connection) = tokio_postgres::connect(url, NoTls).await.unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        while let Some(message) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
            match message {
                Ok(tokio_postgres::AsyncMessage::Notification(notification)) => {
                    if sender
                        .send(notification.payload().to_owned())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(_) => {}
                Err(error) => panic!("listener failed: {error}"),
            }
        }
    });
    client
        .batch_execute(&format!("LISTEN {channel}"))
        .await
        .unwrap();
    (client, receiver)
}

async fn expect_hint(receiver: &mut tokio::sync::mpsc::Receiver<String>, id: &str) {
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("committed transition must notify"),
        Some(id.to_owned())
    );
}

async fn expect_no_hint(receiver: &mut tokio::sync::mpsc::Receiver<String>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), receiver.recv())
            .await
            .is_err(),
        "unrelated or uncommitted transition must not notify"
    );
}

#[tokio::test]
async fn postgres_request_notifications_target_only_live_interested_replicas() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (_listen_a, mut a) = listen_hints(&url, "replica_a").await;
    let (_listen_b, mut b) = listen_hints(&url, "replica_b").await;
    let (_listen_c, mut c) = listen_hints(&url, "replica_c").await;
    let checkpoint = input_wait_graph().checkpoint(&json!(null)).unwrap();
    let request_id = checkpoint.requests()[0].id.clone();
    store
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let mut other = Instance::new("z-unwatched", "input", "1", "review", json!(null));
    other.checkpoint = Some(checkpoint.clone());
    store.create(&other).await.unwrap();
    expect_no_hint(&mut a).await;
    expect_no_hint(&mut b).await;
    db.execute("INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ('watched', 'replica_a', clock_timestamp() + interval '1 minute'), ('watched', 'replica_b', clock_timestamp() + interval '1 minute')", &[]).await.unwrap();
    store
        .register_worker("owner", &[("input", "1", "review")])
        .await
        .unwrap();
    let claimed = store.claim("owner", 1, 5000).await.unwrap().pop().unwrap();
    assert!(
        store
            .commit_initial_checkpoint(
                "watched",
                "owner",
                claimed.generation,
                &json!(null),
                &checkpoint
            )
            .await
            .unwrap()
    );
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut c).await;
    assert!(
        store
            .commit_checkpoint("watched", "owner", claimed.generation, &checkpoint)
            .await
            .unwrap()
    );
    expect_no_hint(&mut a).await;
    expect_no_hint(&mut b).await;
    store.cancel("z-unwatched").await.unwrap();
    expect_no_hint(&mut a).await;
    store
        .accept_response("watched", &request_id, &json!({"approved": true}))
        .await
        .unwrap();
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut c).await;
    store.cancel("watched").await.unwrap();
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut a).await;
}

#[tokio::test]
async fn postgres_retired_request_and_completed_instance_notify_only_interested_replicas() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (_listen_a, mut a) = listen_hints(&url, "replica_a").await;
    let (_listen_b, mut b) = listen_hints(&url, "replica_b").await;
    let (_listen_c, mut c) = listen_hints(&url, "replica_c").await;
    let checkpoint = input_wait_graph().checkpoint(&json!(null)).unwrap();
    let mut instance = Instance::new("watched", "input", "1", "review", json!(null));
    instance.checkpoint = Some(checkpoint.clone());
    store.create(&instance).await.unwrap();
    db.execute("INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ('watched', 'replica_a', clock_timestamp() + interval '1 minute'), ('watched', 'replica_b', clock_timestamp() + interval '1 minute')", &[]).await.unwrap();
    store
        .register_worker("owner", &[("input", "1", "review")])
        .await
        .unwrap();
    let claim = store.claim("owner", 1, 5000).await.unwrap().pop().unwrap();
    assert!(
        store
            .commit_checkpoint("watched", "owner", claim.generation, &checkpoint)
            .await
            .unwrap()
    );
    expect_no_hint(&mut a).await;
    expect_no_hint(&mut b).await;
    assert!(
        store
            .commit_checkpoint(
                "watched",
                "owner",
                claim.generation,
                &GraphCheckpoint::default()
            )
            .await
            .unwrap()
    );
    assert!(store.requests("watched").await.unwrap().is_empty());
    assert_eq!(
        store.get("watched").await.unwrap().unwrap().instance.status,
        "running"
    );
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut c).await;
    assert!(
        store
            .finish_owned(
                "watched",
                "owner",
                claim.generation,
                "completed",
                Some(json!(true)),
                None,
                None
            )
            .await
            .unwrap()
    );
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut c).await;
    expect_no_hint(&mut a).await;
}

#[tokio::test]
async fn postgres_request_notifications_rollback_expiry_and_interest_removal() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (_listen_a, mut a) = listen_hints(&url, "replica_a").await;
    let (_listen_b, mut b) = listen_hints(&url, "replica_b").await;
    store
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    assert!(db.execute("INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ('watched', $1, clock_timestamp() + interval '1 minute')", &[&"é".repeat(32)]).await.is_err(), "PostgreSQL channel names cannot exceed 63 bytes");
    db.execute("INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ('watched', 'replica_a', clock_timestamp() + interval '1 minute'), ('watched', 'replica_b', clock_timestamp() + interval '100 milliseconds')", &[]).await.unwrap();
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    let tx = db.transaction().await.unwrap();
    tx.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched', 'rolled-back', 'Prompt', $1)", &[&schema]).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(store.requests("watched").await.unwrap().is_empty());
    expect_no_hint(&mut a).await;
    // The second replica's lease lapses while its LISTEN connection stays open.
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched', 'committed', 'Prompt', $1)", &[&schema]).await.unwrap();
    expect_hint(&mut a, "watched").await;
    expect_no_hint(&mut b).await;
    db.execute("UPDATE instance_interests SET expires_at=clock_timestamp() + interval '1 minute' WHERE channel='replica_b'", &[]).await.unwrap();
    db.execute("UPDATE input_requests SET status='answered', response='{}' WHERE instance_id='watched' AND request_id='committed'", &[]).await.unwrap();
    expect_hint(&mut a, "watched").await;
    expect_hint(&mut b, "watched").await;
    db.execute(
        "DELETE FROM instance_interests WHERE channel='replica_a'",
        &[],
    )
    .await
    .unwrap();
    db.execute(
        "UPDATE instances SET status='cancelled' WHERE id='watched'",
        &[],
    )
    .await
    .unwrap();
    expect_hint(&mut b, "watched").await;
    expect_no_hint(&mut a).await;
}

#[tokio::test]
async fn remote_sse_listener_startup_failure_returns_http_error_and_retries() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let admin_url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    managed_runtime::prepare(&admin_url).await;
    let (admin, connection) = tokio_postgres::connect(&admin_url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    admin
        .batch_execute("ALTER ROLE blkit_api CONNECTION LIMIT 1")
        .await
        .unwrap();
    let url = managed_runtime::role_url(&admin_url, "api");
    let store =
        PostgresStore::connect_with_options(&url, 1, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    // Seed internal fixture state as admin; GET/SSE remain under the managed API login.
    let fixture =
        PostgresStore::connect_with_options(&admin_url, 1, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    fixture
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    drop(fixture);
    let app = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    // The request pool still serves GET, but the separate listener cannot acquire a session.
    assert!(store.get("watched").await.unwrap().is_some());
    let response = tokio::time::timeout(
        Duration::from_secs(4),
        app.clone().oneshot(
            Request::builder()
                .uri("/instances/watched/events")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .expect("SSE startup must return an HTTP error rather than hang")
    .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    admin
        .batch_execute("ALTER ROLE blkit_api CONNECTION LIMIT -1")
        .await
        .unwrap();
    let mut body = tokio::time::timeout(Duration::from_secs(5), remote_sse_body(&app, "watched"))
        .await
        .expect("background listener must retry once a session is available");
    assert_eq!(remote_sse_event(&mut body).await.0, "snapshot");
}

#[test]
fn remote_subscription_drop_after_runtime_stops_does_not_panic() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (node, registration, receiver) = runtime.block_on(async {
        let node = Postgres::default()
            .with_tag("17.6-alpine")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(node.get_host().await.unwrap().to_string());
        let url = format!(
            "postgres://postgres:postgres@{host}:{}/postgres",
            node.get_host_port_ipv4(5432).await.unwrap()
        );
        postgres_fixture::prepare_fresh_database_url(&url).await;
        let store =
            PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
                .await
                .unwrap();
        store
            .create(&Instance::new(
                "watched",
                "input",
                "1",
                "review",
                json!(null),
            ))
            .await
            .unwrap();
        let control = DistributedControl::new(store);
        let (_, receiver, registration) = control.subscribe("watched").await.unwrap();
        (node, registration, receiver)
    });
    runtime.block_on(async { drop(node) });
    drop(runtime);
    drop(receiver);
    drop(registration);
}

#[tokio::test]
async fn remote_sse_cancelled_registration_does_not_leave_a_live_interest() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["watched", "other"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let app = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut healthy = remote_sse_body(&app, "other").await;
    assert_eq!(remote_sse_event(&mut healthy).await.0, "snapshot");
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id='watched'",
        &[],
    )
    .await
    .unwrap();
    let pending = tokio::spawn({
        let app = app.clone();
        async move { remote_sse_body(&app, "watched").await }
    });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = observer.query_one("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("registration must be blocked by worker update");
    pending.abort();
    pending.await.unwrap_err();
    tx.commit().await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let count: i64 = observer
        .query_one(
            "SELECT count(*) FROM instance_interests WHERE instance_id='watched'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "aborted SSE registration must not be renewed");
}

#[tokio::test]
async fn remote_sse_blocked_registration_does_not_delay_other_instance_hints() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["blocked-a", "watched-b"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let app = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let mut b = remote_sse_body(&app, "watched-b").await;
    assert_eq!(remote_sse_event(&mut b).await.1["requests"], json!([]));
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id='blocked-a'",
        &[],
    )
    .await
    .unwrap();
    let connecting = tokio::spawn({
        let app = app.clone();
        async move { remote_sse_body(&app, "blocked-a").await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = observer.query_one("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("A registration must be waiting on the row lock");

    // A stalled registration must not prevent B's lapsed lease from renewing either.
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE instance_id='watched-b'", &[]).await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut b).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["id"], "watched-b");
    assert_eq!(snapshot["requests"], json!([]));
    let live: bool = observer.query_one("SELECT expires_at > clock_timestamp() FROM instance_interests WHERE instance_id='watched-b'", &[]).await.unwrap().get(0);
    assert!(live, "B interest must be renewed while A waits");

    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    observer.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched-b','b1','Prompt',$1)", &[&schema]).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), remote_sse_event(&mut b))
            .await
            .expect("B hint must arrive while A remains locked"),
        ("changed".into(), json!({"id":"watched-b"}))
    );
    assert!(!connecting.is_finished(), "A must still be blocked");
    tx.commit().await.unwrap();
    let mut a = connecting.await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut a).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["id"], "blocked-a");
    assert_eq!(snapshot["status"], "running");
    drop(a);
    drop(b);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let count: i64 = observer.query_one("SELECT count(*) FROM instance_interests WHERE instance_id IN ('blocked-a', 'watched-b')", &[]).await.unwrap().get(0);
            if count == 0 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("last subscribers must remove both interests");
}

#[tokio::test]
async fn remote_sse_duplicate_blocked_instance_hints_do_not_starve_other_instances() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["blocked-a", "watched-b"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let app = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut a = remote_sse_body(&app, "blocked-a").await;
    let mut b = remote_sse_body(&app, "watched-b").await;
    assert_eq!(remote_sse_event(&mut a).await.0, "snapshot");
    assert_eq!(remote_sse_event(&mut b).await.0, "snapshot");
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let channel: String = observer
        .query_one(
            "SELECT channel FROM instance_interests WHERE instance_id='blocked-a'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id='blocked-a'",
        &[],
    )
    .await
    .unwrap();
    let connecting = tokio::spawn({
        let app = app.clone();
        async move { remote_sse_body(&app, "blocked-a").await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = observer.query_one("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("A registration must hold the per-instance order lock");
    for _ in 0..32 {
        observer
            .execute("SELECT pg_notify($1, 'blocked-a')", &[&channel])
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    observer.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched-b','b1','Prompt',$1)", &[&schema]).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), remote_sse_event(&mut b))
            .await
            .expect("B hint must arrive while 32 A hints wait"),
        ("changed".into(), json!({"id":"watched-b"}))
    );
    assert!(!connecting.is_finished());
    tx.commit().await.unwrap();
    let mut connected = connecting.await.unwrap();
    assert_eq!(
        remote_sse_event(&mut connected).await.1["status"],
        "running"
    );
}

#[tokio::test]
async fn remote_sse_renews_more_than_32_instances_without_starving_hints() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let app = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let mut watched = Vec::new();
    for index in 0..36 {
        let id = format!("renew-{index}");
        store
            .create(&Instance::new(&id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
        let mut body = remote_sse_body(&app, &id).await;
        assert_eq!(remote_sse_event(&mut body).await.0, "snapshot");
        watched.push(body);
    }
    store
        .create(&Instance::new(
            "watched-b",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let mut b = remote_sse_body(&app, "watched-b").await;
    assert_eq!(remote_sse_event(&mut b).await.0, "snapshot");
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE instance_id LIKE 'renew-%'", &[]).await.unwrap();
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id IN ('renew-0', 'renew-1')",
        &[],
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = observer.query_one("SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock'", &[]).await.unwrap().get(0);
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("renewals must reach the locked instance rows");
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    observer.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched-b','b1','Prompt',$1)", &[&schema]).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), remote_sse_event(&mut b))
            .await
            .expect("B hint must not wait for renewal of 36 watched instances"),
        ("changed".into(), json!({"id":"watched-b"}))
    );
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE instance_id='watched-b'", &[]).await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut b).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["requests"][0]["id"], "b1");
    let b_live: bool = observer.query_one("SELECT expires_at > clock_timestamp() FROM instance_interests WHERE instance_id='watched-b'", &[]).await.unwrap().get(0);
    assert!(
        b_live,
        "B lease must be renewed before the other rows unlock"
    );
    tx.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let live: i64 = observer.query_one("SELECT count(*) FROM instance_interests WHERE instance_id LIKE 'renew-%' AND expires_at > clock_timestamp()", &[]).await.unwrap().get(0);
            if live == 36 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("all lapsed interests must eventually renew");
    assert_eq!(remote_sse_event(&mut watched[0]).await.0, "snapshot");
}

#[tokio::test]
async fn remote_sse_retry_waiting_behind_registrations_does_not_starve_other_renewals() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["locked-a0", "locked-a1", "watched-b"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let app = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut existing = Vec::new();
    for id in ["locked-a0", "locked-a1", "watched-b"] {
        let mut body = remote_sse_body(&app, id).await;
        assert_eq!(remote_sse_event(&mut body).await.0, "snapshot");
        existing.push(body);
    }
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE instance_id LIKE 'locked-a%'", &[]).await.unwrap();
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id IN ('locked-a0', 'locked-a1')",
        &[],
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = observer.query_one("SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock'", &[]).await.unwrap().get(0);
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("both renewal transactions must wait for the A row locks");
    let mut registrations = Vec::new();
    for id in ["locked-a0", "locked-a1"] {
        let app = app.clone();
        registrations.push(tokio::spawn(async move { remote_sse_body(&app, id).await }));
    }
    // A renewal query cannot stay locked for this long: these are the queued subscriptions.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = observer.query_one("SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock' AND clock_timestamp() - query_start > interval '850 milliseconds'", &[]).await.unwrap().get(0);
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("both A subscriptions must hold their order locks after renewal timeout");
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE instance_id='watched-b'", &[]).await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut existing[2]).await;
    assert_eq!(
        kind, "snapshot",
        "B renewal must not wait for the two A registrations"
    );
    assert_eq!(snapshot["id"], "watched-b");
    let live: bool = observer.query_one("SELECT expires_at > clock_timestamp() FROM instance_interests WHERE instance_id='watched-b'", &[]).await.unwrap().get(0);
    assert!(live, "B interest must renew while A remains locked");
    assert!(
        registrations
            .iter()
            .all(|registration| !registration.is_finished())
    );
    tx.commit().await.unwrap();
    for registration in registrations {
        let mut body = registration.await.unwrap();
        assert_eq!(remote_sse_event(&mut body).await.1["status"], "running");
    }
}

#[tokio::test]
async fn remote_sse_lagged_client_closes_and_reconnects_to_current_state() {
    use http_body_util::BodyExt;
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let app = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut slow = remote_sse_body(&app, "watched").await;
    assert_eq!(remote_sse_event(&mut slow).await.0, "snapshot");
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let channel: String = db
        .query_one(
            "SELECT channel FROM instance_interests WHERE instance_id='watched'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    for _ in 0..24 {
        db.execute("SELECT pg_notify($1, 'watched')", &[&channel])
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(frame) = slow.frame().await {
            frame.unwrap();
        }
    })
    .await
    .expect("lagged stream must close rather than silently drop events");
    let mut fresh = remote_sse_body(&app, "watched").await;
    assert_eq!(remote_sse_event(&mut fresh).await.0, "snapshot");
    let unknown = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/instances/absent/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    drop(fresh);
    drop(slow);
    drop(app);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let count: i64 = db
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1",
                    &[&channel],
                )
                .await
                .unwrap()
                .get(0);
            if count == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("dropping the replica must release its dedicated listener");
}

#[tokio::test]
async fn remote_sse_registration_serializes_both_orders_of_request_commit() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let app = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (mut registering, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    for id in ["before", "after"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let mut second = remote_sse_body(&app, "after").await;
    assert_eq!(remote_sse_event(&mut second).await.1["requests"], json!([]));
    // LISTEN is committed; worker has updated the row but its request INSERT/trigger has not committed.
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id='before'",
        &[],
    )
    .await
    .unwrap();
    tx.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('before','r1','Prompt',$1)", &[&schema]).await.unwrap();
    let connecting = tokio::spawn({
        let app = app.clone();
        async move { remote_sse_body(&app, "before").await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = observer.query_one("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("registration must reach the instance lock before worker commit");
    assert!(!connecting.is_finished());
    tx.commit().await.unwrap();
    let mut first = connecting.await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut first).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["requests"][0]["id"], "r1");

    // The registration transaction's SHARE lock forces the later worker update to wait.
    let registering_tx = registering.transaction().await.unwrap();
    registering_tx
        .query_one("SELECT id FROM instances WHERE id='after' FOR SHARE", &[])
        .await
        .unwrap();
    registering_tx.execute("UPDATE instance_interests SET expires_at=clock_timestamp() + interval '10 seconds' WHERE instance_id='after'", &[]).await.unwrap();
    let worker = tokio::spawn(async move {
        let tx = db.transaction().await.unwrap();
        tx.execute(
            "UPDATE instances SET status='running' WHERE id='after'",
            &[],
        )
        .await
        .unwrap();
        tx.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('after','r2','Prompt',$1)", &[&schema]).await.unwrap();
        tx.commit().await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = observer.query_one("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query LIKE 'UPDATE instances SET status=''running'' WHERE id=''after''%' AND wait_event_type='Lock')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("worker update must wait for registration commit");
    registering_tx.commit().await.unwrap();
    worker.await.unwrap();
    assert_eq!(
        remote_sse_event(&mut second).await,
        ("changed".into(), json!({"id":"after"}))
    );
    assert_eq!(
        store.get("after").await.unwrap().unwrap().requests[0].id,
        "r2"
    );
}

#[tokio::test]
async fn remote_sse_fans_out_only_to_each_replicas_local_instance_clients() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["watched", "other"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let a = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let b = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let c = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let mut a1 = remote_sse_body(&a, "watched").await;
    let mut a2 = remote_sse_body(&a, "watched").await;
    let mut b1 = remote_sse_body(&b, "watched").await;
    let mut c1 = remote_sse_body(&c, "other").await;
    for body in [&mut a1, &mut a2, &mut b1, &mut c1] {
        assert_eq!(remote_sse_event(body).await.0, "snapshot");
    }
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let count: i64 = db
        .query_one(
            "SELECT count(*) FROM instance_interests WHERE instance_id='watched'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        count, 2,
        "two replicas register once each, not once per client"
    );
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched','r1','Prompt',$1)", &[&schema]).await.unwrap();
    for body in [&mut a1, &mut a2, &mut b1] {
        assert_eq!(
            remote_sse_event(body).await,
            ("changed".into(), json!({"id":"watched"}))
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(150), remote_sse_event(&mut c1))
            .await
            .is_err()
    );
    drop(a1);
    drop(a2);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let count: i64 = db
                .query_one(
                    "SELECT count(*) FROM instance_interests WHERE instance_id='watched'",
                    &[],
                )
                .await
                .unwrap()
                .get(0);
            if count == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    db.execute("UPDATE input_requests SET status='answered', response='{}' WHERE instance_id='watched' AND request_id='r1'", &[]).await.unwrap();
    assert_eq!(remote_sse_event(&mut b1).await.1, json!({"id":"watched"}));
    assert!(
        tokio::time::timeout(Duration::from_millis(150), remote_sse_event(&mut c1))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn remote_sse_two_replicas_use_distinct_listener_sessions_and_fan_out() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 1, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let first = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let second = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut first_stream = remote_sse_body(&first, "watched").await;
    let mut second_stream = remote_sse_body(&second, "watched").await;
    assert_eq!(remote_sse_event(&mut first_stream).await.0, "snapshot");
    assert_eq!(remote_sse_event(&mut second_stream).await.0, "snapshot");

    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let sessions = db
        .query(
            "SELECT channel, pid, state FROM instance_interests JOIN pg_stat_activity ON application_name=channel WHERE instance_id='watched' AND expires_at > clock_timestamp() ORDER BY channel",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        sessions.len(),
        2,
        "each replica needs a registered listener session"
    );
    let first_channel: String = sessions[0].get(0);
    let second_channel: String = sessions[1].get(0);
    assert_ne!(first_channel, second_channel);
    assert_ne!(sessions[0].get::<_, i32>(1), sessions[1].get::<_, i32>(1));
    for session in &sessions {
        assert_eq!(
            session.get::<_, String>(2),
            "idle",
            "LISTEN must not hold a transaction"
        );
    }
    let queue_before: f64 = db
        .query_one("SELECT pg_notification_queue_usage()", &[])
        .await
        .unwrap()
        .get(0);

    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched','r1','Prompt',$1)", &[&schema]).await.unwrap();
    for stream in [&mut first_stream, &mut second_stream] {
        assert_eq!(
            remote_sse_event(stream).await,
            ("changed".into(), json!({"id":"watched"}))
        );
    }
    let queue_after: f64 = db
        .query_one("SELECT pg_notification_queue_usage()", &[])
        .await
        .unwrap()
        .get(0);
    assert!((0.0..=1.0).contains(&queue_before));
    assert!((0.0..=1.0).contains(&queue_after));
    println!("pg_notification_queue_usage(): before={queue_before}, after={queue_after}");
}

#[tokio::test]
async fn postgres_request_transition_fanout_load_check() {
    use std::{collections::HashMap, time::Instant};
    use tokio::{sync::Semaphore, task::JoinSet};

    const N: usize = 24;
    const CONCURRENCY: usize = 8;
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let worker = PostgresStore::connect_with_options(
        &format!("{url}?application_name=load_worker"),
        4,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let api_a = router_distributed(Arc::new(DistributedControl::new(
        PostgresStore::connect_with_options(
            &format!("{url}?application_name=load_api_a"),
            2,
            &postgres_fixture::plaintext_options(),
        )
        .await
        .unwrap(),
    )));
    let api_b = router_distributed(Arc::new(DistributedControl::new(
        PostgresStore::connect_with_options(
            &format!("{url}?application_name=load_api_b"),
            2,
            &postgres_fixture::plaintext_options(),
        )
        .await
        .unwrap(),
    )));
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let max_connections: String = observer
        .query_one("SHOW max_connections", &[])
        .await
        .unwrap()
        .get(0);
    let version: String = observer
        .query_one("SHOW server_version", &[])
        .await
        .unwrap()
        .get(0);
    let mut graph = input_wait_graph();
    graph.deadline = None;
    let checkpoint = graph.checkpoint(&json!(null)).unwrap();
    let request_id = checkpoint.requests()[0].id.clone();
    for prefix in ["watched", "unwatched"] {
        for i in 0..N {
            worker
                .create(&Instance::new(
                    &format!("{prefix}-{i}"),
                    "input",
                    "1",
                    "review",
                    json!(null),
                ))
                .await
                .unwrap();
        }
    }
    worker
        .register_worker("load-owner", &[("input", "1", "review")])
        .await
        .unwrap();
    let claims = worker.claim("load-owner", N * 2, 120_000).await.unwrap();
    assert_eq!(claims.len(), N * 2);
    assert!(
        claims
            .iter()
            .all(|claim| claim.instance.deadline_origin.is_none()
                && claim.instance.deadline_duration_ms.is_none()
                && claim.instance.deadline_at_ms.is_none()),
        "claimed load instances must not have deadlines"
    );
    let claims: HashMap<_, _> = claims
        .into_iter()
        .map(|claim| (claim.instance.id, claim.generation))
        .collect();
    let mut streams = HashMap::new();
    for i in 0..N {
        let id = format!("watched-{i}");
        let mut a = remote_sse_body(&api_a, &id).await;
        let mut b = remote_sse_body(&api_b, &id).await;
        assert_eq!(remote_sse_event(&mut a).await.0, "snapshot");
        assert_eq!(remote_sse_event(&mut b).await.0, "snapshot");
        streams.insert(id, (a, b));
    }
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let monitor = tokio::spawn(async move {
        let mut peak = [0_i64; 5]; // API A, API B, worker, dedicated listeners, all sessions.
        let mut queues = Vec::new();
        loop {
            let row = observer
                .query_one(
                    "SELECT pg_notification_queue_usage(),
                    (SELECT count(*) FROM pg_stat_activity WHERE application_name='load_api_a'),
                    (SELECT count(*) FROM pg_stat_activity WHERE application_name='load_api_b'),
                    (SELECT count(*) FROM pg_stat_activity WHERE application_name='load_worker'),
                    (SELECT count(*) FROM pg_stat_activity WHERE application_name LIKE 'blkit_%'),
                    (SELECT count(*) FROM pg_stat_activity WHERE datname=current_database())",
                    &[],
                )
                .await
                .unwrap();
            queues.push(row.get::<_, f64>(0));
            for (i, max) in peak.iter_mut().enumerate() {
                *max = (*max).max(row.get::<_, i64>(i + 1));
            }
            if *stopped.borrow() {
                break;
            }
            tokio::select! {
                _ = stopped.changed() => {},
                _ = tokio::time::sleep(Duration::from_millis(20)) => {},
            }
        }
        (peak, queues)
    });
    let limit = Arc::new(Semaphore::new(CONCURRENCY));
    let mut watched_commits = Vec::new();
    let mut unwatched_commits = Vec::new();
    let mut hints = Vec::new();
    let workload_start = Instant::now();
    for prefix in ["watched", "unwatched"] {
        let mut running = JoinSet::new();
        for i in 0..N {
            let id = format!("{prefix}-{i}");
            let generation = claims[&id];
            let worker = worker.clone();
            let checkpoint = checkpoint.clone();
            let streams = streams.remove(&id);
            let permit = limit.clone().acquire_owned().await.unwrap();
            running.spawn(async move {
                let _permit = permit;
                let start = Instant::now();
                assert!(
                    worker
                        .commit_initial_checkpoint(
                            &id,
                            "load-owner",
                            generation,
                            &json!(null),
                            &checkpoint
                        )
                        .await
                        .unwrap()
                );
                let commit = start.elapsed();
                let (hints, streams) = if let Some((mut a, mut b)) = streams {
                    assert_eq!(
                        remote_sse_event(&mut a).await,
                        ("changed".into(), json!({"id": id}))
                    );
                    let first = start.elapsed();
                    assert_eq!(
                        remote_sse_event(&mut b).await,
                        ("changed".into(), json!({"id": id}))
                    );
                    (vec![first, start.elapsed()], Some((a, b)))
                } else {
                    (Vec::new(), None)
                };
                (id, commit, hints, streams)
            });
        }
        while let Some(result) = running.join_next().await {
            let (id, commit, notified, bodies) = result.unwrap();
            if let Some(bodies) = bodies {
                watched_commits.push(commit);
                hints.extend(notified);
                streams.insert(id, bodies);
            } else {
                unwatched_commits.push(commit);
            }
        }
    }
    assert_eq!(watched_commits.len(), N);
    assert_eq!(unwatched_commits.len(), N);
    assert_eq!(hints.len(), N * 2);
    let initial_elapsed = workload_start.elapsed();

    // Updating a checkpoint with unchanged requests must produce no event on either replica.
    assert!(
        worker
            .commit_checkpoint("watched-0", "load-owner", claims["watched-0"], &checkpoint)
            .await
            .unwrap()
    );
    let (a, b) = streams.get_mut("watched-0").unwrap();
    let (no_a, no_b) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(200), remote_sse_event(a)),
        tokio::time::timeout(Duration::from_millis(200), remote_sse_event(b)),
    );
    assert!(
        no_a.is_err() && no_b.is_err(),
        "ordinary checkpoint must not NOTIFY"
    );

    let mut replies = Vec::new();
    let reply_start = Instant::now();
    let mut running = JoinSet::new();
    for i in 0..N {
        let id = format!("watched-{i}");
        let app = if i % 2 == 0 {
            api_a.clone()
        } else {
            api_b.clone()
        };
        let request_id = request_id.clone();
        let (mut a, mut b) = streams.remove(&id).unwrap();
        let permit = limit.clone().acquire_owned().await.unwrap();
        running.spawn(async move {
            let _permit = permit;
            let start = Instant::now();
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/instances/{id}/requests/{request_id}/response"))
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"approved":true}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let commit = start.elapsed(); // HTTP acknowledgement includes validation and committed POST.
            assert_eq!(
                remote_sse_event(&mut a).await,
                ("changed".into(), json!({"id": id}))
            );
            let first = start.elapsed();
            assert_eq!(
                remote_sse_event(&mut b).await,
                ("changed".into(), json!({"id": id}))
            );
            (commit, [first, start.elapsed()])
        });
    }
    while let Some(result) = running.join_next().await {
        let (commit, notified) = result.unwrap();
        replies.push(commit);
        hints.extend(notified);
    }
    assert_eq!(replies.len(), N);
    assert_eq!(hints.len(), N * 4);
    let reply_elapsed = reply_start.elapsed();
    stop.send(true).unwrap();
    let (peak, queues) = monitor.await.unwrap();
    assert!(peak[0] <= 2 && peak[1] <= 2 && peak[2] <= 4);
    assert_eq!(peak[3], 2, "one dedicated LISTEN session per API replica");
    assert!(
        peak[4] <= 11,
        "two API pools (2 each), worker pool (4), listeners (2), observer (1)"
    );
    assert!(queues.iter().all(|usage| (0.0..=1.0).contains(usage)));
    assert!(
        queues.last().unwrap() <= &queues[0],
        "notification queue must not remain elevated after load"
    );
    let stats = |label: &str, mut values: Vec<Duration>| {
        values.sort();
        let ms = |i: usize| values[i].as_secs_f64() * 1000.0;
        println!(
            "{label}: n={} median={:.3}ms p95={:.3}ms max={:.3}ms",
            values.len(),
            ms(values.len() / 2),
            ms(values.len() * 95 / 100),
            ms(values.len() - 1)
        );
    };
    stats("watched worker commit", watched_commits);
    stats("unwatched worker commit", unwatched_commits);
    stats("watched HTTP response acknowledgement", replies);
    stats("SSE hints (pre-commit to both replicas)", hints);
    println!(
        "initial_elapsed={initial_elapsed:?} reply_elapsed={reply_elapsed:?} concurrency={CONCURRENCY} instances_per_group={N} queue_samples={} queue_fraction_start={} queue_fraction_max={} queue_fraction_end={} peak_sessions=[api_a:{},api_b:{},worker:{},listeners:{},total:{}] pg_version={version} max_connections={max_connections}",
        queues.len(),
        queues[0],
        queues.iter().copied().fold(0.0_f64, f64::max),
        queues.last().unwrap(),
        peak[0],
        peak[1],
        peak[2],
        peak[3],
        peak[4]
    );
}

#[tokio::test]
async fn remote_sse_renews_resynced_b_while_a_reconnect_is_locked() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    for id in ["locked-a0", "locked-a1", "watched-b"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
    }
    let app = router_distributed(Arc::new(DistributedControl::new(store)));
    let mut a0 = remote_sse_body(&app, "locked-a0").await;
    let mut a1 = remote_sse_body(&app, "locked-a1").await;
    let mut b = remote_sse_body(&app, "watched-b").await;
    for body in [&mut a0, &mut a1, &mut b] {
        assert_eq!(remote_sse_event(body).await.0, "snapshot");
    }
    let (mut db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let channel: String = observer
        .query_one(
            "SELECT channel FROM instance_interests WHERE instance_id='watched-b'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let tx = db.transaction().await.unwrap();
    tx.execute(
        "UPDATE instances SET status='running' WHERE id IN ('locked-a0','locked-a1')",
        &[],
    )
    .await
    .unwrap();
    let pid: i32 = observer
        .query_one(
            "SELECT pid FROM pg_stat_activity WHERE application_name=$1",
            &[&channel],
        )
        .await
        .unwrap()
        .get(0);
    observer
        .execute("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = observer.query_one("SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT id FROM instances WHERE id=$1 FOR SHARE%' AND wait_event_type='Lock'", &[]).await.unwrap().get(0);
            if blocked == 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("both A registrations must block the two resync slots");
    let (kind, snapshot) = remote_sse_event(&mut b).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["id"], "watched-b");
    // Expire B before A unlocks: its recovered stream must remain able to receive hints.
    observer.execute("UPDATE instance_interests SET expires_at=clock_timestamp() + interval '3 seconds' WHERE instance_id='watched-b'", &[]).await.unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    let live: bool = observer.query_one("SELECT expires_at > clock_timestamp() FROM instance_interests WHERE instance_id='watched-b'", &[]).await.unwrap().get(0);
    assert!(live, "B must renew during A's blocked resync");
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    observer.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched-b','b1','Prompt',$1)", &[&schema]).await.unwrap();
    assert_eq!(
        remote_sse_event(&mut b).await,
        ("changed".into(), json!({"id":"watched-b"}))
    );
    for body in [&mut a0, &mut a1] {
        assert!(
            tokio::time::timeout(Duration::from_millis(150), remote_sse_event(body))
                .await
                .is_err(),
            "A must not claim a resync snapshot before its lock is released"
        );
    }
    tx.commit().await.unwrap();
    for body in [&mut a0, &mut a1] {
        assert_eq!(remote_sse_event(body).await.1["status"], "running");
    }
}

#[tokio::test]
async fn remote_sse_recovers_expired_interest_duplicate_hints_and_listener_outage() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 1, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .create(&Instance::new(
            "watched",
            "input",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    store
        .create(&Instance::new("other", "input", "1", "review", json!(null)))
        .await
        .unwrap();
    let app = router_distributed(Arc::new(DistributedControl::new(store.clone())));
    let mut body = remote_sse_body(&app, "watched").await;
    assert_eq!(remote_sse_event(&mut body).await.1["requests"], json!([]));
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let channel: String = db
        .query_one(
            "SELECT channel FROM instance_interests WHERE instance_id='watched'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let state: String = db
        .query_one(
            "SELECT state FROM pg_stat_activity WHERE application_name=$1",
            &[&channel],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        state, "idle",
        "LISTEN must commit without leaving an open transaction"
    );
    db.execute("SELECT pg_notify($1, 'watched')", &[&channel])
        .await
        .unwrap();
    assert_eq!(remote_sse_event(&mut body).await.1, json!({"id":"watched"}));
    db.execute("SELECT pg_notify($1, 'watched')", &[&channel])
        .await
        .unwrap();
    assert_eq!(remote_sse_event(&mut body).await.1, json!({"id":"watched"}));
    db.execute("UPDATE instance_interests SET expires_at=clock_timestamp() - interval '1 second' WHERE channel=$1", &[&channel]).await.unwrap();
    db.execute(
        "UPDATE instances SET status='cancelled' WHERE id='other'",
        &[],
    )
    .await
    .unwrap();
    let schema = serde_json::to_string(
        &input_wait_graph()
            .checkpoint(&json!(null))
            .unwrap()
            .requests()[0]
            .schema,
    )
    .unwrap();
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('watched','r1','Prompt',$1)", &[&schema]).await.unwrap();
    // A new local subscriber must repair the old subscriber even before the renewal tick.
    let mut joining = remote_sse_body(&app, "watched").await;
    assert_eq!(
        remote_sse_event(&mut joining).await.1["requests"][0]["id"],
        "r1"
    );
    let (kind, snapshot) =
        tokio::time::timeout(Duration::from_secs(1), remote_sse_event(&mut body))
            .await
            .expect("joining client repairs lapsed live interest");
    assert_eq!(
        kind, "snapshot",
        "expired live registration must force a resync"
    );
    assert_eq!(snapshot["requests"][0]["id"], "r1");
    let pid: i32 = db.query_one("SELECT pid FROM pg_stat_activity WHERE application_name=$1 AND backend_type='client backend'", &[&channel]).await.unwrap().get(0);
    db.execute("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .unwrap();
    db.execute("UPDATE input_requests SET status='answered', response='{}' WHERE instance_id='watched' AND request_id='r1'", &[]).await.unwrap();
    let (kind, snapshot) = remote_sse_event(&mut body).await;
    assert_eq!(
        kind, "snapshot",
        "attached client must resync after listener restart"
    );
    assert_eq!(snapshot["requests"], json!([]));
    drop(body);
    drop(joining);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let count: i64 = db
                .query_one(
                    "SELECT count(*) FROM instance_interests WHERE channel=$1",
                    &[&channel],
                )
                .await
                .unwrap()
                .get(0);
            if count == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

fn input_wait_graph() -> GraphDefinition {
    use blkit::compiled_graph::{RequestSchema, RequestType};
    GraphDefinition {
        namespace: "input",
        version: "1",
        name: "review",
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin: "first_claimed",
            duration: Duration::from_secs(10),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "review",
                kind: GraphNodeKind::InputRequest {
                    prompt: "Approve this order?",
                    schema: RequestSchema {
                        ports: [("approved".into(), RequestType::Named("Bool".into()))].into(),
                        records: Default::default(),
                        enums: Default::default(),
                    },
                    decode_response: Box::new(Ok),
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "review",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "review",
                target: "done",
                value: Some(Arc::new(|_, values| {
                    Ok(values["review"]["approved"].clone())
                })),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn distributed_http_graph_free_request_visibility_validation_and_response() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let worker_store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker = DistributedWorker::new(
        worker_store.clone(),
        "w1",
        vec![input_wait_graph()],
        1,
        1000,
    )
    .unwrap();
    worker.advertise().await.unwrap();
    let api_store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let app = router_distributed(Arc::new(DistributedControl::new(api_store)));
    let start = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/processes/input/1/review/instances")
                .header("content-type", "application/json")
                .body(Body::from("null"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::ACCEPTED);
    let started: serde_json::Value =
        serde_json::from_slice(&to_bytes(start.into_body(), 4096).await.unwrap()).unwrap();
    let id = started["id"].as_str().unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let get = |id: String| {
        Request::builder()
            .uri(format!("/instances/{id}"))
            .body(Body::empty())
            .unwrap()
    };
    let state = app.clone().oneshot(get(id.into())).await.unwrap();
    assert_eq!(state.status(), StatusCode::OK);
    let state: serde_json::Value =
        serde_json::from_slice(&to_bytes(state.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(state["status"], "waiting-for-input");
    assert_eq!(state["requests"].as_array().unwrap().len(), 1);
    assert_eq!(state["requests"][0]["prompt"], "Approve this order?");
    assert_eq!(
        state["requests"][0]["schema"]["ports"]["approved"],
        json!({"Named":"Bool"})
    );
    assert!(state.get("checkpoint").is_none());
    let events = app
        .clone()
        .oneshot(get(format!("{id}/events")))
        .await
        .unwrap();
    assert_eq!(events.status(), StatusCode::OK);
    let mut events = events.into_body();
    assert_eq!(remote_sse_event(&mut events).await.0, "snapshot");
    let request_id = state["requests"][0]["id"].as_str().unwrap();
    let post = |id: &str, request_id: &str, body: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/instances/{id}/requests/{request_id}/response"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(post(id, request_id, "{"))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    for invalid in [
        "null",
        "{}",
        r#"{"approved":"yes"}"#,
        r#"{"approved":true,"extra":0}"#,
    ] {
        assert_eq!(
            app.clone()
                .oneshot(post(id, request_id, invalid))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(post("unknown", request_id, r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.clone()
            .oneshot(post(id, "missing", r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let accepted = app
        .clone()
        .oneshot(post(id, request_id, r#"{"approved":true}"#))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let accepted: serde_json::Value =
        serde_json::from_slice(&to_bytes(accepted.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        accepted,
        json!({"id": id, "request_id": request_id, "status": "accepted"})
    );
    assert_eq!(
        app.clone()
            .oneshot(post(id, request_id, r#"{"approved":false}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let answered = app.clone().oneshot(get(id.into())).await.unwrap();
    let answered: serde_json::Value =
        serde_json::from_slice(&to_bytes(answered.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(answered["requests"], json!([]));
    assert!(answered.get("checkpoint").is_none());
    assert_eq!(worker_store.answers(id).await.unwrap().len(), 1);
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let done = app.clone().oneshot(get(id.into())).await.unwrap();
    let done: serde_json::Value =
        serde_json::from_slice(&to_bytes(done.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(done["status"], "completed");
    assert_eq!(done["result"], json!(true));
    let late = DistributedControl::new(worker_store.clone())
        .start("input", "1", "review", json!(null))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let late_request = worker_store
        .requests(&late)
        .await
        .unwrap()
        .pop()
        .unwrap()
        .id;
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/instances/{late}/cancel"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        app.clone()
            .oneshot(post(&late, &late_request, r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(post(&late, "missing", r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn distributed_input_wait_releases_fenced_claim_without_retry_or_clock_wake() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "w1", vec![input_wait_graph()], 1, 300).unwrap();
    worker.advertise().await.unwrap();
    let control = DistributedControl::new(store.clone());
    let id = control
        .start("input", "1", "review", json!(null))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting-for-input");
    assert!(waiting.owner_id.is_none());
    assert!(waiting.lease_until.is_none());
    assert!(waiting.instance.wake_at_ms.is_none());
    assert_eq!(waiting.instance.attempt, 1);
    assert!(waiting.instance.first_failure_at.is_none());
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(control.reconcile_once().await.unwrap(), 0);
    assert_eq!(worker.run_once().await.unwrap(), 0);
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().instance.status,
        "waiting-for-input"
    );
    let request = store.requests(&id).await.unwrap().pop().unwrap();
    store
        .accept_response(&id, &request.id, &json!({"approved": true}))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let done = store.get(&id).await.unwrap().unwrap();
    assert_eq!(done.instance.status, "completed");
    assert_eq!(done.instance.result, Some(json!(true)));
    assert_eq!(done.instance.attempt, 1);
    assert_eq!(
        done.instance.first_claim_at_ms,
        waiting.instance.first_claim_at_ms
    );
    assert_eq!(
        done.instance.deadline_at_ms,
        waiting.instance.deadline_at_ms
    );
}

#[tokio::test]
async fn postgres_reply_survives_no_worker_and_reclaims_only_matching_identity_without_retry() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "w1", vec![input_wait_graph()], 1, 5000).unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(store.clone())
        .start("input", "1", "review", json!(null))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let waiting = store.get(&id).await.unwrap().unwrap();
    let first_claim = waiting.instance.first_claim_at_ms;
    assert!(store.drain("w1").await.unwrap());
    assert!(store.unregister_drained("w1").await.unwrap());
    let request = store.requests(&id).await.unwrap().pop().unwrap();
    assert_eq!(request.prompt, "Approve this order?");
    assert!(
        store
            .accept_response(&id, &request.id, &json!({"approved": "yes"}))
            .await
            .is_err()
    );
    assert_eq!(store.requests(&id).await.unwrap().len(), 1);
    assert!(
        store
            .accept_response(&id, &request.id, &json!({"approved": true}))
            .await
            .is_ok()
    );
    assert!(
        store
            .accept_response(&id, &request.id, &json!({"approved": false}))
            .await
            .is_err()
    );
    let reopened =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    assert_eq!(
        reopened.answers(&id).await.unwrap(),
        vec![(request.id.clone(), json!({"approved": true}))]
    );
    store
        .register_worker("wrong", &[("input", "2", "review")])
        .await
        .unwrap();
    assert!(reopened.claim("wrong", 1, 5000).await.unwrap().is_empty());
    assert_eq!(reopened.answers(&id).await.unwrap().len(), 1);
    let replacement =
        DistributedWorker::new(store.clone(), "w2", vec![input_wait_graph()], 1, 5000).unwrap();
    replacement.advertise().await.unwrap();
    let resumed = reopened.claim("w2", 1, 5000).await.unwrap().pop().unwrap();
    assert_eq!(resumed.instance.attempt, 1);
    assert_eq!(resumed.instance.first_claim_at_ms, first_claim);
    assert!(
        !reopened
            .release_input_owned(
                &id,
                "w2",
                resumed.generation,
                resumed.instance.checkpoint.as_ref().unwrap()
            )
            .await
            .unwrap()
    );
    assert!(
        reopened
            .commit_checkpoint(
                &id,
                "w2",
                resumed.generation,
                resumed.instance.checkpoint.as_ref().unwrap()
            )
            .await
            .unwrap()
    );
    assert_eq!(reopened.answers(&id).await.unwrap().len(), 1);
    assert!(
        reopened
            .consume_response(
                &id,
                "w2",
                resumed.generation,
                &request.id,
                resumed.instance.checkpoint.as_ref().unwrap()
            )
            .await
            .unwrap()
    );
    assert!(reopened.answers(&id).await.unwrap().is_empty());
    assert!(
        !reopened
            .commit_checkpoint(
                &id,
                "w1",
                waiting.generation,
                resumed.instance.checkpoint.as_ref().unwrap()
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn postgres_input_reply_cancel_and_timeout_retire_outstanding_requests() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let worker =
        DistributedWorker::new(store.clone(), "w1", vec![input_wait_graph()], 1, 5000).unwrap();
    worker.advertise().await.unwrap();
    for id in ["cancelled", "expired"] {
        store
            .create(&Instance::new(id, "input", "1", "review", json!(null)))
            .await
            .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), 1);
        let request = store.requests(id).await.unwrap().pop().unwrap();
        if id == "cancelled" {
            store.cancel(id).await.unwrap();
        } else {
            let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
            tokio::spawn(async move { connection.await.unwrap() });
            db.execute("UPDATE instances SET deadline_at_ms=0 WHERE id=$1", &[&id])
                .await
                .unwrap();
            assert_eq!(store.expire_due().await.unwrap(), vec![id.to_string()]);
        }
        assert!(
            store
                .accept_response(id, &request.id, &json!({"approved": true}))
                .await
                .is_err()
        );
        assert!(
            store.requests(id).await.unwrap().is_empty(),
            "terminal request must be retired"
        );
    }
}

#[tokio::test]
async fn postgres_request_checkpoint_write_rolls_back_when_request_insert_fails() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.batch_execute("CREATE FUNCTION reject_request() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'request unavailable'; END $$; CREATE TRIGGER reject_request BEFORE INSERT ON input_requests FOR EACH ROW EXECUTE FUNCTION reject_request()") .await.unwrap();
    let mut instance = Instance::new("job", "input", "1", "review", json!(null));
    instance.checkpoint = Some(input_wait_graph().checkpoint(&json!(null)).unwrap());
    assert_eq!(instance.checkpoint.as_ref().unwrap().requests().len(), 1);
    assert!(store.create(&instance).await.is_err());
    assert!(
        store.get("job").await.unwrap().is_none(),
        "checkpoint cannot commit without request row"
    );
    db.batch_execute(
        "DROP TRIGGER reject_request ON input_requests; DROP FUNCTION reject_request()",
    )
    .await
    .unwrap();
    store.create(&instance).await.unwrap();
    assert_eq!(store.get("job").await.unwrap().unwrap().requests.len(), 1);
}

#[tokio::test]
async fn postgres_reply_racing_claim_release_cannot_strand_pending_response() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("w1", &[("input", "1", "review")])
        .await
        .unwrap();
    let mut instance = Instance::new("job", "input", "1", "review", json!(null));
    instance.checkpoint = Some(input_wait_graph().checkpoint(&json!(null)).unwrap());
    let request_id = instance.checkpoint.as_ref().unwrap().requests()[0]
        .id
        .clone();
    store.create(&instance).await.unwrap();
    let owner = store.claim("w1", 1, 5000).await.unwrap().pop().unwrap();
    let checkpoint = owner.instance.checkpoint.as_ref().unwrap();
    let response = json!({"approved": true});
    let (released, accepted) = tokio::join!(
        store.release_input_owned("job", "w1", owner.generation, checkpoint),
        store.accept_response("job", &request_id, &response)
    );
    assert!(accepted.is_ok());
    let current = store.get("job").await.unwrap().unwrap();
    if released.unwrap() {
        assert_eq!(current.instance.status, "pending");
        assert!(current.owner_id.is_none());
        let next = store.claim("w1", 1, 5000).await.unwrap().pop().unwrap();
        assert_eq!(next.instance.attempt, 1);
    } else {
        assert_eq!(current.instance.status, "running");
        assert_eq!(current.owner_id.as_deref(), Some("w1"));
    }
    assert_eq!(store.answers("job").await.unwrap().len(), 1);
}

#[tokio::test]
async fn postgres_owned_request_checkpoint_rolls_back_on_request_write_failure() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    store
        .register_worker("owner", &[("input", "1", "review")])
        .await
        .unwrap();
    let mut instance = Instance::new("job", "input", "1", "review", json!(null));
    instance.checkpoint = Some(GraphCheckpoint::default());
    store.create(&instance).await.unwrap();
    let claim = store.claim("owner", 1, 5000).await.unwrap().pop().unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.batch_execute("CREATE FUNCTION reject_request() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'request unavailable'; END $$; CREATE TRIGGER reject_request BEFORE INSERT ON input_requests FOR EACH ROW EXECUTE FUNCTION reject_request()") .await.unwrap();
    let checkpoint = input_wait_graph().checkpoint(&json!(null)).unwrap();
    assert!(
        store
            .commit_checkpoint("job", "owner", claim.generation, &checkpoint)
            .await
            .is_err()
    );
    assert!(
        store
            .get("job")
            .await
            .unwrap()
            .unwrap()
            .instance
            .checkpoint
            .unwrap()
            .requests()
            .is_empty()
    );
    db.batch_execute(
        "DROP TRIGGER reject_request ON input_requests; DROP FUNCTION reject_request()",
    )
    .await
    .unwrap();
    assert!(
        store
            .commit_checkpoint("job", "owner", claim.generation, &checkpoint)
            .await
            .unwrap()
    );
    let saved = store.get("job").await.unwrap().unwrap();
    assert_eq!(saved.requests, checkpoint.requests());
}

fn input_with_sibling_graph(sibling: GraphNodeKind, process: &'static str) -> GraphDefinition {
    let mut review = input_wait_graph().nodes;
    let review = review.remove(1);
    let edge = |source, target, value, label| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label,
    };
    GraphDefinition {
        namespace: "input",
        version: "1",
        name: process,
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin: "first_claimed",
            duration: Duration::from_secs(30),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split("and"),
            },
            review,
            GraphNode {
                name: "sibling",
                kind: sibling,
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "fork",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            edge("start", "fork", None, None),
            edge("fork", "review", None, Some("left")),
            edge("fork", "sibling", None, Some("right")),
            edge(
                "review",
                "joined",
                Some(Arc::new(|_, values| Ok(values["review"].clone()))),
                None,
            ),
            edge(
                "sibling",
                "joined",
                Some(Arc::new(|input, _| Ok(input.clone()))),
                None,
            ),
            edge(
                "joined",
                "done",
                Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
                None,
            ),
        ],
    }
}

#[tokio::test]
async fn distributed_sibling_completion_releases_input_claim_and_preserves_checkpoint() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let graph = input_with_sibling_graph(
        GraphNodeKind::Task(Arc::new({
            let hits = hits.clone();
            move |input, _| {
                std::thread::sleep(Duration::from_millis(80));
                hits.fetch_add(1, Ordering::SeqCst);
                Ok(input.clone())
            }
        })),
        "parallel",
    );
    let worker = DistributedWorker::new(store.clone(), "w1", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(store.clone())
        .start("input", "1", "parallel", json!(7))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting-for-input");
    assert!(waiting.owner_id.is_none());
    assert_eq!(waiting.requests.len(), 1);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let origin = waiting.instance.first_claim_at_ms;
    let deadline = waiting.instance.deadline_at_ms;
    store
        .accept_response(&id, &waiting.requests[0].id, &json!({"approved": true}))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let done = store.get(&id).await.unwrap().unwrap();
    assert_eq!(done.instance.status, "completed");
    assert_eq!(done.instance.attempt, 1);
    assert_eq!(done.instance.first_claim_at_ms, origin);
    assert_eq!(done.instance.deadline_at_ms, deadline);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn distributed_reply_after_timer_release_reclaims_before_clock_and_keeps_attempt() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = input_with_sibling_graph(GraphNodeKind::PauseFor(Duration::from_secs(3)), "timer");
    let worker =
        DistributedWorker::new(store.clone(), "timer-worker", vec![graph], 1, 5000).unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(store.clone())
        .start("input", "1", "timer", json!(9))
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting");
    assert!(waiting.owner_id.is_none());
    assert_eq!(waiting.requests.len(), 1);
    let wake = waiting.instance.wake_at_ms.unwrap();
    store
        .accept_response(&id, &waiting.requests[0].id, &json!({"approved": true}))
        .await
        .unwrap();
    let answered = store.get(&id).await.unwrap().unwrap();
    assert_eq!(
        answered.instance.status, "pending",
        "reply must wake timer wait early"
    );
    assert_eq!(answered.instance.wake_at_ms, Some(wake));
    assert_eq!(worker.run_once().await.unwrap(), 1);
    let resumed = store.get(&id).await.unwrap().unwrap();
    assert_eq!(resumed.instance.attempt, 1);
    assert_eq!(
        resumed.instance.first_claim_at_ms,
        waiting.instance.first_claim_at_ms
    );
    assert_eq!(
        resumed.instance.deadline_at_ms,
        waiting.instance.deadline_at_ms
    );
    assert!(resumed.requests.is_empty());
    assert!(store.answers(&id).await.unwrap().is_empty());
    assert_eq!(
        resumed.instance.status, "waiting",
        "unexpired timer still waits after consuming the answer"
    );
    tokio::time::sleep(Duration::from_millis(
        wake.saturating_sub(
            i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap(),
        )
        .max(0) as u64
            + 10,
    ))
    .await;
    assert_eq!(worker.run_once().await.unwrap(), 1);
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().instance.status,
        "completed"
    );
}

#[tokio::test]
async fn distributed_reply_before_timer_release_prevents_unclaimable_wait() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let graph = input_with_sibling_graph(GraphNodeKind::PauseFor(Duration::from_secs(5)), "timer");
    store
        .register_worker("owner", &[("input", "1", "timer")])
        .await
        .unwrap();
    let mut instance = Instance::new("job", "input", "1", "timer", json!(null));
    instance.checkpoint = Some(graph.checkpoint(&json!(null)).unwrap());
    let checkpoint = instance.checkpoint.clone().unwrap();
    assert_eq!(checkpoint.requests().len(), 1);
    let wake = graph.waiting_until(&checkpoint).unwrap();
    store.create(&instance).await.unwrap();
    let owner = store.claim("owner", 1, 5000).await.unwrap().pop().unwrap();
    store
        .accept_response(
            "job",
            &checkpoint.requests()[0].id,
            &json!({"approved": true}),
        )
        .await
        .unwrap();
    assert!(
        !store
            .release_wait_owned("job", "owner", owner.generation, &checkpoint, wake)
            .await
            .unwrap()
    );
    let saved = store.get("job").await.unwrap().unwrap();
    assert_eq!(saved.instance.status, "running");
    assert_eq!(saved.owner_id.as_deref(), Some("owner"));
    assert_eq!(store.answers("job").await.unwrap().len(), 1);
}

#[tokio::test]
async fn postgres_get_never_mixes_pre_reply_status_with_post_reply_requests() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let mut instance = Instance::new("job", "input", "1", "review", json!(null));
    instance.status = "waiting-for-input".into();
    let mut checkpoint = input_wait_graph().checkpoint(&json!(null)).unwrap();
    let request_id = checkpoint.requests()[0].id.clone();
    // Make the first row slow to transfer/deserialize so a reply can commit before the request read.
    checkpoint
        .completed
        .insert("padding".into(), json!("x".repeat(20_000_000)));
    instance.checkpoint = Some(checkpoint);
    store.create(&instance).await.unwrap();
    let reader = PostgresStore::connect_with_options(
        &format!("{url}?application_name=blkit_snapshot_reader"),
        4,
        &postgres_fixture::plaintext_options(),
    )
    .await
    .unwrap();
    let (observer, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let reading = tokio::spawn(async move { reader.get("job").await.unwrap().unwrap() });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = observer.query_one("SELECT coalesce(bool_or(query LIKE 'SELECT id, namespace, version, process, input, status, result, error,%'),FALSE) FROM pg_stat_activity WHERE application_name='blkit_snapshot_reader'", &[]).await.unwrap();
            if row.get::<_, bool>(0) { break; }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }).await.expect("instance read must start before the reply");
    store
        .accept_response("job", &request_id, &json!({"approved": true}))
        .await
        .unwrap();
    let observed = reading.await.unwrap();
    assert!(
        observed.instance.status != "waiting-for-input" || observed.requests.len() == 1,
        "one GET cannot combine an input wait before reply with requests after reply"
    );
    let fresh = store.get("job").await.unwrap().unwrap();
    assert_eq!(fresh.instance.status, "pending");
    assert!(fresh.requests.is_empty());
}

#[tokio::test]
async fn distributed_reply_wins_before_timer_release_without_retrying_sibling() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    postgres_fixture::prepare_fresh_database_url(&url).await;
    let store =
        PostgresStore::connect_with_options(&url, 4, &postgres_fixture::plaintext_options())
            .await
            .unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicUsize::new(0));
    let mut graph = input_with_sibling_graph(
        GraphNodeKind::PauseFor(Duration::from_secs(10)),
        "timer_reply_wins",
    );
    graph.nodes.push(GraphNode {
        name: "slow",
        kind: GraphNodeKind::Task(Arc::new({
            let release = release.clone();
            let hits = hits.clone();
            move |input, _| {
                let start = std::time::Instant::now();
                while !release.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                hits.fetch_add(1, Ordering::SeqCst);
                Ok(input.clone())
            }
        })),
    });
    graph.links.push(GraphLink {
        source: "fork",
        target: "slow",
        label: Some("third"),
        value: None,
        condition: None,
        fallback: false,
    });
    graph.links.push(GraphLink {
        source: "slow",
        target: "joined",
        value: Some(Arc::new(|input, _| Ok(input.clone()))),
        condition: None,
        fallback: false,
        label: None,
    });
    let worker = DistributedWorker::new(store.clone(), "w1", vec![graph], 2, 5000).unwrap();
    worker.advertise().await.unwrap();
    let id = DistributedControl::new(store.clone())
        .start("input", "1", "timer_reply_wins", json!(7))
        .await
        .unwrap();
    let running = tokio::spawn(async move { worker.run_once().await });
    let request = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let row = store.get(&id).await.unwrap().unwrap();
            if row.instance.status == "running" && !row.requests.is_empty() {
                break row.requests[0].id.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let origin = store
        .get(&id)
        .await
        .unwrap()
        .unwrap()
        .instance
        .first_claim_at_ms;
    store
        .accept_response(&id, &request, &json!({"approved": true}))
        .await
        .unwrap();
    release.store(true, Ordering::SeqCst);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        1
    );
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting");
    assert!(waiting.owner_id.is_none());
    assert!(waiting.requests.is_empty());
    assert!(store.answers(&id).await.unwrap().is_empty());
    assert_eq!(waiting.instance.attempt, 1);
    assert_eq!(waiting.instance.first_claim_at_ms, origin);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
