use blkit::compiled_graph::{
    GraphCheckpoint, GraphDefinition, GraphLink, GraphNode, GraphNodeKind, GraphTerminal,
};
use blkit::{
    RetryPolicy,
    runtime::{Engine, Evaluate, Instance, LocalStore, Registry, next_retry_at},
};
use blkit_core as blkit;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn subprocess_child_terminals_are_caught_or_propagated_with_name() {
    for (outcome, expected) in [
        ("error", "business-error"),
        ("cancel", "cancelled"),
        ("terminate", "terminated"),
    ] {
        for handled in [false, true] {
            let link = |source, target, value, label| GraphLink {
                source,
                target,
                value,
                condition: None,
                fallback: false,
                label,
            };
            let child = GraphDefinition {
                namespace: "test",
                version: "1",
                name: "child",
                retry: None,
                deadline: None,
                decode_input: Box::new(Ok),
                nodes: vec![
                    GraphNode {
                        name: "start",
                        kind: GraphNodeKind::Start,
                    },
                    GraphNode {
                        name: "work",
                        kind: echo_task(),
                    },
                    GraphNode {
                        name: "failure",
                        kind: match outcome {
                            "error" => GraphNodeKind::Error,
                            "cancel" => GraphNodeKind::Cancel,
                            _ => GraphNodeKind::Terminate,
                        },
                    },
                ],
                links: vec![
                    link("start", "work", None, None),
                    link("work", "failure", None, None),
                ],
            };
            let mut links = vec![
                link("start", "called", None, None),
                link(
                    "called",
                    "done",
                    Some(Arc::new(|_, values| Ok(values["called"].clone()))),
                    None,
                ),
                link(
                    "recovered",
                    "done",
                    Some(Arc::new(|_, values| Ok(values["recovered"].clone()))),
                    None,
                ),
            ];
            if handled {
                links.push(link("called", "recovered", None, Some(outcome)));
            }
            let parent = GraphDefinition {
                namespace: "test",
                version: "1",
                name: "parent",
                retry: None,
                deadline: None,
                decode_input: Box::new(Ok),
                nodes: vec![
                    GraphNode {
                        name: "start",
                        kind: GraphNodeKind::Start,
                    },
                    GraphNode {
                        name: "called",
                        kind: GraphNodeKind::Subprocess {
                            process: "child",
                            input: Arc::new(|input, _| Ok(input.clone())),
                        },
                    },
                    GraphNode {
                        name: "recovered",
                        kind: echo_task(),
                    },
                    GraphNode {
                        name: "done",
                        kind: GraphNodeKind::End,
                    },
                ],
                links,
            };
            let path = std::env::temp_dir().join(format!(
                "blkit-subprocess-terminal-{}-{outcome}-{handled}.db",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let store = LocalStore::open(&path).await.unwrap();
            let engine = Engine::new(
                Registry::new(vec![parent, child]).unwrap(),
                store.clone(),
                1,
            )
            .unwrap();
            let id = engine.start("test", "1", "parent", json!(3)).await.unwrap();
            let status = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let status = engine.status(&id).await.unwrap().unwrap();
                    if matches!(
                        status.status.as_str(),
                        "completed" | "business-error" | "cancelled" | "terminated" | "failed"
                    ) {
                        break status;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                status.status,
                if handled { "completed" } else { expected },
                "{outcome}"
            );
            assert_eq!(
                status.terminal_name.as_deref(),
                if handled { None } else { Some("failure") }
            );
            if handled {
                assert_eq!(status.result, Some(json!(3)));
            }
            drop(engine);
            drop(store);
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[tokio::test]
async fn subprocess_parallel_and_repeated_activations_share_capacity_one() {
    let link = |source, target, value, condition, fallback, label| GraphLink {
        source,
        target,
        value,
        condition,
        fallback,
        label,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let child = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "child",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task({
                    let hits = hits.clone();
                    Arc::new(move |input, _| {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Ok(json!(input.as_i64().unwrap() + 1))
                    })
                }),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "work", None, None, false, None),
            link(
                "work",
                "done",
                Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                None,
                false,
                None,
            ),
        ],
    };
    let parent = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parallel",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "split",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "left",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "right",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "split",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "split", None, None, false, None),
            link("split", "left", None, None, false, Some("left")),
            link("split", "right", None, None, false, Some("right")),
            link(
                "left",
                "joined",
                Some(Arc::new(|_, values| Ok(values["left"].clone()))),
                None,
                false,
                None,
            ),
            link(
                "right",
                "joined",
                Some(Arc::new(|_, values| Ok(values["right"].clone()))),
                None,
                false,
                None,
            ),
            link(
                "joined",
                "done",
                Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
                None,
                false,
                None,
            ),
        ],
    };
    let repeated = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "repeated",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "called",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, values| {
                        Ok(values
                            .get("called")
                            .cloned()
                            .unwrap_or_else(|| input.clone()))
                    }),
                },
            },
            GraphNode {
                name: "gate",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "called", None, None, false, None),
            link("called", "gate", None, None, false, None),
            link(
                "gate",
                "called",
                None,
                Some(Arc::new(|_, values| {
                    Ok(json!(values["called"].as_i64().unwrap() < 3))
                })),
                false,
                None,
            ),
            link(
                "gate",
                "done",
                Some(Arc::new(|_, values| Ok(values["called"].clone()))),
                None,
                true,
                None,
            ),
        ],
    };
    let path = std::env::temp_dir().join(format!(
        "blkit-subprocess-parallel-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![child, parent, repeated]).unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    for (process, expected, calls) in [
        ("parallel", json!({"left":1,"right":1}), 2),
        ("repeated", json!(3), 5),
    ] {
        let id = engine.start("test", "1", process, json!(0)).await.unwrap();
        let status = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = engine.status(&id).await.unwrap().unwrap();
                if matches!(status.status.as_str(), "completed" | "failed") {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status.status, "completed", "{process}: {:?}", status.error);
        assert_eq!(status.result, Some(expected));
        assert_eq!(hits.load(Ordering::SeqCst), calls);
    }
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn handled_child_terminal_signals_its_inflight_siblings_only() {
    let signalled = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let link = |source, target, value, label| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label,
    };
    let child = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "child",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "split",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "slow",
                kind: GraphNodeKind::TaskWithCancel(
                    {
                        let entered = entered.clone();
                        let signalled = signalled.clone();
                        Arc::new(move |input, _| {
                            entered.store(true, Ordering::SeqCst);
                            for _ in 0..100 {
                                if signalled.load(Ordering::SeqCst) {
                                    return Ok(input.clone());
                                }
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            Err("child task was not cancelled".into())
                        })
                    },
                    {
                        let signalled = signalled.clone();
                        Arc::new(move || {
                            signalled.store(true, Ordering::SeqCst);
                        })
                    },
                ),
            },
            GraphNode {
                name: "fast",
                kind: GraphNodeKind::Task({
                    let entered = entered.clone();
                    Arc::new(move |input, _| {
                        while !entered.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Ok(input.clone())
                    })
                }),
            },
            GraphNode {
                name: "failure",
                kind: GraphNodeKind::Error,
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "split",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "split", None, None),
            link("split", "slow", None, Some("slow")),
            link("split", "fast", None, Some("fast")),
            link("fast", "failure", None, None),
            link(
                "slow",
                "joined",
                Some(Arc::new(|input, _| Ok(input.clone()))),
                None,
            ),
            link(
                "joined",
                "done",
                Some(Arc::new(|input, _| Ok(input.clone()))),
                None,
            ),
        ],
    };
    let parent = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "called",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "recovered",
                kind: echo_task(),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "called", None, None),
            link("called", "recovered", None, Some("error")),
            link(
                "called",
                "done",
                Some(Arc::new(|input, _| Ok(input.clone()))),
                None,
            ),
            link(
                "recovered",
                "done",
                Some(Arc::new(|input, _| Ok(input.clone()))),
                None,
            ),
        ],
    };
    let path = std::env::temp_dir().join(format!("blkit-child-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![parent, child]).unwrap(),
        store.clone(),
        2,
    )
    .unwrap();
    let id = engine.start("test", "1", "parent", json!(2)).await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if matches!(status.status.as_str(), "completed" | "failed") {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.status, "completed", "{:?}", status.error);
    assert!(
        signalled.load(Ordering::SeqCst),
        "child sibling was not signalled"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn subprocess_wait_checkpoints_child_progress_and_resumes_without_replay() {
    let hits = Arc::new(AtomicUsize::new(0));
    let link = |source, target, value| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label: None,
    };
    let child = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "child",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task({
                    let hits = hits.clone();
                    Arc::new(move |input, _| {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Ok(input.clone())
                    })
                }),
            },
            GraphNode {
                name: "wait",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(150)),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "work", None),
            link("work", "wait", None),
            link(
                "wait",
                "done",
                Some(Arc::new(|_, values| Ok(values["work"].clone()))),
            ),
        ],
    };
    let parent = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "called",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            link("start", "called", None),
            link(
                "called",
                "done",
                Some(Arc::new(|_, values| Ok(values["called"].clone()))),
            ),
        ],
    };
    let path = std::env::temp_dir().join(format!("blkit-child-wait-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![parent, child]).unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    let id = engine.start("test", "1", "parent", json!(7)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "waiting" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let checkpoint = store.get(&id).await.unwrap().unwrap().checkpoint.unwrap();
    assert!(
        serde_json::to_value(&checkpoint).unwrap()["children"]
            .as_object()
            .is_some_and(|entries| entries.len() == 1)
    );
    let restored: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(&checkpoint).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["children"],
        serde_json::to_value(&checkpoint).unwrap()["children"]
    );
    let wake = store.get(&id).await.unwrap().unwrap().wake_at_ms;
    drop(engine);
    let mut recovered = child_policy_graphs(
        GraphNodeKind::Task({
            let hits = hits.clone();
            Arc::new(move |input, _| {
                hits.fetch_add(1, Ordering::SeqCst);
                Ok(input.clone())
            })
        }),
        None,
        None,
        None,
        None,
    );
    recovered[0].nodes.push(GraphNode {
        name: "wait",
        kind: GraphNodeKind::PauseFor(Duration::from_millis(150)),
    });
    recovered[0].links[1].source = "wait";
    recovered[0].links.push(link("work", "wait", None));
    let engine = Engine::new(Registry::new(recovered).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    assert_eq!(engine.status(&id).await.unwrap().unwrap().wake_at_ms, wake);
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().result,
        Some(json!(7))
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn child_policy_graphs(
    work: GraphNodeKind,
    child_retry: Option<RetryPolicy>,
    child_deadline: Option<blkit::DeadlinePolicy>,
    parent_retry: Option<RetryPolicy>,
    parent_deadline: Option<blkit::DeadlinePolicy>,
) -> Vec<GraphDefinition> {
    let link = |source, target, value, label| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label,
    };
    vec![
        GraphDefinition {
            namespace: "test",
            version: "1",
            name: "child",
            retry: child_retry,
            deadline: child_deadline,
            decode_input: Box::new(Ok),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "work",
                    kind: work,
                },
                GraphNode {
                    name: "done",
                    kind: GraphNodeKind::End,
                },
            ],
            links: vec![
                link("start", "work", None, None),
                link(
                    "work",
                    "done",
                    Some(Arc::new(|input, _| Ok(input.clone()))),
                    None,
                ),
            ],
        },
        GraphDefinition {
            namespace: "test",
            version: "1",
            name: "parent",
            retry: parent_retry,
            deadline: parent_deadline,
            decode_input: Box::new(Ok),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "called",
                    kind: GraphNodeKind::Subprocess {
                        process: "child",
                        input: Arc::new(|input, _| Ok(input.clone())),
                    },
                },
                GraphNode {
                    name: "recovered",
                    kind: echo_task(),
                },
                GraphNode {
                    name: "done",
                    kind: GraphNodeKind::End,
                },
            ],
            links: vec![
                link("start", "called", None, None),
                link(
                    "called",
                    "done",
                    Some(Arc::new(|_, values| Ok(values["called"].clone()))),
                    None,
                ),
                link("called", "recovered", None, Some("error")),
                link(
                    "recovered",
                    "done",
                    Some(Arc::new(|input, _| Ok(input.clone()))),
                    None,
                ),
            ],
        },
    ]
}

#[test]
fn subprocess_registry_rejects_recursive_compiled_definitions() {
    let mut graphs = child_policy_graphs(echo_task(), None, None, None, None);
    graphs[0].nodes[1].kind = GraphNodeKind::Subprocess {
        process: "child",
        input: Arc::new(|input, _| Ok(input.clone())),
    };
    assert!(
        Registry::new(graphs)
            .err()
            .unwrap()
            .contains("recursive subprocess")
    );
}

