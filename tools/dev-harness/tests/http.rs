use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
extern crate blkit_core as blkit;
use blkit::{
    RetryPolicy,
    compiled_graph::{
        GraphDefinition, GraphLink, GraphNode, GraphNodeKind, RequestSchema, RequestType,
    },
    runtime::{Engine, LocalStore, Registry},
    server::router,
};
use blkit_transpiler::transpile;
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
    let store = LocalStore::open(&path).await.unwrap();
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
        Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
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
    let store = LocalStore::open(&path).await.unwrap();
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
        Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
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
    let store = LocalStore::open(&path).await.unwrap();
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
        Engine::new(Registry::new(definitions).unwrap(), store.clone(), 2).unwrap(),
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

async fn http_response(app: &axum::Router, id: &str, request_id: &str, body: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/instances/{id}/requests/{request_id}/response"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

fn request_graph(delayed: bool) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "review",
        retry: None,
        deadline: None,
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
        ]
        .into_iter()
        .chain(delayed.then_some(GraphNode {
            name: "delay",
            kind: GraphNodeKind::PauseFor(Duration::from_millis(500)),
        }))
        .collect(),
        links: vec![
            GraphLink {
                source: "start",
                target: if delayed { "delay" } else { "review" },
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "delay",
                target: "review",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
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
        ]
        .into_iter()
        .filter(|link| delayed || link.source != "delay")
        .collect(),
    }
}

