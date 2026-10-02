use std::{env, sync::Arc, time::Duration};

use blkit::{
    distributed::DistributedControl, postgres_store::PostgresStore, server::router_distributed,
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
        println!("usage: blkit-api POSTGRES_URL [BIND_ADDRESS]\nDefault bind: 127.0.0.1:3000");
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let url = args
        .get(1)
        .ok_or("usage: blkit-api POSTGRES_URL [BIND_ADDRESS]")?;
    if args.len() > 3 {
        return Err("usage: blkit-api POSTGRES_URL [BIND_ADDRESS]".into());
    }
    let bind = args.get(2).map_or("127.0.0.1:3000", String::as_str);
    let store = PostgresStore::connect(url).await?;
    let control = Arc::new(DistributedControl::new(
        store,
        compiled::named_graph_definitions(),
    )?);
    let reconciler = control.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if let Err(error) = reconciler.reconcile_once().await {
                eprintln!("claim reconciliation failed: {error}");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("blkit-api listening on {}", listener.local_addr()?);
    axum::serve(listener, router_distributed(control)).await?;
    Ok(())
}