#[tokio::test]
async fn child_deadline_interrupts_inflight_task_and_routes_timeout() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut graphs = child_policy_graphs(
        GraphNodeKind::TaskWithCancel(
            {
                let cancelled = cancelled.clone();
                Arc::new(move |input, _| {
                    for _ in 0..180 {
                        if cancelled.load(Ordering::SeqCst) {
                            return Ok(input.clone());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(input.clone())
                })
            },
            {
                let cancelled = cancelled.clone();
                Arc::new(move || cancelled.store(true, Ordering::SeqCst))
            },
        ),
        None,
        Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(100),
        }),
        None,
        None,
    );
    // Return a distinct value from the error handler so success cannot mask the timeout.
    graphs[1].links[3].value = Some(Arc::new(|_, _| Ok(json!("timeout-handled"))));
    let path = std::env::temp_dir().join(format!(
        "blkit-child-running-deadline-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(graphs).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let status = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if status.status == "completed" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("child deadline did not interrupt in-flight work");
    assert_eq!(status.result, Some(json!("timeout-handled")));
    assert!(cancelled.load(Ordering::SeqCst));
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn child_deadline_times_out_through_parent_handler_before_wait_finishes() {
    let definitions = child_policy_graphs(
        GraphNodeKind::PauseFor(Duration::from_secs(2)),
        None,
        Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(200),
        }),
        None,
        None,
    );
    let path = std::env::temp_dir().join(format!("blkit-child-deadline-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(definitions).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if matches!(
                status.status.as_str(),
                "completed" | "failed" | "business-error"
            ) {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result.status, "completed", "{:?}", result.error);
    assert_eq!(result.result, Some(json!(9)));
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn child_queued_deadline_starts_when_subprocess_node_is_entered() {
    for (origin, expected_hits) in [("queued", 0), ("first_claimed", 1)] {
        let hits = Arc::new(AtomicUsize::new(0));
        let definitions = child_policy_graphs(
            GraphNodeKind::Task({
                let hits = hits.clone();
                Arc::new(move |input, _| {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Ok(input.clone())
                })
            }),
            None,
            Some(blkit::DeadlinePolicy {
                origin,
                duration: Duration::from_millis(100),
            }),
            None,
            None,
        );
        let path = std::env::temp_dir().join(format!(
            "blkit-child-origin-{}-{origin}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).await.unwrap();
        let mut instance = Instance::new("job", "test", "1", "parent", json!(9));
        instance.checkpoint = Some(definitions[1].checkpoint(&instance.input).unwrap());
        store.create(&instance).await.unwrap();
        tokio::time::sleep(Duration::from_millis(180)).await;
        let engine = Engine::new(Registry::new(definitions).unwrap(), store.clone(), 1).unwrap();
        engine.recover().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = engine.status("job").await.unwrap().unwrap();
                if matches!(status.status.as_str(), "completed" | "failed") {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(result.status, "completed", "{origin}: {:?}", result.error);
        assert_eq!(hits.load(Ordering::SeqCst), expected_hits, "{origin}");
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn child_retry_resumes_without_using_parent_attempt() {
    let hits = Arc::new(AtomicUsize::new(0));
    let work = GraphNodeKind::Task({
        let hits = hits.clone();
        Arc::new(move |input, _| {
            if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("transient child failure".into())
            } else {
                Ok(input.clone())
            }
        })
    });
    let definitions = child_policy_graphs(
        work,
        Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(2),
            retry_delay: Duration::from_millis(50),
            backoff: "exponential",
        }),
        None,
        None,
        None,
    );
    let path = std::env::temp_dir().join(format!("blkit-child-retry-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(definitions).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if matches!(status.status.as_str(), "completed" | "failed") {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result.status, "completed", "{:?}", result.error);
    assert_eq!(result.attempt, 1);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn intermediate_subprocess_retries_leaf_failure_before_top_level_attempt() {
    let hits = Arc::new(AtomicUsize::new(0));
    let mut graphs = child_policy_graphs(
        GraphNodeKind::Task({
            let hits = hits.clone();
            Arc::new(move |input, _| {
                if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err("leaf failed".into())
                } else {
                    Ok(input.clone())
                }
            })
        }),
        None,
        None,
        None,
        None,
    );
    graphs[1].name = "middle";
    graphs[1].retry = Some(RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(20),
        backoff: "exponential",
    });
    let mut parent = child_policy_graphs(echo_task(), None, None, None, None)
        .pop()
        .unwrap();
    if let GraphNodeKind::Subprocess { process, .. } = &mut parent.nodes[1].kind {
        *process = "middle";
    }
    graphs.push(parent);
    let path = std::env::temp_dir().join(format!(
        "blkit-intermediate-retry-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(graphs).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if matches!(status.status.as_str(), "completed" | "failed") {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.status, "completed", "{:?}", status.error);
    assert_eq!(status.attempt, 1);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn exhausted_child_retries_fail_parent_attempt_without_replaying_committed_work() {
    let before = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(0));
    let work = GraphNodeKind::Task({
        let failures = failures.clone();
        Arc::new(move |_, _| {
            failures.fetch_add(1, Ordering::SeqCst);
            Err("child failed".into())
        })
    });
    let mut graphs = child_policy_graphs(
        work,
        Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(3),
            retry_delay: Duration::from_millis(20),
            backoff: "exponential",
        }),
        None,
        Some(RetryPolicy {
            max_retries: 1,
            retry_for: Duration::from_secs(3),
            retry_delay: Duration::from_millis(40),
            backoff: "exponential",
        }),
        None,
    );
    let child = &mut graphs[0];
    child.nodes.insert(
        1,
        GraphNode {
            name: "before",
            kind: GraphNodeKind::Task({
                let before = before.clone();
                Arc::new(move |input, _| {
                    before.fetch_add(1, Ordering::SeqCst);
                    Ok(input.clone())
                })
            }),
        },
    );
    child.links[0].target = "before";
    child.links.insert(
        1,
        GraphLink {
            source: "before",
            target: "work",
            value: None,
            condition: None,
            fallback: false,
            label: None,
        },
    );
    let path =
        std::env::temp_dir().join(format!("blkit-child-exhausted-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(graphs).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if status.status == "failed" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result.attempt, 2);
    assert_eq!(before.load(Ordering::SeqCst), 1);
    assert!(failures.load(Ordering::SeqCst) >= 3);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn parent_deadline_cannot_be_caught_by_child_timeout_handler() {
    let definitions = child_policy_graphs(
        GraphNodeKind::PauseFor(Duration::from_secs(2)),
        None,
        Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(400),
        }),
        None,
        Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_millis(100),
        }),
    );
    let path =
        std::env::temp_dir().join(format!("blkit-parent-deadline-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(definitions).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = engine.status(&id).await.unwrap().unwrap();
            if status.status == "business-error" || status.status == "completed" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result.status, "business-error");
    assert_eq!(result.terminal_name.as_deref(), Some("timeout"));
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn external_parent_cancellation_overrides_child_handler() {
    let entered = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let work = GraphNodeKind::TaskWithCancel(
        {
            let entered = entered.clone();
            let cancelled = cancelled.clone();
            Arc::new(move |input, _| {
                entered.store(true, Ordering::SeqCst);
                for _ in 0..100 {
                    if cancelled.load(Ordering::SeqCst) {
                        return Ok(input.clone());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err("child never cancelled".into())
            })
        },
        {
            let cancelled = cancelled.clone();
            Arc::new(move || {
                cancelled.store(true, Ordering::SeqCst);
            })
        },
    );
    let mut graphs = child_policy_graphs(work, None, None, None, None);
    graphs[1].links.push(GraphLink {
        source: "called",
        target: "recovered",
        value: None,
        condition: None,
        fallback: false,
        label: Some("cancel"),
    });
    let path = std::env::temp_dir().join(format!(
        "blkit-parent-cancel-child-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(graphs).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "parent", json!(9)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    engine.cancel(&id).await.unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().status,
        "cancelled"
    );
    assert!(cancelled.load(Ordering::SeqCst));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(engine.status(&id).await.unwrap().unwrap().result, None);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn echo_task() -> GraphNodeKind {
    GraphNodeKind::Task(Arc::new(|input, _| Ok(input.clone())))
}

fn long_split_graph(first: GraphNodeKind, second: Evaluate) -> GraphDefinition {
    let mut nodes = vec![
        GraphNode {
            name: "start",
            kind: GraphNodeKind::Start,
        },
        GraphNode {
            name: "fork",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "first",
            kind: first,
        },
        GraphNode {
            name: "second",
            kind: GraphNodeKind::Task(second),
        },
        GraphNode {
            name: "joined",
            kind: GraphNodeKind::Join {
                kind: "and",
                split: "fork",
            },
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ];
    let edge = |source, target, value, label| GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label,
    };
    let input_value: Evaluate = Arc::new(|input, _| Ok(input.clone()));
    let mut links = vec![
        edge("start", "fork", None, None),
        edge("fork", "first", None, Some("left")),
        edge("first", "joined", Some(input_value.clone()), None),
        edge("fork", "split_0", None, Some("right")),
        edge("second", "joined", Some(input_value.clone()), None),
        edge(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
            None,
        ),
    ];
    for i in 0..40 {
        let split: &'static str = Box::leak(format!("split_{i}").into_boxed_str());
        let join: &'static str = Box::leak(format!("join_{i}").into_boxed_str());
        let next: &'static str = if i == 39 {
            "second"
        } else {
            Box::leak(format!("split_{}", i + 1).into_boxed_str())
        };
        nodes.push(GraphNode {
            name: split,
            kind: GraphNodeKind::Split("and"),
        });
        nodes.push(GraphNode {
            name: join,
            kind: GraphNodeKind::Join { kind: "and", split },
        });
        links.push(edge(split, join, Some(input_value.clone()), Some("only")));
        links.push(edge(join, next, None, None));
    }
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "long_split",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes,
        links,
    }
}

#[tokio::test]
async fn pending_split_branch_advances_past_wait_and_inflight_task() {
    let path = std::env::temp_dir().join(format!("blkit-pending-split-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let second: Evaluate = {
        let hits = hits.clone();
        Arc::new(move |input, _| {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(input.clone())
        })
    };
    let graph = long_split_graph(GraphNodeKind::PauseFor(Duration::from_millis(500)), second);
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "long_split", json!(4))
        .await
        .unwrap();
    assert_eq!(engine.status(&id).await.unwrap().unwrap().status, "pending");
    tokio::time::timeout(Duration::from_millis(300), async {
        while hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    engine.cancel(&id).await.unwrap();
    drop(engine);
    drop(store);
    std::fs::remove_file(&path).unwrap();

    let store = LocalStore::open(&path).await.unwrap();
    let signal = Arc::new(AtomicBool::new(false));
    let first: Evaluate = {
        let signal = signal.clone();
        Arc::new(move |input, _| {
            for _ in 0..100 {
                if signal.load(Ordering::SeqCst) {
                    return Ok(input.clone());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("second branch never ran".into())
        })
    };
    let second: Evaluate = {
        let signal = signal.clone();
        Arc::new(move |input, _| {
            signal.store(true, Ordering::SeqCst);
            Ok(input.clone())
        })
    };
    let graph = long_split_graph(GraphNodeKind::Task(first), second);
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "long_split", json!(4))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(350), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().result,
        Some(json!({"left":4,"right":4}))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn immediate_pending_branches_are_advanced_fairly() {
    let mut graph = task_free_cycle_graph("fair", "queued");
    graph.nodes.push(GraphNode {
        name: "finite",
        kind: GraphNodeKind::Start,
    });
    graph.nodes.push(GraphNode {
        name: "work",
        kind: echo_task(),
    });
    graph.links.push(GraphLink {
        source: "finite",
        target: "work",
        value: None,
        condition: None,
        fallback: false,
        label: None,
    });
    let mut state: GraphCheckpoint = serde_json::from_value(json!({
        "version": 2, "next_activation_id": 10,
        "ready": [], "pending": [
            {"node":"finite","path":[],"id":1,"values":{},"generations":[]},
            {"node":"gate","path":[],"id":2,"values":{},"generations":[]}
        ], "waiting":[], "completed":{}, "selected":{}, "progress":{}, "outcome":null, "terminal":null
    })).unwrap();
    for _ in 0..3 {
        graph.resume_pending(&json!(1), &mut state).unwrap();
    }
    assert!(graph.ready(&state).contains(&"work"));
}

fn task_free_cycle_graph(name: &'static str, origin: &'static str) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name,
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin,
            duration: Duration::from_millis(60),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "gate",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "xor",
                    split: "gate",
                },
            },
            GraphNode {
                name: "failed",
                kind: GraphNodeKind::Error,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "joined",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
                condition: Some(Arc::new(|input, _| Ok(json!(input.as_i64().unwrap() > 0)))),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "failed",
                value: None,
                condition: None,
                fallback: true,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn task_free_cycle_yields_and_respects_both_deadline_origins() {
    let graph = task_free_cycle_graph("probe", "queued");
    let mut checkpoint = graph.checkpoint(&json!(1)).unwrap();
    assert!(graph.has_pending(&checkpoint));
    assert!(
        graph
            .run(&json!(1), &mut checkpoint)
            .unwrap_err()
            .contains("timeout")
    );
    let path = std::env::temp_dir().join(format!("blkit-task-free-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![
            task_free_cycle_graph("queued", "queued"),
            task_free_cycle_graph("claimed", "first_claimed"),
        ])
        .unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    for name in ["queued", "claimed"] {
        let id = tokio::time::timeout(
            Duration::from_secs(2),
            engine.start("test", "1", name, json!(1)),
        )
        .await
        .unwrap()
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while engine.status(&id).await.unwrap().unwrap().status != "business-error" {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let result = store.get(&id).await.unwrap().unwrap();
        assert_eq!(result.terminal_name.as_deref(), Some("timeout"));
        if name == "claimed" {
            assert!(result.first_claim_at_ms.is_some());
        }
    }
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn wait_graph() -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "wait",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "pause",
                kind: GraphNodeKind::PauseFor(Duration::from_millis(200)),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "pause",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "pause",
                target: "done",
                value: Some(Arc::new(|input, _| Ok(input.clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn local_worker_claims_durable_work_not_started_by_an_http_request() {
    let path = std::env::temp_dir().join(format!("blkit-queued-pending-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = wait_graph();
    let mut instance = Instance::new("queued", "test", "1", "wait", json!(17));
    instance.checkpoint = Some(graph.checkpoint(&instance.input).unwrap());
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    store.create(&instance).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status("queued").await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status("queued").await.unwrap().unwrap().result,
        Some(json!(17))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_queued_work_can_be_cancelled_before_a_worker_claims_it() {
    let path = std::env::temp_dir().join(format!("blkit-queued-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = wait_graph();
    let mut instance = Instance::new("queued-cancel", "test", "1", "wait", json!(18));
    instance.checkpoint = Some(graph.checkpoint(&instance.input).unwrap());
    store.create(&instance).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.cancel(&instance.id).await.unwrap();
    engine.recover().await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        engine.status(&instance.id).await.unwrap().unwrap().status,
        "cancelled"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_wait_keeps_original_wake_on_restart_and_accepts_cancellation() {
    let path = std::env::temp_dir().join(format!("blkit-local-wait-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(Registry::new(vec![wait_graph()]).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "wait", json!(17)).await.unwrap();
    let before = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(before.status, "waiting");
    let wake = before.wake_at_ms.unwrap();
    drop(engine);
    let engine = Engine::new(Registry::new(vec![wait_graph()]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().wake_at_ms,
        Some(wake)
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().result,
        Some(json!(17))
    );
    let cancelled = engine.start("test", "1", "wait", json!(18)).await.unwrap();
    engine.cancel(&cancelled).await.unwrap();
    tokio::time::sleep(Duration::from_millis(220)).await;
    assert_eq!(
        engine.status(&cancelled).await.unwrap().unwrap().status,
        "cancelled"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_queue_deadline_expires_while_waiting_without_reaching_wake() {
    let path = std::env::temp_dir().join(format!("blkit-queue-deadline-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = wait_graph();
    graph.deadline = Some(blkit::DeadlinePolicy {
        origin: "queued",
        duration: Duration::from_millis(50),
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "wait", json!(2)).await.unwrap();
    let wake = store.get(&id).await.unwrap().unwrap().wake_at_ms.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let item = store.get(&id).await.unwrap().unwrap();
    assert_eq!(item.status, "business-error");
    assert_eq!(item.terminal_name.as_deref(), Some("timeout"));
    assert!(item.deadline_at_ms.unwrap() < wake);
    tokio::time::sleep(Duration::from_millis(130)).await;
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().status,
        "business-error"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn first_claim_deadline_begins_after_wait_and_rejects_late_task_result() {
    let path = std::env::temp_dir().join(format!(
        "blkit-first-claim-timeout-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = wait_graph();
    graph.deadline = Some(blkit::DeadlinePolicy {
        origin: "first_claimed",
        duration: Duration::from_millis(50),
    });
    graph.nodes[1].kind = GraphNodeKind::PauseFor(Duration::from_millis(80));
    graph.nodes.push(GraphNode {
        name: "slow",
        kind: GraphNodeKind::Task(Arc::new(|input, _| {
            std::thread::sleep(Duration::from_millis(160));
            Ok(input.clone())
        })),
    });
    graph.links[1].target = "slow";
    graph.links.push(GraphLink {
        source: "slow",
        target: "done",
        value: Some(Arc::new(|input, _| Ok(input.clone()))),
        condition: None,
        fallback: false,
        label: None,
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "wait", json!(2)).await.unwrap();
    assert!(
        store
            .get(&id)
            .await
            .unwrap()
            .unwrap()
            .deadline_at_ms
            .is_none()
    );
    tokio::time::sleep(Duration::from_millis(155)).await;
    let timed_out = store.get(&id).await.unwrap().unwrap();
    assert!(timed_out.first_claim_at_ms.is_some());
    assert_eq!(timed_out.status, "business-error");
    assert_eq!(timed_out.terminal_name.as_deref(), Some("timeout"));
    tokio::time::sleep(Duration::from_millis(170)).await;
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().status,
        "business-error"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn recovered_wait_without_original_runner_resumes() {
    let path = std::env::temp_dir().join(format!("blkit-wait-recover-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = wait_graph();
    let checkpoint = graph.checkpoint(&json!(13)).unwrap();
    let mut instance = Instance::new("wait-recover", "test", "1", "wait", json!(13));
    instance.status = "waiting".into();
    instance.wake_at_ms = graph.waiting_until(&checkpoint);
    instance.checkpoint = Some(checkpoint);
    store.create(&instance).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status("wait-recover").await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status("wait-recover").await.unwrap().unwrap().result,
        Some(json!(13))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn local_wait_after_task_does_not_replay_committed_task() {
    let path =
        std::env::temp_dir().join(format!("blkit-after-task-wait-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let make_graph = || {
        let mut graph = wait_graph();
        let hits = calls.clone();
        graph.nodes.push(GraphNode {
            name: "step",
            kind: GraphNodeKind::Task(Arc::new(move |input, _| {
                hits.fetch_add(1, Ordering::SeqCst);
                Ok(input.clone())
            })),
        });
        graph.links[0].target = "step";
        graph.links.push(GraphLink {
            source: "step",
            target: "pause",
            value: None,
            condition: None,
            fallback: false,
            label: None,
        });
        graph
    };
    let engine = Engine::new(Registry::new(vec![make_graph()]).unwrap(), store.clone(), 1).unwrap();
    let id = engine.start("test", "1", "wait", json!(7)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.status(&id).await.unwrap().unwrap().status != "waiting" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let at = engine
        .status(&id)
        .await
        .unwrap()
        .unwrap()
        .wake_at_ms
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(engine);
    let engine = Engine::new(Registry::new(vec![make_graph()]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().wake_at_ms,
        Some(at)
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn multi_graph(parallel: bool, task: Evaluate) -> GraphDefinition {
    GraphDefinition {
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
                name: "batch",
                kind: GraphNodeKind::MultiInstance {
                    task,
                    items: Arc::new(|input, _| Ok(input.clone())),
                    parallel,
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
                target: "batch",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "batch",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["batch"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[test]
fn multi_instance_empty_sequential_resume_and_parallel_order() {
    let graph = multi_graph(false, Arc::new(|item, _| Ok(item.clone())));
    let empty = graph.checkpoint(&json!([])).unwrap();
    assert!(graph.ready(&empty).is_empty());
    assert_eq!(empty.outcome, Some(json!([])));
    let input = json!([3, 1, 2]);
    let mut state = graph.checkpoint(&input).unwrap();
    let first = graph.ready_activations(&state)[0].0;
    assert_eq!(
        graph.activation_input(&state, first, &input).unwrap(),
        json!(3)
    );
    graph
        .complete_activation(&input, &mut state, first, json!(3))
        .unwrap();
    assert!(!state.completed.contains_key("batch"));
    let mut restored: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(graph.run(&input, &mut restored).unwrap(), input);
    let graph = multi_graph(true, Arc::new(|item, _| Ok(item.clone())));
    let mut state = graph.checkpoint(&input).unwrap();
    let ids: Vec<_> = graph
        .ready_activations(&state)
        .iter()
        .map(|(id, _)| *id)
        .collect();
    graph
        .complete_activation(&input, &mut state, ids[2], json!(2))
        .unwrap();
    graph
        .complete_activation(&input, &mut state, ids[0], json!(3))
        .unwrap();
    graph
        .complete_activation(&input, &mut state, ids[1], json!(1))
        .unwrap();
    assert_eq!(state.outcome, Some(input));
}

#[tokio::test]
async fn multi_instance_retry_skips_committed_items() {
    let path = std::env::temp_dir().join(format!("blkit-batch-retry-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let visited = Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls = visited.clone();
    let task: Evaluate = Arc::new(move |item, _| {
        let number = item.as_i64().unwrap();
        let mut calls = calls.lock().unwrap();
        calls.push(number);
        if number == 2 && calls.iter().filter(|&&n| n == 2).count() == 1 {
            return Err("retry item".into());
        }
        Ok(item.clone())
    });
    let mut graph = multi_graph(false, task);
    graph.retry = Some(RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(10),
        backoff: "exponential",
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "batch", json!([1, 2, 3]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.get(&id).await.unwrap().unwrap().result,
        Some(json!([1, 2, 3]))
    );
    assert_eq!(*visited.lock().unwrap(), vec![1, 2, 2, 3]);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn bounded_loop_graph(before: bool, limit: u32, condition: bool) -> GraphDefinition {
    use blkit::compiled_graph::LoopPolicy;
    let call: Evaluate = Arc::new(|_, values| {
        Ok(json!(
            values.get("repeat").and_then(Value::as_i64).unwrap_or(0) + 1
        ))
    });
    let check: Evaluate =
        Arc::new(move |_, values| Ok(json!(condition && values["repeat"].as_i64().unwrap() < 2)));
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "bounded",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "repeat",
                kind: GraphNodeKind::TaskLoop(
                    call,
                    LoopPolicy {
                        condition: check,
                        initial: before.then(|| {
                            Arc::new(|_: &Value, _: &blkit::runtime::Values| Ok(json!(0)))
                                as Evaluate
                        }),
                        before,
                        max_iterations: Some(limit),
                        max_duration: None,
                    },
                ),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "repeat",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "repeat",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["repeat"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[test]
fn task_loop_zero_iterations_post_check_and_bound_are_checkpointed() {
    let input = json!(null);
    let graph = bounded_loop_graph(true, 3, false);
    let state = graph.checkpoint(&input).unwrap();
    assert!(graph.ready(&state).is_empty());
    assert_eq!(state.outcome, Some(json!(0)));
    let graph = bounded_loop_graph(false, 3, true);
    let mut state = graph.checkpoint(&input).unwrap();
    let first = graph.ready_activations(&state)[0].0;
    graph
        .complete_activation(&input, &mut state, first, json!(1))
        .unwrap();
    let mut state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_ne!(first, graph.ready_activations(&state)[0].0);
    assert_eq!(graph.run(&input, &mut state).unwrap(), json!(2));
    assert_eq!(state.completed["repeat"], json!(2));
    let graph = bounded_loop_graph(false, 1, true);
    let mut state = graph.checkpoint(&input).unwrap();
    let id = graph.ready_activations(&state)[0].0;
    graph
        .complete_activation(&input, &mut state, id, json!(1))
        .unwrap();
    assert_eq!(
        state.terminal,
        Some(GraphTerminal::Error("task-iteration-limit".into()))
    );
    assert!(graph.ready(&state).is_empty());
}

#[tokio::test]
async fn loop_limit_is_business_error_and_not_retried() {
    let path = std::env::temp_dir().join(format!("blkit-loop-limit-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = bounded_loop_graph(false, 1, true);
    graph.retry = Some(RetryPolicy {
        max_retries: 2,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(10),
        backoff: "exponential",
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "bounded", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "business-error" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let item = store.get(&id).await.unwrap().unwrap();
    assert_eq!(item.terminal_name.as_deref(), Some("task-iteration-limit"));
    assert_eq!(item.attempt, 1);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn restart_resumes_only_uncommitted_task_loop_iteration() {
    let path =
        std::env::temp_dir().join(format!("blkit-task-loop-resume-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = bounded_loop_graph(false, 3, true);
    graph.retry = Some(RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(10),
        backoff: "exponential",
    });
    let calls = Arc::new(AtomicUsize::new(0));
    if let GraphNodeKind::TaskLoop(call, _) = &mut graph.nodes[1].kind {
        let hits = calls.clone();
        *call = Arc::new(move |_, values| {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(json!(values["repeat"].as_i64().unwrap() + 1))
        });
    }
    let input = json!(null);
    let mut checkpoint = graph.checkpoint(&input).unwrap();
    let first = graph.ready_activations(&checkpoint)[0].0;
    graph
        .complete_activation(&input, &mut checkpoint, first, json!(1))
        .unwrap();
    let mut item = Instance::new("loop-recovery", "test", "1", "bounded", input);
    item.status = "running".into();
    item.attempt = 1;
    item.checkpoint = Some(checkpoint);
    store.create(&item).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine
            .status("loop-recovery")
            .await
            .unwrap()
            .unwrap()
            .status
            != "completed"
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.get("loop-recovery").await.unwrap().unwrap().result,
        Some(json!(2))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn task_loop_elapsed_bound_prevents_next_invocation() {
    let mut graph = bounded_loop_graph(false, 10, true);
    if let GraphNodeKind::TaskLoop(_, policy) = &mut graph.nodes[1].kind {
        policy.max_duration = Some(Duration::from_millis(20));
    }
    let input = json!(null);
    let mut state = graph.checkpoint(&input).unwrap();
    let id = graph.ready_activations(&state)[0].0;
    graph
        .complete_activation(&input, &mut state, id, json!(1))
        .unwrap();
    std::thread::sleep(Duration::from_millis(25));
    graph.check_loop_bounds(&mut state, i64::MAX).unwrap();
    assert_eq!(
        state.terminal,
        Some(GraphTerminal::Error("task-iteration-limit".into()))
    );
}

#[test]
fn repeated_node_visits_have_distinct_committed_activations() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "repeat",
        retry: None,
        deadline: Some(blkit::DeadlinePolicy {
            origin: "queued",
            duration: Duration::from_secs(1),
        }),
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "gate",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(Arc::new(|_, values| {
                    Ok(json!(
                        values.get("joined").and_then(Value::as_i64).unwrap_or(0) + 1
                    ))
                })),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "xor",
                    split: "gate",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::Error,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "work",
                value: None,
                condition: Some(Arc::new(|_, values| {
                    Ok(json!(
                        values.get("joined").and_then(Value::as_i64).unwrap_or(0) < 2
                    ))
                })),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "gate",
                target: "done",
                value: None,
                condition: None,
                fallback: true,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "gate",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let input = json!(0);
    let mut state = graph.checkpoint(&input).unwrap();
    let first = graph.ready_activations(&state)[0].0;
    graph
        .complete_activation(&input, &mut state, first, json!(1))
        .unwrap();
    let mut restored: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    let second = graph.ready_activations(&restored)[0].0;
    assert_ne!(first, second);
    graph
        .complete_activation(&input, &mut restored, second, json!(2))
        .unwrap();
    assert_eq!(restored.terminal, Some(GraphTerminal::Error("done".into())));
    assert!(
        graph
            .complete_activation(&input, &mut restored, first, json!(3))
            .is_err()
    );
    assert_eq!(restored.completed["work"], json!(2));
}

#[test]
fn parallel_branches_evaluate_only_their_own_route_values() {
    let b: Evaluate = Arc::new(|_, values| {
        assert!(!values.contains_key("c"));
        Ok(json!(3))
    });
    let c: Evaluate = Arc::new(|_, values| {
        assert!(!values.contains_key("b"));
        Ok(json!(5))
    });
    let graph = parallel_named_graph(b, c);
    let input = json!(null);
    let mut state = graph.checkpoint(&input).unwrap();
    assert_eq!(
        graph.run(&input, &mut state).unwrap(),
        json!({"left":3,"right":5})
    );
}

#[test]
fn split_generations_join_only_their_own_branches() {
    let mut graph = parallel_named_graph(echo_task_call(), echo_task_call());
    graph
        .nodes
        .iter_mut()
        .find(|node| node.name == "done")
        .unwrap()
        .kind = echo_task();
    let mut state: GraphCheckpoint = serde_json::from_value(json!({
        "version":2, "next_activation_id":100,
        "ready":[
            {"node":"b","id":11,"path":[["fork",1]],"generations":[1],"values":{}},
            {"node":"c","id":12,"path":[["fork",2]],"generations":[1],"values":{}},
            {"node":"b","id":21,"path":[["fork",1]],"generations":[2],"values":{}},
            {"node":"c","id":22,"path":[["fork",2]],"generations":[2],"values":{}}
        ],
        "completed":{}, "selected":{"fork#1":[1,2],"fork#2":[1,2]}, "progress":{},
        "outcome":null, "terminal":null
    }))
    .unwrap();
    let input = json!(null);
    graph
        .complete_activation(&input, &mut state, 11, json!(10))
        .unwrap();
    graph
        .complete_activation(&input, &mut state, 21, json!(20))
        .unwrap();
    graph
        .complete_activation(&input, &mut state, 22, json!(50))
        .unwrap();
    assert_eq!(graph.ready(&state), vec!["c", "done"]);
    let done = graph
        .ready_activations(&state)
        .into_iter()
        .find(|(_, name)| *name == "done")
        .unwrap()
        .0;
    assert_eq!(
        graph.activation_values(&state, done).unwrap()["both"],
        json!({"left":20,"right":50})
    );
    assert!(state.selected.contains_key("fork#1"));
    graph
        .complete_activation(&input, &mut state, 12, json!(5))
        .unwrap();
    let mut results: Vec<_> = graph
        .ready_activations(&state)
        .iter()
        .map(|(id, _)| graph.activation_values(&state, *id).unwrap()["both"].clone())
        .collect();
    results.sort_by_key(|value| value["left"].as_i64().unwrap());
    assert_eq!(
        results,
        vec![json!({"left":10,"right":5}), json!({"left":20,"right":50})]
    );
    assert!(state.selected.is_empty());
}

fn echo_task_call() -> Evaluate {
    Arc::new(|input, _| Ok(input.clone()))
}

#[test]
fn legacy_checkpoint_is_rejected_without_replaying_committed_work() {
    let b_calls = Arc::new(AtomicUsize::new(0));
    let b: Evaluate = {
        let calls = b_calls.clone();
        Arc::new(move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!(3))
        })
    };
    let c: Evaluate = Arc::new(|_, _| Ok(json!(5)));
    let graph = parallel_named_graph(b, c);
    let saved = json!({
        "ready": [{"node":"c", "path":[["fork",2]]}],
        "waiting": [], "completed": {"b":3},
        "selected": {"fork":[1,2]}, "progress": {"both":[[1,3]]},
        "outcome": null, "terminal": null
    });
    let mut restored: GraphCheckpoint = serde_json::from_value(saved).unwrap();
    let original = serde_json::to_value(&restored).unwrap();
    assert!(
        graph
            .run(&json!(null), &mut restored)
            .unwrap_err()
            .contains("unsupported checkpoint version")
    );
    assert_eq!(serde_json::to_value(&restored).unwrap(), original);
    assert!(
        graph
            .resume_pending(&json!(null), &mut restored)
            .unwrap_err()
            .contains("unsupported checkpoint version")
    );
    assert_eq!(serde_json::to_value(&restored).unwrap(), original);
    assert_eq!(b_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_parallel_tokens_do_not_share_uncommitted_route_outputs() {
    let path = std::env::temp_dir().join(format!("blkit-route-values-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let b: Evaluate = Arc::new(|_, values| {
        if values.contains_key("c") {
            return Err("sibling c leaked".into());
        }
        Ok(json!(3))
    });
    let c: Evaluate = Arc::new(|_, values| {
        if values.contains_key("b") {
            return Err("sibling b leaked".into());
        }
        Ok(json!(5))
    });
    let engine = Engine::new(
        Registry::new(vec![parallel_named_graph(b, c)]).unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    let id = engine
        .start("test", "1", "parallel", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !matches!(
            engine.status(&id).await.unwrap().unwrap().status.as_str(),
            "completed" | "failed"
        ) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let result = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(result.status, "completed", "{:?}", result.error);
    assert_eq!(result.result, Some(json!({"left":3,"right":5})));
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn named_graph_resumes_from_serialized_completed_output() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "simple",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "first",
                kind: echo_task(),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "first",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "first",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["first"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let input = json!(41);
    let mut state = graph.checkpoint(&input).unwrap();
    assert_eq!(graph.ready(&state), vec!["first"]);
    graph
        .complete(&input, &mut state, "first", json!(42))
        .unwrap();
    let state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert!(graph.ready(&state).is_empty());
    assert_eq!(state.completed["first"], json!(42));
    assert_eq!(state.outcome.unwrap(), json!(42));
}

#[test]
fn named_and_join_restores_partial_progress() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "and",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "b",
                kind: echo_task(),
            },
            GraphNode {
                name: "c",
                kind: echo_task(),
            },
            GraphNode {
                name: "both",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "fork",
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
                target: "fork",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "b",
                value: None,
                condition: None,
                fallback: false,
                label: Some("left"),
            },
            GraphLink {
                source: "fork",
                target: "c",
                value: None,
                condition: None,
                fallback: false,
                label: Some("right"),
            },
            GraphLink {
                source: "b",
                target: "both",
                value: Some(Arc::new(|_, values| Ok(values["b"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "c",
                target: "both",
                value: Some(Arc::new(|_, values| Ok(values["c"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "both",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["both"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let input = json!(0);
    let mut state = graph.checkpoint(&input).unwrap();
    assert_eq!(graph.ready(&state), vec!["b", "c"]);
    graph.complete(&input, &mut state, "b", json!(3)).unwrap();
    let mut restored: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(graph.ready(&restored), vec!["c"]);
    assert!(restored.outcome.is_none());
    graph
        .complete(&input, &mut restored, "c", json!(5))
        .unwrap();
    assert_eq!(restored.outcome, Some(json!({"left":3,"right":5})));
    assert_eq!(restored.completed["b"], json!(3));
}

fn conditional_graph(kind: &'static str) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "conditional",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split(kind),
            },
            GraphNode {
                name: "a",
                kind: echo_task(),
            },
            GraphNode {
                name: "b",
                kind: echo_task(),
            },
            GraphNode {
                name: "c",
                kind: echo_task(),
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind,
                    split: "fork",
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
                target: "fork",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "a",
                value: None,
                condition: Some(Arc::new(|input, _| Ok(json!(input.as_i64().unwrap() > 0)))),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "b",
                value: None,
                condition: Some(Arc::new(|input, _| Ok(json!(input.as_i64().unwrap() > 1)))),
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "c",
                value: None,
                condition: None,
                fallback: true,
                label: None,
            },
            GraphLink {
                source: "a",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["a"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "b",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["b"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "c",
                target: "joined",
                value: Some(Arc::new(|_, values| Ok(values["c"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "joined",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[test]
fn xor_first_match_and_fallback_survive_checkpoint() {
    let graph = conditional_graph("xor");
    let input = json!(2);
    let state = graph.checkpoint(&input).unwrap();
    assert_eq!(graph.ready(&state), vec!["a"]);
    let mut state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    graph.complete(&input, &mut state, "a", json!(10)).unwrap();
    assert_eq!(state.outcome, Some(json!(10)));
    state = graph.checkpoint(&json!(0)).unwrap();
    assert_eq!(graph.ready(&state), vec!["c"]);
}

#[test]
fn or_join_waits_for_selected_branches_and_orders_their_values_by_split_link() {
    let graph = conditional_graph("or");
    let input = json!(2);
    let mut state = graph.checkpoint(&input).unwrap();
    assert_eq!(graph.ready(&state), vec!["a", "b"]);
    graph.complete(&input, &mut state, "b", json!(20)).unwrap();
    let mut state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(state.selected.values().next().unwrap(), &vec![1, 2]);
    assert_eq!(graph.ready(&state), vec!["a"]);
    assert!(state.outcome.is_none());
    graph.complete(&input, &mut state, "a", json!(10)).unwrap();
    assert_eq!(state.outcome, Some(json!([10, 20])));
}

#[test]
fn named_graph_executes_compiled_task_from_restored_checkpoint() {
    let invoked = Arc::new(AtomicUsize::new(0));
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "compiled",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "first",
                kind: GraphNodeKind::Task(Arc::new({
                    let invoked = invoked.clone();
                    move |input, _| {
                        invoked.fetch_add(1, Ordering::SeqCst);
                        Ok(json!(input.as_i64().unwrap() + 1))
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
                target: "first",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "first",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["first"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let input = json!(41);
    let saved = serde_json::to_value(graph.checkpoint(&input).unwrap()).unwrap();
    let mut restored = serde_json::from_value(saved).unwrap();
    assert_eq!(graph.run(&input, &mut restored).unwrap(), json!(42));
    assert_eq!(graph.run(&input, &mut restored).unwrap(), json!(42));
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
}

#[test]
fn named_exceptional_terminal_records_its_identity_in_checkpoint() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "error",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "rejected",
                kind: GraphNodeKind::Error,
            },
        ],
        links: vec![GraphLink {
            source: "start",
            target: "rejected",
            value: None,
            condition: None,
            fallback: false,
            label: None,
        }],
    };
    let state = graph.checkpoint(&json!(null)).unwrap();
    let restored: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(
        restored.terminal,
        Some(GraphTerminal::Error("rejected".into()))
    );
    assert!(graph.ready(&restored).is_empty());
}

#[test]
fn failed_route_evaluation_does_not_commit_a_task_completion() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "error",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: echo_task(),
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
                value: Some(Arc::new(|_, _| Err("route failed".into()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    let mut state = graph.checkpoint(&json!(null)).unwrap();
    assert_eq!(
        graph
            .complete(&json!(null), &mut state, "work", json!(5))
            .unwrap_err(),
        "route failed"
    );
    assert_eq!(graph.ready(&state), vec!["work"]);
    assert!(state.completed.is_empty());
    assert!(state.outcome.is_none());
}

fn parallel_named_graph(b: Evaluate, c: Evaluate) -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parallel",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok::<Value, String>),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "fork",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "b",
                kind: GraphNodeKind::Task(b),
            },
            GraphNode {
                name: "c",
                kind: GraphNodeKind::Task(c),
            },
            GraphNode {
                name: "both",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "fork",
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
                target: "fork",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "fork",
                target: "b",
                value: None,
                condition: None,
                fallback: false,
                label: Some("left"),
            },
            GraphLink {
                source: "fork",
                target: "c",
                value: None,
                condition: None,
                fallback: false,
                label: Some("right"),
            },
            GraphLink {
                source: "b",
                target: "both",
                value: Some(Arc::new(|_, values| Ok(values["b"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "c",
                target: "both",
                value: Some(Arc::new(|_, values| Ok(values["c"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "both",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["both"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    }
}

#[tokio::test]
async fn parallel_named_completions_commit_only_before_failure_and_resume_unfinished_work() {
    for b_first in [true, false] {
        let path = std::env::temp_dir().join(format!(
            "blkit-named-parallel-{}-{b_first}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).await.unwrap();
        let b_started = Arc::new(AtomicBool::new(false));
        let c_started = Arc::new(AtomicBool::new(false));
        let release_b = Arc::new(AtomicBool::new(b_first));
        let release_c = Arc::new(AtomicBool::new(false));
        let b_hits = Arc::new(AtomicUsize::new(0));
        let c_hits = Arc::new(AtomicUsize::new(0));
        let b: Evaluate = Arc::new({
            let (b_started, c_started, release_b, b_hits) = (
                b_started.clone(),
                c_started.clone(),
                release_b.clone(),
                b_hits.clone(),
            );
            move |_, _| {
                b_hits.fetch_add(1, Ordering::SeqCst);
                b_started.store(true, Ordering::SeqCst);
                for _ in 0..200 {
                    if c_started.load(Ordering::SeqCst) && release_b.load(Ordering::SeqCst) {
                        return Ok(json!(3));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err("b never released".into())
            }
        });
        let c: Evaluate = Arc::new({
            let (c_started, release_c, c_hits) =
                (c_started.clone(), release_c.clone(), c_hits.clone());
            move |_, _| {
                let hit = c_hits.fetch_add(1, Ordering::SeqCst);
                c_started.store(true, Ordering::SeqCst);
                if hit > 0 {
                    return Ok(json!(5));
                }
                for _ in 0..200 {
                    if release_c.load(Ordering::SeqCst) {
                        return Err("c failed".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err("c never released".into())
            }
        });
        let engine = Engine::new(
            Registry::new(vec![parallel_named_graph(b.clone(), c.clone())]).unwrap(),
            store.clone(),
            2,
        )
        .unwrap();
        let id = engine
            .start("test", "1", "parallel", json!(null))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !b_started.load(Ordering::SeqCst) || !c_started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("both independent tasks should run concurrently");
        if b_first {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if store
                        .get(&id)
                        .await
                        .unwrap()
                        .unwrap()
                        .checkpoint
                        .as_ref()
                        .is_some_and(|state| state.completed.contains_key("b"))
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("b should commit before c fails");
        }
        release_c.store(true, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(3), async {
            while store.get(&id).await.unwrap().unwrap().status != "failed" {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        release_b.store(true, Ordering::SeqCst);
        let saved = store.get(&id).await.unwrap().unwrap().checkpoint.unwrap();
        assert_eq!(saved.completed.contains_key("b"), b_first);
        let mut restored = saved;
        let graph = parallel_named_graph(b, c);
        assert_eq!(
            graph.run(&json!(null), &mut restored).unwrap(),
            json!({"left": 3, "right": 5})
        );
        assert_eq!(b_hits.load(Ordering::SeqCst), if b_first { 1 } else { 2 });
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn retry_eligibility_respects_additional_attempts_window_and_exponential_minimum() {
    let policy = RetryPolicy {
        max_retries: 2,
        retry_for: Duration::from_millis(3_000),
        retry_delay: Duration::from_millis(1_000),
        backoff: "exponential",
    };
    assert_eq!(next_retry_at(&policy, 1, 1_000, 1_000), Some(2_000));
    assert_eq!(next_retry_at(&policy, 2, 1_000, 2_000), Some(4_000));
    assert_eq!(next_retry_at(&policy, 3, 1_000, 3_000), None);
    assert_eq!(next_retry_at(&policy, 2, 1_000, 2_100), None);
    assert_eq!(next_retry_at(&policy, 1, 1_000, 4_001), None);
    assert_eq!(next_retry_at(&policy, 100, 1_000, 1_000), None);
}

#[tokio::test]
async fn execution_failure_retries_from_committed_checkpoint_not_from_input() {
    let path = std::env::temp_dir().join(format!("blkit-retry-engine-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let release_c = Arc::new(AtomicBool::new(false));
    let b_hits = Arc::new(AtomicUsize::new(0));
    let c_hits = Arc::new(AtomicUsize::new(0));
    let b: Evaluate = Arc::new({
        let b_hits = b_hits.clone();
        move |_, _| {
            b_hits.fetch_add(1, Ordering::SeqCst);
            Ok(json!(3))
        }
    });
    let c: Evaluate = Arc::new({
        let c_hits = c_hits.clone();
        let release_c = release_c.clone();
        move |_, _| {
            if c_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                for _ in 0..200 {
                    if release_c.load(Ordering::SeqCst) {
                        return Err("retry me".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err("never released".into());
            }
            Ok(json!(5))
        }
    });
    let mut graph = parallel_named_graph(b, c);
    graph.retry = Some(RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(100),
        backoff: "exponential",
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "parallel", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !store
            .get(&id)
            .await
            .unwrap()
            .unwrap()
            .checkpoint
            .as_ref()
            .is_some_and(|state| state.completed.contains_key("b"))
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    release_c.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.get(&id).await.unwrap().unwrap().status != "retry-waiting" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a retryable failure should wait, not fail terminally");
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert_eq!(waiting.attempt, 1);
    assert!(waiting.first_failure_at.is_some());
    assert!(waiting.next_eligible_at.unwrap() >= waiting.first_failure_at.unwrap() + 100);
    assert_eq!(waiting.error.as_deref(), Some("retry me"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.get(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let item = store.get(&id).await.unwrap().unwrap();
    assert_eq!(item.result, Some(json!({"left": 3, "right": 5})));
    assert_eq!(item.attempt, 2);
    assert_eq!(b_hits.load(Ordering::SeqCst), 1);
    assert_eq!(c_hits.load(Ordering::SeqCst), 2);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn restart_recovers_running_checkpoint_or_fails_when_retry_is_absent() {
    for retry in [true, false] {
        let path = std::env::temp_dir().join(format!(
            "blkit-restart-running-{}-{retry}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let b_hits = Arc::new(AtomicUsize::new(0));
        let b: Evaluate = Arc::new({
            let b_hits = b_hits.clone();
            move |_, _| {
                b_hits.fetch_add(1, Ordering::SeqCst);
                Ok(json!(3))
            }
        });
        let c: Evaluate = Arc::new(|_, _| Ok(json!(5)));
        let graph = parallel_named_graph(b.clone(), c.clone());
        let input = json!(null);
        let mut checkpoint = graph.checkpoint(&input).unwrap();
        graph
            .complete(&input, &mut checkpoint, "b", json!(3))
            .unwrap();
        let store = LocalStore::open(&path).await.unwrap();
        let mut instance = Instance::new("old", "test", "1", "parallel", input);
        instance.checkpoint = Some(checkpoint);
        store.create(&instance).await.unwrap();
        store.begin_attempt("old").await.unwrap();
        drop(store);
        let mut graph = parallel_named_graph(b, c);
        if retry {
            graph.retry = Some(RetryPolicy {
                max_retries: 1,
                retry_for: Duration::from_secs(2),
                retry_delay: Duration::from_millis(10),
                backoff: "exponential",
            });
        }
        let reopened = LocalStore::open(&path).await.unwrap();
        let engine = Engine::new(Registry::new(vec![graph]).unwrap(), reopened.clone(), 2).unwrap();
        engine.recover().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while engine.status("old").await.unwrap().unwrap().status
                != if retry { "completed" } else { "failed" }
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let item = engine.status("old").await.unwrap().unwrap();
        assert_eq!(item.checkpoint.unwrap().completed["b"], json!(3));
        assert_eq!(b_hits.load(Ordering::SeqCst), 0);
        assert_eq!(item.attempt, if retry { 2 } else { 1 });
        if retry {
            assert_eq!(item.result, Some(json!({"left": 3, "right": 5})));
        } else {
            assert!(item.error.unwrap().contains("interrupted"));
        }
        drop(engine);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn restart_dispatches_pending_named_work_but_preserves_accepted_cancellation() {
    let path =
        std::env::temp_dir().join(format!("blkit-restart-pending-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let b_hits = Arc::new(AtomicUsize::new(0));
    let b: Evaluate = Arc::new({
        let b_hits = b_hits.clone();
        move |_, _| {
            b_hits.fetch_add(1, Ordering::SeqCst);
            Ok(json!(3))
        }
    });
    let c: Evaluate = Arc::new(|_, _| Ok(json!(5)));
    let graph = parallel_named_graph(b.clone(), c.clone());
    let store = LocalStore::open(&path).await.unwrap();
    for (id, status) in [("pending", "pending"), ("cancelling", "cancelling")] {
        let mut item = Instance::new(id, "test", "1", "parallel", json!(null));
        item.checkpoint = Some(graph.checkpoint(&json!(null)).unwrap());
        store.create(&item).await.unwrap();
        if status == "cancelling" {
            store.finish(id, status, None, None).await.unwrap();
        }
    }
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![parallel_named_graph(b, c)]).unwrap(),
        store.clone(),
        2,
    )
    .unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status("pending").await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(b_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine.status("cancelling").await.unwrap().unwrap().status,
        "cancelled"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn named_terminals_stop_parallel_work_without_becoming_execution_failures() {
    for (event, expected_status) in [
        ("error", "business-error"),
        ("cancel", "cancelled"),
        ("terminate", "terminated"),
    ] {
        let path = std::env::temp_dir().join(format!(
            "blkit-named-terminal-{}-{event}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).await.unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let signalled = Arc::new(AtomicUsize::new(0));
        let terminal = match event {
            "error" => GraphNodeKind::Error,
            "cancel" => GraphNodeKind::Cancel,
            _ => GraphNodeKind::Terminate,
        };
        let graph = GraphDefinition {
            namespace: "test",
            version: "1",
            name: "event",
            deadline: None,
            retry: Some(RetryPolicy {
                max_retries: 2,
                retry_for: Duration::from_secs(1),
                retry_delay: Duration::from_millis(10),
                backoff: "exponential",
            }),
            decode_input: Box::new(Ok::<Value, String>),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "fork",
                    kind: GraphNodeKind::Split("and"),
                },
                GraphNode {
                    name: "slow",
                    kind: GraphNodeKind::TaskWithCancel(
                        Arc::new({
                            let started = started.clone();
                            let released = released.clone();
                            move |_, _| {
                                started.store(true, Ordering::SeqCst);
                                for _ in 0..200 {
                                    if released.load(Ordering::SeqCst) {
                                        return Ok(json!(3));
                                    }
                                    std::thread::sleep(Duration::from_millis(5));
                                }
                                Err("slow not signalled".into())
                            }
                        }),
                        Arc::new({
                            let released = released.clone();
                            let signalled = signalled.clone();
                            move || {
                                signalled.fetch_add(1, Ordering::SeqCst);
                                released.store(true, Ordering::SeqCst);
                            }
                        }),
                    ),
                },
                GraphNode {
                    name: "trigger",
                    kind: GraphNodeKind::Task(Arc::new({
                        let started = started.clone();
                        move |_, _| {
                            for _ in 0..200 {
                                if started.load(Ordering::SeqCst) {
                                    return Ok(json!(1));
                                }
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err("slow never started".into())
                        }
                    })),
                },
                GraphNode {
                    name: "exit",
                    kind: terminal,
                },
            ],
            links: vec![
                GraphLink {
                    source: "start",
                    target: "fork",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: None,
                },
                GraphLink {
                    source: "fork",
                    target: "slow",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: Some("slow"),
                },
                GraphLink {
                    source: "fork",
                    target: "trigger",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: Some("trigger"),
                },
                GraphLink {
                    source: "trigger",
                    target: "exit",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: None,
                },
            ],
        };
        let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
        let id = engine
            .start("test", "1", "event", json!(null))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while engine.status(&id).await.unwrap().unwrap().status != expected_status {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("named terminal should be recorded");
        let item = engine.status(&id).await.unwrap().unwrap();
        assert_eq!(item.terminal_name.as_deref(), Some("exit"));
        assert_eq!(item.attempt, 1);
        assert_eq!(signalled.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(
            engine.status(&id).await.unwrap().unwrap().status,
            expected_status
        );
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn normal_end_waits_for_the_other_branch_before_completing() {
    let path = std::env::temp_dir().join(format!("blkit-named-join-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let b: Evaluate = Arc::new(|_, _| Ok(json!(3)));
    let c: Evaluate = Arc::new({
        let release = release.clone();
        let entered = entered.clone();
        move |_, _| {
            entered.store(true, Ordering::SeqCst);
            for _ in 0..200 {
                if release.load(Ordering::SeqCst) {
                    return Ok(json!(5));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("not released".into())
        }
    });
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![parallel_named_graph(b, c)]).unwrap(),
        store.clone(),
        2,
    )
    .unwrap();
    let id = engine
        .start("test", "1", "parallel", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if entered.load(Ordering::SeqCst)
                && item
                    .checkpoint
                    .as_ref()
                    .is_some_and(|state| state.completed.contains_key("b"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let before = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(before.status, "running");
    assert_eq!(before.result, None);
    release.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().result,
        Some(json!({"left": 3, "right": 5}))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn queued_instance_cancels_before_its_first_task_is_dispatched() {
    let path = std::env::temp_dir().join(format!("blkit-engine-pending-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let occupied = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let other_started = Arc::new(AtomicBool::new(false));
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "queue",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "work",
                kind: GraphNodeKind::Task(Arc::new({
                    let occupied = occupied.clone();
                    let release = release.clone();
                    let other_started = other_started.clone();
                    move |source, _| {
                        if *source == json!(0) {
                            occupied.store(true, Ordering::SeqCst);
                            for _ in 0..200 {
                                if release.load(Ordering::SeqCst) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        } else {
                            other_started.store(true, Ordering::SeqCst);
                        }
                        Ok(source.clone())
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
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let first = engine.start("test", "1", "queue", json!(0)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !occupied.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let second = engine.start("test", "1", "queue", json!(1)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
        engine.status(&second).await.unwrap().unwrap().status,
        "pending"
    );
    engine.cancel(&second).await.unwrap();
    release.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!other_started.load(Ordering::SeqCst));
    assert_eq!(
        engine.status(&second).await.unwrap().unwrap().status,
        "cancelled"
    );
    assert_eq!(
        engine.status(&first).await.unwrap().unwrap().status,
        "completed"
    );
    assert!(
        engine
            .cancel(&first)
            .await
            .unwrap_err()
            .contains("terminal")
    );
    assert_eq!(
        engine.status(&first).await.unwrap().unwrap().status,
        "completed"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn request_node() -> GraphNodeKind {
    use blkit::compiled_graph::{RequestSchema, RequestType};
    GraphNodeKind::InputRequest {
        prompt: "Approve?",
        schema: RequestSchema {
            ports: [("approved".into(), RequestType::Named("Bool".into()))].into(),
            records: Default::default(),
            enums: Default::default(),
        },
        decode_response: Box::new(|value| match value {
            Value::Object(ref object)
                if object.len() == 1 && object.get("approved").is_some_and(Value::is_boolean) =>
            {
                Ok(value)
            }
            _ => Err("invalid approved response".into()),
        }),
    }
}

fn request_link(source: &'static str, target: &'static str, value: Option<Evaluate>) -> GraphLink {
    GraphLink {
        source,
        target,
        value,
        condition: None,
        fallback: false,
        label: None,
    }
}

fn request_graph() -> GraphDefinition {
    GraphDefinition {
        namespace: "test",
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
                kind: request_node(),
            },
            GraphNode {
                name: "route",
                kind: GraphNodeKind::Split("xor"),
            },
            GraphNode {
                name: "approved",
                kind: GraphNodeKind::End,
            },
            GraphNode {
                name: "rejected",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            request_link("start", "review", None),
            request_link("review", "route", None),
            GraphLink {
                source: "route",
                target: "approved",
                condition: Some(Arc::new(|_, values| {
                    Ok(values["review"]["approved"].clone())
                })),
                fallback: false,
                value: Some(Arc::new(|_, _| Ok(json!("yes")))),
                label: None,
            },
            GraphLink {
                source: "route",
                target: "rejected",
                condition: None,
                fallback: true,
                value: Some(Arc::new(|_, _| Ok(json!("no")))),
                label: None,
            },
        ],
    }
}

#[test]
fn request_response_routes_only_after_valid_response_and_survives_checkpoint() {
    let graph = request_graph();
    let input = json!(null);
    let mut state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(graph.checkpoint(&input).unwrap()).unwrap())
            .unwrap();
    let requests = state.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].prompt, "Approve?");
    assert_eq!(requests[0].schema.ports.len(), 1);
    let id = requests[0].id.clone();
    assert!(
        graph
            .complete_request(&input, &mut state, &id, json!({"approved": 7}))
            .is_err()
    );
    assert_eq!(state.requests()[0].id, id);
    assert!(state.outcome.is_none());
    graph
        .complete_request(&input, &mut state, &id, json!({"approved": false}))
        .unwrap();
    assert_eq!(state.outcome, Some(json!("no")));
    assert!(state.requests().is_empty());
    assert!(
        graph
            .complete_request(&input, &mut state, &id, json!({"approved": true}))
            .is_err()
    );
}

#[test]
fn generic_task_completion_cannot_bypass_request_response_validation() {
    let graph = request_graph();
    let input = json!(null);
    let mut state = graph.checkpoint(&input).unwrap();
    let id = graph.ready_activations(&state)[0].0;
    let saved = serde_json::to_value(&state).unwrap();
    assert!(
        graph
            .complete_activation(&input, &mut state, id, json!({"approved": 7}))
            .is_err()
    );
    assert_eq!(serde_json::to_value(&state).unwrap(), saved);
    assert!(
        graph
            .complete(&input, &mut state, "review", json!({"approved": true}))
            .is_err()
    );
    assert_eq!(serde_json::to_value(&state).unwrap(), saved);
    graph
        .complete_request(
            &input,
            &mut state,
            &id.to_string(),
            json!({"approved": true}),
        )
        .unwrap();
    assert_eq!(state.outcome, Some(json!("yes")));
}

#[test]
fn parallel_and_repeated_requests_have_independent_checkpointed_ids() {
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.push(GraphNode {
        name: "split",
        kind: GraphNodeKind::Split("and"),
    });
    graph.nodes.push(GraphNode {
        name: "other",
        kind: request_node(),
    });
    graph.nodes.push(GraphNode {
        name: "joined",
        kind: GraphNodeKind::Join {
            kind: "and",
            split: "split",
        },
    });
    graph.nodes.push(GraphNode {
        name: "done",
        kind: GraphNodeKind::End,
    });
    graph.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("left"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "other",
            label: Some("right"),
            ..request_link("split", "other", None)
        },
        request_link(
            "review",
            "joined",
            Some(Arc::new(|_, values| Ok(values["review"].clone()))),
        ),
        request_link(
            "other",
            "joined",
            Some(Arc::new(|_, values| Ok(values["other"].clone()))),
        ),
        request_link(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
        ),
    ];
    let input = json!(null);
    let mut state = graph.checkpoint(&input).unwrap();
    let requests = state.requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].id, requests[1].id);
    let first = requests[0].id.clone();
    let second = requests[1].id.clone();
    graph
        .complete_request(&input, &mut state, &second, json!({"approved": false}))
        .unwrap();
    let mut state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(state.requests().len(), 1);
    assert_eq!(state.requests()[0].id, first);
    assert!(state.outcome.is_none());
    graph
        .complete_request(&input, &mut state, &first, json!({"approved": true}))
        .unwrap();
    assert_eq!(
        state.outcome,
        Some(json!({"left": {"approved": true}, "right": {"approved": false}}))
    );

    let mut repeated = request_graph();
    repeated.nodes.truncate(2);
    repeated.nodes.push(GraphNode {
        name: "again",
        kind: GraphNodeKind::Task(Arc::new(|_, values| Ok(values["review"].clone()))),
    });
    repeated.nodes.push(GraphNode {
        name: "done",
        kind: GraphNodeKind::End,
    });
    repeated.links = vec![
        request_link("start", "review", None),
        request_link("review", "again", None),
        request_link("again", "review", None),
    ];
    let mut state = repeated.checkpoint(&input).unwrap();
    let first = state.requests()[0].id.clone();
    repeated
        .complete_request(&input, &mut state, &first, json!({"approved": true}))
        .unwrap();
    let task_id = repeated.ready_activations(&state)[0].0;
    repeated
        .complete_activation(&input, &mut state, task_id, json!({"approved": true}))
        .unwrap();
    let second = state.requests()[0].id.clone();
    assert_ne!(first, second);
    assert!(
        repeated
            .complete_activation(
                &input,
                &mut state,
                second.parse().unwrap(),
                json!({"approved": true})
            )
            .is_err()
    );
    let state: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(state.requests()[0].id, second);
}

#[tokio::test]
async fn unanswered_request_before_restart_does_not_fail_or_retry() {
    let path = std::env::temp_dir().join(format!("blkit-request-seam-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph.retry = Some(RetryPolicy {
        max_retries: 1,
        retry_for: Duration::from_secs(2),
        retry_delay: Duration::from_millis(5),
        backoff: "exponential",
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "review", json!(null))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let instance = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(instance.status, "waiting-for-input");
    assert!(instance.error.is_none());
    assert_eq!(instance.attempt, 0);
    let request_id = instance.checkpoint.unwrap().requests()[0].id.clone();
    assert_eq!(store.requests(&id).await.unwrap()[0].id, request_id);
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![request_graph()]).unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    engine.recover().await.unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().status,
        "waiting-for-input"
    );
    engine
        .respond(&id, &request_id, json!({"approved": true}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if engine.status(&id).await.unwrap().unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let done = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(done.result, Some(json!("yes")));
    assert_eq!(done.attempt, 0);
    assert_eq!(done.first_failure_at, None);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn first_claimed_deadline_expires_during_task_free_input_wait() {
    let path = std::env::temp_dir().join(format!(
        "blkit-request-first-claim-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph.deadline = Some(blkit::DeadlinePolicy {
        origin: "first_claimed",
        duration: Duration::from_millis(100),
    });
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "review", json!(null))
        .await
        .unwrap();
    let waiting = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if item.status == "waiting-for-input" {
                break item;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let request_id = waiting.checkpoint.as_ref().unwrap().requests()[0]
        .id
        .clone();
    let first_claim = waiting
        .first_claim_at_ms
        .expect("input handoff claims instance");
    assert_eq!(waiting.deadline_at_ms, Some(first_claim + 100));
    assert_eq!(waiting.attempt, 0);
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.status(&id).await.unwrap().unwrap().status != "business-error" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let expired = store.get(&id).await.unwrap().unwrap();
    assert_eq!(expired.terminal_name.as_deref(), Some("timeout"));
    assert_eq!(expired.first_claim_at_ms, Some(first_claim));
    assert_eq!(expired.attempt, 0);
    assert!(
        engine
            .respond(&id, &request_id, json!({"approved": true}))
            .await
            .is_err()
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn nested_parallel_requests_remain_scoped_when_answered_out_of_order() {
    let path = std::env::temp_dir().join(format!("blkit-nested-request-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut child = request_graph();
    child.name = "child";
    let parent = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "split",
                kind: GraphNodeKind::Split("and"),
            },
            GraphNode {
                name: "left",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "right",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "joined",
                kind: GraphNodeKind::Join {
                    kind: "and",
                    split: "split",
                },
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            request_link("start", "split", None),
            GraphLink {
                source: "split",
                target: "left",
                label: Some("left"),
                ..request_link("split", "left", None)
            },
            GraphLink {
                source: "split",
                target: "right",
                label: Some("right"),
                ..request_link("split", "right", None)
            },
            request_link(
                "left",
                "joined",
                Some(Arc::new(|_, values| Ok(values["left"].clone()))),
            ),
            request_link(
                "right",
                "joined",
                Some(Arc::new(|_, values| Ok(values["right"].clone()))),
            ),
            request_link(
                "joined",
                "done",
                Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
            ),
        ],
    };
    let registry = Registry::new(vec![parent, child]).unwrap();
    let engine = Engine::new(registry.clone(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "parent", json!(null))
        .await
        .unwrap();
    let mut checkpoint = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = store.get(&id).await.unwrap().unwrap();
            let state = item.checkpoint.unwrap();
            if state.requests().len() == 2 {
                break state;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let requests = checkpoint.requests();
    assert_ne!(requests[0].id, requests[1].id);
    assert!(requests.iter().all(|request| request.id.contains('/')));
    let second = requests[1].id.clone();
    let first = requests[0].id.clone();
    registry
        .complete_request(
            "test",
            "1",
            "parent",
            &json!(null),
            &mut checkpoint,
            &second,
            json!({"approved": true}),
        )
        .unwrap();
    assert!(checkpoint.outcome.is_none());
    let mut checkpoint: GraphCheckpoint =
        serde_json::from_value(serde_json::to_value(checkpoint).unwrap()).unwrap();
    assert_eq!(checkpoint.requests()[0].id, first);
    registry
        .complete_request(
            "test",
            "1",
            "parent",
            &json!(null),
            &mut checkpoint,
            &first,
            json!({"approved": false}),
        )
        .unwrap();
    assert_eq!(
        checkpoint.outcome,
        Some(json!({"left": "no", "right": "yes"}))
    );
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(registry, store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    let reopened = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(
        reopened
            .checkpoint
            .unwrap()
            .requests()
            .iter()
            .map(|request| request.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str(), second.as_str()]
    );
    engine
        .respond(&id, &second, json!({"approved": true}))
        .await
        .unwrap();
    engine
        .respond(&id, &first, json!({"approved": false}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if engine.status(&id).await.unwrap().unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().result,
        Some(json!({"left": "no", "right": "yes"}))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn nested_request_terminal_cancels_inflight_child_sibling_before_parent_continues() {
    use std::sync::{Mutex, mpsc};

    let path = std::env::temp_dir().join(format!(
        "blkit-nested-reply-cancel-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (release, receiver) = mpsc::channel::<()>();
    let receiver = Arc::new(Mutex::new(receiver));
    let mut child = request_graph();
    child.name = "child";
    child.nodes.truncate(2);
    child.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "slow",
            kind: GraphNodeKind::TaskWithCancel(
                Arc::new({
                    let (started, receiver) = (started.clone(), receiver.clone());
                    move |_, _| {
                        started.notify_one();
                        receiver
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(5))
                            .map_err(|_| "child sibling was not cancelled".to_string())?;
                        Ok(json!(7))
                    }
                }),
                Arc::new({
                    let (cancelled, release) = (cancelled.clone(), release.clone());
                    move || {
                        cancelled.store(true, Ordering::SeqCst);
                        release.send(()).unwrap();
                    }
                }),
            ),
        },
        GraphNode {
            name: "failure",
            kind: GraphNodeKind::Error,
        },
    ]);
    child.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("review"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "slow",
            label: Some("slow"),
            ..request_link("split", "slow", None)
        },
        request_link("review", "failure", None),
    ];
    let parent = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "parent",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "called",
                kind: GraphNodeKind::Subprocess {
                    process: "child",
                    input: Arc::new(|input, _| Ok(input.clone())),
                },
            },
            GraphNode {
                name: "recovered",
                kind: GraphNodeKind::Task(Arc::new(|_, _| Ok(json!("handled")))),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            request_link("start", "called", None),
            GraphLink {
                source: "called",
                target: "recovered",
                label: Some("error"),
                ..request_link("called", "recovered", None)
            },
            request_link(
                "recovered",
                "done",
                Some(Arc::new(|_, values| Ok(values["recovered"].clone()))),
            ),
        ],
    };
    let engine = Engine::new(
        Registry::new(vec![parent, child]).unwrap(),
        store.clone(),
        2,
    )
    .unwrap();
    let id = engine
        .start("test", "1", "parent", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    let running = store.get(&id).await.unwrap().unwrap();
    assert_eq!(running.status, "running");
    let request_id = running.checkpoint.as_ref().unwrap().requests()[0]
        .id
        .clone();
    assert!(request_id.contains('/'));
    engine
        .respond(&id, &request_id, json!({"approved": true}))
        .await
        .unwrap();
    let done = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if matches!(item.status.as_str(), "completed" | "failed") {
                break item;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let was_cancelled = cancelled.load(Ordering::SeqCst);
    let _ = release.send(());
    assert_eq!(done.status, "completed", "{done:?}");
    assert_eq!(done.result, Some(json!("handled")));
    assert_eq!(done.attempt, 1);
    assert!(
        was_cancelled,
        "child cancellation hook must run before parent completion"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn request_does_not_prevent_independent_sibling_task_from_running() {
    let path =
        std::env::temp_dir().join(format!("blkit-request-sibling-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "work",
            kind: GraphNodeKind::Task(Arc::new(|_, _| Ok(json!(42)))),
        },
        GraphNode {
            name: "joined",
            kind: GraphNodeKind::Join {
                kind: "and",
                split: "split",
            },
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ]);
    graph.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("review"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "work",
            label: Some("work"),
            ..request_link("split", "work", None)
        },
        request_link(
            "review",
            "joined",
            Some(Arc::new(|_, values| Ok(values["review"].clone()))),
        ),
        request_link(
            "work",
            "joined",
            Some(Arc::new(|_, values| Ok(values["work"].clone()))),
        ),
        request_link(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
        ),
    ];
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "review", json!(null))
        .await
        .unwrap();
    let item = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = store.get(&id).await.unwrap().unwrap();
            if item
                .checkpoint
                .as_ref()
                .is_some_and(|state| state.completed.get("work") == Some(&json!(42)))
            {
                break item;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let request_id = item.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    assert_eq!(item.attempt, 1);
    assert!(item.error.is_none());
    let mut interrupted = Instance::new("interrupted", "test", "1", "review", json!(null));
    interrupted.status = "running".into();
    interrupted.attempt = 1;
    interrupted.first_claim_at_ms = item.first_claim_at_ms;
    interrupted.deadline_origin = Some("first_claimed".into());
    interrupted.deadline_duration_ms = Some(300_000);
    interrupted.deadline_at_ms = item.first_claim_at_ms.map(|at| at + 300_000);
    interrupted.checkpoint = item.checkpoint.clone();
    store.create(&interrupted).await.unwrap();
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "work",
            kind: GraphNodeKind::Task(Arc::new(|_, _| Ok(json!(42)))),
        },
        GraphNode {
            name: "joined",
            kind: GraphNodeKind::Join {
                kind: "and",
                split: "split",
            },
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ]);
    graph.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("review"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "work",
            label: Some("work"),
            ..request_link("split", "work", None)
        },
        request_link(
            "review",
            "joined",
            Some(Arc::new(|_, values| Ok(values["review"].clone()))),
        ),
        request_link(
            "work",
            "joined",
            Some(Arc::new(|_, values| Ok(values["work"].clone()))),
        ),
        request_link(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
        ),
    ];
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    let interrupted = engine.status("interrupted").await.unwrap().unwrap();
    assert_eq!(interrupted.status, "waiting-for-input");
    assert_eq!(interrupted.attempt, 1);
    assert_eq!(interrupted.first_failure_at, None);
    assert_eq!(interrupted.first_claim_at_ms, item.first_claim_at_ms);
    assert_eq!(
        interrupted.deadline_at_ms,
        item.first_claim_at_ms.map(|at| at + 300_000)
    );
    assert_eq!(interrupted.checkpoint.unwrap().requests()[0].id, request_id);
    let waiting = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(waiting.status, "waiting-for-input");
    assert_eq!(waiting.attempt, 1);
    assert_eq!(waiting.first_failure_at, None);
    assert_eq!(
        waiting.checkpoint.as_ref().unwrap().completed["work"],
        json!(42)
    );
    assert_eq!(waiting.checkpoint.unwrap().requests()[0].id, request_id);
    engine
        .respond(&id, &request_id, json!({"approved": true}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if engine.status(&id).await.unwrap().unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let done = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(
        done.result,
        Some(json!({"review": {"approved": true}, "work": 42}))
    );
    assert_eq!(done.attempt, 1);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn response_advances_while_independent_sibling_is_in_flight() {
    use std::sync::{Mutex, mpsc};

    let path = std::env::temp_dir().join(format!(
        "blkit-live-sibling-request-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let attempts = Arc::new(AtomicUsize::new(0));
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "work",
            kind: GraphNodeKind::Task(Arc::new({
                let (started, attempts, release_rx) =
                    (started.clone(), attempts.clone(), release_rx.clone());
                move |_, _| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| {
                            "response-dependent task did not run while sibling was in flight"
                                .to_string()
                        })?;
                    Ok(json!(42))
                }
            })),
        },
        GraphNode {
            name: "use_response",
            kind: GraphNodeKind::Task(Arc::new(move |_, values| {
                release_tx.send(()).map_err(|error| error.to_string())?;
                Ok(values["review"].clone())
            })),
        },
        GraphNode {
            name: "joined",
            kind: GraphNodeKind::Join {
                kind: "and",
                split: "split",
            },
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ]);
    graph.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("review"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "work",
            label: Some("work"),
            ..request_link("split", "work", None)
        },
        request_link("review", "use_response", None),
        request_link(
            "use_response",
            "joined",
            Some(Arc::new(|_, values| Ok(values["use_response"].clone()))),
        ),
        request_link(
            "work",
            "joined",
            Some(Arc::new(|_, values| Ok(values["work"].clone()))),
        ),
        request_link(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
        ),
    ];
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "review", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    let before = engine.status(&id).await.unwrap().unwrap();
    assert_eq!(before.status, "running");
    assert_eq!(before.attempt, 1);
    let request_id = before.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    engine
        .respond(&id, &request_id, json!({"approved": true}))
        .await
        .unwrap();
    assert!(store.requests(&id).await.unwrap().is_empty());
    let done = tokio::time::timeout(Duration::from_secs(7), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if matches!(item.status.as_str(), "completed" | "failed") {
                break item;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(done.status, "completed", "{done:?}");
    assert_eq!(
        done.result,
        Some(json!({"review": {"approved": true}, "work": 42}))
    );
    assert_eq!(done.attempt, 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn reply_and_release_commit_in_either_order_without_losing_answer() {
    for reply_first in [true, false] {
        let path = std::env::temp_dir().join(format!(
            "blkit-request-order-{}-{reply_first}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).await.unwrap();
        let graph = request_graph();
        let mut item = blkit::runtime::Instance::new("job", "test", "1", "review", json!(null));
        let checkpoint = graph.checkpoint(&item.input).unwrap();
        let request_id = checkpoint.requests()[0].id.clone();
        item.checkpoint = Some(checkpoint.clone());
        store.create(&item).await.unwrap();
        let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
        if reply_first {
            engine
                .respond("job", &request_id, json!({"approved": true}))
                .await
                .unwrap();
            assert!(!store.set_input_wait("job", &checkpoint).await.unwrap());
        } else {
            assert!(store.set_input_wait("job", &checkpoint).await.unwrap());
            assert_eq!(
                store.get("job").await.unwrap().unwrap().status,
                "waiting-for-input"
            );
            engine
                .respond("job", &request_id, json!({"approved": true}))
                .await
                .unwrap();
        }
        assert!(store.requests("job").await.unwrap().is_empty());
        assert!(
            engine
                .respond("job", &request_id, json!({"approved": false}))
                .await
                .is_err()
        );
        engine.recover().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if engine.status("job").await.unwrap().unwrap().status == "completed" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            store.get("job").await.unwrap().unwrap().result,
            Some(json!("yes"))
        );
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn valid_reply_commits_before_downstream_route_failure() {
    let path =
        std::env::temp_dir().join(format!("blkit-reply-route-error-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph
        .links
        .iter_mut()
        .find(|link| link.target == "approved")
        .unwrap()
        .condition = Some(Arc::new(|_, _| Err("downstream route failed".into())));
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    let request_id = item.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    store.create(&item).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine
        .respond("job", &request_id, json!({"approved": true}))
        .await
        .unwrap();
    assert!(store.requests("job").await.unwrap().is_empty());
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph
        .links
        .iter_mut()
        .find(|link| link.target == "approved")
        .unwrap()
        .condition = Some(Arc::new(|_, _| Err("downstream route failed".into())));
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status("job").await.unwrap().unwrap().status != "failed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        engine
            .status("job")
            .await
            .unwrap()
            .unwrap()
            .error
            .unwrap()
            .contains("downstream route failed")
    );
    assert!(
        engine
            .respond("job", &request_id, json!({"approved": false}))
            .await
            .is_err()
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn final_execution_failure_retires_other_unanswered_request() {
    let path = std::env::temp_dir().join(format!(
        "blkit-failed-sibling-request-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "other",
            kind: request_node(),
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ]);
    graph.links = vec![
        request_link("start", "split", None),
        GraphLink {
            source: "split",
            target: "review",
            label: Some("first"),
            ..request_link("split", "review", None)
        },
        GraphLink {
            source: "split",
            target: "other",
            label: Some("second"),
            ..request_link("split", "other", None)
        },
        request_link(
            "review",
            "done",
            Some(Arc::new(|_, _| Err("response route failed".into()))),
        ),
        request_link("other", "done", None),
    ];
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    store.create(&item).await.unwrap();
    assert_eq!(store.requests("job").await.unwrap().len(), 2);
    let first = graph
        .ready_activations(item.checkpoint.as_ref().unwrap())
        .iter()
        .find(|(_, node)| *node == "review")
        .unwrap()
        .0
        .to_string();
    let other = graph
        .ready_activations(item.checkpoint.as_ref().unwrap())
        .iter()
        .find(|(_, node)| *node == "other")
        .unwrap()
        .0
        .to_string();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine
        .respond("job", &first, json!({"approved": true}))
        .await
        .unwrap();
    assert_eq!(store.requests("job").await.unwrap()[0].id, other);
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.status("job").await.unwrap().unwrap().status != "failed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        engine
            .status("job")
            .await
            .unwrap()
            .unwrap()
            .error
            .unwrap()
            .contains("response route failed")
    );
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    assert_eq!(store.get("job").await.unwrap().unwrap().status, "failed");
    assert!(store.requests("job").await.unwrap().is_empty());
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn.query("SELECT response FROM input_requests WHERE instance_id='job' AND request_id=?1 AND status='answered'", [first]).await.unwrap();
    let row = rows
        .next()
        .await
        .unwrap()
        .expect("accepted reply remains durable");
    let response: String = row.get(0).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap(),
        json!({"approved": true})
    );
    drop(rows);
    drop(conn);
    drop(db);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn worker_revalidates_persisted_schema_before_consuming_response() {
    let mut graph = request_graph();
    if let GraphNodeKind::InputRequest {
        decode_response, ..
    } = &mut graph.nodes[1].kind
    {
        *decode_response = Box::new(Ok);
    } else {
        panic!("missing request node");
    }
    let input = json!(null);
    let mut checkpoint = graph.checkpoint(&input).unwrap();
    let request_id = checkpoint.requests()[0].id.clone();
    let before = serde_json::to_value(&checkpoint).unwrap();
    assert!(
        graph
            .complete_request(&input, &mut checkpoint, &request_id, json!({"approved": 1}))
            .is_err()
    );
    assert_eq!(serde_json::to_value(&checkpoint).unwrap(), before);
}

#[tokio::test]
async fn local_response_uses_persisted_schema_even_if_compiled_decoder_accepts_more() {
    let path =
        std::env::temp_dir().join(format!("blkit-schema-boundary-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut graph = request_graph();
    if let GraphNodeKind::InputRequest {
        decode_response, ..
    } = &mut graph.nodes[1].kind
    {
        *decode_response = Box::new(Ok);
    } else {
        panic!("missing request node");
    }
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    let request_id = item.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    store.create(&item).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    assert!(
        engine
            .respond("job", &request_id, json!({"approved": 1}))
            .await
            .is_err()
    );
    assert_eq!(store.requests("job").await.unwrap().len(), 1);
    assert_eq!(store.get("job").await.unwrap().unwrap().status, "pending");
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn invalid_and_late_replies_leave_requests_unchanged() {
    let path = std::env::temp_dir().join(format!("blkit-request-reject-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = request_graph();
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    let request_id = item.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    store.create(&item).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    assert!(
        engine
            .respond("job", &request_id, json!({"approved": 1}))
            .await
            .is_err()
    );
    assert!(
        engine
            .respond("job", &request_id, json!({"approved": true, "extra": 1}))
            .await
            .is_err()
    );
    assert!(
        engine
            .respond("job", "unknown", json!({"approved": true}))
            .await
            .is_err()
    );
    assert!(store.requests("job").await.unwrap().len() == 1);
    store.finish("job", "cancelled", None, None).await.unwrap();
    assert!(
        engine
            .respond("job", &request_id, json!({"approved": true}))
            .await
            .is_err()
    );
    assert_eq!(store.get("job").await.unwrap().unwrap().status, "cancelled");
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn reply_during_unrelated_timed_wait_advances_request_before_timer() {
    let path = std::env::temp_dir().join(format!("blkit-reply-timed-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let advanced = Arc::new(AtomicBool::new(false));
    let mut graph = request_graph();
    graph.nodes = vec![
        GraphNode {
            name: "start",
            kind: GraphNodeKind::Start,
        },
        GraphNode {
            name: "kick",
            kind: echo_task(),
        },
        GraphNode {
            name: "fork",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "review",
            kind: request_node(),
        },
        GraphNode {
            name: "advanced",
            kind: GraphNodeKind::Task(Arc::new({
                let advanced = advanced.clone();
                move |_, _| {
                    advanced.store(true, Ordering::SeqCst);
                    Ok(json!("answered"))
                }
            })),
        },
        GraphNode {
            name: "pause",
            kind: GraphNodeKind::PauseFor(Duration::from_secs(60)),
        },
        GraphNode {
            name: "joined",
            kind: GraphNodeKind::Join {
                kind: "and",
                split: "fork",
            },
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ];
    graph.links = vec![
        request_link("start", "kick", None),
        request_link("kick", "fork", None),
        GraphLink {
            label: Some("left"),
            ..request_link("fork", "review", None)
        },
        GraphLink {
            label: Some("right"),
            ..request_link("fork", "pause", None)
        },
        request_link("review", "advanced", None),
        request_link(
            "advanced",
            "joined",
            Some(Arc::new(|_, values| Ok(values["advanced"].clone()))),
        ),
        request_link("pause", "joined", Some(Arc::new(|_, _| Ok(json!("timer"))))),
        request_link(
            "joined",
            "done",
            Some(Arc::new(|_, values| Ok(values["joined"].clone()))),
        ),
    ];
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let id = engine
        .start("test", "1", "review", json!(null))
        .await
        .unwrap();
    let waiting = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if item.status == "waiting" && !store.requests(&id).await.unwrap().is_empty() {
                break item;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let wake = waiting.wake_at_ms.unwrap();
    let request_id = store.requests(&id).await.unwrap()[0].id.clone();
    engine
        .respond(&id, &request_id, json!({"approved": true}))
        .await
        .unwrap();
    let item = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let item = engine.status(&id).await.unwrap().unwrap();
            if advanced.load(Ordering::SeqCst)
                && item.status == "waiting"
                && item.checkpoint.as_ref().unwrap().requests().is_empty()
            {
                break item;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(item.wake_at_ms, Some(wake));
    assert_eq!(item.attempt, 1);
    assert!(store.requests(&id).await.unwrap().is_empty());
    engine.cancel(&id).await.unwrap();
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn reply_during_clock_wait_remains_durable_without_timer_resume() {
    let path = std::env::temp_dir().join(format!("blkit-timed-reply-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = request_graph();
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    let checkpoint = graph.checkpoint(&item.input).unwrap();
    let request_id = checkpoint.requests()[0].id.clone();
    item.checkpoint = Some(checkpoint.clone());
    store.create(&item).await.unwrap();
    store.set_wait("job", &checkpoint, 0).await.unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    engine
        .respond("job", &request_id, json!({"approved": true}))
        .await
        .unwrap();
    let saved = store.get("job").await.unwrap().unwrap();
    assert_eq!(saved.status, "pending");
    assert_eq!(saved.wake_at_ms, None);
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if engine.status("job").await.unwrap().unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.get("job").await.unwrap().unwrap().result,
        Some(json!("yes"))
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn concurrent_replies_accept_exactly_one_response() {
    let path = std::env::temp_dir().join(format!("blkit-reply-cas-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let graph = request_graph();
    let mut item = Instance::new("job", "test", "1", "review", json!(null));
    item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
    let request_id = item.checkpoint.as_ref().unwrap().requests()[0].id.clone();
    store.create(&item).await.unwrap();
    store
        .set_input_wait("job", item.checkpoint.as_ref().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
    let (first, second) = tokio::join!(
        engine.respond("job", &request_id, json!({"approved": true})),
        engine.respond("job", &request_id, json!({"approved": false})),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    drop(engine);
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new(vec![request_graph()]).unwrap(),
        store.clone(),
        1,
    )
    .unwrap();
    engine.recover().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if engine.status("job").await.unwrap().unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.get("job").await.unwrap().unwrap().result,
        Some(if first.is_ok() {
            json!("yes")
        } else {
            json!("no")
        })
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn reply_races_with_cancel_and_deadline_in_both_commit_orders() {
    for terminal in ["cancelled", "timeout"] {
        for reply_first in [true, false] {
            let path = std::env::temp_dir().join(format!(
                "blkit-reply-{terminal}-{reply_first}-{}.db",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let store = LocalStore::open(&path).await.unwrap();
            let graph = request_graph();
            let checkpoint = graph.checkpoint(&json!(null)).unwrap();
            let request_id = checkpoint.requests()[0].id.clone();
            let mut item = Instance::new("job", "test", "1", "review", json!(null));
            item.deadline_at_ms = Some(i64::MAX);
            item.checkpoint = Some(checkpoint.clone());
            store.create(&item).await.unwrap();
            store.set_input_wait("job", &checkpoint).await.unwrap();
            let engine =
                Engine::new(Registry::new(vec![graph]).unwrap(), store.clone(), 1).unwrap();
            if reply_first {
                engine
                    .respond("job", &request_id, json!({"approved": true}))
                    .await
                    .unwrap();
                // A stale worker checkpoint cannot replace the separately committed reply.
                store.commit_checkpoint("job", &checkpoint).await.unwrap();
            }
            if terminal == "cancelled" {
                store.finish("job", "cancelled", None, None).await.unwrap();
            } else {
                assert_eq!(store.expire_due(i64::MAX).await.unwrap(), vec!["job"]);
            }
            if !reply_first {
                assert!(
                    engine
                        .respond("job", &request_id, json!({"approved": true}))
                        .await
                        .is_err()
                );
            } else {
                assert!(
                    engine
                        .respond("job", &request_id, json!({"approved": false}))
                        .await
                        .is_err()
                );
            }
            let result = store.get("job").await.unwrap().unwrap();
            assert_eq!(
                result.status,
                if terminal == "timeout" {
                    "business-error"
                } else {
                    "cancelled"
                }
            );
            assert_eq!(result.result, None);
            drop(engine);
            drop(store);
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[tokio::test]
async fn terminal_graph_does_not_expose_or_accept_stale_requests() {
    let mut graph = request_graph();
    graph.nodes.truncate(2);
    graph.nodes.extend([
        GraphNode {
            name: "split",
            kind: GraphNodeKind::Split("and"),
        },
        GraphNode {
            name: "finish",
            kind: GraphNodeKind::Task(Arc::new(|_, _| Ok(json!(0)))),
        },
        GraphNode {
            name: "done",
            kind: GraphNodeKind::End,
        },
    ]);
    graph.links = vec![
        request_link("start", "split", None),
        request_link("split", "review", None),
        request_link("split", "finish", None),
        request_link("finish", "done", Some(Arc::new(|_, _| Ok(json!(0))))),
    ];
    let input = json!(null);
    let mut state = graph.checkpoint(&input).unwrap();
    let request_id = state.requests()[0].id.clone();
    let path =
        std::env::temp_dir().join(format!("blkit-terminal-request-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = LocalStore::open(&path).await.unwrap();
    let mut item = Instance::new("job", "test", "1", "review", input.clone());
    item.checkpoint = Some(state.clone());
    store.create(&item).await.unwrap();
    assert_eq!(store.requests("job").await.unwrap()[0].id, request_id);
    let finish_id = graph
        .ready_activations(&state)
        .iter()
        .find(|(_, name)| *name == "finish")
        .unwrap()
        .0;
    graph
        .complete_activation(&input, &mut state, finish_id, json!(0))
        .unwrap();
    assert_eq!(state.outcome, Some(json!(0)));
    assert!(state.requests().is_empty());
    store.commit_checkpoint("job", &state).await.unwrap();
    drop(store);
    let store = LocalStore::open(&path).await.unwrap();
    assert!(store.requests("job").await.unwrap().is_empty());
    assert!(
        store
            .get("job")
            .await
            .unwrap()
            .unwrap()
            .checkpoint
            .unwrap()
            .requests()
            .is_empty()
    );
    assert!(
        graph
            .complete_request(&input, &mut state, &request_id, json!({"approved": true}))
            .is_err()
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn old_version_two_checkpoint_without_requests_still_loads() {
    let mut json = serde_json::to_value(GraphCheckpoint::default()).unwrap();
    json.as_object_mut().unwrap().remove("requests");
    let restored: GraphCheckpoint = serde_json::from_value(json).unwrap();
    restored.ensure_supported().unwrap();
    assert!(restored.requests().is_empty());
}
