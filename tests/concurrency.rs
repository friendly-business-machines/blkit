use blkit::runtime::{Branch, Definition, Step};
use serde_json::{Value, json};
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};

#[tokio::test]
async fn independent_branches_and_instances_share_one_limit() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let branches: Vec<_> = (0..2).map(|index| {
        let active = active.clone();
        let peak = peak.clone();
        Branch { label: Some(if index == 0 { "left" } else { "right" }), condition: None, steps: vec![
            Step::Run { name: "work", call: Arc::new(move |_, _| {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(json!(index))
            }), cancel: Arc::new(|| {}) }
        ] }
    }).collect();
    let definition = Definition { namespace: "test", version: "1", name: "parallel",
        steps: vec![Step::Gateway { kind: "and", branches, join: "result" }, Step::Return(Arc::new(|_, values| Ok(values["result"].clone())))],
        decode_input: Box::new(Ok::<Value, String>) };
    let permits = Arc::new(tokio::sync::Semaphore::new(2));
    let (a, b) = tokio::join!(definition.evaluate_limited(json!(null), permits.clone()), definition.evaluate_limited(json!(null), permits));
    assert_eq!(a.unwrap(), json!({"left":0, "right":1}));
    assert_eq!(b.unwrap(), json!({"left":0, "right":1}));
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    peak.store(0, Ordering::SeqCst);
    let one = Arc::new(tokio::sync::Semaphore::new(1));
    let (a, b) = tokio::join!(definition.evaluate_limited(json!(null), one.clone()), definition.evaluate_limited(json!(null), one));
    a.unwrap();
    b.unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 1);
}
