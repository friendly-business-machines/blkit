use axum::{body::{Body, to_bytes}, http::{Request, StatusCode}};
use blkit::{runtime::{Definition, Engine, Registry, Step, Store}, server::router, transpile};
use serde_json::{Value, json};
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use tower::ServiceExt;

mod compiled { include!(concat!(env!("OUT_DIR"), "/graph.rs")); }

#[tokio::test]
async fn compiled_graph_http_start_status_and_errors() {
    assert_eq!(transpile(include_str!("../examples/graph.bl")).unwrap(), include_str!(concat!(env!("OUT_DIR"), "/graph.rs")));
    let path = std::env::temp_dir().join(format!("blkit-http-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    store.recover_interrupted().await.unwrap();
    let app = router(Arc::new(Engine::new(Registry::new(compiled::graph_definitions()).unwrap(), store.clone(), 4).unwrap()));
    let request = |uri: &str, body: Value| Request::builder().method("POST").uri(uri).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
    let start = app.clone().oneshot(request("/processes/orders/1.0/decide/instances", json!({"total":"1200"}))).await.unwrap();
    assert_eq!(start.status(), StatusCode::ACCEPTED);
    let payload: Value = serde_json::from_slice(&to_bytes(start.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let id = payload["id"].as_str().unwrap();
    let mut state = Value::Null;
    for _ in 0..100 {
        let response = app.clone().oneshot(Request::builder().uri(format!("/instances/{id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        state = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap();
        if state["status"] == "completed" { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(state["result"], "review");
    let cancel = app.clone().oneshot(request(&format!("/instances/{id}/cancel"), json!({}))).await.unwrap();
    assert_eq!(cancel.status(), StatusCode::CONFLICT);
    let invalid = app.clone().oneshot(request("/processes/orders/1.0/decide/instances", json!({"total":"abc"}))).await.unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let unknown = app.clone().oneshot(request("/processes/orders/2.0/decide/instances", json!({"total":"1200"}))).await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let missing = app.clone().oneshot(Request::builder().uri("/instances/not-found").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    drop(app); drop(store); std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn active_cancellation_is_acknowledged_over_http() {
    let path = std::env::temp_dir().join(format!("blkit-http-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let definition = Definition { namespace: "test", version: "1", name: "wait",
        steps: vec![Step::Run { name: "slow", call: Arc::new({ let started = started.clone(); let release = release.clone(); move |_, _| {
            started.store(true, Ordering::SeqCst);
            for _ in 0..100 { if release.load(Ordering::SeqCst) { break; } std::thread::sleep(Duration::from_millis(10)); }
            Ok(json!(true))
        }}), cancel: Arc::new({ let release = release.clone(); move || { release.store(true, Ordering::SeqCst); } }) },
            Step::Return(Arc::new(|_, values| Ok(values["slow"].clone())))],
        decode_input: Box::new(Ok::<Value, String>) };
    let app = router(Arc::new(Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 1).unwrap()));
    let request = Request::builder().method("POST").uri("/processes/test/1/wait/instances")
        .header("content-type", "application/json").body(Body::from("null")).unwrap();
    let start = app.clone().oneshot(request).await.unwrap();
    let payload: Value = serde_json::from_slice(&to_bytes(start.into_body(), 4096).await.unwrap()).unwrap();
    let id = payload["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !started.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
    let response = app.clone().oneshot(Request::builder().method("POST").uri(format!("/instances/{id}/cancel")).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let status = app.clone().oneshot(Request::builder().uri(format!("/instances/{id}")).body(Body::empty()).unwrap()).await.unwrap();
    let status: Value = serde_json::from_slice(&to_bytes(status.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(status["status"], "cancelled");
    drop(app); drop(store); std::fs::remove_file(path).unwrap();
}
