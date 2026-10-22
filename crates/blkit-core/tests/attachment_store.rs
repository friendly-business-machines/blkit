#![cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]

use blkit_core::attachment_store::AttachmentStore;
#[cfg(feature = "attachment-local")]
use object_store::{ObjectStoreExt, PutPayload};
#[cfg(feature = "attachment-local")]
use std::path::Path;

#[cfg(feature = "attachment-cloud")]
#[test]
fn remote_settings_run_with_isolated_environment() {
    use std::process::Command;

    for (backend, bucket, expected) in [
        (None, None, "BLKIT_ATTACHMENT_BACKEND"),
        (Some("fs"), None, "s3|gcs|azure"),
        (Some("s3"), None, "BLKIT_ATTACHMENT_BUCKET"),
        (Some("s3"), Some(" "), "BLKIT_ATTACHMENT_BUCKET"),
        (Some("azure"), None, "BLKIT_ATTACHMENT_CONTAINER"),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .arg("--exact")
            .arg("remote_settings_child")
            .arg("--nocapture");
        child
            .env_remove("BLKIT_ATTACHMENT_BACKEND")
            .env_remove("BLKIT_ATTACHMENT_BUCKET")
            .env_remove("BLKIT_ATTACHMENT_CONTAINER")
            .env("BLKIT_TEST_EXPECTED_ERROR", expected);
        if let Some(backend) = backend {
            child.env("BLKIT_ATTACHMENT_BACKEND", backend);
        }
        if let Some(bucket) = bucket {
            child.env("BLKIT_ATTACHMENT_BUCKET", bucket);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child test did not run: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[cfg(feature = "attachment-cloud")]
#[tokio::test]
async fn remote_settings_child() {
    use blkit_core::attachment_store::AttachmentRole;

    let expected = match std::env::var("BLKIT_TEST_EXPECTED_ERROR") {
        Ok(expected) => expected,
        Err(_) => return, // The parent test invokes this test with its own environment.
    };
    let role = if std::env::var("BLKIT_ATTACHMENT_BUCKET").as_deref() == Ok(" ") {
        AttachmentRole::Api
    } else {
        AttachmentRole::Worker
    };
    let error = AttachmentStore::remote_from_env(role).await.err().unwrap();
    assert!(error.contains(&expected), "{error}");
}

#[cfg(feature = "attachment-local")]
#[tokio::test]
async fn local_default_is_persistent_beside_database() {
    let directory = std::env::temp_dir().join(format!("blkit-attachment-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = directory.join("instances.db");
    let store = AttachmentStore::local(&database, None).await.unwrap();
    let key = AttachmentStore::new_key();
    store
        .object_store()
        .put(&key, PutPayload::from_static(b"hello"))
        .await
        .unwrap();
    drop(store);
    let reopened = AttachmentStore::local(&database, None).await.unwrap();
    assert_eq!(
        reopened
            .object_store()
            .get(&key)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        b"hello".as_slice()
    );
    assert!(directory.join("attachments").is_dir());
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(feature = "attachment-local")]
#[tokio::test]
async fn local_rejects_unwritable_root() {
    let directory = std::env::temp_dir().join(format!("blkit-attachment-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("not-a-directory");
    std::fs::write(&file, b"occupied").unwrap();
    let error = AttachmentStore::local(Path::new("unused.db"), Some(&file))
        .await
        .err()
        .expect("root must be rejected");
    assert!(error.contains("attachment"), "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn generated_keys_are_unique_and_never_contain_untrusted_paths() {
    let first = AttachmentStore::new_key();
    let second = AttachmentStore::new_key();
    assert_ne!(first, second);
    for key in [first, second] {
        let text = key.to_string();
        assert!(text.starts_with("blkit/attachments/"), "{text}");
        assert!(!text.contains("..") && !text.contains('\\'), "{text}");
    }
}
