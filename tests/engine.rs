use blkit::named_runtime::{
    GraphCheckpoint, GraphDefinition, GraphLink, GraphNode, GraphNodeKind, GraphTerminal,
};
use blkit::{
    RetryPolicy,
    runtime::{
        Branch, Definition, Engine, Evaluate, Instance, Registry, Step, Store, next_retry_at,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

fn echo_task() -> GraphNodeKind {
    GraphNodeKind::Task(Arc::new(|input, _| Ok(input.clone())))
}

#[test]
fn named_graph_resumes_from_serialized_completed_output() {
    let graph = GraphDefinition {
        namespace: "test",
        version: "1",
        name: "simple",
        retry: None,
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
    assert_eq!(state.selected["fork"], vec![1, 2]);
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
        let store = Store::open(&path).await.unwrap();
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
            Registry::new_named(vec![parallel_named_graph(b.clone(), c.clone())]).unwrap(),
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
    let store = Store::open(&path).await.unwrap();
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
    let engine = Engine::new(Registry::new_named(vec![graph]).unwrap(), store.clone(), 2).unwrap();
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
        let store = Store::open(&path).await.unwrap();
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
        let reopened = Store::open(&path).await.unwrap();
        let engine = Engine::new(
            Registry::new_named(vec![graph]).unwrap(),
            reopened.clone(),
            2,
        )
        .unwrap();
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
    let store = Store::open(&path).await.unwrap();
    for (id, status) in [("pending", "pending"), ("cancelling", "cancelling")] {
        let mut item = Instance::new(id, "test", "1", "parallel", json!(null));
        item.checkpoint = Some(graph.checkpoint(&json!(null)).unwrap());
        store.create(&item).await.unwrap();
        if status == "cancelling" {
            store.finish(id, status, None, None).await.unwrap();
        }
    }
    drop(store);
    let store = Store::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new_named(vec![parallel_named_graph(b, c)]).unwrap(),
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
        let store = Store::open(&path).await.unwrap();
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
        let engine =
            Engine::new(Registry::new_named(vec![graph]).unwrap(), store.clone(), 2).unwrap();
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
    let store = Store::open(&path).await.unwrap();
    let engine = Engine::new(
        Registry::new_named(vec![parallel_named_graph(b, c)]).unwrap(),
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
async fn failed_branch_signals_inflight_sibling_and_skips_successor() {
    let path = std::env::temp_dir().join(format!("blkit-engine-fail-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(AtomicBool::new(false));
    let signalled = Arc::new(AtomicUsize::new(0));
    let successor = Arc::new(AtomicBool::new(false));
    let bad = Step::Run {
        name: "bad",
        call: Arc::new({
            let barrier = entered.clone();
            move |_, _| {
                barrier.wait();
                Err("boom".into())
            }
        }),
        cancel: Arc::new(|| {}),
    };
    let slow = Step::Run {
        name: "slow",
        call: Arc::new({
            let barrier = entered.clone();
            let released = released.clone();
            move |_, _| {
                barrier.wait();
                for _ in 0..100 {
                    if released.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(json!(1))
            }
        }),
        cancel: Arc::new({
            let released = released.clone();
            let signalled = signalled.clone();
            move || {
                signalled.fetch_add(1, Ordering::SeqCst);
                released.store(true, Ordering::SeqCst);
            }
        }),
    };
    let definition = Definition {
        namespace: "test",
        version: "1",
        name: "fail",
        steps: vec![
            Step::Gateway {
                kind: "and",
                branches: vec![
                    Branch {
                        label: Some("bad"),
                        condition: None,
                        steps: vec![bad],
                    },
                    Branch {
                        label: Some("slow"),
                        condition: None,
                        steps: vec![slow],
                    },
                ],
                join: "both",
            },
            Step::Run {
                name: "after",
                call: Arc::new({
                    let successor = successor.clone();
                    move |_, _| {
                        successor.store(true, Ordering::SeqCst);
                        Ok(json!(0))
                    }
                }),
                cancel: Arc::new(|| {}),
            },
            Step::Return(Arc::new(|_, values| Ok(values["after"].clone()))),
        ],
        decode_input: Box::new(Ok::<Value, String>),
    };
    let registry = Registry::new(vec![definition]).unwrap();
    let engine = Engine::new(registry, store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "fail", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item: Instance = engine.status(&id).await.unwrap().unwrap();
            if item.status == "failed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(signalled.load(Ordering::SeqCst), 1);
    assert!(!successor.load(Ordering::SeqCst));
    assert!(
        engine
            .status(&id)
            .await
            .unwrap()
            .unwrap()
            .error
            .unwrap()
            .contains("boom")
    );
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
    let definition = Definition {
        namespace: "test",
        version: "1",
        name: "queue",
        steps: vec![
            Step::Run {
                name: "work",
                call: Arc::new({
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
                }),
                cancel: Arc::new(|| {}),
            },
            Step::Return(Arc::new(|_, values| Ok(values["work"].clone()))),
        ],
        decode_input: Box::new(Ok::<Value, String>),
    };
    let engine = Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 1).unwrap();
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

#[tokio::test]
async fn cancellation_signals_all_running_tasks_and_ignores_late_results() {
    let path = std::env::temp_dir().join(format!("blkit-engine-cancel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).await.unwrap();
    let started = Arc::new(AtomicUsize::new(0));
    let signalled = Arc::new(AtomicUsize::new(0));
    let released = Arc::new(AtomicBool::new(false));
    let successor = Arc::new(AtomicBool::new(false));
    let branches = (0..2)
        .map(|index| {
            let started = started.clone();
            let released = released.clone();
            let signalled = signalled.clone();
            let task = Step::Run {
                name: "work",
                call: Arc::new(move |_, _| {
                    started.fetch_add(1, Ordering::SeqCst);
                    for _ in 0..150 {
                        if released.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok(json!(index))
                }),
                cancel: Arc::new(move || {
                    signalled.fetch_add(1, Ordering::SeqCst);
                }),
            };
            Branch {
                label: Some(if index == 0 { "left" } else { "right" }),
                condition: None,
                steps: vec![task],
            }
        })
        .collect();
    let definition = Definition {
        namespace: "test",
        version: "1",
        name: "cancel",
        steps: vec![
            Step::Gateway {
                kind: "and",
                branches,
                join: "both",
            },
            Step::Run {
                name: "after",
                call: Arc::new({
                    let successor = successor.clone();
                    move |_, _| {
                        successor.store(true, Ordering::SeqCst);
                        Ok(json!(3))
                    }
                }),
                cancel: Arc::new(|| {}),
            },
            Step::Return(Arc::new(|_, values| Ok(values["after"].clone()))),
        ],
        decode_input: Box::new(Ok::<Value, String>),
    };
    let engine = Engine::new(Registry::new(vec![definition]).unwrap(), store.clone(), 2).unwrap();
    let id = engine
        .start("test", "1", "cancel", json!(null))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while started.load(Ordering::SeqCst) != 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (first, repeated) = tokio::join!(engine.cancel(&id), engine.cancel(&id));
    first.unwrap();
    repeated.unwrap();
    engine.cancel(&id).await.unwrap();
    assert_eq!(signalled.load(Ordering::SeqCst), 2);
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().status,
        "cancelled"
    );
    released.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!successor.load(Ordering::SeqCst));
    assert_eq!(
        engine.status(&id).await.unwrap().unwrap().status,
        "cancelled"
    );
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
