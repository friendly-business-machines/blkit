use blkit::runtime::{Instance, Store};
use serde_json::json;

#[tokio::test]
async fn persisted_instance_and_terminal_result_survive_reopen() {
    let path = std::env::temp_dir().join(format!("blkit-store-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    store.create(&Instance::new("case-1", "orders", "1.0", "decide", json!({"total":"12.50"}))).await.unwrap();
    drop(store);
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.get("case-1").await.unwrap().unwrap().status, "pending");
    store.finish("case-1", "completed", Some(json!("approved")), None).await.unwrap();
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
    for (id, state) in [("pending", "pending"), ("running", "running"), ("cancelling", "cancelling"), ("completed", "completed")] {
        store.create(&Instance::new(id, "orders", "1.0", "decide", json!(3))).await.unwrap();
        if state != "pending" { store.finish(id, state, None, None).await.unwrap(); }
    }
    drop(store);
    let store = Store::open(&path).await.unwrap();
    store.recover_interrupted().await.unwrap();
    for id in ["pending", "running", "cancelling"] {
        let item = store.get(id).await.unwrap().unwrap();
        assert_eq!(item.status, "failed", "{id}");
        assert!(item.error.unwrap().contains("interrupted"));
    }
    assert_eq!(store.get("completed").await.unwrap().unwrap().status, "completed");
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn acknowledged_write_survives_abrupt_process_exit() {
    const KEY: &str = "BLKIT_STORE_CRASH_CHILD";
    if let Ok(path) = std::env::var(KEY) {
        let store = Store::open(std::path::Path::new(&path)).await.unwrap();
        store.create(&Instance::new("crash-case", "orders", "1.0", "decide", json!(7))).await.unwrap();
        store.finish("crash-case", "completed", Some(json!("ok")), None).await.unwrap();
        std::process::exit(0);
    }
    let path = std::env::temp_dir().join(format!("blkit-crash-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "acknowledged_write_survives_abrupt_process_exit"])
        .env(KEY, &path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.get("crash-case").await.unwrap().unwrap().result, Some(json!("ok")));
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
            store.create(&Instance::new(&id, "orders", "1.0", "decide", json!(index))).await.unwrap();
            store.finish(&id, "completed", Some(json!(index)), None).await.unwrap();
        }));
    }
    for job in jobs { job.await.unwrap(); }
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    for index in 0..24 {
        let item = reopened.get(&format!("job-{index}")).await.unwrap().unwrap();
        assert_eq!(item.status, "completed");
        assert_eq!(item.result, Some(json!(index)));
    }
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
