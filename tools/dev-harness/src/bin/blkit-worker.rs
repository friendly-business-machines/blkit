use blkit_core::{
    attachment_store::{AttachmentRole, AttachmentStore, RemoteAttachmentConfig},
    distributed::DistributedWorker,
    logging,
    postgres_config::{DatabaseRole, EffectivePostgresConfig},
    remote_executable::{ExecutableError, ExecutableOperation, RemoteArgs, RemoteExecutable},
};
use std::{env, time::Duration};

// Generated Rust is checked by compilation and integration tests, not style lints.
#[allow(clippy::all)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/graph.rs"));
}

#[tokio::main]
async fn main() -> Result<(), ExecutableError> {
    let mode = RemoteExecutable::Worker;
    let Some(args) = RemoteArgs::parse(mode, env::args_os().skip(1))? else {
        println!("{}", mode.usage());
        return Ok(());
    };
    let config = EffectivePostgresConfig::from_env(args.env_file.as_deref())?;
    let options = config.connection_options()?;
    let database = config.resolve_role(DatabaseRole::Worker, &options)?;
    let pool_max = config.worker_pool_max()?;
    let attachment_config = RemoteAttachmentConfig::from_lookup(|name| {
        config.get(name).map(|value| value.map(str::to_owned))
    })
    .map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?;
    if let Some(warning) = options.warning() {
        eprintln!("{warning}");
    }
    let _logging = logging::init(concat!(env!("CARGO_PKG_NAME"), "-worker"))
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Logging))?;
    let _attachments =
        AttachmentStore::remote_with_config(AttachmentRole::Worker, &attachment_config)
            .await
            .map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?;
    let store = database
        .connect_store(pool_max)
        .await
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Database))?;
    let id = uuid::Uuid::new_v4().to_string();
    let worker = DistributedWorker::new(
        store,
        &id,
        compiled::named_graph_definitions(),
        args.max_tasks,
        args.lease_ms,
    )
    .map_err(|_| ExecutableError::operation(ExecutableOperation::Worker))?;
    worker
        .advertise()
        .await
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Worker))?;
    eprintln!("blkit-worker {id} ready");
    loop {
        worker
            .run_once()
            .await
            .map_err(|_| ExecutableError::operation(ExecutableOperation::Worker))?;
        if worker
            .drain_if_requested()
            .await
            .map_err(|_| ExecutableError::operation(ExecutableOperation::Worker))?
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
