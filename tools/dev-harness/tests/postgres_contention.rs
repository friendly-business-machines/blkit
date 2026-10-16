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
async fn measure_single_connection_api_and_worker() {
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
    let store = PostgresStore::connect(&url).await.unwrap();
    let api = router_distributed(Arc::new(DistributedControl::new(store.clone())));

    for with_worker in [false, true] {
        db.batch_execute("TRUNCATE instances, workers, process_policies CASCADE")
            .await
            .unwrap();
        let worker =
            DistributedWorker::new(store.clone(), "measure-worker", vec![graph()], 32, 5000)
                .unwrap();
        worker.advertise().await.unwrap();
        let control = DistributedControl::new(store.clone());
        if with_worker {
            for duration in [4200; 8].into_iter().chain([15; 64]) {
                control
                    .start("measure", "1", "job", json!(duration))
                    .await
                    .unwrap();
            }
        }
        db.query_one("SELECT pg_stat_statements_reset()", &[])
            .await
            .unwrap();
        println!(
            "mode={} phase=begin",
            if with_worker { "combined" } else { "api" }
        );
        let started = Instant::now();
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
        for _ in 0..8 {
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
        let api_elapsed = started.elapsed();
        for (name, samples) in ["start", "status", "cancel"].into_iter().zip(&mut all) {
            samples.sort_unstable();
            println!(
                "mode={} operation={} count={} p50_us={} p95_us={}",
                if with_worker { "combined" } else { "api" },
                name,
                samples.len(),
                percentile(samples, 50),
                percentile(samples, 95)
            );
        }
        println!(
            "mode={} api_ops={} api_elapsed_ms={} api_ops_per_s={:.1}",
            if with_worker { "combined" } else { "api" },
            8 * 12 * 3,
            api_elapsed.as_millis(),
            288.0 / api_elapsed.as_secs_f64()
        );
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
            println!(
                "mode=combined work_elapsed_ms={}",
                started.elapsed().as_millis()
            );
            let row = db.query_one("SELECT count(*) FILTER (WHERE status='completed'), count(*) FILTER (WHERE status='completed' AND checkpoint IS NOT NULL), count(*) FILTER (WHERE status='failed') FROM instances", &[]).await.unwrap();
            println!(
                "mode=combined completed={} checkpointed={} failed={}",
                row.get::<_, i64>(0),
                row.get::<_, i64>(1),
                row.get::<_, i64>(2)
            );
            assert_eq!(row.get::<_, i64>(0), 72, "all prequeued tasks must finish");
            assert_eq!(row.get::<_, i64>(2), 0, "no claim may expire");
        }
        reconcile.abort();
        let row = db.query_one("SELECT count(*), coalesce(sum(total_exec_time),0)::float8 FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database())", &[]).await.unwrap();
        println!(
            "mode={} db_statements={} db_exec_ms={:.3}",
            if with_worker { "combined" } else { "api" },
            row.get::<_, i64>(0),
            row.get::<_, f64>(1)
        );
        println!(
            "mode={} phase=end",
            if with_worker { "combined" } else { "api" }
        );
    }
}
