#![cfg(feature = "attachment-cloud")]

use blkit_core::attachment_store::RemoteAttachmentConfig;

#[test]
fn remote_attachment_settings_validate_only_the_selected_backend_location() {
    for (backend, required, unused) in [
        (
            "s3",
            "BLKIT_ATTACHMENT_BUCKET",
            "BLKIT_ATTACHMENT_CONTAINER",
        ),
        (
            "gcs",
            "BLKIT_ATTACHMENT_BUCKET",
            "BLKIT_ATTACHMENT_CONTAINER",
        ),
        (
            "azure",
            "BLKIT_ATTACHMENT_CONTAINER",
            "BLKIT_ATTACHMENT_BUCKET",
        ),
    ] {
        let settings = RemoteAttachmentConfig::from_lookup(|name| -> Result<_, &str> {
            assert_ne!(name, unused, "unused setting must not be read");
            Ok(match name {
                "BLKIT_ATTACHMENT_BACKEND" => Some(backend.to_string()),
                name if name == required => Some("sentinel-private-location".to_string()),
                _ => panic!("unexpected setting lookup"),
            })
        })
        .unwrap();
        assert!(!format!("{settings:?}").contains("sentinel-private-location"));
    }
}

#[test]
fn remote_attachment_settings_reject_missing_empty_invalid_and_non_utf8_safely() {
    for (backend, location, expected) in [
        (None, None, "BLKIT_ATTACHMENT_BACKEND"),
        (Some(""), None, "BLKIT_ATTACHMENT_BACKEND"),
        (
            Some("sentinel-invalid-backend"),
            None,
            "BLKIT_ATTACHMENT_BACKEND",
        ),
        (Some("s3"), None, "BLKIT_ATTACHMENT_BUCKET"),
        (Some("gcs"), Some(""), "BLKIT_ATTACHMENT_BUCKET"),
        (Some("s3"), Some(" \t"), "BLKIT_ATTACHMENT_BUCKET"),
        (Some("azure"), None, "BLKIT_ATTACHMENT_CONTAINER"),
        (Some("azure"), Some(""), "BLKIT_ATTACHMENT_CONTAINER"),
    ] {
        let error = RemoteAttachmentConfig::from_lookup(|name| -> Result<_, &str> {
            Ok(if name == "BLKIT_ATTACHMENT_BACKEND" {
                backend
            } else {
                location
            }
            .map(str::to_owned))
        })
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("sentinel-invalid-backend"), "{error}");
    }
    for setting in [
        "BLKIT_ATTACHMENT_BACKEND",
        "BLKIT_ATTACHMENT_BUCKET",
        "BLKIT_ATTACHMENT_CONTAINER",
    ] {
        let error = RemoteAttachmentConfig::from_lookup(|name| {
            if name == setting {
                Err("sentinel-lookup-error")
            } else {
                Ok(Some(
                    if setting == "BLKIT_ATTACHMENT_CONTAINER" {
                        "azure"
                    } else {
                        "s3"
                    }
                    .to_string(),
                ))
            }
        })
        .unwrap_err();
        assert!(error.contains(setting), "{error}");
        assert!(!error.contains("sentinel-lookup-error"), "{error}");
    }
}

