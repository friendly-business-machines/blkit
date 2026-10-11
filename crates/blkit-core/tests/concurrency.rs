use blkit::{
    compiled_graph::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
    runtime::{Engine, Evaluate, LocalStore, Registry},
};
use blkit_core as blkit;
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
    let store = LocalStore::open(&path).await.unwrap();
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
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
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
async fn async_io_and_synchronous_tasks_share_the_inflight_limit() {
    let path = std::env::temp_dir().join(format!("blkit-async-capacity-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let graphs = ["io", "sync"]
        .into_iter()
        .map(|name| {
            let (active, peak) = (active.clone(), peak.clone());
            let kind = if name == "io" {
                GraphNodeKind::AsyncTask(Arc::new(move |input, _| {
                    let (active, peak) = (active.clone(), peak.clone());
                    Box::pin(async move {
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(count, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(input)
                    })
                }))
            } else {
                GraphNodeKind::Task(Arc::new(move |input, _| {
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(input.clone())
                }))
            };
            GraphDefinition {
                namespace: "test",
                version: "1",
                name,
                retry: None,
                deadline: None,
                decode_input: Box::new(Ok),
                nodes: vec![
                    GraphNode {
                        name: "start",
                        kind: GraphNodeKind::Start,
                    },
                    GraphNode { name: "work", kind },
                    GraphNode {
                        name: "done",
                        kind: GraphNodeKind::End,
                    },
                ],
                links: vec![
                    GraphLink {
                        source: "start",
                        target: "work",
                        value: None,
                        condition: None,
                        fallback: false,
                        label: None,
                    },
                    GraphLink {
                        source: "work",
                        target: "done",
                        value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                        condition: None,
                        fallback: false,
                        label: None,
                    },
                ],
            }
        })
        .collect();
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(graphs).unwrap(), store.clone(), 2).unwrap();
    let a = engine.start("test", "1", "io", json!(1)).await.unwrap();
    let b = engine.start("test", "1", "io", json!(2)).await.unwrap();
    let c = engine.start("test", "1", "sync", json!(3)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (first, second, third) =
                tokio::join!(engine.status(&a), engine.status(&b), engine.status(&c));
            if [first, second, third]
                .into_iter()
                .all(|status| status.unwrap().unwrap().status == "completed")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    for (id, expected) in [(a, 1), (b, 2), (c, 3)] {
        assert_eq!(
            engine.status(&id).await.unwrap().unwrap().result,
            Some(json!(expected))
        );
    }
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn cancelling_async_task_drops_its_pending_future() {
    struct DropFlag(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    for deadline in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "blkit-async-cancel-{}-{deadline}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let graph = GraphDefinition {
            namespace: "test",
            version: "1",
            name: "cancel_async",
            retry: None,
            deadline: deadline.then_some(blkit::DeadlinePolicy {
                origin: "queued",
                duration: Duration::from_millis(60),
            }),
            decode_input: Box::new(Ok),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "work",
                    kind: GraphNodeKind::AsyncTask(Arc::new({
                        let (started, dropped) = (started.clone(), dropped.clone());
                        move |_, _| {
                            let (started, dropped) = (started.clone(), dropped.clone());
                            Box::pin(async move {
                                let _guard = DropFlag(dropped);
                                started.store(true, Ordering::SeqCst);
                                std::future::pending::<()>().await;
                                Ok(Value::Null)
                            })
                        }
                    })),
                },
                GraphNode {
                    name: "done",
                    kind: GraphNodeKind::End,
                },
            ],
            links: vec![
                GraphLink {
                    source: "start",
                    target: "work",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: None,
                },
                GraphLink {
                    source: "work",
                    target: "done",
                    value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                    condition: None,
                    fallback: false,
                    label: None,
                },
            ],
        };
        let store = LocalStore::open(&path).await.unwrap();
        let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
        let id = engine
            .start("test", "1", "cancel_async", Value::Null)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        if deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = engine.status(&id).await.unwrap();
        } else {
            engine.cancel(&id).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let status = engine.status(&id).await.unwrap().unwrap();
        assert_eq!(
            status.status,
            if deadline {
                "business-error"
            } else {
                "cancelled"
            }
        );
        assert!(status.result.is_none());
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn worker_restart_replays_uncommitted_async_effect() {
    use blkit::{RetryPolicy, runtime::Instance};
    let path = std::env::temp_dir().join(format!("blkit-async-replay-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let effects = Arc::new(AtomicUsize::new(0));
    let task = GraphNodeKind::AsyncTask(Arc::new({
        let effects = effects.clone();
        move |input, _| {
            let effects = effects.clone();
            Box::pin(async move {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            })
        }
    }));
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "replay",
        retry: Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(3),
            retry_delay: Duration::from_millis(10),
            backoff: "exponential",
        }),
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: task,
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "work",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let input = json!(7);
    let checkpoint = graph.checkpoint(&input).unwrap();
    let store = LocalStore::open(&path).await.unwrap();
    let mut item = Instance::new("lost", "test", "1", "replay", input.clone());
    item.checkpoint = Some(checkpoint);
    store.create(&item).await.unwrap();
    store.begin_attempt("lost").await.unwrap();
    // The old worker performs an external effect, then disappears before committing its result.
    let GraphNodeKind::AsyncTask(call) = &graph.nodes[1].kind else {
        unreachable!()
    };
    assert_eq!(call(input, Default::default()).await.unwrap(), json!(7));
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.get("lost").await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let result = store.get("lost").await.unwrap().unwrap();
    assert_eq!(result.result, Some(json!(7)));
    assert_eq!(result.attempt, 2);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
