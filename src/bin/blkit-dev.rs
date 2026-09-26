use std::{env, path::Path, sync::Arc};

use blkit::{runtime::{Engine, Registry, Store}, server::router};

mod compiled { include!(concat!(env!("OUT_DIR"), "/graph.rs")); }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!("usage: blkit-dev [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]\nDefault: blkit.db 32 127.0.0.1:3000");
        return Ok(());
    }
    if args.len() > 4 { return Err("usage: blkit-dev [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]".into()); }
    let database = args.get(1).map_or("blkit.db", String::as_str);
    let limit = args.get(2).map_or(Ok(32), |value| value.parse::<usize>())?;
    let bind = args.get(3).map_or("127.0.0.1:3000", String::as_str);
    let store = Store::open(Path::new(database)).await?;
    store.recover_interrupted().await?;
    let registry = Registry::new(compiled::graph_definitions())?;
    let engine = Arc::new(Engine::new(registry, store, limit)?);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("blkit-dev listening on {}", listener.local_addr()?);
    axum::serve(listener, router(engine)).await?;
    Ok(())
}