#[cfg(unix)]
#[test]
fn environment_only_attachment_interface_rejects_non_utf8_settings_without_echo() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Command};
    if let Ok(expected) = std::env::var("BLKIT_ATTACHMENT_NON_UTF8_CHILD") {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(
                blkit_core::attachment_store::AttachmentStore::remote_from_env(
                    blkit_core::attachment_store::AttachmentRole::Api,
                ),
            )
            .err()
            .unwrap();
        assert!(error.contains(&expected), "{error}");
        assert!(!error.contains("sentinel-non-utf8"), "{error}");
        return;
    }
    for setting in [
        "BLKIT_ATTACHMENT_BACKEND",
        "BLKIT_ATTACHMENT_BUCKET",
        "BLKIT_ATTACHMENT_CONTAINER",
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "environment_only_attachment_interface_rejects_non_utf8_settings_without_echo",
                "--nocapture",
            ])
            .env("BLKIT_ATTACHMENT_NON_UTF8_CHILD", setting)
            .env(
                "BLKIT_ATTACHMENT_BACKEND",
                if setting == "BLKIT_ATTACHMENT_CONTAINER" {
                    "azure"
                } else {
                    "s3"
                },
            )
            .env(
                setting,
                OsString::from_vec(b"sentinel-non-utf8\xff".to_vec()),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

#[cfg(feature = "remote-persistence")]
#[path = "../../blkit-cli/tests/support/postgres.rs"]
mod preparation;
#[cfg(feature = "remote-persistence")]
#[path = "../../../tools/test-support/seaweed.rs"]
mod seaweed;

#[cfg(feature = "remote-persistence")]
mod effective_overlay {
    use super::*;
    use blkit_core::{
        attachment_store::{AttachmentRole, AttachmentStore},
        postgres_config::{DatabaseRole, EffectivePostgresConfig},
    };
    use futures_util::StreamExt;
    use std::{collections::BTreeMap, path::PathBuf, process::Command};
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{ImageExt, runners::AsyncRunner},
    };

    struct Files(PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[tokio::test]
    async fn selected_dotenv_feeds_database_pool_and_attachments_without_environment_mutation() {
        const TEST: &str = "effective_overlay::selected_dotenv_feeds_database_pool_and_attachments_without_environment_mutation";
        if let Some(path) = std::env::var_os("BLKIT_ATTACHMENT_OVERLAY_CHILD") {
            let before: BTreeMap<_, _> = std::env::vars_os().collect();
            let effective =
                EffectivePostgresConfig::from_env(Some(std::path::Path::new(&path))).unwrap();
            let settings = RemoteAttachmentConfig::from_lookup(|name| {
                effective.get(name).map(|value| value.map(str::to_owned))
            })
            .unwrap();
            if std::env::var_os("BLKIT_ATTACHMENT_BAD_CREDENTIALS_CHILD").is_some() {
                for (role, context) in [
                    (AttachmentRole::Api, "attachment API readiness probe"),
                    (
                        AttachmentRole::Worker,
                        "attachment worker requires list access",
                    ),
                ] {
                    let explicit = AttachmentStore::remote_with_config(role, &settings)
                        .await
                        .err()
                        .unwrap();
                    let environment_only =
                        AttachmentStore::remote_from_env(role).await.err().unwrap();
                    for error in [explicit, environment_only] {
                        assert!(error.contains(context), "{error}");
                        let diagnostics = format!("{error} {error:?}");
                        for sentinel in [
                            "sentinel-private-location",
                            "rejected-provider-key",
                            &std::env::var("AWS_ENDPOINT").unwrap(),
                        ] {
                            assert!(
                                !diagnostics.contains(sentinel),
                                "remote probe leaked a supplied value: {diagnostics}"
                            );
                        }
                    }
                }
                assert_eq!(before, std::env::vars_os().collect());
                return;
            }
            let options = effective.connection_options().unwrap();
            let database = effective.resolve_role(DatabaseRole::Api, &options).unwrap();
            assert_eq!(
                database.username(),
                "postgres",
                "process URL must override dotenv URL"
            );
            assert_eq!(effective.api_pool_max().unwrap(), 2);
            assert_eq!(effective.worker_pool_max().unwrap(), 7);
            let store = database
                .connect_store(effective.api_pool_max().unwrap())
                .await
                .unwrap();
            assert!(store.get_worker("absent").await.unwrap().is_none());

            let api = AttachmentStore::remote_with_config(AttachmentRole::Api, &settings)
                .await
                .unwrap();
            let worker = AttachmentStore::remote_with_config(AttachmentRole::Worker, &settings)
                .await
                .unwrap();
            assert!(
                api.object_store().list(None).next().await.is_none(),
                "API probe must clean up its object"
            );
            assert!(worker.object_store().list(None).next().await.is_none());
            let error = AttachmentStore::remote_from_env(AttachmentRole::Api)
                .await
                .err()
                .unwrap();
            assert!(
                error.contains("BLKIT_ATTACHMENT_BUCKET"),
                "environment-only wrapper must not see dotenv: {error}"
            );
            assert_eq!(before, std::env::vars_os().collect());
            return;
        }

        let (_cloud, endpoint) = seaweed::start().await;
        let node = Postgres::default()
            .with_tag("17.6-alpine")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(node.get_host().await.unwrap().to_string());
        let url = format!(
            "postgres://postgres:postgres@{host}:{}/postgres",
            node.get_host_port_ipv4(5432).await.unwrap()
        );
        let setup = EffectivePostgresConfig::from_iter(
            None,
            [
                ("BLKIT_API_DATABASE_URL", url.as_str()),
                ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
            ],
        )
        .unwrap();
        let options = setup.connection_options().unwrap();
        let database = setup.resolve_role(DatabaseRole::Api, &options).unwrap();
        let (mut admin, driver) = database.connection().connect().await.unwrap();
        tokio::spawn(driver);
        preparation::prepare_fresh_database(&mut admin)
            .await
            .unwrap();

        let files = Files(
            std::env::temp_dir().join(format!("blkit-attachment-config-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&files.0).unwrap();
        let path = files.0.join("selected.env");
        std::fs::write(&path, "BLKIT_API_DATABASE_URL=postgres://wrong:sentinel@localhost:1/wrong\nBLKIT_POSTGRES_INSECURE_DEV=true\nBLKIT_API_DB_POOL_MAX=8\nBLKIT_WORKER_DB_POOL_MAX=7\nBLKIT_ATTACHMENT_BACKEND=gcs\nBLKIT_ATTACHMENT_BUCKET=my-bucket\nAWS_ACCESS_KEY_ID=dotenv-must-not-be-used\nAWS_SECRET_ACCESS_KEY=dotenv-must-not-be-used\nAWS_ENDPOINT=http://127.0.0.1:1\nAWS_ALLOW_HTTP=false\nGOOGLE_SERVICE_ACCOUNT=dotenv-must-not-be-read\nAZURE_STORAGE_ACCESS_KEY=dotenv-must-not-be-used\n").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        seaweed::configure(&mut child, &endpoint);
        let output = child
            .args(["--exact", TEST, "--nocapture"])
            .env("BLKIT_ATTACHMENT_OVERLAY_CHILD", &path)
            .env("BLKIT_API_DATABASE_URL", &url)
            .env_remove("BLKIT_API_DATABASE_URL_FILE")
            .env_remove("BLKIT_POSTGRES_CA_FILE")
            .env_remove("BLKIT_POSTGRES_INSECURE_DEV")
            .env("BLKIT_API_DB_POOL_MAX", "2")
            .env_remove("BLKIT_WORKER_DB_POOL_MAX")
            .env_remove("BLKIT_ATTACHMENT_BUCKET")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        let rejected = child
            .env("BLKIT_ATTACHMENT_BAD_CREDENTIALS_CHILD", "1")
            .env("AWS_ACCESS_KEY_ID", "rejected-provider-key")
            .env("BLKIT_ATTACHMENT_BUCKET", "sentinel-private-location")
            .output()
            .unwrap();
        assert!(
            rejected.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(String::from_utf8_lossy(&rejected.stdout).contains("1 passed"));
    }
}
