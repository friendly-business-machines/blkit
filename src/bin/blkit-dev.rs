use std::{env, path::Path, sync::Arc};

use blkit::{
    runtime::{Engine, Registry, Store},
    server::router,
};

// Generated Rust is checked by compilation and integration tests, not style lints.
#[allow(clippy::all)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!(
            "usage: blkit-dev [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]\nDefault: blkit.db 32 127.0.0.1:3000"
        );
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    if args.len() > 4 {
        return Err("usage: blkit-dev [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]".into());
    }
    let database = args.get(1).map_or("blkit.db", String::as_str);
    let limit = args.get(2).map_or(Ok(32), |value| value.parse::<usize>())?;
    let bind = args.get(3).map_or("127.0.0.1:3000", String::as_str);
    let store = Store::open(Path::new(database)).await?;
    let registry = Registry::new_named(compiled::named_graph_definitions())?;
    let engine = Arc::new(Engine::new(registry, store, limit)?);
    engine.recover().await?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("blkit-dev listening on {}", listener.local_addr()?);
    axum::serve(listener, router(engine)).await?;
    Ok(())
}
