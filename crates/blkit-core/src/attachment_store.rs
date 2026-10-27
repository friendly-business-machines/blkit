use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use std::sync::Arc;

#[cfg(feature = "attachment-local")]
use std::path::Path as FilePath;

#[derive(Clone)]
pub struct AttachmentStore(Arc<dyn ObjectStore>);

#[derive(Clone, Copy)]
pub enum AttachmentRole {
    Api,
    Worker,
}

/// Validated BLKIT cloud settings, independent of PostgreSQL and provider credentials.
#[cfg(feature = "attachment-cloud")]
pub struct RemoteAttachmentConfig {
    backend: String,
    location: String,
}

#[cfg(feature = "attachment-cloud")]
impl std::fmt::Debug for RemoteAttachmentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteAttachmentConfig")
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "attachment-cloud")]
impl RemoteAttachmentConfig {
    /// Reads only BLKIT settings for the selected backend from an explicit source.
    /// Provider credentials and endpoints are deliberately not looked up here.
    ///
    /// # Errors
    /// Rejects missing, empty, non-UTF-8 or invalid settings without echoing values
    /// or lookup errors. Unused bucket/container settings are not read.
    pub fn from_lookup<E>(
        mut get: impl FnMut(&str) -> Result<Option<String>, E>,
    ) -> Result<Self, String> {
        let backend = get("BLKIT_ATTACHMENT_BACKEND")
            .ok()
            .flatten()
            .ok_or("BLKIT_ATTACHMENT_BACKEND must be s3|gcs|azure")?;
        let setting = match backend.as_str() {
            "s3" | "gcs" => "BLKIT_ATTACHMENT_BUCKET",
            "azure" => "BLKIT_ATTACHMENT_CONTAINER",
            _ => return Err("BLKIT_ATTACHMENT_BACKEND must be s3|gcs|azure".into()),
        };
        let location = get(setting)
            .map_err(|_| format!("{setting} must be UTF-8"))?
            .ok_or_else(|| format!("{setting} is required"))?;
        if location.trim().is_empty() {
            return Err(format!("{setting} must not be empty"));
        }
        Ok(Self { backend, location })
    }
}

impl AttachmentStore {
    pub fn object_store(&self) -> &dyn ObjectStore {
        self.0.as_ref()
    }

    pub fn new_key() -> Path {
        Path::from(format!("blkit/attachments/{}", uuid::Uuid::new_v4()))
    }

