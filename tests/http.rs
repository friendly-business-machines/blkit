use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use blkit::{
    RetryPolicy,
    named_runtime::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
    runtime::{Definition, Engine, Registry, Step, Store},
    server::router,
    transpile,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

async fn http_start(app: &axum::Router, name: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/processes/test/1/{name}/instances"))
                .header("content-type", "application/json")
                .body(Body::from("null"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    body["id"].as_str().unwrap().into()
}

async fn http_status(app: &axum::Router, id: &str) -> Value {
    let response = app
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
    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap()
}

async fn wait_http_status(app: &axum::Router, id: &str, expected: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let state = http_status(app, id).await;
            if state["status"] == expected {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn http_reports_intermediate_wait_and_accepts_cancellation() {
    let path = std::env::temp_dir().join(format!("blkit-http-pause-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let graph = GraphDefinition {
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
                name: "delay",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(200)),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "delay",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "delay",
                target: "done",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let app = router(Arc::new(
        Engine::new(Registry::new_named(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
    ));
    let id = http_start(&app, "pause").await;
    let item = http_status(&app, &id).await;
    assert_eq!(item["status"], "waiting");
    assert!(item["wake_at"].as_i64().is_some());
    let reply = app
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
    assert_eq!(reply.status(), StatusCode::ACCEPTED);
    tokio::time::sleep(Duration::from_millis(220)).await;
    assert_eq!(http_status(&app, &id).await["status"], "cancelled");
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn http_exposes_deadline_timeout_and_rejects_late_cancellation() {
    let path = std::env::temp_dir().join(format!("blkit-http-timeout-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "timeout",
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(50),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "pause",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(200)),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "pause",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "pause",
                target: "done",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let app = router(Arc::new(
        Engine::new(Registry::new_named(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
    ));
    let id = http_start(&app, "timeout").await;
    let initial = http_status(&app, &id).await;
    assert_eq!(initial["status"], "waiting");
    assert!(initial["deadline_at_ms"].as_i64().unwrap() < initial["wake_at"].as_i64().unwrap());
    let timed_out = wait_http_status(&app, &id, "business-error").await;
    assert_eq!(timed_out["terminal_name"], "timeout");
    let reply = app
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
    assert_eq!(reply.status(), StatusCode::CONFLICT);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn http_reports_retry_waiting_business_failure_termination_and_cancel_while_waiting() {
    let path = std::env::temp_dir().join(format!("blkit-http-outcomes-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let graphs = [
        ("business", GraphNodeKind::Error, None),
        ("terminated", GraphNodeKind::Terminate, None),
        (
            "failure",
            GraphNodeKind::Task(Arc::new(|_, _| Err("execution failed".into()))),
            None,
        ),
        (
            "waiting",
            GraphNodeKind::Task(Arc::new(|_, _| Err("retry me".into()))),
            Some(RetryPolicy {
                max_retries: 1,
                retry_for: Duration::from_secs(3),
                retry_delay: Duration::from_secs(2),
                backoff: "exponential",
            }),
        ),
    ];
    let definitions = graphs
        .into_iter()
        .map(|(name, kind, retry)| {
            let task = matches!(kind, GraphNodeKind::Task(_));
            GraphDefinition {
                namespace: "test",
                version: "1",
                name,
                retry,
                deadline: None,
                decode_input: Box::new(Ok::<Value, String>),
                nodes: vec![
                    GraphNode {
                        name: "start",
                        kind: GraphNodeKind::Start,
                    },
                    GraphNode { name: "exit", kind },
                    GraphNode {
                        name: "done",
                        kind: GraphNodeKind::End,
                    },
                ],
                links: {
                    let mut links = vec![GraphLink {
                        source: "start",
                        target: "exit",
                        value: None,
                        condition: None,
                        fallback: false,
                        label: None,
                    }];
                    if task {
                        links.push(GraphLink {
                            source: "exit",
                            target: "done",
                            value: Some(Arc::new(|_, _| Ok(json!(null)))),
                            condition: None,
                            fallback: false,
                            label: None,
                        });
                    }
                    links
                },
            }
        })
        .collect();
    let app = router(Arc::new(
        Engine::new(Registry::new_named(definitions).unwrap(), store.clone(), 2).unwrap(),
    ));
    let business = http_start(&app, "business").await;
    let item = wait_http_status(&app, &business, "business-error").await;
    assert_eq!(item["terminal_name"], "exit");
    assert!(item["next_eligible_at"].is_null());
    let terminated = http_start(&app, "terminated").await;
    assert_eq!(
        wait_http_status(&app, &terminated, "terminated").await["terminal_name"],
        "exit"
    );
    let failure = http_start(&app, "failure").await;
    assert_eq!(
        wait_http_status(&app, &failure, "failed").await["error"],
        "execution failed"
    );
    let waiting = http_start(&app, "waiting").await;
    let item = wait_http_status(&app, &waiting, "retry-waiting").await;
    assert_eq!(item["error"], "retry me");
    assert_eq!(item["attempt"], 1);
    assert!(
        item["next_eligible_at"].as_i64().unwrap() > item["first_failure_at"].as_i64().unwrap()
    );
    let cancelled = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/instances/{waiting}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status(), StatusCode::ACCEPTED);
    assert_eq!(http_status(&app, &waiting).await["status"], "cancelled");
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::test]
async fn compiled_graph_http_start_status_and_errors() {
    assert_eq!(
        transpile(include_str!("../examples/graph.bl")).unwrap(),
        include_str!(concat!(env!("OUT_DIR"), "/graph.rs"))
    );
    let path = std::env::temp_dir().join(format!("blkit-http-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    store.recover_interrupted().await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new_named(compiled::named_graph_definitions()).unwrap(),
            store.clone(),
            4,
        )
        .unwrap(),
    ));
    let request = |uri: &str, body: Value| {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let start = app
        .clone()
        .oneshot(request(
            "/processes/orders/1.0/decide/instances",
            json!({"total":"1200"}),
        ))
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::ACCEPTED);
    let payload: Value =
        serde_json::from_slice(&to_bytes(start.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let id = payload["id"].as_str().unwrap();
    let mut state = Value::Null;
    for _ in 0..100 {
        let response = app
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
        state = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
        if state["status"] == "completed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(state["result"], "review");
    let cancel = app
        .clone()
        .oneshot(request(&format!("/instances/{id}/cancel"), json!({})))
        .await
        .unwrap();
    assert_eq!(cancel.status(), StatusCode::CONFLICT);
    let invalid = app
        .clone()
        .oneshot(request(
            "/processes/orders/1.0/decide/instances",
            json!({"total":"abc"}),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let unknown = app
        .clone()
        .oneshot(request(
            "/processes/orders/2.0/decide/instances",
            json!({"total":"1200"}),
        ))
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/instances/not-found")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn active_cancellation_is_acknowledged_over_http() {
    let path = std::env::temp_dir().join(format!("blkit-http-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let definition = Definition {
        namespace: "test",
        version: "1",
        name: "wait",
        steps: vec![
            Step::Run {
                name: "slow",
                call: Arc::new({
                    let started = started.clone();
                    let release = release.clone();
                    move |_, _| {
                        started.store(true, Ordering::SeqCst);
                        for _ in 0..100 {
                            if release.load(Ordering::SeqCst) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Ok(json!(true))
                    }
                }),
                cancel: Arc::new({
                    let release = release.clone();
                    move || {
                        release.store(true, Ordering::SeqCst);
                    }
                }),
            },
            Step::Return(Arc::new(|_, values| Ok(values["slow"].clone()))),
        ],
        decode_input: Box::new(Ok::<Value, String>),
    };
    let app = router(Arc::new(
        Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 1).unwrap(),
    ));
    let request = Request::builder()
        .method("POST")
        .uri("/processes/test/1/wait/instances")
        .header("content-type", "application/json")
        .body(Body::from("null"))
        .unwrap();
    let start = app.clone().oneshot(request).await.unwrap();
    let payload: Value =
        serde_json::from_slice(&to_bytes(start.into_body(), 4096).await.unwrap()).unwrap();
    let id = payload["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let response = app
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
    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/instances/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status: Value =
        serde_json::from_slice(&to_bytes(status.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(status["status"], "cancelled");
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
