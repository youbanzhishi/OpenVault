//! Cloudflare R2 storage backend for OpenVault.
//!
//! R2 is S3-compatible with a custom endpoint. This implementation wraps
//! S3VaultStorage with R2-specific configuration.

use openvault_core::error::VaultResult;
use openvault_core::snapshot::Snapshot;
use openvault_core::storage::VaultStorage;

use super::s3::S3VaultStorage;

/// Cloudflare R2 storage backend.
///
/// R2 uses the S3 API with a custom endpoint format:
/// `https://<account_id>.r2.cloudflarestorage.com`
///
/// Authentication uses AWS SigV4 signing, same as S3.
pub struct R2VaultStorage {
    inner: S3VaultStorage,
}

impl R2VaultStorage {
    /// Create a new R2 storage backend.
    ///
    /// # Arguments
    /// * `account_id` - Cloudflare account ID
    /// * `bucket` - R2 bucket name
    /// * `prefix` - Key prefix within the bucket
    /// * `access_key_id` - R2 access key ID
    /// * `secret_access_key` - R2 secret access key
    pub fn new(
        account_id: String,
        bucket: String,
        prefix: String,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
    ) -> Self {
        let endpoint = format!("https://{}.r2.cloudflarestorage.com", account_id);
        let inner = S3VaultStorage::new(
            bucket,
            prefix,
            Some(endpoint),
            Some("auto".to_string()), // R2 uses "auto" region
            access_key_id,
            secret_access_key,
        );
        Self { inner }
    }
}

impl VaultStorage for R2VaultStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> VaultResult<()> {
        self.inner.store_file(snapshot_id, rel_path, data)
    }

    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<Vec<u8>> {
        self.inner.retrieve_file(snapshot_id, rel_path)
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> VaultResult<()> {
        self.inner.store_snapshot(snapshot)
    }

    fn load_snapshot(&self, id: &str) -> VaultResult<Snapshot> {
        self.inner.load_snapshot(id)
    }

    fn list_snapshots(&self) -> VaultResult<Vec<Snapshot>> {
        self.inner.list_snapshots()
    }

    fn delete_snapshot(&self, id: &str) -> VaultResult<()> {
        self.inner.delete_snapshot(id)
    }

    fn latest_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        self.inner.latest_snapshot(source)
    }

    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        self.inner.latest_full_snapshot(source)
    }

    fn backend_name(&self) -> &str {
        "r2"
    }

    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()> {
        self.inner.restore_snapshot(snapshot, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_r2_backend_name() {
        let r2 = R2VaultStorage::new(
            "abc123".to_string(),
            "my-bucket".to_string(),
            "backups/".to_string(),
            None,
            None,
        );
        assert_eq!(r2.backend_name(), "r2");
    }

    #[test]
    fn test_r2_endpoint_construction() {
        let r2 = R2VaultStorage::new(
            "myaccount123".to_string(),
            "test-bucket".to_string(),
            "vault/".to_string(),
            Some("key_id".to_string()),
            Some("secret".to_string()),
        );
        // The inner S3 storage should have the R2 endpoint
        assert_eq!(
            r2.inner.endpoint,
            "https://myaccount123.r2.cloudflarestorage.com"
        );
        assert_eq!(r2.inner.region, "auto");
    }
}