    #[cfg(feature = "attachment-local")]
    pub async fn local(database: &FilePath, root: Option<&FilePath>) -> Result<Self, String> {
        let directory = root
            .map(FilePath::to_path_buf)
            .or_else(|| std::env::var_os("BLKIT_ATTACHMENT_ROOT").map(Into::into))
            .unwrap_or_else(|| {
                database
                    .parent()
                    .unwrap_or(FilePath::new("."))
                    .join("attachments")
            });
        if directory.as_os_str().is_empty() {
            return Err("BLKIT_ATTACHMENT_ROOT must not be empty".into());
        }
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("attachment directory {}: {e}", directory.display()))?;
        let store = Self(Arc::new(
            object_store::local::LocalFileSystem::new_with_prefix(&directory)
                .map_err(|e| format!("attachment directory {}: {e}", directory.display()))?,
        ));
        store.probe_write().await?;
        Ok(store)
    }

    /// Opens using process BLKIT settings only; no configuration file is discovered.
    #[cfg(feature = "attachment-cloud")]
    pub async fn remote_from_env(role: AttachmentRole) -> Result<Self, String> {
        let config = RemoteAttachmentConfig::from_lookup(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error),
        })?;
        Self::remote_with_config(role, &config).await
    }

    /// Opens cloud storage with explicit BLKIT settings and the existing role probes.
    /// Provider builders still discover credentials/endpoints from the ambient
    /// environment and their normal provider chain, never from the BLKIT overlay.
    #[cfg(feature = "attachment-cloud")]
    pub async fn remote_with_config(
        role: AttachmentRole,
        config: &RemoteAttachmentConfig,
    ) -> Result<Self, String> {
        let location = &config.location;
        let store: Arc<dyn ObjectStore> = match config.backend.as_str() {
            "s3" => Arc::new(
                object_store::aws::AmazonS3Builder::from_env()
                    .with_bucket_name(location)
                    .build()
                    .map_err(|_| "attachment S3 configuration failed; check BLKIT_ATTACHMENT_BUCKET and provider settings".to_string())?,
            ),
            "gcs" => Arc::new(
                object_store::gcp::GoogleCloudStorageBuilder::from_env()
                    .with_bucket_name(location)
                    .build()
                    .map_err(|_| "attachment GCS configuration failed; check BLKIT_ATTACHMENT_BUCKET and provider settings".to_string())?,
            ),
            "azure" => Arc::new(
                object_store::azure::MicrosoftAzureBuilder::from_env()
                    .with_container_name(location)
                    .build()
                    .map_err(|_| "attachment Azure configuration failed; check BLKIT_ATTACHMENT_CONTAINER and provider settings".to_string())?,
            ),
            _ => unreachable!(),
        };
        let store = Self(store);
        match role {
            AttachmentRole::Api => store.probe_write().await.map_err(|_| {
                "attachment API readiness probe failed; requires write, read and delete access"
                    .to_string()
            })?,
            AttachmentRole::Worker => {
                use futures_util::StreamExt;
                // Workers need list and read permissions under the private attachment prefix.
                let prefix = Path::from("blkit/attachments");
                tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    store.object_store().list(Some(&prefix)).next(),
                )
                .await
                .map_err(|_| "attachment worker list probe timed out".to_string())?
                .transpose()
                .map_err(|_| "attachment worker requires list access".to_string())?;
                match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    store.object_store().get(&Self::new_key()),
                )
                .await
                .map_err(|_| "attachment worker read probe timed out".to_string())?
                {
                    Err(object_store::Error::NotFound { .. }) => {}
                    Err(_) => return Err("attachment worker requires read access".into()),
                    Ok(_) => {
                        return Err(
                            "attachment worker read probe found an unexpected object".into()
                        );
                    }
                }
            }
        }
        Ok(store)
    }

    async fn probe_write(&self) -> Result<(), String> {
        const PROBE_CONTENT: &[u8] = b"attachment readiness";
        let key = Self::new_key();
        self.object_store()
            .put(&key, PutPayload::from_static(PROBE_CONTENT))
            .await
            .map_err(|e| format!("attachment write probe: {e}"))?;
        let read = async {
            let content = self
                .object_store()
                .get(&key)
                .await
                .map_err(|e| format!("attachment read probe: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("attachment read probe: {e}"))?;
            if content != PROBE_CONTENT {
                return Err("attachment read probe returned unexpected bytes".to_string());
            }
            Ok(())
        }
        .await;
        let delete = self.object_store().delete(&key).await;
        read?;
        delete.map_err(|e| format!("attachment delete probe: {e}"))?;
        Ok(())
    }
}

#[cfg(all(test, feature = "attachment-local"))]
mod tests {
    use super::*;
    use async_trait::async_trait;
    #[cfg(feature = "api-server")]
    use axum::{
        Router,
        body::Body,
        extract::{DefaultBodyLimit, Multipart, State},
        http::{Request, StatusCode},
        routing::post,
    };
    #[cfg(feature = "api-server")]
    use futures_util::stream;
    use futures_util::{StreamExt, stream::BoxStream};
    #[cfg(feature = "api-server")]
    use object_store::UploadPart;
    use object_store::{
        CopyOptions, GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload,
        ObjectMeta, PutMultipartOptions, PutOptions, PutResult, memory::InMemory,
    };
    #[cfg(feature = "api-server")]
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(feature = "api-server")]
    use tower::ServiceExt;

    #[derive(Debug)]
    struct BrokenRead {
        inner: InMemory,
        body_error: bool,
        #[cfg(feature = "api-server")]
        upload_concurrency: Option<Arc<UploadConcurrency>>,
    }

    #[cfg(feature = "api-server")]
    #[derive(Debug, Default)]
    struct UploadConcurrency {
        active: AtomicUsize,
        peak: AtomicUsize,
    }

    #[cfg(feature = "api-server")]
    #[derive(Debug)]
    struct MeasuredUpload {
        inner: Box<dyn MultipartUpload>,
        concurrency: Arc<UploadConcurrency>,
    }

    #[cfg(feature = "api-server")]
    #[async_trait]
    impl MultipartUpload for MeasuredUpload {
        fn put_part(&mut self, data: PutPayload) -> UploadPart {
            let part = self.inner.put_part(data);
            let concurrency = self.concurrency.clone();
            Box::pin(async move {
                let current = concurrency.active.fetch_add(1, Ordering::SeqCst) + 1;
                concurrency.peak.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                let result = part.await;
                concurrency.active.fetch_sub(1, Ordering::SeqCst);
                result
            })
        }

        async fn complete(&mut self) -> object_store::Result<PutResult> {
            self.inner.complete().await
        }

        async fn abort(&mut self) -> object_store::Result<()> {
            self.inner.abort().await
        }
    }

    impl std::fmt::Display for BrokenRead {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("BrokenRead")
        }
    }

    #[async_trait]
    impl ObjectStore for BrokenRead {
        async fn put_opts(
            &self,
            key: &Path,
            payload: PutPayload,
            opts: PutOptions,
        ) -> object_store::Result<PutResult> {
            self.inner.put_opts(key, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            key: &Path,
            opts: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            let upload = self.inner.put_multipart_opts(key, opts).await?;
            #[cfg(feature = "api-server")]
            if let Some(concurrency) = &self.upload_concurrency {
                return Ok(Box::new(MeasuredUpload {
                    inner: upload,
                    concurrency: concurrency.clone(),
                }));
            }
            Ok(upload)
        }

        async fn get_opts(&self, key: &Path, opts: GetOptions) -> object_store::Result<GetResult> {
            let mut result = self.inner.get_opts(key, opts).await?;
            result.payload = GetResultPayload::Stream(if self.body_error {
                futures_util::stream::once(async {
                    Err(object_store::Error::Generic {
                        store: "test",
                        source: std::io::Error::other("body read failed").into(),
                    })
                })
                .boxed()
            } else {
                futures_util::stream::once(async { Ok("attachment readinesx".into()) }).boxed()
            });
            Ok(result)
        }

        fn delete_stream(
            &self,
            keys: BoxStream<'static, object_store::Result<Path>>,
        ) -> BoxStream<'static, object_store::Result<Path>> {
            self.inner.delete_stream(keys)
        }

        fn list(
            &self,
            prefix: Option<&Path>,
        ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&Path>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &Path,
            to: &Path,
            opts: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, opts).await
        }
    }

    #[cfg(feature = "api-server")]
    #[tokio::test]
    async fn multipart_large_streamed_field_never_starts_second_upload_while_first_runs() {
        async fn stage(State(store): State<AttachmentStore>, multipart: Multipart) -> StatusCode {
            let staged = crate::multipart::decode(multipart, "input", &store)
                .await
                .expect("streamed request should be accepted");
            assert_eq!(staged.files.len(), 1);
            assert_eq!(staged.files[0].byte_length, 12 * 1024 * 1024);
            assert_eq!(
                store
                    .object_store()
                    .head(&staged.files[0].key)
                    .await
                    .unwrap()
                    .size,
                12 * 1024 * 1024
            );
            staged.cleanup(&store).await.unwrap();
            StatusCode::OK
        }

        let concurrency = Arc::new(UploadConcurrency::default());
        let store = AttachmentStore(Arc::new(BrokenRead {
            inner: InMemory::new(),
            body_error: false,
            upload_concurrency: Some(concurrency.clone()),
        }));
        let app = Router::new()
            .route("/upload", post(stage))
            .layer(DefaultBodyLimit::max(crate::multipart::MAX_BODY_BYTES))
            .with_state(store.clone());
        let frames = vec![
            b"--boundary\r\nContent-Disposition: form-data; name=\"input\"\r\n\r\n{}\r\n--boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"large.bin\"\r\n\r\n".to_vec(),
            vec![b'x'; 12 * 1024 * 1024],
            b"\r\n--boundary--\r\n".to_vec(),
        ];
        let response = app
            .oneshot(
                Request::post("/upload")
                    .header("content-type", "multipart/form-data; boundary=boundary")
                    .body(Body::from_stream(stream::iter(
                        frames.into_iter().map(Ok::<_, std::io::Error>),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            concurrency.peak.load(Ordering::SeqCst),
            1,
            "upload backpressure must cap in-flight parts"
        );
        assert!(store.object_store().list(None).next().await.is_none());
    }

    #[tokio::test]
    async fn readiness_rejects_bad_body_and_removes_probe_object() {
        for body_error in [true, false] {
            let store = AttachmentStore(Arc::new(BrokenRead {
                inner: InMemory::new(),
                body_error,
                #[cfg(feature = "api-server")]
                upload_concurrency: None,
            }));
            let error = store
                .probe_write()
                .await
                .err()
                .expect("invalid body must fail readiness");
            assert!(error.contains("attachment read probe"), "{error}");
            assert!(
                store.object_store().list(None).next().await.is_none(),
                "probe object was not deleted"
            );
        }
    }
}
