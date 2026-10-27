//! Compiled only in the generated worker fixture, with its existing cloud dependencies.
use blkit_core::attachment_store::{AttachmentRole, AttachmentStore};
use serde_json::Value;
use std::{fs, path::Path};

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    if !["seed", "backup", "restore", "verify"].contains(&mode) {
        return Err("invalid object fixture operation");
    }
    let entries: Vec<Value> =
        serde_json::from_slice(&fs::read(&args[2]).map_err(|_| "read private fixture manifest")?)
            .map_err(|_| "parse private fixture manifest")?;
    let store = AttachmentStore::remote_from_env(AttachmentRole::Worker)
        .await
        .map_err(|_| "open private fixture objects")?;
    for (index, entry) in entries.iter().enumerate() {
        let key = entry["object_key"]
            .as_str()
            .ok_or("missing fixture key")?
            .into();
        let expected: Vec<u8> =
            serde_json::from_value(entry["bytes"].clone()).map_err(|_| "parse fixture bytes")?;
        let file = Path::new(&args[3]).join(format!("{index}.bin"));
        if mode == "seed" || mode == "restore" {
            let bytes = if mode == "restore" {
                fs::read(&file).map_err(|_| "read private object backup")?
            } else {
                expected.clone()
            };
            if bytes != expected {
                return Err("private object backup bytes differ");
            }
            store
                .object_store()
                .put_opts(&key, bytes.into(), Default::default())
                .await
                .map_err(|_| "write private fixture object")?;
        }
        let actual = store
            .object_store()
            .get_opts(&key, Default::default())
            .await
            .map_err(|_| "get private fixture object")?
            .bytes()
            .await
            .map_err(|_| "read private fixture object")?;
        if actual.as_ref() != expected {
            return Err("private object bytes differ");
        }
        if mode == "backup" {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(file)
                .map_err(|_| "create private object backup")?
                .write_all(&actual)
                .map_err(|_| "write private object backup")?;
        }
    }
    println!("{mode}: {} private objects checked", entries.len());
    Ok(())
}
