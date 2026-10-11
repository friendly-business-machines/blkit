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
use tower::ServiceExt;

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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let first_store = PostgresStore::connect(&url).await.unwrap();
    let second_store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
            .status
            != "running"
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let reopened = PostgresStore::connect(&url).await.unwrap();
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
    let first = PostgresStore::connect(&url).await.unwrap();
    let second = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let first = PostgresStore::connect(&url).await.unwrap();
    let second = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    execution.await.unwrap();
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
        "pending"
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
    let store = PostgresStore::connect(&url).await.unwrap();
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

// Generated Rust is checked by compilation and integration tests, not style lints.
#[allow(clippy::all)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::test]
async fn worker_binary_advertises_only_linked_identities_and_runs_without_source_files() {
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
    let store = PostgresStore::connect(&url).await.unwrap();
    let mut child = WorkerChild(
        Command::new(env!("CARGO_BIN_EXE_blkit-worker"))
            .args([&url, "2", "1000"])
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
    store
        .register_worker("test-worker", &[("orders", "1.0", "decide")])
        .await
        .unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let _api = WorkerChild(
        Command::new(env!("CARGO_BIN_EXE_blkit-api"))
            .args([&url, &format!("127.0.0.1:{port}")])
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

#[tokio::test]
async fn process_worker_child() {
    let Ok(url) = std::env::var("BLKIT_CRASH_CHILD_URL") else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var("BLKIT_CRASH_CHILD_DIR").unwrap());
    let id = std::env::var("BLKIT_CRASH_CHILD_ID").unwrap();
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let dir = std::env::temp_dir().join(format!("blkit-crash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _cleanup = CrashDir(dir.clone());
    std::fs::write(dir.join("hold"), "").unwrap();
    let store = PostgresStore::connect(&url).await.unwrap();
    let graph = crash_test_graph(dir.join("b.log"), dir.join("c.started"), dir.join("hold"));
    let mut item = Instance::new("crash-job", "orders", "1.0", "parallel", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    let spawn = |id: &str| {
        WorkerChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "process_worker_child", "--nocapture"])
                .env("BLKIT_CRASH_CHILD_URL", &url)
                .env("BLKIT_CRASH_CHILD_DIR", &dir)
                .env("BLKIT_CRASH_CHILD_ID", id)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
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
    let first_generation = store.get("crash-job").await.unwrap().unwrap().generation;
    let survivor = spawn("survivor");
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
    let store = PostgresStore::connect(&url).await.unwrap();
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
