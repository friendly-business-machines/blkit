#![cfg(all(
    feature = "api-server",
    feature = "worker",
    feature = "local-persistence"
))]

use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use blkit_core::{
    runtime::{Engine, LocalStore, Registry},
    server::router,
};
use std::sync::Arc;
use tower::ServiceExt;

fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, data) in parts {
        body.extend_from_slice(b"--x\r\nContent-Disposition: form-data; name=\"");
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(b"\"");
        if let Some(filename) = filename {
            body.extend_from_slice(b"; filename=\"");
            body.extend_from_slice(filename.as_bytes());
            body.extend_from_slice(b"\"");
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b"--x--\r\n");
    body
}

fn multipart_request(path: &str, body: Body) -> Request<Body> {
    Request::post(path)
        .header("content-type", "multipart/form-data; boundary=x")
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn json_routes_keep_existing_validation_while_multipart_cannot_be_acknowledged_without_atomic_admission()
 {
    let root = std::env::temp_dir().join(format!("blkit-route-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = LocalStore::open(&root.join("db")).await.unwrap();
    let engine = Arc::new(Engine::new(Registry::new(vec![]).unwrap(), store, 1).unwrap());
    let app = router(engine);
    let start = "/processes/demo/1/unknown/instances";
    let reply = "/instances/missing/requests/missing/response";
    for (path, content_type, body, status) in [
        (start, "application/json", "{}", StatusCode::NOT_FOUND),
        (start, "application/json", "bad", StatusCode::BAD_REQUEST),
        (reply, "application/json", "{}", StatusCode::NOT_FOUND),
        (
            start,
            "multipart/form-data; boundary=x",
            "--x\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nhello\r\n--x--\r\n",
            StatusCode::BAD_REQUEST,
        ),
        (
            reply,
            "multipart/form-data; boundary=x",
            "--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nhello\r\n--x--\r\n",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{path} {content_type}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "attachment-local")]
#[tokio::test]
async fn multipart_start_commits_two_ordered_files_and_rejects_invalid_typed_input() {
    use blkit_core::{
        attachment_store::AttachmentStore,
        compiled_graph::{GraphDefinition, GraphNode, GraphNodeKind},
        server::router_with_attachments,
    };
    use futures_util::StreamExt;
    use object_store::ObjectStoreExt;
    use serde_json::{Value, json};
    let root = std::env::temp_dir().join(format!("blkit-start-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let db = root.join("db");
    let metadata = LocalStore::open(&db).await.unwrap();
    let objects = AttachmentStore::local(&db, None).await.unwrap();
    let graph = GraphDefinition {
        namespace: "demo",
        version: "1",
        name: "typed",
        retry: None,
        deadline: None,
        decode_input: Box::new(|value| {
            if value == json!({"amount": 3}) {
                Ok(value)
            } else {
                Err("amount must be 3".into())
            }
        }),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "end",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![blkit_core::compiled_graph::GraphLink {
            source: "start",
            target: "end",
            value: Some(Arc::new(|input, _| Ok(input.clone()))),
            condition: None,
            fallback: false,
            label: None,
        }],
    };
    let engine =
        Arc::new(Engine::new(Registry::new(vec![graph]).unwrap(), metadata.clone(), 1).unwrap());
    let app = router_with_attachments(engine, objects.clone());
    let path = "/processes/demo/1/typed/instances";
    let request = |json: &str| {
        // A file before JSON is allowed; neither the other file nor its metadata may be dropped.
        format!(
            "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nfirst\r\n--x\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{json}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"b.txt\"\r\n\r\nsecond\r\n--x--\r\n"
        )
    };
    let send = |body: String| {
        Request::post(path)
            .header("content-type", "multipart/form-data; boundary=x")
            .body(Body::from(body))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(send(request(r#"{"amount":3}"#)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        metadata.get(&id).await.unwrap().unwrap().input,
        json!({"amount":3})
    );
    let files = metadata.attachments(&id, None).await.unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| (f.ordinal, f.filename.as_str(), f.byte_length))
            .collect::<Vec<_>>(),
        vec![(0, "a.txt", 5), (1, "b.txt", 6)]
    );
    for (file, expected) in files.iter().zip(["first", "second"]) {
        let bytes = objects
            .object_store()
            .get(&object_store::path::Path::from(file.object_key.as_str()))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), expected.as_bytes());
    }
    // A valid JSON part cannot bypass the shared parser's field/filename rules.
    let first_file = ("files", Some("kept.txt"), &b"kept"[..]);
    for parts in [
        vec![
            first_file,
            ("other", None, &b"x"[..]),
            ("input", None, &br#"{"amount":3}"#[..]),
        ],
        vec![first_file, ("response", None, &br#"{"amount":3}"#[..])],
        vec![first_file, ("input", None, &b"invalid"[..])],
        vec![
            first_file,
            ("input", None, &br#"{"amount":3}"#[..]),
            ("input", None, &br#"{"amount":3}"#[..]),
        ],
        vec![
            first_file,
            ("input", Some("input.json"), &br#"{"amount":3}"#[..]),
        ],
        vec![
            first_file,
            ("files", None, &b"file"[..]),
            ("input", None, &br#"{"amount":3}"#[..]),
        ],
        vec![
            first_file,
            ("files", Some(""), &b"file"[..]),
            ("input", None, &br#"{"amount":3}"#[..]),
        ],
        vec![
            first_file,
            ("files", Some("a.txt"), &b""[..]),
            ("input", None, &br#"{"amount":3}"#[..]),
        ],
    ] {
        let response = app
            .clone()
            .oneshot(multipart_request(path, Body::from(multipart_body(&parts))))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{parts:?}");
    }
    let json_over_default_limit = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"amount\":\"{}\"}}",
                    "a".repeat(2 * 1024 * 1024)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_over_default_limit.status(), StatusCode::BAD_REQUEST);
    let bad = app
        .clone()
        .oneshot(send(request(r#"{"amount":"bad"}"#)))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let invalid = app.clone().oneshot(send(request("invalid"))).await.unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let oversized = app.clone().oneshot(send(format!("--x\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{{\"amount\":3}}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"small\"\r\n\r\nfirst\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"large\"\r\n\r\n{}\r\n--x--\r\n", "z".repeat(25*1024*1024+1)))).await.unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(metadata.attachments(&id, None).await.unwrap().len(), 2);
    let prefix = object_store::path::Path::from("blkit/attachments");
    let count = || async { objects.object_store().list(Some(&prefix)).count().await };
    assert_eq!(
        count().await,
        2,
        "invalid typed input and oversized uploads must be removed"
    );
    let broken = futures_util::stream::iter([
        Ok::<_, std::io::Error>(
            "--x\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{\"amount\":3}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"done.txt\"\r\n\r\ndone\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"partial.txt\"\r\n\r\npartial",
        ),
        Err(std::io::Error::other("injected body read failure")),
    ]);
    let failed = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "multipart/form-data; boundary=x")
                .body(Body::from_stream(broken))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        count().await,
        2,
        "a failed second upload must remove the completed first object"
    );
    let db = turso::Builder::new_local(db.to_str().unwrap())
        .build()
        .await
        .unwrap();
    db.connect().unwrap().execute("CREATE TRIGGER reject_start_attachment BEFORE INSERT ON instance_attachments BEGIN SELECT RAISE(FAIL, 'injected metadata failure'); END", ()).await.unwrap();
    let failed = app
        .clone()
        .oneshot(send(request(r#"{"amount":3}"#)))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        count().await,
        2,
        "DB failure must clean uncommitted objects"
    );
    db.connect()
        .unwrap()
        .execute("DROP TRIGGER reject_start_attachment", ())
        .await
        .unwrap();
    let location = root.join("attachments");
    let backup = root.join("saved-attachments");
    std::fs::rename(&location, &backup).unwrap();
    std::fs::write(&location, "not a directory").unwrap();
    let failed = app
        .clone()
        .oneshot(send(request(r#"{"amount":3}"#)))
        .await
        .unwrap();
    assert_eq!(
        failed.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "failed object upload must not admit start"
    );
    std::fs::remove_file(&location).unwrap();
    std::fs::rename(&backup, &location).unwrap();
    assert_eq!(count().await, 2);
    let mut rows = db
        .connect()
        .unwrap()
        .query("SELECT count(*) FROM instances", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        1
    );
    drop(rows);
    let mut ten = vec![("input", None, &br#"{"amount":3}"#[..])];
    ten.extend((0..10).map(|_| ("files", Some("../a.txt"), &b"x"[..])));
    let response = app
        .clone()
        .oneshot(multipart_request(path, Body::from(multipart_body(&ten))))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let ten_id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ten_files = metadata.attachments(&ten_id, None).await.unwrap();
    assert_eq!(ten_files.len(), 10);
    assert!(
        ten_files
            .iter()
            .all(|file| file.filename == "a.txt" && file.byte_length == 1)
    );
    assert_eq!(count().await, 12);
    assert_eq!(
        app.oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"amount":3}"#))
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(count().await, 12, "JSON-only start must not upload files");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(all(feature = "remote-persistence", feature = "attachment-local"))]
#[tokio::test]
async fn distributed_multipart_start_requires_live_worker_and_defers_typed_input() {
    use blkit_core::{
        attachment_store::AttachmentStore, distributed::DistributedControl,
        postgres_store::PostgresStore, server::router_distributed_with_attachments,
    };
    use futures_util::StreamExt;
    use serde_json::{Value, json};
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
    let metadata = PostgresStore::connect_with_pool_size(&url, 1)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("blkit-remote-start-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let objects = AttachmentStore::local(&root.join("db"), None)
        .await
        .unwrap();
    let app = router_distributed_with_attachments(
        Arc::new(DistributedControl::new(metadata.clone())),
        objects.clone(),
    );
    let send = || {
        Request::post("/processes/demo/1/typed/instances")
        .header("content-type", "multipart/form-data; boundary=x")
        .body(Body::from("--x\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{\"amount\":\"worker-validates\"}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nhello\r\n--x--\r\n"))
        .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(send()).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    metadata
        .register_worker("live", &[("demo", "1", "typed")])
        .await
        .unwrap();
    let response = app.clone().oneshot(send()).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        metadata.get(&id).await.unwrap().unwrap().instance.input,
        json!({"amount":"worker-validates"})
    );
    assert_eq!(metadata.attachments(&id, None).await.unwrap().len(), 1);
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client.batch_execute("CREATE FUNCTION fail_start_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'deferred metadata failure'; END $$; CREATE CONSTRAINT TRIGGER fail_start_commit AFTER INSERT ON instance_attachments DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_start_commit();").await.unwrap();
    let failed = app.clone().oneshot(send()).await.unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        objects.object_store().list(None).count().await,
        1,
        "a rejected COMMIT must clean its unreferenced upload"
    );
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM instances", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    client.batch_execute("DROP TRIGGER fail_start_commit ON instance_attachments; DROP FUNCTION fail_start_commit(); CREATE FUNCTION uncertain_start_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'connection interrupted' USING ERRCODE = '57P01'; END $$; CREATE CONSTRAINT TRIGGER uncertain_start_commit AFTER INSERT ON instance_attachments DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION uncertain_start_commit();").await.unwrap();
    assert_eq!(
        app.clone().oneshot(send()).await.unwrap().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        objects.object_store().list(None).count().await,
        2,
        "an uncertain SQLSTATE must retain the upload despite an empty association read"
    );
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM instances", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    client.batch_execute("CREATE OR REPLACE FUNCTION uncertain_start_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'completion unknown' USING ERRCODE = '40003'; END $$;").await.unwrap();
    assert_eq!(
        app.clone().oneshot(send()).await.unwrap().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        objects.object_store().list(None).count().await,
        3,
        "unknown completion cannot be classified as a rolled-back transaction"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "attachment-local")]
#[tokio::test]
async fn local_multipart_reply_commits_ordered_files_once_and_preserves_json_validation() {
    use blkit_core::{
        attachment_store::AttachmentStore, runtime::Instance, server::router_with_attachments,
    };
    use futures_util::StreamExt;
    use serde_json::json;
    let root = std::env::temp_dir().join(format!("blkit-reply-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let db = root.join("db");
    let metadata = LocalStore::open(&db).await.unwrap();
    metadata
        .create(&Instance::new("job", "demo", "1", "p", json!(null)))
        .await
        .unwrap();
    let database = turso::Builder::new_local(db.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    conn.execute("INSERT INTO input_requests (instance_id,request_id,prompt,schema) VALUES ('job','ask','Approve?',?1)", [r#"{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}"#]).await.unwrap();
    let objects = AttachmentStore::local(&db, None).await.unwrap();
    let engine =
        Arc::new(Engine::new(Registry::new(vec![]).unwrap(), metadata.clone(), 1).unwrap());
    let app = router_with_attachments(engine, objects.clone());
    let path = "/instances/job/requests/ask/response";
    let send = |body: String, content_type: &str| {
        Request::post(path)
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap()
    };
    let multipart = |value: &str| {
        format!(
            "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nfirst\r\n--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{value}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"b.txt\"\r\n\r\nsecond\r\n--x--\r\n"
        )
    };
    let prefix = object_store::path::Path::from("blkit/attachments");
    let bad = app
        .clone()
        .oneshot(send(
            multipart(r#"{"approved":1}"#),
            "multipart/form-data; boundary=x",
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 0);
    let duplicate_json = format!(
        "{}--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{{\"approved\":true}}\r\n--x--\r\n",
        multipart(r#"{"approved":true}"#).trim_end_matches("--x--\r\n")
    );
    assert_eq!(
        app.clone()
            .oneshot(send(duplicate_json, "multipart/form-data; boundary=x"))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Wrong parts and filenames must fail before a reply is recorded, including after an uploaded file.
    for parts in [
        vec![
            ("files", Some("kept.txt"), &b"kept"[..]),
            ("input", None, &br#"{"approved":true}"#[..]),
        ],
        vec![
            ("response", None, &br#"{"approved":true}"#[..]),
            ("files", None, &b"file"[..]),
        ],
        vec![
            ("response", None, &br#"{"approved":true}"#[..]),
            ("files", Some(""), &b"file"[..]),
        ],
        vec![
            ("response", None, &b"invalid"[..]),
            ("files", Some("a"), &b"file"[..]),
        ],
    ] {
        assert_eq!(
            app.clone()
                .oneshot(multipart_request(path, Body::from(multipart_body(&parts))))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST,
            "{parts:?}"
        );
    }
    let oversized = format!(
        "--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{{\"approved\":true}}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"huge.txt\"\r\n\r\n{}\r\n--x--\r\n",
        "z".repeat(25 * 1024 * 1024 + 1)
    );
    assert_eq!(
        app.clone()
            .oneshot(send(oversized, "multipart/form-data; boundary=x"))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 0);
    let mut eleven = vec![("response", None, &br#"{"approved":true}"#[..])];
    eleven.extend((0..11).map(|_| ("files", Some("a.txt"), &b"x"[..])));
    assert_eq!(
        app.clone()
            .oneshot(multipart_request(path, Body::from(multipart_body(&eleven))))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let large_json = vec![b'x'; 2 * 1024 * 1024 + 1];
    assert_eq!(
        app.clone()
            .oneshot(multipart_request(
                path,
                Body::from(multipart_body(&[
                    ("response", None, &large_json),
                    ("files", Some("a.txt"), b"x")
                ]))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // Reuse one 25 MiB chunk for four files so the wire crosses 100 MiB without four copies in RAM.
    let file = Bytes::from(vec![b'x'; 25 * 1024 * 1024]);
    let mut chunks = vec![Bytes::from_static(
        b"--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{\"approved\":true}\r\n",
    )];
    for _ in 0..4 {
        chunks.push(Bytes::from_static(
            b"--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\n",
        ));
        chunks.push(file.clone());
        chunks.push(Bytes::from_static(b"\r\n"));
    }
    chunks.push(Bytes::from_static(b"--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nx\r\n--x--\r\n"));
    let stream = futures_util::stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>));
    assert_eq!(
        app.clone()
            .oneshot(multipart_request(path, Body::from_stream(stream)))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // A never-ending multipart preamble exercises the envelope independently of per-file limits.
    let preamble = Bytes::from(vec![b'x'; 1024 * 1024]);
    let stream = futures_util::stream::iter(
        (0..104).map(move |_| Ok::<_, std::io::Error>(preamble.clone())),
    );
    assert_eq!(
        app.clone()
            .oneshot(multipart_request(path, Body::from_stream(stream)))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 0);
    assert!(
        metadata
            .attachments("job", Some("ask"))
            .await
            .unwrap()
            .is_empty()
    );
    let mut rows = conn
        .query(
            "SELECT response FROM input_requests WHERE instance_id='job' AND request_id='ask'",
            (),
        )
        .await
        .unwrap();
    assert!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<Option<String>>(0)
            .unwrap()
            .is_none()
    );
    drop(rows);
    let accepted = app
        .clone()
        .oneshot(send(
            multipart(r#"{"approved":true}"#),
            "multipart/form-data; boundary=x",
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let files = metadata.attachments("job", Some("ask")).await.unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| (file.ordinal, file.filename.as_str(), file.byte_length))
            .collect::<Vec<_>>(),
        vec![(0, "a.txt", 5), (1, "b.txt", 6)]
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
    let mut rows = conn
        .query(
            "SELECT response FROM input_requests WHERE instance_id='job' AND request_id='ask'",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&row.get::<String>(0).unwrap()).unwrap(),
        json!({"approved":true})
    );
    drop(rows);
    let duplicate = app
        .clone()
        .oneshot(send(
            multipart(r#"{"approved":false}"#),
            "multipart/form-data; boundary=x",
        ))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
    let json_duplicate = app
        .clone()
        .oneshot(send(r#"{"approved":false}"#.into(), "application/json"))
        .await
        .unwrap();
    assert_eq!(json_duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    conn.execute("UPDATE input_requests SET status='outstanding',response=NULL WHERE instance_id='job' AND request_id='ask'", ()).await.unwrap();
    // Hold a multipart file open while another reply wins the transaction.
    let (sender, receiver) = tokio::sync::mpsc::channel::<String>(2);
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver
            .recv()
            .await
            .map(|chunk| (Ok::<_, std::io::Error>(chunk), receiver))
    });
    let pending = tokio::spawn(
        app.clone().oneshot(
            Request::post(path)
                .header("content-type", "multipart/form-data; boundary=x")
                .body(Body::from_stream(stream))
                .unwrap(),
        ),
    );
    sender.send("--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{\"approved\":true}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"uploaded.txt\"\r\n\r\nuploaded\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"race.txt\"\r\n\r\nheld".into()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while objects.object_store().list(Some(&prefix)).count().await != 3 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":false}"#.into(), "application/json"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    sender.send("\r\n--x--\r\n".into()).await.unwrap();
    drop(sender);
    assert_eq!(
        pending.await.unwrap().unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
    conn.execute("UPDATE input_requests SET status='outstanding',response=NULL WHERE instance_id='job' AND request_id='ask'", ()).await.unwrap();
    conn.execute("CREATE TRIGGER reject_reply_attachment BEFORE INSERT ON instance_attachments WHEN NEW.ordinal=1 BEGIN SELECT RAISE(FAIL, 'injected metadata failure'); END", ()).await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(
                multipart(r#"{"approved":true}"#),
                "multipart/form-data; boundary=x"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
    let row = conn.query("SELECT status,response FROM input_requests WHERE instance_id='job' AND request_id='ask'", ()).await.unwrap().next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "outstanding");
    assert!(row.get::<Option<String>>(1).unwrap().is_none());
    conn.execute("DROP TRIGGER reject_reply_attachment", ())
        .await
        .unwrap();
    for transition in [
        "UPDATE instances SET status='cancelled' WHERE id='job'",
        "UPDATE instances SET status='waiting-for-input',deadline_at_ms=1 WHERE id='job'",
    ] {
        let (sender, receiver) = tokio::sync::mpsc::channel::<String>(2);
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver
                .recv()
                .await
                .map(|chunk| (Ok::<_, std::io::Error>(chunk), receiver))
        });
        let pending = tokio::spawn(
            app.clone().oneshot(
                Request::post(path)
                    .header("content-type", "multipart/form-data; boundary=x")
                    .body(Body::from_stream(stream))
                    .unwrap(),
            ),
        );
        sender.send("--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{\"approved\":true}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"uploaded.txt\"\r\n\r\nuploaded\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"race.txt\"\r\n\r\nheld".into()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while objects.object_store().list(Some(&prefix)).count().await != 3 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        conn.execute(transition, ()).await.unwrap();
        sender.send("\r\n--x--\r\n".into()).await.unwrap();
        drop(sender);
        assert_eq!(
            pending.await.unwrap().unwrap().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
        assert_eq!(
            metadata.attachments("job", Some("ask")).await.unwrap(),
            files
        );
        conn.execute(
            "UPDATE instances SET status='pending',deadline_at_ms=NULL WHERE id='job'",
            (),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":1}"#.into(), "application/json"))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.oneshot(send(r#"{"approved":true}"#.into(), "application/json"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    assert_eq!(objects.object_store().list(Some(&prefix)).count().await, 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(all(feature = "remote-persistence", feature = "attachment-local"))]
#[tokio::test]
async fn distributed_multipart_reply_notifies_and_resumes_after_atomic_commit() {
    use blkit_core::{
        attachment_store::AttachmentStore, distributed::DistributedControl,
        postgres_store::PostgresStore, server::router_distributed_with_attachments,
    };
    use futures_util::StreamExt;
    use object_store::ObjectStoreExt;
    use serde_json::json;
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
    let metadata = PostgresStore::connect(&url).await.unwrap();
    metadata
        .register_worker("live", &[("demo", "1", "p")])
        .await
        .unwrap();
    metadata
        .admit("job", "demo", "1", "p", &json!(null))
        .await
        .unwrap();
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client.execute("INSERT INTO input_requests(instance_id,request_id,prompt,schema) VALUES ('job','ask','Approve?',$1)", &[&r#"{"ports":{"approved":{"Named":"Bool"}},"records":{},"enums":{}}"#]).await.unwrap();
    client
        .execute(
            "UPDATE instances SET status='waiting-for-input' WHERE id='job'",
            &[],
        )
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("blkit-remote-reply-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let objects = AttachmentStore::local(&root.join("db"), None)
        .await
        .unwrap();
    let control = Arc::new(DistributedControl::new(metadata.clone()));
    let (_snapshot, mut receiver, _registration) = control.subscribe("job").await.unwrap();
    let app = router_distributed_with_attachments(control, objects.clone());
    let path = "/instances/job/requests/ask/response";
    let send = |value: &str| {
        Request::post(path).header("content-type", "multipart/form-data; boundary=x")
        .body(Body::from(format!("--x\r\nContent-Disposition: form-data; name=\"response\"\r\n\r\n{value}\r\n--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\n\r\nfirst\r\n--x--\r\n"))).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":false,"extra":1}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    client.batch_execute("CREATE FUNCTION reject_reply_file() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'metadata failure'; END $$; CREATE TRIGGER reject_reply_file BEFORE INSERT ON instance_attachments FOR EACH ROW EXECUTE FUNCTION reject_reply_file();").await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        client
            .query_one(
                "SELECT status FROM input_requests WHERE instance_id='job' AND request_id='ask'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, String>(0),
        "outstanding"
    );
    assert!(
        metadata
            .attachments("job", Some("ask"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        objects
            .object_store()
            .list(Some(&object_store::path::Path::from("blkit/attachments")))
            .count()
            .await,
        0
    );
    client.batch_execute("DROP TRIGGER reject_reply_file ON instance_attachments; DROP FUNCTION reject_reply_file(); CREATE FUNCTION fail_reply_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'deferred reply failure'; END $$; CREATE CONSTRAINT TRIGGER fail_reply_commit AFTER INSERT ON instance_attachments DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_reply_commit();").await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(
        metadata
            .attachments("job", Some("ask"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        objects.object_store().list(None).count().await,
        0,
        "rejected reply COMMIT must clean its upload"
    );
    client.batch_execute("DROP TRIGGER fail_reply_commit ON instance_attachments; DROP FUNCTION fail_reply_commit();").await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let instance = metadata.get("job").await.unwrap().unwrap();
    assert_eq!(instance.instance.status, "pending");
    let files = metadata.attachments("job", Some("ask")).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, "a.txt");
    assert_eq!(client.query_one("SELECT status, response FROM input_requests WHERE instance_id='job' AND request_id='ask'", &[]).await.unwrap().get::<_, String>(1), r#"{"approved":true}"#);
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":false}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    let claims = metadata.claim("live", 1, 30_000).await.unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].instance.id, "job");
    let contents = objects
        .object_store()
        .get(&object_store::path::Path::from(
            files[0].object_key.as_str(),
        ))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(contents.as_ref(), b"first");
    client.execute("UPDATE input_requests SET status='outstanding',response=NULL WHERE instance_id='job' AND request_id='ask'", &[]).await.unwrap();
    client
        .execute(
            "UPDATE instances SET status='cancelled' WHERE id='job'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    client
        .execute(
            "UPDATE instances SET status='waiting-for-input',deadline_at_ms=1 WHERE id='job'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(send(r#"{"approved":true}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    client
        .execute(
            "UPDATE instances SET status='pending',deadline_at_ms=NULL WHERE id='job'",
            &[],
        )
        .await
        .unwrap();
    let json_only = Request::post(path)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"approved":false}"#))
        .unwrap();
    assert_eq!(
        app.oneshot(json_only).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        metadata.attachments("job", Some("ask")).await.unwrap(),
        files
    );
    std::fs::remove_dir_all(root).unwrap();
}
