use blkit_core::{
    Attachment,
    runtime::{Instance, LocalStore},
};
use serde_json::json;

#[tokio::test]
async fn local_attachment_scope_order_fk_and_reopen() {
    let path = std::env::temp_dir().join(format!("blkit-attachments-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    store
        .create(&Instance::new("one", "ns", "1", "p", json!({})))
        .await
        .unwrap();
    store
        .create(&Instance::new("two", "ns", "1", "p", json!({})))
        .await
        .unwrap();
    let request = "reply";
    // Exercise the real request lifecycle through its existing checkpoint path in store tests;
    // here the request row is inserted directly to isolate the composite FK.
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    conn.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('one','reply','Prompt','{}')", ()).await.unwrap();
    let start = |ordinal, key: &str| Attachment {
        instance_id: "one".into(),
        request_id: None,
        ordinal,
        object_key: key.into(),
        filename: "file.txt".into(),
        media_type: Some("text/plain".into()),
        byte_length: 4,
    };
    store
        .insert_attachment(&start(1, "objects/second"))
        .await
        .unwrap();
    store
        .insert_attachment(&start(0, "objects/first"))
        .await
        .unwrap();
    assert!(
        store
            .insert_attachment(&start(0, "objects/duplicate"))
            .await
            .is_err()
    );
    let mut reply = start(0, "objects/reply");
    reply.request_id = Some(request.into());
    store.insert_attachment(&reply).await.unwrap();
    assert!(store.insert_attachment(&reply).await.is_err());
    let mut wrong = reply.clone();
    wrong.instance_id = "two".into();
    assert!(
        store.insert_attachment(&wrong).await.is_err(),
        "request must belong to same instance"
    );
    wrong.instance_id = "missing".into();
    wrong.request_id = None;
    assert!(
        store.insert_attachment(&wrong).await.is_err(),
        "instance FK"
    );
    assert_eq!(
        store.attachments("one", None).await.unwrap(),
        vec![start(0, "objects/first"), start(1, "objects/second")]
    );
    assert_eq!(
        store.attachments("one", Some(request)).await.unwrap(),
        vec![reply.clone()]
    );
    let mut columns = conn
        .query("PRAGMA table_info(instance_attachments)", ())
        .await
        .unwrap();
    let mut count = 0;
    while columns.next().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(
        count, 7,
        "only metadata and object keys belong in the database"
    );
    drop(columns);
    conn.execute("UPDATE input_requests SET status='consumed' WHERE instance_id='one' AND request_id='reply'", ()).await.unwrap();
    drop(conn);
    drop(db);
    drop(store);
    let reopened = LocalStore::open(&path).await.unwrap();
    assert_eq!(
        reopened.attachments("one", Some(request)).await.unwrap(),
        vec![reply]
    );
    assert!(
        reopened
            .attachments("two", Some(request))
            .await
            .unwrap()
            .is_empty()
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_start_rejects_mismatched_attachment_scope_before_insert() {
    let path = std::env::temp_dir().join(format!("blkit-start-scope-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    store
        .create(&Instance::new("other", "ns", "1", "p", json!({})))
        .await
        .unwrap();
    let instance = Instance::new("new", "ns", "1", "p", json!({}));
    for (wrong_id, request_id) in [("other", None), ("new", Some("reply"))] {
        let file = Attachment {
            instance_id: wrong_id.into(),
            request_id: request_id.map(str::to_owned),
            ordinal: 0,
            object_key: "objects/wrong".into(),
            filename: "wrong".into(),
            media_type: None,
            byte_length: 1,
        };
        let mut valid = file.clone();
        valid.instance_id = instance.id.clone();
        valid.request_id = None;
        valid.ordinal = 1;
        let error = store
            .create_with_attachments(&instance, &[valid, file])
            .await
            .unwrap_err();
        assert!(error.contains("attachment scope"), "{error}");
        assert!(store.get("new").await.unwrap().is_none());
        assert!(store.attachments("other", None).await.unwrap().is_empty());
        assert!(store.attachments("new", None).await.unwrap().is_empty());
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_start_metadata_failure_rolls_back_instance_and_first_file() {
    let path =
        std::env::temp_dir().join(format!("blkit-start-rollback-{}.db", uuid::Uuid::new_v4()));
    let store = LocalStore::open(&path).await.unwrap();
    let instance = Instance::new("rollback", "ns", "1", "p", json!({}));
    let file = Attachment {
        instance_id: instance.id.clone(),
        request_id: None,
        ordinal: 0,
        object_key: "objects/first".into(),
        filename: "first".into(),
        media_type: None,
        byte_length: 1,
    };
    let mut duplicate = file.clone();
    duplicate.object_key = "objects/second".into();
    assert!(
        store
            .create_with_attachments(&instance, &[file, duplicate])
            .await
            .is_err()
    );
    assert!(store.get(&instance.id).await.unwrap().is_none());
    assert!(
        store
            .attachments(&instance.id, None)
            .await
            .unwrap()
            .is_empty()
    );
    std::fs::remove_file(path).unwrap();
}
