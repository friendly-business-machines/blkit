use blkit::{
    named_runtime::GraphCheckpoint,
    runtime::{Instance, Store},
};
use serde_json::json;

#[tokio::test]
async fn local_cancel_timeout_race_has_one_terminal_winner() {
    let path = std::env::temp_dir().join(format!("blkit-timeout-race-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for (id, deadline) in [("expired", now - 100), ("cancelled", now + 500)] {
        let mut item = Instance::new(id, "test", "1", "process", json!(null));
        item.deadline_at_ms = Some(deadline);
        store.create(&item).await.unwrap();
    }
    let (timeout, cancel) = tokio::join!(
        store.expire_due(now),
        store.finish("expired", "cancelled", None, None)
    );
    assert_eq!(timeout.unwrap(), vec!["expired"]);
    assert!(cancel.is_err());
    assert_eq!(
        store
            .get("expired")
            .await
            .unwrap()
            .unwrap()
            .terminal_name
            .as_deref(),
        Some("timeout")
    );
    store
        .finish("cancelled", "cancelled", None, None)
        .await
        .unwrap();
    assert!(store.expire_due(i64::MAX).await.unwrap().is_empty());
    assert_eq!(
        store.get("cancelled").await.unwrap().unwrap().status,
        "cancelled"
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn expired_deadline_wins_over_late_completion_and_cancellation() {
    let path = std::env::temp_dir().join(format!("blkit-timeout-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    for (id, status) in [
        ("queued", "pending"),
        ("waiting", "waiting"),
        ("running", "running"),
        ("retry", "retry-waiting"),
    ] {
        let mut item = Instance::new(id, "test", "1", "process", json!(null));
        item.status = status.into();
        item.deadline_origin = Some("queued".into());
        item.deadline_duration_ms = Some(5);
        item.deadline_at_ms = Some(100);
        store.create(&item).await.unwrap();
    }
    assert!(store.expire_due(99).await.unwrap().is_empty());
    assert_eq!(store.expire_due(100).await.unwrap().len(), 4);
    assert!(store.expire_due(101).await.unwrap().is_empty());
    for id in ["queued", "waiting", "running", "retry"] {
        assert_eq!(
            store.get(id).await.unwrap().unwrap().status,
            "business-error"
        );
        assert_eq!(
            store
                .get(id)
                .await
                .unwrap()
                .unwrap()
                .terminal_name
                .as_deref(),
            Some("timeout")
        );
        assert!(
            store
                .finish(id, "completed", Some(json!(1)), None)
                .await
                .is_err()
        );
        assert!(store.finish(id, "cancelled", None, None).await.is_err());
        assert_eq!(
            store.get(id).await.unwrap().unwrap().status,
            "business-error"
        );
    }
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn wake_and_deadline_metadata_survive_reopen_and_due_selection() {
    let path = std::env::temp_dir().join(format!("blkit-wake-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let mut item = Instance::new("due", "orders", "1", "route", json!(1));
    item.status = "waiting".into();
    item.wake_at_ms = Some(100);
    item.first_claim_at_ms = Some(25);
    item.deadline_origin = Some("queued".into());
    item.deadline_duration_ms = Some(300_000);
    item.deadline_at_ms = Some(item.queued_at_ms + 300_000);
    store.create(&item).await.unwrap();
    assert!(store.due_waits(99).await.unwrap().is_empty());
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let loaded = store.get("due").await.unwrap().unwrap();
    assert_eq!(loaded.wake_at_ms, Some(100));
    assert_eq!(loaded.first_claim_at_ms, Some(25));
    assert_eq!(loaded.deadline_origin.as_deref(), Some("queued"));
    assert_eq!(loaded.deadline_at_ms, item.deadline_at_ms);
    assert_eq!(store.due_waits(100).await.unwrap()[0].id, "due");
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_first_claim_deadline_is_not_reset_by_retry() {
    let path = std::env::temp_dir().join(format!("blkit-first-claim-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let mut item = Instance::new("job", "orders", "1", "route", json!(1));
    item.deadline_origin = Some("first_claimed".into());
    item.deadline_duration_ms = Some(300_000);
    store.create(&item).await.unwrap();
    store.begin_attempt("job").await.unwrap();
    let first = store.get("job").await.unwrap().unwrap();
    let start = first.first_claim_at_ms.unwrap();
    assert_eq!(first.deadline_at_ms, Some(start + 300_000));
    store
        .record_retry("job", 1, 100, Some(1), "retry")
        .await
        .unwrap();
    store.begin_attempt("job").await.unwrap();
    let second = store.get("job").await.unwrap().unwrap();
    assert_eq!(second.first_claim_at_ms, Some(start));
    assert_eq!(second.deadline_at_ms, Some(start + 300_000));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn legacy_checkpoint_bytes_survive_store_upgrade_without_losing_committed_results() {
    let path =
        std::env::temp_dir().join(format!("blkit-legacy-checkpoint-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let mut item = Instance::new("legacy", "test", "1", "work", json!(null));
    item.checkpoint = Some(
        serde_json::from_value(json!({
            "ready": [{"node":"remaining", "path":[]}], "completed":{"finished":4},
            "selected":{}, "progress":{}, "outcome":null, "terminal":null
        }))
        .unwrap(),
    );
    store.create(&item).await.unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let saved = store.get("legacy").await.unwrap().unwrap();
    assert_eq!(saved.checkpoint.unwrap().completed["finished"], json!(4));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn checkpoint_retry_metadata_and_named_terminal_survive_reopen() {
    let path = std::env::temp_dir().join(format!("blkit-checkpoint-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let mut checkpoint = GraphCheckpoint::default();
    checkpoint.completed.insert("b".into(), json!(12));
    checkpoint.selected.insert("fork".into(), vec![1, 2]);
    let mut instance = Instance::new("checkpoint", "orders", "1.0", "decide", json!(7));
    instance.checkpoint = Some(checkpoint.clone());
    store.create(&instance).await.unwrap();
    checkpoint.completed.insert("c".into(), json!(15));
    store
        .commit_checkpoint("checkpoint", &checkpoint)
        .await
        .unwrap();
    store
        .record_retry("checkpoint", 1, 1_000, Some(1_010), "task failed")
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let retry = store.get("checkpoint").await.unwrap().unwrap();
    assert_eq!(retry.status, "retry-waiting");
    assert_eq!(retry.attempt, 1);
    assert_eq!(retry.first_failure_at, Some(1_000));
    assert_eq!(retry.next_eligible_at, Some(1_010));
    assert_eq!(retry.error.as_deref(), Some("task failed"));
    assert_eq!(retry.checkpoint.as_ref().unwrap().completed["b"], json!(12));
    assert_eq!(retry.checkpoint.as_ref().unwrap().completed["c"], json!(15));
    assert_eq!(
        retry.checkpoint.as_ref().unwrap().selected["fork"],
        vec![1, 2]
    );
    store
        .finish_named("checkpoint", "business-error", "rejected", None, None)
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let terminal = store.get("checkpoint").await.unwrap().unwrap();
    assert_eq!(terminal.terminal_name.as_deref(), Some("rejected"));
    assert_eq!(terminal.status, "business-error");
    assert_eq!(terminal.attempt, 1);
    assert_eq!(terminal.checkpoint.unwrap().completed["b"], json!(12));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn upgrading_an_old_store_keeps_terminal_records_and_adds_checkpoint_columns() {
    let path = std::env::temp_dir().join(format!("blkit-migration-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE instances (id TEXT PRIMARY KEY, namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL, input TEXT NOT NULL, status TEXT NOT NULL, result TEXT, error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)", ()).await.unwrap();
    conn.execute("INSERT INTO instances VALUES ('old', 'orders', '1.0', 'decide', '7', 'completed', '\"ok\"', NULL, 10, 11)", ()).await.unwrap();
    drop(conn);
    drop(db);
    let store = Store::open(&path).await.unwrap();
    let old = store.get("old").await.unwrap().unwrap();
    assert_eq!(old.status, "completed");
    assert_eq!(old.result, Some(json!("ok")));
    assert_eq!(old.attempt, 0);
    assert!(old.checkpoint.is_none());
    assert!(old.terminal_name.is_none());
    assert_eq!(old.queued_at_ms, 10_000);
    assert!(old.first_claim_at_ms.is_none());
    store
        .create(&Instance::new("new", "orders", "1.0", "decide", json!(8)))
        .await
        .unwrap();
    store
        .commit_checkpoint("new", &GraphCheckpoint::default())
        .await
        .unwrap();
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    assert!(
        reopened
            .get("new")
            .await
            .unwrap()
            .unwrap()
            .checkpoint
            .is_some()
    );
    assert_eq!(
        reopened.get("old").await.unwrap().unwrap().result,
        Some(json!("ok"))
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn persisted_instance_and_terminal_result_survive_reopen() {
    let path = std::env::temp_dir().join(format!("blkit-store-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    store
        .create(&Instance::new(
            "case-1",
            "orders",
            "1.0",
            "decide",
            json!({"total":"12.50"}),
        ))
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.get("case-1").await.unwrap().unwrap().status,
        "pending"
    );
    store
        .finish("case-1", "completed", Some(json!("approved")), None)
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let instance = store.get("case-1").await.unwrap().unwrap();
    assert_eq!(instance.namespace, "orders");
    assert_eq!(instance.version, "1.0");
    assert_eq!(instance.input, json!({"total":"12.50"}));
    assert_eq!(instance.status, "completed");
    assert_eq!(instance.result, Some(json!("approved")));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn startup_fails_incomplete_instances_without_replaying_terminal_records() {
    let path = std::env::temp_dir().join(format!("blkit-recovery-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    for (id, state) in [
        ("pending", "pending"),
        ("running", "running"),
        ("cancelling", "cancelling"),
        ("completed", "completed"),
    ] {
        store
            .create(&Instance::new(id, "orders", "1.0", "decide", json!(3)))
            .await
            .unwrap();
        if state != "pending" {
            store.finish(id, state, None, None).await.unwrap();
        }
    }
    drop(store);
    let store = Store::open(&path).await.unwrap();
    store.recover_interrupted().await.unwrap();
    for id in ["pending", "running", "cancelling"] {
        let item = store.get(id).await.unwrap().unwrap();
        assert_eq!(item.status, "failed", "{id}");
        assert!(item.error.unwrap().contains("interrupted"));
    }
    assert_eq!(
        store.get("completed").await.unwrap().unwrap().status,
        "completed"
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn acknowledged_write_survives_abrupt_process_exit() {
    const KEY: &str = "BLKIT_STORE_CRASH_CHILD";
    if let Ok(path) = std::env::var(KEY) {
        let store = Store::open(std::path::Path::new(&path)).await.unwrap();
        store
            .create(&Instance::new(
                "crash-case",
                "orders",
                "1.0",
                "decide",
                json!(7),
            ))
            .await
            .unwrap();
        store
            .finish("crash-case", "completed", Some(json!("ok")), None)
            .await
            .unwrap();
        std::process::exit(0);
    }
    let path = std::env::temp_dir().join(format!("blkit-crash-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "acknowledged_write_survives_abrupt_process_exit"])
        .env(KEY, &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.get("crash-case").await.unwrap().unwrap().result,
        Some(json!("ok"))
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn concurrent_acknowledged_transitions_survive_reopen() {
    let path = std::env::temp_dir().join(format!("blkit-concurrent-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let mut jobs = Vec::new();
    for index in 0..24 {
        let store = store.clone();
        jobs.push(tokio::spawn(async move {
            let id = format!("job-{index}");
            store
                .create(&Instance::new(&id, "orders", "1.0", "decide", json!(index)))
                .await
                .unwrap();
            store
                .finish(&id, "completed", Some(json!(index)), None)
                .await
                .unwrap();
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    for index in 0..24 {
        let item = reopened
            .get(&format!("job-{index}"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(item.status, "completed");
        assert_eq!(item.result, Some(json!(index)));
    }
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
