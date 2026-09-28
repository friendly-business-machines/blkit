use blkit::{
    named_runtime::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
    runtime::{Branch, Definition, Engine, Evaluate, Registry, Step, Store},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn parallel_multi_instances_share_capacity_and_cancel_inflight_work() {
    let path = std::env::temp_dir().join(format!("blkit-multi-capacity-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let task: Evaluate = {
        let active = active.clone();
        let peak = peak.clone();
        Arc::new(move |item, _| {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(item.clone())
        })
    };
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "batch",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "items",
                kind: GraphNodeKind::MultiInstance {
                    task,
                    items: Arc::new(|input, _| Ok(input.clone())),
                    parallel: true,
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "items",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "items",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["items"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let engine = Engine::new(Registry::new_named(vec![graph]).unwrap(), store.clone(), 2).unwrap();
    let first = engine
        .start("test", "1", "batch", json!([1, 2, 3]))
        .await
        .unwrap();
    let second = engine
        .start("test", "1", "batch", json!([4, 5, 6]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&first).await.unwrap().unwrap().status != "completed"
            || engine.status(&second).await.unwrap().unwrap().status != "completed"
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    assert_eq!(
        engine.status(&first).await.unwrap().unwrap().result,
        Some(json!([1, 2, 3]))
    );
    let cancelled = engine
        .start("test", "1", "batch", json!([7, 8, 9]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while active.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    engine.cancel(&cancelled).await.unwrap();
    tokio::time::sleep(Duration::from_millis(110)).await;
    let result = engine.status(&cancelled).await.unwrap().unwrap();
    assert_eq!(result.status, "cancelled");
    assert!(result.result.is_none());
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn independent_branches_and_instances_share_one_limit() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let branches: Vec<_> = (0..2)
        .map(|index| {
            let active = active.clone();
            let peak = peak.clone();
            Branch {
                label: Some(if index == 0 { "left" } else { "right" }),
                condition: None,
                steps: vec![Step::Run {
                    name: "work",
                    call: Arc::new(move |_, _| {
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(count, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(40));
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(json!(index))
                    }),
                    cancel: Arc::new(|| {}),
                }],
            }
        })
        .collect();
    let definition = Definition {
        namespace: "test",
        version: "1",
        name: "parallel",
        steps: vec![
            Step::Gateway {
                kind: "and",
                branches,
                join: "result",
            },
            Step::Return(Arc::new(|_, values| Ok(values["result"].clone()))),
        ],
        decode_input: Box::new(Ok::<Value, String>),
    };
    let permits = Arc::new(tokio::sync::Semaphore::new(2));
    let (a, b) = tokio::join!(
        definition.evaluate_limited(json!(null), permits.clone()),
        definition.evaluate_limited(json!(null), permits)
    );
    assert_eq!(a.unwrap(), json!({"left":0, "right":1}));
    assert_eq!(b.unwrap(), json!({"left":0, "right":1}));
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    peak.store(0, Ordering::SeqCst);
    let one = Arc::new(tokio::sync::Semaphore::new(1));
    let (a, b) = tokio::join!(
        definition.evaluate_limited(json!(null), one.clone()),
        definition.evaluate_limited(json!(null), one)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 1);
}
