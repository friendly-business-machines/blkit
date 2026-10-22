use axum::{extract::Multipart, http::StatusCode};
use object_store::{ObjectStoreExt, WriteMultipart, path::Path};
use serde_json::Value;

use crate::attachment_store::AttachmentStore;

const MAX_FILES: usize = 10;
const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 100 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
// Multipart framing is capped separately from file bytes to bound header and JSON overhead.
pub const MAX_BODY_BYTES: usize = MAX_TOTAL_BYTES + MAX_JSON_BYTES + 1024 * 1024;

pub struct StagedFile {
    pub key: Path,
    pub filename: String,
    pub media_type: Option<String>,
    pub byte_length: i64,
}

pub struct StagedRequest {
    pub value: Value,
    pub files: Vec<StagedFile>,
}

impl StagedRequest {
    /// Call on admission failure; successful admission owns these object keys afterward.
    pub async fn cleanup(self, store: &AttachmentStore) -> Result<(), String> {
        let mut first_error = None;
        for file in self.files {
            if let Err(error) = store.object_store().delete(&file.key).await {
                first_error.get_or_insert_with(|| error.to_string());
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn multipart_error(error: axum::extract::multipart::MultipartError) -> StatusCode {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    }
}

pub async fn decode(
    multipart: Multipart,
    json_part: &str,
    store: &AttachmentStore,
) -> Result<StagedRequest, StatusCode> {
    decode_validated(multipart, json_part, store, |_| Box::pin(async { Ok(()) })).await
}

pub async fn decode_validated(
    mut multipart: Multipart,
    json_part: &str,
    store: &AttachmentStore,
    validate: impl for<'a> Fn(
        &'a Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), StatusCode>> + Send + 'a>,
    >,
) -> Result<StagedRequest, StatusCode> {
    let mut files = Vec::new();
    let result = async {
        let mut value = None;
        let mut total_bytes = 0;
        while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
            match field.name() {
                Some(name) if name == json_part => {
                    if value.is_some() || field.file_name().is_some() {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    let mut body = Vec::new();
                    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                        if body.len() + chunk.len() > MAX_JSON_BYTES {
                            return Err(StatusCode::PAYLOAD_TOO_LARGE);
                        }
                        body.extend_from_slice(&chunk);
                    }
                    let parsed =
                        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
                    validate(&parsed).await?;
                    value = Some(parsed);
                }
                Some("files") => {
                    if files.len() == MAX_FILES {
                        return Err(StatusCode::PAYLOAD_TOO_LARGE);
                    }
                    let filename = field
                        .file_name()
                        .unwrap_or("")
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_owned();
                    if filename.is_empty() {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    let media_type = field.content_type().map(str::to_owned);
                    let key = AttachmentStore::new_key();
                    let upload = store
                        .object_store()
                        .put_multipart(&key)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    let mut writer = WriteMultipart::new(upload);
                    let mut length = 0;
                    let file_result = async {
                        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                            if chunk.len() > MAX_FILE_BYTES - length
                                || chunk.len() > MAX_TOTAL_BYTES - total_bytes
                            {
                                return Err(StatusCode::PAYLOAD_TOO_LARGE);
                            }
                            length += chunk.len();
                            total_bytes += chunk.len();
                            // WriteMultipart::new uses 5 MiB parts; each put can start at most one.
                            let mut chunk = chunk;
                            while !chunk.is_empty() {
                                writer
                                    .wait_for_capacity(1)
                                    .await
                                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                                writer.put(chunk.split_to(chunk.len().min(5 * 1024 * 1024)));
                            }
                        }
                        if length == 0 {
                            return Err(StatusCode::BAD_REQUEST);
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(error) = file_result {
                        if let Err(abort_error) = writer.abort().await {
                            tracing::warn!(%abort_error, "multipart upload abort failed");
                        }
                        return Err(error);
                    }
                    if let Err(upload_error) = writer.finish().await {
                        tracing::warn!(%upload_error, "multipart upload failed");
                        if let Err(cleanup_error) = store.object_store().delete(&key).await {
                            tracing::warn!(%cleanup_error, "failed upload cleanup failed");
                        }
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                    files.push(StagedFile {
                        key,
                        filename,
                        media_type,
                        byte_length: length as i64,
                    });
                }
                _ => return Err(StatusCode::BAD_REQUEST),
            }
        }
        let value = value.ok_or(StatusCode::BAD_REQUEST)?;
        if files.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(value)
    }
    .await;
    match result {
        Ok(value) => Ok(StagedRequest { value, files }),
        Err(error) => {
            if let Err(cleanup_error) = (StagedRequest {
                value: Value::Null,
                files,
            })
            .cleanup(store)
            .await
            {
                tracing::warn!(%cleanup_error, "multipart staging cleanup failed");
            }
            Err(error)
        }
    }
}
