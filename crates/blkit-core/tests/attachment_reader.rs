#![cfg(feature = "attachment-local")]

use blkit_core::{
    Attachment,
    attachment_reader::{AttachmentMetadata, AttachmentReader},
    attachment_store::AttachmentStore,
    runtime::Instance,
};
use object_store::{ObjectStoreExt, PutPayload};
use serde_json::json;

#[cfg(feature = "local-persistence")]
#[tokio::test]
async fn local_reader_reopens_start_and_consumed_reply_without_leaking_keys() {
    use blkit_core::runtime::LocalStore;

    let directory = std::env::temp_dir().join(format!("blkit-reader-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = directory.join("instances.db");
    let metadata = LocalStore::open(&database).await.unwrap();
    let instance = Instance::new("one", "ns", "1", "p", json!({}));
    metadata.create(&instance).await.unwrap();
    let db = turso::Builder::new_local(database.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    conn.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema, status) VALUES ('one', 'reply', 'Prompt', '{}', 'consumed')", ()).await.unwrap();
    let objects = AttachmentStore::local(&database, None).await.unwrap();
    let start_key = AttachmentStore::new_key();
    let reply_key = AttachmentStore::new_key();
    for (key, bytes) in [
        (&start_key, b"private attachment payload".as_slice()),
        (&reply_key, b"answer".as_slice()),
    ] {
        objects
            .object_store()
            .put(key, PutPayload::from(bytes.to_vec()))
            .await
            .unwrap();
    }
    for (request_id, key, filename, length) in [
        (None, &start_key, "start.txt", 26),
        (Some("reply"), &reply_key, "reply.txt", 6),
    ] {
        metadata
            .insert_attachment(&Attachment {
                instance_id: "one".into(),
                request_id: request_id.map(str::to_owned),
                ordinal: 0,
                object_key: key.to_string(),
                filename: filename.into(),
                media_type: Some("text/plain".into()),
                byte_length: length,
            })
            .await
            .unwrap();
    }
    drop(conn);
    drop(db);
    drop(metadata);
    drop(objects);

    let reader = AttachmentReader::local(
        LocalStore::open(&database).await.unwrap(),
        AttachmentStore::local(&database, None).await.unwrap(),
    );
    assert_eq!(
        reader.list("one", None).await.unwrap(),
        vec![AttachmentMetadata {
            ordinal: 0,
            filename: "start.txt".into(),
            media_type: Some("text/plain".into()),
            byte_length: 26,
        }]
    );
    assert_eq!(
        reader.list("one", Some("reply")).await.unwrap(),
        vec![AttachmentMetadata {
            ordinal: 0,
            filename: "reply.txt".into(),
            media_type: Some("text/plain".into()),
            byte_length: 6,
        }]
    );
    assert_eq!(
        reader.read("one", None, 0).await.unwrap(),
        Some(b"private attachment payload".to_vec())
    );
    assert_eq!(
        reader.read("one", Some("reply"), 0).await.unwrap(),
        Some(b"answer".to_vec())
    );
    assert_eq!(reader.read("one", Some("other"), 0).await.unwrap(), None);
    assert_eq!(reader.read("missing", None, 0).await.unwrap(), None);
    assert_eq!(reader.read("one", None, 1).await.unwrap(), None);
    let status = serde_json::to_string(&instance).unwrap();
    assert!(
        !status.contains(&start_key.to_string())
            && !status.contains("start.txt")
            && !status.contains("private attachment payload")
    );

    drop(reader);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(feature = "remote-persistence")]
#[tokio::test]
async fn separate_postgres_worker_reads_shared_objects_by_scope_and_reports_missing_object() {
    use blkit_core::postgres_store::PostgresStore;
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
    let directory =
        std::env::temp_dir().join(format!("blkit-reader-shared-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let store_path = directory.join("instances.db");
    let api_metadata = PostgresStore::connect(&url).await.unwrap();
    api_metadata
        .create(&Instance::new("one", "ns", "1", "p", json!({})))
        .await
        .unwrap();
    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema, status) VALUES ('one', 'reply', 'Prompt', '{}', 'consumed')", &[]).await.unwrap();
    let api_objects = AttachmentStore::local(&store_path, None).await.unwrap();
    let start_key = AttachmentStore::new_key();
    let reply_key = AttachmentStore::new_key();
    for (request_id, key, filename, bytes) in [
        (None, &start_key, "first", b"first".as_slice()),
        (Some("reply"), &reply_key, "second", b"second".as_slice()),
    ] {
        api_objects
            .object_store()
            .put(key, PutPayload::from(bytes.to_vec()))
            .await
            .unwrap();
        api_metadata
            .insert_attachment(&Attachment {
                instance_id: "one".into(),
                request_id: request_id.map(str::to_owned),
                ordinal: 0,
                object_key: key.to_string(),
                filename: filename.into(),
                media_type: None,
                byte_length: bytes.len() as i64,
            })
            .await
            .unwrap();
    }
    drop(api_metadata);
    drop(api_objects);
    let worker = AttachmentReader::remote(
        PostgresStore::connect(&url).await.unwrap(),
        AttachmentStore::local(&store_path, None).await.unwrap(),
    );
    assert_eq!(worker.list("one", None).await.unwrap()[0].filename, "first");
    assert_eq!(
        worker.list("one", Some("reply")).await.unwrap()[0].filename,
        "second"
    );
    assert_eq!(
        worker.read("one", None, 0).await.unwrap(),
        Some(b"first".to_vec())
    );
    assert_eq!(
        worker.read("one", Some("reply"), 0).await.unwrap(),
        Some(b"second".to_vec())
    );
    assert_eq!(worker.read("one", Some("other"), 0).await.unwrap(), None);
    std::fs::remove_file(directory.join("attachments").join(start_key.to_string())).unwrap();
    let error = worker.read("one", None, 0).await.unwrap_err();
    assert!(error.contains("attachment object missing"), "{error}");
    drop(worker);
    std::fs::remove_dir_all(directory).unwrap();
}
