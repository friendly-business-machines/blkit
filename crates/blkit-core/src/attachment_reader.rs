use object_store::{ObjectStoreExt, path::Path};

#[cfg(feature = "remote-persistence")]
use crate::postgres_store::PostgresStore;
#[cfg(feature = "local-persistence")]
use crate::store::Store;
use crate::{attachment_store::AttachmentStore, store::Attachment};

/// Worker-facing attachment metadata; object keys and bytes are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMetadata {
    pub ordinal: i64,
    pub filename: String,
    pub media_type: Option<String>,
    pub byte_length: i64,
}

enum MetadataStore {
    #[cfg(feature = "local-persistence")]
    Local(Store),
    #[cfg(feature = "remote-persistence")]
    Remote(Box<PostgresStore>),
}

/// Construct with the same durable metadata database and object store used by the API.
/// Only trusted worker-side Rust callers should receive this handle.
pub struct AttachmentReader {
    metadata: MetadataStore,
    objects: AttachmentStore,
}

impl AttachmentReader {
    #[cfg(feature = "local-persistence")]
    pub fn local(metadata: Store, objects: AttachmentStore) -> Self {
        Self {
            metadata: MetadataStore::Local(metadata),
            objects,
        }
    }

    #[cfg(feature = "remote-persistence")]
    pub fn remote(metadata: PostgresStore, objects: AttachmentStore) -> Self {
        Self {
            metadata: MetadataStore::Remote(Box::new(metadata)),
            objects,
        }
    }

    async fn attachments(
        &self,
        instance_id: &str,
        request_id: Option<&str>,
    ) -> Result<Vec<Attachment>, String> {
        match &self.metadata {
            #[cfg(feature = "local-persistence")]
            MetadataStore::Local(store) => store.attachments(instance_id, request_id).await,
            #[cfg(feature = "remote-persistence")]
            MetadataStore::Remote(store) => store.attachments(instance_id, request_id).await,
        }
    }

    /// `None` selects start files; `Some(request_id)` selects that reply, including consumed replies.
    pub async fn list(
        &self,
        instance_id: &str,
        request_id: Option<&str>,
    ) -> Result<Vec<AttachmentMetadata>, String> {
        Ok(self
            .attachments(instance_id, request_id)
            .await?
            .into_iter()
            .map(|attachment| AttachmentMetadata {
                ordinal: attachment.ordinal,
                filename: attachment.filename,
                media_type: attachment.media_type,
                byte_length: attachment.byte_length,
            })
            .collect())
    }

    /// Missing metadata returns `None`; a missing/unreadable stored object is an error, not an absent file.
    pub async fn read(
        &self,
        instance_id: &str,
        request_id: Option<&str>,
        ordinal: i64,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(attachment) = self
            .attachments(instance_id, request_id)
            .await?
            .into_iter()
            .find(|attachment| attachment.ordinal == ordinal)
        else {
            return Ok(None);
        };
        let key = Path::parse(&attachment.object_key)
            .map_err(|_| "invalid attachment object key".to_owned())?;
        let result = self
            .objects
            .object_store()
            .get(&key)
            .await
            .map_err(|error| match error {
                object_store::Error::NotFound { .. } => "attachment object missing".to_owned(),
                _ => "attachment object read failed".to_owned(),
            })?;
        let bytes = result
            .bytes()
            .await
            .map_err(|_| "attachment content read failed".to_owned())?;
        Ok(Some(bytes.to_vec()))
    }
}
