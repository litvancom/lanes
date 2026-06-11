/// Pluggable object storage initialization (DETAIL-08, CLAUDE.md storage pattern).
///
/// Returns an `Arc<dyn ObjectStore>`:
/// - `S3_BUCKET` env var set → `AmazonS3` (reads standard AWS_* / S3_ENDPOINT env vars)
/// - Unset → `LocalFileSystem` under `attachments_root` (created if absent)
///
/// This is the ENV-dispatch pattern from research Pattern 3.
#[cfg(feature = "ssr")]
pub fn init_storage(
    attachments_root: &std::path::Path,
) -> std::sync::Arc<dyn object_store::ObjectStore> {
    use std::sync::Arc;

    if let Ok(bucket) = std::env::var("S3_BUCKET") {
        // S3 / MinIO branch: read credentials + endpoint from standard env vars.
        // object_store reads AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_DEFAULT_REGION,
        // and S3_ENDPOINT (mapped to with_endpoint) automatically via the builder.
        let mut builder = object_store::aws::AmazonS3Builder::new()
            .with_bucket_name(&bucket);

        // Optional custom endpoint (S3_ENDPOINT → MinIO, DigitalOcean Spaces, etc.)
        if let Ok(endpoint) = std::env::var("S3_ENDPOINT") {
            builder = builder.with_endpoint(endpoint);
        }

        // Allow HTTP endpoints for self-hosted setups when explicitly configured
        if std::env::var("S3_ALLOW_HTTP").as_deref() == Ok("true") {
            builder = builder.with_allow_http(true);
        }

        let store = builder
            .build()
            .expect("S3 storage init: check S3_BUCKET, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY");

        tracing::info!("Storage: S3 bucket '{}'", bucket);
        Arc::new(store)
    } else {
        // Local disk default: create the attachments directory and use LocalFileSystem.
        std::fs::create_dir_all(attachments_root)
            .expect("Failed to create attachments directory");

        let store = object_store::local::LocalFileSystem::new_with_prefix(attachments_root)
            .expect("Local storage init failed");

        tracing::info!("Storage: local disk at {}", attachments_root.display());
        Arc::new(store)
    }
}
