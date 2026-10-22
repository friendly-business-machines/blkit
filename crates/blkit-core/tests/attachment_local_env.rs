#![cfg(feature = "attachment-local")]

use blkit_core::attachment_store::AttachmentStore;

#[tokio::test]
async fn attachment_root_env_overrides_database_sibling() {
    let directory = std::env::temp_dir().join(format!("blkit-attachment-{}", uuid::Uuid::new_v4()));
    let override_root = directory.join("configured");
    let database = directory.join("db").join("instances.db");
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    // This integration binary's only test changes the process environment.
    unsafe { std::env::set_var("BLKIT_ATTACHMENT_ROOT", &override_root) };
    AttachmentStore::local(&database, None).await.unwrap();
    assert!(override_root.is_dir());
    assert!(!database.parent().unwrap().join("attachments").exists());
    unsafe { std::env::set_var("BLKIT_ATTACHMENT_ROOT", "") };
    let error = AttachmentStore::local(&database, None).await.err().unwrap();
    assert!(error.contains("BLKIT_ATTACHMENT_ROOT"), "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}
