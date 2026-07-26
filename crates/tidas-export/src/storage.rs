use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ClientOptions, ObjectStore, ObjectStoreExt};
use tokio::io::AsyncWriteExt;

use crate::{ExportError, ExportRequest, S3Config, validate_relative_path};

#[derive(Clone, Copy, Debug, Default)]
pub struct StorageCounts {
    pub documents: u64,
    pub bytes: u64,
    pub skipped: bool,
}

impl StorageCounts {
    pub const fn skipped() -> Self {
        Self {
            documents: 0,
            bytes: 0,
            skipped: true,
        }
    }
}

pub async fn download_external_documents(
    config: &S3Config,
    output_dir: &Path,
    request: &ExportRequest,
) -> Result<StorageCounts, ExportError> {
    request.cancellation.check()?;
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.region)
        .with_access_key_id(config.access_key_id.expose())
        .with_secret_access_key(config.secret_access_key.expose());
    if let Some(token) = &config.session_token {
        builder = builder.with_token(token.expose());
    }
    if let Some(endpoint) = &config.endpoint {
        let client_options = ClientOptions::new().with_allow_http(endpoint.starts_with("http://"));
        builder = builder
            .with_endpoint(endpoint)
            .with_virtual_hosted_style_request(false)
            .with_client_options(client_options);
    }
    let store = builder.build().map_err(ExportError::StorageConfiguration)?;
    let prefix = config
        .prefix
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(ObjectPath::from);
    let mut listing = store.list(prefix.as_ref());
    let mut counts = StorageCounts::default();
    loop {
        request.cancellation.check()?;
        let next = tokio::time::timeout(request.network_timeout, listing.next())
            .await
            .map_err(|_| ExportError::StorageTimeout)?;
        let Some(meta) = next else {
            break;
        };
        let meta = meta.map_err(ExportError::Storage)?;
        let relative = object_key_path(meta.location.as_ref())?;
        let target = output_dir.join(&relative);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let result = tokio::time::timeout(request.network_timeout, store.get(&meta.location))
            .await
            .map_err(|_| ExportError::StorageTimeout)?
            .map_err(ExportError::Storage)?;
        let mut chunks = result.into_stream();
        let mut file = tokio::fs::File::create(&target).await?;
        while let Some(chunk) = tokio::time::timeout(request.network_timeout, chunks.next())
            .await
            .map_err(|_| ExportError::StorageTimeout)?
        {
            request.cancellation.check()?;
            let chunk = chunk.map_err(ExportError::Storage)?;
            let reservation = request.memory_budget.reserve(
                u64::try_from(chunk.len())
                    .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
            )?;
            file.write_all(&chunk).await?;
            counts.bytes = counts
                .bytes
                .checked_add(
                    u64::try_from(chunk.len())
                        .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
                )
                .ok_or(tidas_runtime::RuntimeError::SizeOverflow)?;
            drop(reservation);
        }
        file.flush().await?;
        counts.documents += 1;
    }
    Ok(counts)
}

fn object_key_path(key: &str) -> Result<PathBuf, ExportError> {
    // S3 keys always use `/` as their hierarchy delimiter. Reject `\` before
    // converting to a platform path so Windows cannot reinterpret an unsafe key
    // as an otherwise-valid path.
    if key.contains('\\') {
        return Err(ExportError::UnsafePath(PathBuf::from(key)));
    }
    let path = PathBuf::from(key);
    validate_relative_path(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_keys_with_backslashes_are_rejected_before_platform_path_parsing() {
        assert!(matches!(
            object_key_path("external_docs/a\\b.txt"),
            Err(ExportError::UnsafePath(_))
        ));
    }
}
