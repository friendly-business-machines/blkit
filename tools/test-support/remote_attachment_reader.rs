// Worker-only generated fixture used by live attachment deployment checks.
pub const SOURCE: &str = r#"
use blkit_core::{attachment_reader::AttachmentReader, attachment_store::{AttachmentStore, AttachmentRole, RemoteAttachmentConfig}, postgres_config::{DatabaseRole, EffectivePostgresConfig}, remote_executable::{ExecutableError, ExecutableOperation}};
#[tokio::main]
async fn main() -> Result<(), ExecutableError> {
    let args: Vec<String> = std::env::args().collect();
    let config = EffectivePostgresConfig::from_env(None)?;
    let options = config.connection_options()?;
    let database = config.resolve_role(DatabaseRole::Worker, &options)?;
    let attachments = RemoteAttachmentConfig::from_lookup(|name| config.get(name).map(|value| value.map(str::to_owned)))
        .map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?;
    let reader = AttachmentReader::remote(
        database.connect_store(4).await.map_err(|_| ExecutableError::operation(ExecutableOperation::Database))?,
        AttachmentStore::remote_with_config(AttachmentRole::Worker, &attachments).await.map_err(|_| ExecutableError::operation(ExecutableOperation::Attachments))?,
    );
    let scope = (args[2] != "-").then_some(args[2].as_str());
    let mut files = Vec::new();
    for entry in reader.list(&args[1], scope).await.unwrap() {
        let bytes = reader.read(&args[1], scope, entry.ordinal).await.unwrap().unwrap();
        files.push(serde_json::json!({"ordinal": entry.ordinal, "filename": entry.filename,
            "media_type": entry.media_type, "byte_length": entry.byte_length, "bytes": bytes}));
    }
    println!("{}", serde_json::to_string(&files).unwrap());
    Ok(())
}
"#;
