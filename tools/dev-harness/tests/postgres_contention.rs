//! Run only with BLKIT_MEASURE_DATABASE_URL set to a disposable PostgreSQL 17 database.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use blkit_core::{
    compiled_graph::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
    distributed::{DistributedControl, DistributedWorker},
    postgres_store::PostgresStore,
    runtime::Evaluate,
    server::router_distributed,
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_postgres::NoTls;
use tower::ServiceExt;

fn graph() -> GraphDefinition {
    let task: Evaluate = Arc::new(|input, _| {
        std::thread::sleep(Duration::from_millis(input.as_u64().unwrap()));
        Ok(input.clone())
    });
    GraphDefinition {
        namespace: "measure",
        version: "1",
        name: "job",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
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

fn percentile(sorted: &[u64], percent: usize) -> u64 {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn single_connection_measurement_budget_is_enforced_under_concurrency() {
    let Ok(url) = std::env::var("BLKIT_MEASURE_DATABASE_URL") else {
        return;
    };
    assert_eq!(
        std::env::var("BLKIT_MEASURE_DISPOSABLE").as_deref(),
        Ok("yes")
    );
    let tagged_url = format!(
        "{url}{}application_name=blkit_task41_budget",
        if url.contains('?') { '&' } else { '?' }
    );
    let (observer, connection) = tokio_postgres::connect(&tagged_url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let store = PostgresStore::connect_with_pool_size(&tagged_url, 1)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..32 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..100 {
                store.heartbeat("missing-measurement-worker").await.unwrap();
            }
        }));
    }
    let mut peak = 0;
    while tasks.iter().any(|task| !task.is_finished()) {
        let count: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND application_name='blkit_task41_budget'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        peak = peak.max(count);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(
        peak, 2,
        "one store session plus one observer, even under concurrency"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn measure_api_worker_and_combined_connections() {
    let Ok(url) = std::env::var("BLKIT_MEASURE_DATABASE_URL") else {
        eprintln!("SKIP: set BLKIT_MEASURE_DATABASE_URL to a disposable PostgreSQL database");
        return;
    };
    assert_eq!(
        std::env::var("BLKIT_MEASURE_DISPOSABLE").as_deref(),
        Ok("yes"),
        "this benchmark truncates tables; explicitly opt in only for a disposable database"
    );
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    db.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await
        .unwrap();
    let max: usize = std::env::var("BLKIT_MEASURE_POOL_MAX")
        .expect("set BLKIT_MEASURE_POOL_MAX to 1, 4, 8, or 16")
        .parse()
        .unwrap();
    assert!([1, 4, 8, 16].contains(&max));
    let capacity: i32 = db.query_one("SELECT current_setting('max_connections')::int - current_setting('superuser_reserved_connections')::int - current_setting('reserved_connections')::int", &[]).await.unwrap().get(0);
    assert!(
        2 * max + 2 <= capacity as usize,
        "combined role budgets plus two observers exceed disposable PostgreSQL capacity {capacity}"
    );

    for mode in ["api", "worker", "combined"] {
        let with_worker = mode != "api";
        let mode_url = format!(
            "{url}{}application_name=blkit_task41_{mode}",
            if url.contains('?') { '&' } else { '?' }
        );
        let tag = format!("blkit_task41_{mode}");
        let session_sql = "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND (application_name=$1 OR pid=pg_backend_pid())";
        let store = PostgresStore::connect_with_pool_size(&mode_url, max)
            .await
            .unwrap();
        let api = router_distributed(Arc::new(DistributedControl::new(store.clone())));
        db.batch_execute("TRUNCATE instances, workers, process_policies CASCADE")
            .await
            .unwrap();
        let worker_store = if with_worker {
            PostgresStore::connect_with_pool_size(&mode_url, max)
                .await
                .unwrap()
        } else {
            store.clone()
        };
        let worker =
            DistributedWorker::new(worker_store, "measure-worker", vec![graph()], 32, 5000)
                .unwrap();
        worker.advertise().await.unwrap();
        let control = DistributedControl::new(store.clone());
        if with_worker {
            let durations: Vec<_> = if mode == "worker" {
                vec![15; 256]
            } else {
                [4200; 8].into_iter().chain([15; 64]).collect()
            };
            for duration in durations {
                control
                    .start("measure", "1", "job", json!(duration))
                    .await
                    .unwrap();
            }
        }
        let connections: i64 = db.query_one(session_sql, &[&tag]).await.unwrap().get(0);
        assert!((connections as usize) <= if with_worker { 2 * max + 1 } else { max + 1 });
        println!("mode={mode} pool_max={max} begin_sessions={connections}");
        db.query_one("SELECT pg_stat_statements_reset()", &[])
            .await
            .unwrap();
        println!("mode={mode} phase=begin");
        let started = Instant::now();
        let (stop_samples, mut stop_receiver) = tokio::sync::oneshot::channel::<()>();
        let sampler = tokio::spawn({
            let mode_url = mode_url.clone();
            let tag = tag.clone();
            async move {
                let (observer, connection) =
                    tokio_postgres::connect(&mode_url, NoTls).await.unwrap();
                let driver = tokio::spawn(async move { connection.await.unwrap() });
                let mut peak = connections;
                let mut samples = 0;
                loop {
                    let count: i64 = observer.query_one("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1", &[&tag]).await.unwrap().get(0);
                    peak = peak.max(count + 1); // The untagged main observer is also connected.
                    samples += 1;
                    tokio::select! {
                        _ = &mut stop_receiver => break,
                        _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                    }
                }
                drop(observer);
                driver.abort();
                (peak, samples)
            }
        });
        let reconcile = tokio::spawn({
            let control = DistributedControl::new(store.clone());
            async move {
                loop {
                    control.reconcile_once().await.unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        });
        let running = if with_worker {
            Some(tokio::spawn(async move {
                // Same 100 ms outer loop as generated/harness workers.
                for _ in 0..50 {
                    worker.run_once().await.unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }))
        } else {
            None
        };
        let mut clients = Vec::new();
        for _ in 0..(if mode == "worker" { 0 } else { 8 }) {
            let api = api.clone();
            clients.push(tokio::spawn(async move {
                let mut latencies = [Vec::new(), Vec::new(), Vec::new()];
                for _ in 0..12 {
                    let start = Instant::now();
                    let response = api
                        .clone()
                        .oneshot(
                            Request::builder()
                                .method("POST")
                                .uri("/processes/measure/1/job/instances")
                                .header("content-type", "application/json")
                                .body(Body::from("400"))
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::ACCEPTED);
                    latencies[0].push(start.elapsed().as_micros() as u64);
                    let value: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 4096).await.unwrap(),
                    )
                    .unwrap();
                    let id = value["id"].as_str().unwrap();
                    let start = Instant::now();
                    let response = api
                        .clone()
                        .oneshot(
                            Request::builder()
                                .uri(format!("/instances/{id}"))
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    latencies[1].push(start.elapsed().as_micros() as u64);
                    let start = Instant::now();
                    let response = api
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
                    assert_eq!(response.status(), StatusCode::ACCEPTED);
                    latencies[2].push(start.elapsed().as_micros() as u64);
                }
                latencies
            }));
        }
        let mut all = [Vec::new(), Vec::new(), Vec::new()];
        for client in clients {
            for (out, samples) in all.iter_mut().zip(client.await.unwrap()) {
                out.extend(samples);
            }
        }
        let db_sql = "SELECT count(*), coalesce(sum(total_exec_time),0)::float8 FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database())";
        if mode != "worker" {
            let api_elapsed = started.elapsed();
            println!("mode={mode} phase=api-end");
            let snapshot_started = Instant::now();
            let api_db = db.query_one(db_sql, &[]).await.unwrap();
            println!(
                "mode={mode} db_api_exec_ms={:.3} db_api_statements={} db_snapshot_lag_us={}",
                api_db.get::<_, f64>(1),
                api_db.get::<_, i64>(0),
                snapshot_started.elapsed().as_micros()
            );
            for (name, samples) in ["start", "status", "cancel"].into_iter().zip(&mut all) {
                samples.sort_unstable();
                println!(
                    "mode={mode} operation={name} count={} p50_us={} p95_us={}",
                    samples.len(),
                    percentile(samples, 50),
                    percentile(samples, 95)
                );
            }
            println!(
                "mode={mode} api_ops={} api_elapsed_ms={} api_ops_per_s={:.1}",
                8 * 12 * 3,
                api_elapsed.as_millis(),
                288.0 / api_elapsed.as_secs_f64()
            );
        }
        if let Some(running) = running {
            // Stop once all prequeued work is completed, rather than benchmarking idle polling.
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let row = db.query_one("SELECT count(*) FROM instances WHERE status IN ('pending','running','waiting','retry-waiting')", &[]).await.unwrap();
                    if row.get::<_, i64>(0) == 0 { break; }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }).await.unwrap();
            running.abort();
            let _ = running.await;
            let work_elapsed = started.elapsed();
            println!("mode={mode} work_elapsed_ms={}", work_elapsed.as_millis());
            let row = db.query_one("SELECT count(*) FILTER (WHERE status='completed'), count(*) FILTER (WHERE status='completed' AND checkpoint IS NOT NULL), count(*) FILTER (WHERE status='failed') FROM instances", &[]).await.unwrap();
            println!(
                "mode={mode} completed={} checkpointed={} failed={}",
                row.get::<_, i64>(0),
                row.get::<_, i64>(1),
                row.get::<_, i64>(2)
            );
            let expected = if mode == "worker" { 256 } else { 72 };
            assert_eq!(
                row.get::<_, i64>(0),
                expected,
                "all prequeued tasks must finish"
            );
            assert_eq!(
                row.get::<_, i64>(1),
                expected,
                "all prequeued tasks must checkpoint"
            );
            println!(
                "mode={mode} work_per_s={:.1}",
                expected as f64 / work_elapsed.as_secs_f64()
            );
            assert_eq!(row.get::<_, i64>(2), 0, "no claim may expire");
        }
        reconcile.abort();
        let _ = reconcile.await;
        let _ = stop_samples.send(());
        let (peak, samples) = sampler.await.unwrap();
        let end_sessions: i64 = db.query_one(session_sql, &[&tag]).await.unwrap().get(0);
        let peak_limit = if with_worker { 2 * max + 2 } else { max + 2 };
        assert!(
            (peak as usize) <= peak_limit,
            "observed {peak} sessions exceeds budget {peak_limit}"
        );
        println!(
            "mode={mode} pool_max={max} end_sessions={end_sessions} peak_sessions={peak} peak_samples={samples} peak_limit={peak_limit}"
        );
        let row = db.query_one(db_sql, &[]).await.unwrap();
        println!(
            "mode={} db_full_statements={} db_full_exec_ms={:.3}",
            mode,
            row.get::<_, i64>(0),
            row.get::<_, f64>(1)
        );
        println!("mode={mode} phase=end");
    }
}

#[tokio::test]
async fn concurrent_postgres_responses_have_one_committed_winner() {
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{ImageExt, runners::AsyncRunner},
    };
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
        .create(&blkit_core::runtime::Instance::new(
            "job",
            "test",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('job','1','Approve?', $1)", &[&serde_json::to_string(&blkit_core::compiled_graph::RequestSchema {
        ports: [("approved".into(), blkit_core::compiled_graph::RequestType::Named("Bool".into()))].into(), records: Default::default(), enums: Default::default(),
    }).unwrap()]).await.unwrap();
    let approved = json!({"approved":true});
    let rejected = json!({"approved":false});
    let (a, b) = tokio::join!(
        first.accept_response("job", "1", &approved),
        second.accept_response("job", "1", &rejected)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let answer = first.answers("job").await.unwrap();
    assert_eq!(answer.len(), 1);
    assert_eq!(
        answer[0].1,
        if a.is_ok() {
            json!({"approved":true})
        } else {
            json!({"approved":false})
        }
    );
}

#[tokio::test]
async fn concurrent_postgres_cancel_and_reply_have_one_terminal_order() {
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{ImageExt, runners::AsyncRunner},
    };
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
        .create(&blkit_core::runtime::Instance::new(
            "job",
            "test",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let (db, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    db.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('job','1','Approve?', $1)", &[&serde_json::to_string(&blkit_core::compiled_graph::RequestSchema {
        ports: [("approved".into(), blkit_core::compiled_graph::RequestType::Named("Bool".into()))].into(), records: Default::default(), enums: Default::default(),
    }).unwrap()]).await.unwrap();
    let response = json!({"approved": true});
    let (reply, cancel) = tokio::join!(
        first.accept_response("job", "1", &response),
        second.cancel("job")
    );
    cancel.unwrap();
    assert_eq!(
        first.get("job").await.unwrap().unwrap().instance.status,
        "cancelled"
    );
    assert!(first.requests("job").await.unwrap().is_empty());
    let answered = first.answers("job").await.unwrap();
    assert_eq!(reply.is_ok(), !answered.is_empty());
    assert!(first.accept_response("job", "1", &response).await.is_err());
}
