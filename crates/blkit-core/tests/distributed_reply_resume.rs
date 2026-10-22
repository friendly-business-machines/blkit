#![cfg(all(
    feature = "api-server",
    feature = "worker",
    feature = "remote-persistence",
    feature = "attachment-local"
))]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use blkit_core::{
    attachment_reader::AttachmentReader,
    attachment_store::AttachmentStore,
    compiled_graph::{
        GraphDefinition, GraphLink, GraphNode, GraphNodeKind, RequestSchema, RequestType,
    },
    distributed::{DistributedControl, DistributedWorker},
    postgres_store::PostgresStore,
    server::router_distributed_with_attachments,
};
use serde_json::{Value, json};
use std::sync::Arc;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};
use tower::ServiceExt;

fn review_graph() -> GraphDefinition {
    GraphDefinition {
        namespace: "demo",
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
                    prompt: "Approve?",
                    schema: RequestSchema {
                        ports: [("approved".into(), RequestType::Named("Bool".into()))].into(),
                        records: Default::default(),
                        enums: Default::default(),
                    },
                    decode_response: Box::new(Ok),
                },
            },
            GraphNode {
                name: "end",
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
                target: "end",
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
async fn remote_worker_resumes_file_bearing_input_reply_from_http() {
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
    let metadata = PostgresStore::connect(&url).await.unwrap();
    let first_worker = DistributedWorker::new(
        PostgresStore::connect(&url).await.unwrap(),
        "first",
        vec![review_graph()],
        1,
        30_000,
    )
    .unwrap();
    first_worker.advertise().await.unwrap();
    let root = std::env::temp_dir().join(format!("blkit-reply-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let db = root.join("db");
    let objects = AttachmentStore::local(&db, None).await.unwrap();
    let control = Arc::new(DistributedControl::new(metadata.clone()));
    let app = router_distributed_with_attachments(control.clone(), objects);
    let start = app
        .clone()
        .oneshot(
            Request::post("/processes/demo/1/review/instances")
                .header("content-type", "application/json")
                .body(Body::from("null"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(start.into_body(), 1024).await.unwrap();
    let id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), first_worker.run_once())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    let waiting = metadata.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.instance.status, "waiting-for-input");
    let request_id = waiting.requests[0].id.clone();
    let (_snapshot, mut notified, _registration) = control.subscribe(&id).await.unwrap();
    let reply = "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"proof.txt\"\r\n\r\nproof bytes\r\n--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{\"approved\":true}\r\n--x--\r\n";
    let accepted = app
        .oneshot(
            Request::post(format!("/instances/{id}/requests/{request_id}/response"))
                .header("content-type", "multipart/form-data; boundary=x")
                .body(Body::from(reply))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    tokio::time::timeout(std::time::Duration::from_secs(3), notified.recv())
        .await
        .unwrap()
        .unwrap();
    let reader = AttachmentReader::remote(
        PostgresStore::connect(&url).await.unwrap(),
        AttachmentStore::local(&db, None).await.unwrap(),
    );
    assert_eq!(
        reader.list(&id, Some(&request_id)).await.unwrap()[0].filename,
        "proof.txt"
    );
    assert_eq!(
        reader.read(&id, Some(&request_id), 0).await.unwrap(),
        Some(b"proof bytes".to_vec())
    );
    let second_worker = DistributedWorker::new(
        PostgresStore::connect(&url).await.unwrap(),
        "second",
        vec![review_graph()],
        1,
        30_000,
    )
    .unwrap();
    second_worker.advertise().await.unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), second_worker.run_once())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    let completed = metadata.get(&id).await.unwrap().unwrap();
    assert_eq!(completed.instance.status, "completed");
    assert_eq!(completed.instance.result, Some(json!(true)));
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    assert_eq!(
        client
            .query_one(
                "SELECT status FROM input_requests WHERE instance_id=$1 AND request_id=$2",
                &[&id, &request_id]
            )
            .await
            .unwrap()
            .get::<_, String>(0),
        "consumed"
    );
    assert_eq!(
        reader.read(&id, Some(&request_id), 0).await.unwrap(),
        Some(b"proof bytes".to_vec())
    );
    drop(reader);
    std::fs::remove_dir_all(root).unwrap();
}
