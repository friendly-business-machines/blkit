#![cfg(all(feature = "api-server", feature = "attachment-local"))]

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{Request, StatusCode},
    routing::post,
};
use blkit_core::{attachment_store::AttachmentStore, multipart::decode};
use futures_util::StreamExt;
use object_store::ObjectStoreExt;
use std::sync::Arc;
use tower::ServiceExt;

async fn stage(
    State(store): State<Arc<AttachmentStore>>,
    Path(part): Path<String>,
    multipart: Multipart,
) -> StatusCode {
    match decode(multipart, &part, &store).await {
        Ok(staged) => {
            let files = &staged.files;
            assert_eq!(
                staged.value,
                if part == "input" {
                    serde_json::json!({"amount": 3})
                } else {
                    serde_json::json!({"approved": true})
                }
            );
            for (index, file) in files.iter().enumerate() {
                assert_eq!(file.filename, format!("{index}.txt"));
                assert_eq!(file.byte_length, 6);
                let bytes = store
                    .object_store()
                    .get(&file.key)
                    .await
                    .unwrap()
                    .bytes()
                    .await
                    .unwrap();
                assert_eq!(&bytes[..], format!("file-{index}").as_bytes());
            }
            staged.cleanup(&store).await.unwrap();
            StatusCode::OK
        }
        Err(code) => code,
    }
}

fn part(body: &mut Vec<u8>, name: &str, filename: Option<&str>, data: &[u8]) {
    body.extend_from_slice(b"--boundary\r\nContent-Disposition: form-data; name=\"");
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

async fn send_to(part_name: &str, parts: &[(&str, Option<&str>, &[u8])], expected: StatusCode) {
    let mut body = Vec::new();
    for (name, filename, data) in parts {
        part(&mut body, name, *filename, data);
    }
    body.extend_from_slice(b"--boundary--\r\n");
    send_body(part_name, Body::from(body), expected).await;
}

async fn send_body(part_name: &str, body: Body, expected: StatusCode) {
    let root = std::env::temp_dir().join(format!("blkit-multipart-{}", uuid::Uuid::new_v4()));
    let store = Arc::new(
        AttachmentStore::local(&root.join("db"), None)
            .await
            .unwrap(),
    );
    let app = Router::new()
        .route("/upload/{part}", post(stage))
        .layer(DefaultBodyLimit::max(blkit_core::multipart::MAX_BODY_BYTES))
        .with_state(store.clone());
    let response = app
        .oneshot(
            Request::post(format!("/upload/{part_name}"))
                .header("content-type", "multipart/form-data; boundary=boundary")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), expected);
    assert!(
        store.object_store().list(None).next().await.is_none(),
        "staged objects must be cleaned up"
    );
    std::fs::remove_dir_all(root).unwrap();
}

async fn send(parts: &[(&str, Option<&str>, &[u8])], expected: StatusCode) {
    send_to("input", parts, expected).await;
}

#[tokio::test]
async fn multipart_stages_multiple_files_without_losing_order_or_bytes() {
    send(
        &[
            ("input", None, br#"{"amount":3}"#),
            ("files", Some("0.txt"), b"file-0"),
            ("files", Some("1.txt"), b"file-1"),
        ],
        StatusCode::OK,
    )
    .await;
}

#[tokio::test]
async fn multipart_stages_response_file_with_the_response_json_part() {
    send_to(
        "response",
        &[
            ("response", None, br#"{"approved":true}"#),
            ("files", Some("0.txt"), b"file-0"),
        ],
        StatusCode::OK,
    )
    .await;
}

#[tokio::test]
async fn multipart_rejects_invalid_parts_and_cleans_up_earlier_files() {
    for parts in [
        vec![("files", Some("a"), &b"file-0"[..])],
        vec![("input", None, &b"{}"[..])],
        vec![
            ("input", None, &b"{}"[..]),
            ("files", Some("a"), &b"file-0"[..]),
            ("input", None, &b"{}"[..]),
        ],
        vec![
            ("input", None, &b"not-json"[..]),
            ("files", Some("a"), &b"file-0"[..]),
        ],
        vec![("input", None, &b"{}"[..]), ("other", None, &b"x"[..])],
        vec![("input", None, &b"{}"[..]), ("files", None, &b"file-0"[..])],
        vec![("input", Some("input.json"), &b"{}"[..])],
        vec![
            ("input", None, &b"{}"[..]),
            ("files", Some(""), &b"file-0"[..]),
        ],
        vec![("input", None, &b"{}"[..]), ("files", Some("a"), &b""[..])],
    ] {
        send(&parts, StatusCode::BAD_REQUEST).await;
    }
}

#[tokio::test]
async fn multipart_rejects_eleven_files() {
    let mut parts = vec![("input", None, &br#"{"amount":3}"#[..])];
    parts.extend((0..11).map(|_| ("files", Some("a"), &b"file-0"[..])));
    send(&parts, StatusCode::PAYLOAD_TOO_LARGE).await;
}

#[tokio::test]
async fn multipart_rejects_file_over_25_mib() {
    let oversized = vec![b'x'; 25 * 1024 * 1024 + 1];
    send(
        &[
            ("input", None, br#"{"amount":3}"#),
            ("files", Some("a"), &oversized),
        ],
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;
}

#[tokio::test]
async fn multipart_rejects_json_part_over_2_mib() {
    let json = vec![b'x'; 2 * 1024 * 1024 + 1];
    send(&[("input", None, &json)], StatusCode::PAYLOAD_TOO_LARGE).await;
}

#[tokio::test]
async fn multipart_rejects_combined_files_over_100_mib() {
    let data = Bytes::from(vec![b'x'; 25 * 1024 * 1024]);
    let mut chunks = vec![Bytes::from_static(
        b"--boundary\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{\"amount\":3}\r\n",
    )];
    for _ in 0..4 {
        chunks.push(Bytes::from_static(
            b"--boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a\"\r\n\r\n",
        ));
        chunks.push(data.clone());
        chunks.push(Bytes::from_static(b"\r\n"));
    }
    chunks.push(Bytes::from_static(b"--boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a\"\r\n\r\nx\r\n--boundary--\r\n"));
    let stream = futures_util::stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>));
    send_body(
        "input",
        Body::from_stream(stream),
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;
}
