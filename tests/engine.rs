use blkit::runtime::{Branch, Definition, Engine, Instance, Registry, Step, Store};
use serde_json::{Value, json};
use std::{sync::{Arc, Barrier, atomic::{AtomicBool, AtomicUsize, Ordering}}, time::Duration};

#[tokio::test]
async fn failed_branch_signals_inflight_sibling_and_skips_successor() {
    let path = std::env::temp_dir().join(format!("blkit-engine-fail-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(AtomicBool::new(false));
    let signalled = Arc::new(AtomicUsize::new(0));
    let successor = Arc::new(AtomicBool::new(false));
    let bad = Step::Run { name: "bad", call: Arc::new({ let barrier = entered.clone(); move |_, _| { barrier.wait(); Err("boom".into()) } }), cancel: Arc::new(|| {}) };
    let slow = Step::Run { name: "slow", call: Arc::new({ let barrier = entered.clone(); let released = released.clone(); move |_, _| {
        barrier.wait();
        for _ in 0..100 { if released.load(Ordering::SeqCst) { break; } std::thread::sleep(Duration::from_millis(10)); }
        Ok(json!(1))
    }}), cancel: Arc::new({ let released = released.clone(); let signalled = signalled.clone(); move || {
        signalled.fetch_add(1, Ordering::SeqCst);
        released.store(true, Ordering::SeqCst);
    }}) };
    let definition = Definition { namespace: "test", version: "1", name: "fail",
        steps: vec![Step::Gateway { kind: "and", branches: vec![
            Branch { label: Some("bad"), condition: None, steps: vec![bad] },
            Branch { label: Some("slow"), condition: None, steps: vec![slow] },
        ], join: "both" }, Step::Run { name: "after", call: Arc::new({ let successor = successor.clone(); move |_, _| {
            successor.store(true, Ordering::SeqCst); Ok(json!(0))
        }}), cancel: Arc::new(|| {}) }, Step::Return(Arc::new(|_, values| Ok(values["after"].clone())))],
        decode_input: Box::new(Ok::<Value, String>) };
    let registry = Registry::new(vec![definition]).unwrap();
    let engine = Engine::new(registry, store.clone(), 2).unwrap();
    let id = engine.start("test", "1", "fail", json!(null)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item: Instance = engine.status(&id).await.unwrap().unwrap();
            if item.status == "failed" { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(signalled.load(Ordering::SeqCst), 1);
    assert!(!successor.load(Ordering::SeqCst));
    assert!(engine.status(&id).await.unwrap().unwrap().error.unwrap().contains("boom"));
    assert!(engine.cancel(&id).await.unwrap_err().contains("terminal"));
    assert_eq!(engine.status(&id).await.unwrap().unwrap().status, "failed");
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn queued_instance_cancels_before_its_first_task_is_dispatched() {
    let path = std::env::temp_dir().join(format!("blkit-engine-pending-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let occupied = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let other_started = Arc::new(AtomicBool::new(false));
    let definition = Definition { namespace: "test", version: "1", name: "queue",
        steps: vec![Step::Run { name: "work", call: Arc::new({
            let occupied = occupied.clone(); let release = release.clone(); let other_started = other_started.clone();
            move |source, _| {
                if *source == json!(0) {
                    occupied.store(true, Ordering::SeqCst);
                    for _ in 0..200 { if release.load(Ordering::SeqCst) { break; } std::thread::sleep(Duration::from_millis(5)); }
                } else { other_started.store(true, Ordering::SeqCst); }
                Ok(source.clone())
            }
        }), cancel: Arc::new(|| {}) }, Step::Return(Arc::new(|_, values| Ok(values["work"].clone())))],
        decode_input: Box::new(Ok::<Value, String>) };
    let engine = Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 1).unwrap();
    let first = engine.start("test", "1", "queue", json!(0)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !occupied.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
    let second = engine.start("test", "1", "queue", json!(1)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(engine.status(&second).await.unwrap().unwrap().status, "pending");
    engine.cancel(&second).await.unwrap();
    release.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!other_started.load(Ordering::SeqCst));
    assert_eq!(engine.status(&second).await.unwrap().unwrap().status, "cancelled");
    assert_eq!(engine.status(&first).await.unwrap().unwrap().status, "completed");
    assert!(engine.cancel(&first).await.unwrap_err().contains("terminal"));
    assert_eq!(engine.status(&first).await.unwrap().unwrap().status, "completed");
    drop(engine); drop(store); std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn cancellation_signals_all_running_tasks_and_ignores_late_results() {
    let path = std::env::temp_dir().join(format!("blkit-engine-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let started = Arc::new(AtomicUsize::new(0));
    let signalled = Arc::new(AtomicUsize::new(0));
    let released = Arc::new(AtomicBool::new(false));
    let successor = Arc::new(AtomicBool::new(false));
    let branches = (0..2).map(|index| {
        let started = started.clone();
        let released = released.clone();
        let signalled = signalled.clone();
        let task = Step::Run { name: "work", call: Arc::new(move |_, _| {
            started.fetch_add(1, Ordering::SeqCst);
            for _ in 0..150 { if released.load(Ordering::SeqCst) { break; } std::thread::sleep(Duration::from_millis(10)); }
            Ok(json!(index))
        }), cancel: Arc::new(move || { signalled.fetch_add(1, Ordering::SeqCst); }) };
        Branch { label: Some(if index == 0 { "left" } else { "right" }), condition: None, steps: vec![task] }
    }).collect();
    let definition = Definition { namespace: "test", version: "1", name: "cancel",
        steps: vec![Step::Gateway { kind: "and", branches, join: "both" }, Step::Run {
            name: "after", call: Arc::new({ let successor = successor.clone(); move |_, _| {
                successor.store(true, Ordering::SeqCst); Ok(json!(3))
            }}), cancel: Arc::new(|| {}) }, Step::Return(Arc::new(|_, values| Ok(values["after"].clone())))],
        decode_input: Box::new(Ok::<Value, String>) };
    let engine = Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 2).unwrap();
    let id = engine.start("test", "1", "cancel", json!(null)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while started.load(Ordering::SeqCst) != 2 { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
    let (first, repeated) = tokio::join!(engine.cancel(&id), engine.cancel(&id));
    first.unwrap();
    repeated.unwrap();
    engine.cancel(&id).await.unwrap();
    assert_eq!(signalled.load(Ordering::SeqCst), 2);
    assert_eq!(engine.status(&id).await.unwrap().unwrap().status, "cancelled");
    released.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!successor.load(Ordering::SeqCst));
    assert_eq!(engine.status(&id).await.unwrap().unwrap().status, "cancelled");
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
