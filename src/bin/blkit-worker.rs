use std::{env, time::Duration};

use blkit::{distributed::DistributedWorker, postgres_store::PostgresStore};

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!("usage: blkit-worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]");
        return Ok(());
    }
    let url = args
        .get(1)
        .ok_or("usage: blkit-worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]")?;
    if args.len() > 4 {
        return Err("usage: blkit-worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]".into());
    }
    let limit = args.get(2).map_or(Ok(32), |n| n.parse::<usize>())?;
    let lease_ms = args.get(3).map_or(Ok(5000), |n| n.parse::<i64>())?;
    let store = PostgresStore::connect(url).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let worker = DistributedWorker::new(
        store,
        &id,
        compiled::named_graph_definitions(),
        limit,
        lease_ms,
    )?;
    worker.advertise().await?;
    eprintln!("blkit-worker {id} ready");
    loop {
        worker.run_once().await?;
        if worker.drain_if_requested().await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
