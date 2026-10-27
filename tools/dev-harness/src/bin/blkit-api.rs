use blkit_core::{
    attachment_store::{AttachmentRole, AttachmentStore, RemoteAttachmentConfig},
    distributed::DistributedControl,
    postgres_config::{DatabaseRole, EffectivePostgresConfig},
    remote_executable::{ExecutableError, ExecutableOperation, RemoteArgs, RemoteExecutable},
    server::router_distributed_with_attachments,
};
use std::{env, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), ExecutableError> {
    let mode = RemoteExecutable::Api;
    let Some(args) = RemoteArgs::parse(mode, env::args_os().skip(1))? else {
        println!("{}", mode.usage());
        return Ok(());
    };
    let config = EffectivePostgresConfig::from_env(args.env_file.as_deref())?;
    let options = config.connection_options()?;
    let database = config.resolve_role(DatabaseRole::Api, &options)?;
    let pool_max = config.api_pool_max()?;
    let attachment_config = RemoteAttachmentConfig::from_lookup(|name| {
        config.get(name).map(|value| value.map(str::to_owned))
    })
    .map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?;
    if let Some(warning) = options.warning() {
        eprintln!("{warning}");
    }
    let _logging = blkit_core::logging::init("blkit-api")
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Logging))?;
    let attachments = AttachmentStore::remote_with_config(AttachmentRole::Api, &attachment_config)
        .await
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?;
    let store = database
        .connect_store(pool_max)
        .await
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Database))?;
    let control = Arc::new(DistributedControl::new(store));
    let reconciler = control.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if reconciler.reconcile_once().await.is_err() {
                eprintln!("claim reconciliation failed");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Bind))?;
    eprintln!(
        "blkit-api listening on {}",
        listener
            .local_addr()
            .map_err(|_| ExecutableError::operation(ExecutableOperation::Bind))?
    );
    axum::serve(
        listener,
        router_distributed_with_attachments(control, attachments),
    )
    .await
    .map_err(|_| ExecutableError::operation(ExecutableOperation::Serve))?;
    Ok(())
}