#[tokio::test]
async fn local_http_request_visibility_validation_and_one_committed_response() {
    let path = std::env::temp_dir().join(format!("blkit-http-input-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = request_graph(false);
    let app = router(Arc::new(
        Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
    ));
    let id = http_start(&app, "review").await;
    let waiting = wait_http_status(&app, &id, "waiting-for-input").await;
    assert_eq!(waiting["requests"].as_array().unwrap().len(), 1);
    let request = &waiting["requests"][0];
    assert_eq!(request["prompt"], "Approve this order?");
    assert_eq!(
        request["schema"]["ports"]["approved"],
        json!({"Named":"Bool"})
    );
    assert!(
        waiting.get("checkpoint").is_none(),
        "GET must not expose checkpointed answers"
    );
    let request_id = request["id"].as_str().unwrap();
    assert_eq!(
        http_response(&app, &id, request_id, "{").await,
        StatusCode::BAD_REQUEST
    );
    for invalid in [
        "null",
        "{}",
        r#"{"approved":"yes"}"#,
        r#"{"approved":true,"extra":0}"#,
    ] {
        assert_eq!(
            http_response(&app, &id, request_id, invalid).await,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        http_status(&app, &id).await["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        http_response(&app, "unknown", request_id, r#"{"approved":true}"#).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        http_response(&app, &id, "missing", r#"{"approved":true}"#).await,
        StatusCode::NOT_FOUND
    );
    let accepted = app
        .clone()
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
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let accepted: Value =
        serde_json::from_slice(&to_bytes(accepted.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        accepted,
        json!({"id": id, "request_id": request_id, "status": "accepted"})
    );
    assert_eq!(
        http_response(&app, &id, request_id, r#"{"approved":false}"#).await,
        StatusCode::CONFLICT
    );
    let done = wait_http_status(&app, &id, "completed").await;
    assert_eq!(done["result"], json!(true));
    assert_eq!(done["requests"], json!([]));
    assert!(done.get("checkpoint").is_none());
    let late = http_start(&app, "review").await;
    let late_request =
        wait_http_status(&app, &late, "waiting-for-input").await["requests"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
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
        http_response(&app, &late, &late_request, r#"{"approved":true}"#).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        http_response(&app, &late, "missing", r#"{"approved":true}"#).await,
        StatusCode::CONFLICT
    );
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

// Each frame is one SSE event; keep reading separate from the status endpoint.
async fn sse_event(body: &mut Body) -> (String, Value) {
    use http_body_util::BodyExt;
    let frame = tokio::time::timeout(Duration::from_secs(3), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(frame.data_ref().unwrap()).unwrap();
    let kind = text
        .lines()
        .find_map(|line| line.strip_prefix("event: "))
        .unwrap();
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    (kind.to_string(), serde_json::from_str(data).unwrap())
}

async fn sse_body(app: &axum::Router, id: &str) -> Body {
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
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    response.into_body()
}

#[tokio::test]
async fn local_sse_snapshot_precedes_live_request_resolution_and_cancel_hints() {
    let path = std::env::temp_dir().join(format!("blkit-sse-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new(vec![request_graph(true)]).unwrap(),
            store.clone(),
            1,
        )
        .unwrap(),
    ));
    let id = http_start(&app, "review").await;
    let mut body = sse_body(&app, &id).await;
    let (event, initial) = sse_event(&mut body).await;
    assert_eq!(event, "snapshot");
    assert_eq!(initial["id"], id);
    assert_eq!(initial["status"], "waiting");
    assert_eq!(initial["requests"], json!([]));
    let (event, hint) = sse_event(&mut body).await;
    assert_eq!(event, "changed");
    assert_eq!(hint, json!({"id": id}));
    let waiting = wait_http_status(&app, &id, "waiting-for-input").await;
    let request_id = waiting["requests"][0]["id"].as_str().unwrap();
    assert_eq!(
        http_response(&app, &id, request_id, r#"{"approved":true}"#).await,
        StatusCode::ACCEPTED
    );
    let (event, hint) = sse_event(&mut body).await;
    assert_eq!((event.as_str(), hint), ("changed", json!({"id": id})));
    assert_eq!(
        wait_http_status(&app, &id, "completed").await["result"],
        json!(true)
    );
    let mut reconnected = sse_body(&app, &id).await;
    let (event, current) = sse_event(&mut reconnected).await;
    assert_eq!(event, "snapshot");
    assert_eq!(current["status"], "completed");
    assert_eq!(current["requests"], json!([]));
    assert!(current.get("checkpoint").is_none());
    drop(body);
    drop(reconnected);

    let another = http_start(&app, "review").await;
    let waiting = wait_http_status(&app, &another, "waiting-for-input").await;
    let mut watching = sse_body(&app, &another).await;
    let (kind, snapshot) = sse_event(&mut watching).await;
    assert_eq!(kind, "snapshot");
    assert_eq!(snapshot["id"], another);
    assert_eq!(snapshot["status"], "waiting-for-input");
    assert_eq!(snapshot["requests"], waiting["requests"]);
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/instances/{another}/cancel"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        sse_event(&mut watching).await,
        ("changed".into(), json!({"id": another}))
    );
    assert_eq!(
        wait_http_status(&app, &another, "cancelled").await["requests"],
        json!([])
    );
    drop(watching);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_sse_slow_client_closes_and_reconnects_without_replay() {
    use http_body_util::BodyExt;
    let path = std::env::temp_dir().join(format!("blkit-sse-slow-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new(vec![request_graph(false)]).unwrap(),
            store.clone(),
            1,
        )
        .unwrap(),
    ));
    let id = http_start(&app, "review").await;
    let waiting = wait_http_status(&app, &id, "waiting-for-input").await;
    let mut slow = sse_body(&app, &id).await;
    assert_eq!(sse_event(&mut slow).await.0, "snapshot");
    let request_id = waiting["requests"][0]["id"].as_str().unwrap();
    assert_eq!(
        http_response(&app, &id, request_id, r#"{"approved":true}"#).await,
        StatusCode::ACCEPTED
    );
    wait_http_status(&app, &id, "completed").await;
    // A bounded queue must not silently discard an event for a stalled client.
    let drained = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(frame) = slow.frame().await {
            frame.unwrap();
        }
    })
    .await;
    assert!(drained.is_ok(), "slow stream should close after lag");
    let mut fresh = sse_body(&app, &id).await;
    assert_eq!(sse_event(&mut fresh).await.1["status"], "completed");
    drop(fresh);
    let unknown = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/instances/unknown/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_get_never_mixes_waiting_status_with_committed_answer() {
    let path = std::env::temp_dir().join(format!("blkit-sse-race-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new(vec![request_graph(false)]).unwrap(),
            store.clone(),
            2,
        )
        .unwrap(),
    ));
    for _ in 0..12 {
        let id = http_start(&app, "review").await;
        let waiting = wait_http_status(&app, &id, "waiting-for-input").await;
        let request_id = waiting["requests"][0]["id"].as_str().unwrap();
        let reply = http_response(&app, &id, request_id, r#"{"approved":true}"#);
        let reads = async {
            for _ in 0..12 {
                let state = http_status(&app, &id).await;
                if state["status"] == "waiting-for-input" {
                    assert_eq!(state["requests"].as_array().unwrap().len(), 1);
                }
                if state["status"] == "completed" {
                    assert_eq!(state["requests"], json!([]));
                }
            }
        };
        let (code, ()) = tokio::join!(reply, reads);
        assert_eq!(code, StatusCode::ACCEPTED);
    }
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_sse_filters_other_instances_and_disconnected_subscriptions() {
    let path = std::env::temp_dir().join(format!("blkit-sse-isolate-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new(vec![request_graph(false)]).unwrap(),
            store.clone(),
            2,
        )
        .unwrap(),
    ));
    let first = http_start(&app, "review").await;
    let second = http_start(&app, "review").await;
    wait_http_status(&app, &first, "waiting-for-input").await;
    wait_http_status(&app, &second, "waiting-for-input").await;
    let mut watching = sse_body(&app, &first).await;
    sse_event(&mut watching).await;
    let request_id = http_status(&app, &second).await["requests"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        http_response(&app, &second, &request_id, r#"{"approved":true}"#).await,
        StatusCode::ACCEPTED
    );
    wait_http_status(&app, &second, "completed").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), sse_event(&mut watching))
            .await
            .is_err()
    );
    drop(watching);
    let mut reconnected = sse_body(&app, &first).await;
    assert_eq!(
        sse_event(&mut reconnected).await.1["status"],
        "waiting-for-input"
    );
    drop(reconnected);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_sse_reports_terminal_failure_commits() {
    let path = std::env::temp_dir().join(format!("blkit-sse-failed-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    store
        .create(&blkit::runtime::Instance::new(
            &id,
            "test",
            "1",
            "review",
            json!(null),
        ))
        .await
        .unwrap();
    let app = router(Arc::new(
        Engine::new(Registry::new(vec![]).unwrap(), store.clone(), 1).unwrap(),
    ));
    let mut body = sse_body(&app, &id).await;
    assert_eq!(sse_event(&mut body).await.1["status"], "pending");
    store.record_retry(&id, 1, 0, None, "failed").await.unwrap();
    assert_eq!(
        sse_event(&mut body).await,
        ("changed".into(), json!({"id": id}))
    );
    assert_eq!(http_status(&app, &id).await["status"], "failed");
    drop(body);
    drop(app);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

// Generated Rust is checked by compilation and integration tests, not style lints.
#[allow(clippy::all)]
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
    let store = LocalStore::open(&path).await.unwrap();
    store.recover_interrupted().await.unwrap();
    let app = router(Arc::new(
        Engine::new(
            Registry::new(compiled::named_graph_definitions()).unwrap(),
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
    let store = LocalStore::open(&path).await.unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "wait",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "slow",
                kind: GraphNodeKind::TaskWithCancel(
                    Arc::new({
                        let started = started.clone();
                        let release = release.clone();
                        move |_, _| {
                            started.store(true, Ordering::SeqCst);
                            while !release.load(Ordering::SeqCst) {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Ok(json!(true))
                        }
                    }),
                    Arc::new({
                        let release = release.clone();
                        move || release.store(true, Ordering::SeqCst)
                    }),
                ),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "slow",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "slow",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["slow"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let app = router(Arc::new(
        Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap(),
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
