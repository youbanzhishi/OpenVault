//! S3-compatible storage backend for OpenVault.
//!
//! Implements VaultStorage using S3 REST API with AWS SigV4 signing.
//! Supports AWS S3, MinIO, and other S3-compatible services.

use openvault_core::error::{VaultError, VaultResult};
use openvault_core::snapshot::{BackupStrategy, Snapshot};
use openvault_core::storage::VaultStorage;

/// S3-compatible storage backend.
///
/// Supports AWS S3, MinIO, and other S3-compatible services by configuring
/// a custom endpoint. Uses AWS SigV4 request signing for authentication.
///
/// # Directory layout in S3
/// ```text
/// <bucket>/<prefix>/
/// ├── snapshots/
/// │   ├── snap-20260509120000-0000.json
/// │   └── ...
/// └── data/
///     ├── snap-20260509120000-0000/
///     │   ├── path/to/file.txt
///     │   └── ...
///     └── ...
/// ```
pub struct S3VaultStorage {
    bucket: String,
    prefix: String,
    pub(crate) endpoint: String,
    pub(crate) region: String,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    /// HTTP client for S3 operations.
    client: reqwest::blocking::Client,
}

impl S3VaultStorage {
    /// Create a new S3 storage backend.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `prefix` - Key prefix within the bucket (e.g., "backups/")
    /// * `endpoint` - S3 endpoint URL (defaults to AWS S3)
    /// * `region` - AWS region (defaults to "us-east-1")
    /// * `access_key_id` - AWS access key ID
    /// * `secret_access_key` - AWS secret access key
    pub fn new(
        bucket: String,
        prefix: String,
        endpoint: Option<String>,
        region: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
    ) -> Self {
        let endpoint = endpoint.unwrap_or_else(|| "https://s3.amazonaws.com".to_string());
        let region = region.unwrap_or_else(|| "us-east-1".to_string());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default();

        Self {
            bucket,
            prefix,
            endpoint,
            region,
            access_key_id,
            secret_access_key,
            client,
        }
    }

    /// Create a MinIO-compatible S3 backend.
    pub fn minio(
        endpoint: String,
        bucket: String,
        prefix: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Self {
        Self::new(
            bucket,
            prefix,
            Some(endpoint),
            Some("us-east-1".to_string()),
            Some(access_key_id),
            Some(secret_access_key),
        )
    }

    /// Build the full S3 key for a given object path.
    fn s3_key(&self, path: &str) -> String {
        let prefix = self.prefix.trim_end_matches('/');
        if prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", prefix, path)
        }
    }

    /// Build the URL for an S3 object.
    fn object_url(&self, key: &str) -> String {
        let endpoint = self.endpoint.trim_end_matches('/');
        format!("{}/{}/{}", endpoint, self.bucket, key)
    }

    /// Sign a request using AWS SigV4.
    ///
    /// This is a simplified SigV4 implementation suitable for S3 operations.
    /// For production use, consider using the full aws-sdk-rust.
    fn sign_request(
        &self,
        method: &str,
        key: &str,
        headers: &mut Vec<(String, String)>,
        body: &[u8],
    ) {
        let now = chrono::Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        let host = self.endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');

        headers.push(("host".to_string(), format!("{}/{}", host, self.bucket)));
        headers.push(("x-amz-date".to_string(), amz_date.clone()));
        headers.push(("x-amz-content-sha256".to_string(), sha256_hex(body)));

        if let (Some(akid), Some(secret)) = (&self.access_key_id, &self.secret_access_key) {
            // Build canonical request
            let canonical_headers: String = headers
                .iter()
                .map(|(k, v)| format!("{}:{}", k.to_lowercase(), v.trim()))
                .collect::<Vec<_>>()
                .join("\n");

            let signed_headers: String = headers
                .iter()
                .map(|(k, _)| k.to_lowercase())
                .collect::<Vec<_>>()
                .join(";");

            let canonical_uri = format!("/{}/{}", self.bucket, key);
            let canonical_request = format!(
                "{}\n{}\n\n{}\n\n{}\n{}",
                method,
                canonical_uri,
                canonical_headers,
                signed_headers,
                sha256_hex(body),
            );

            // Build string to sign
            let credential_scope = format!("{}/s3/aws4_request", date_stamp);
            let string_to_sign = format!(
                "AWS4-HMAC-SHA256\n{}\n{}\n{}",
                amz_date,
                credential_scope,
                sha256_hex(canonical_request.as_bytes()),
            );

            // Calculate signing key
            let signing_key = hmac_sha256_chain(
                format!("AWS4{}", secret).as_bytes(),
                &[
                    date_stamp.as_bytes(),
                    self.region.as_bytes(),
                    b"s3",
                    b"aws4_request",
                ],
            );

            let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

            // Build authorization header
            let auth_header = format!(
                "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
                akid, credential_scope, signed_headers, signature
            );
            headers.push(("Authorization".to_string(), auth_header));
        }
    }

    /// PUT an object to S3.
    fn put_object(&self, key: &str, data: &[u8]) -> VaultResult<()> {
        let url = self.object_url(key);
        let mut headers = Vec::new();
        headers.push(("content-type".to_string(), "application/octet-stream".to_string()));
        self.sign_request("PUT", key, &mut headers, data);

        let mut request = self.client.put(&url).body(data.to_vec());
        for (k, v) in &headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let response = request.send().map_err(|e| {
            VaultError::Storage(format!("S3 PUT failed for key {}: {}", key, e))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(VaultError::Storage(format!(
                "S3 PUT failed for key {}: {} - {}",
                key, status, body
            )));
        }

        Ok(())
    }

    /// GET an object from S3.
    fn get_object(&self, key: &str) -> VaultResult<Vec<u8>> {
        let url = self.object_url(key);
        let mut headers = Vec::new();
        self.sign_request("GET", key, &mut headers, &[]);

        let mut request = self.client.get(&url);
        for (k, v) in &headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let response = request.send().map_err(|e| {
            VaultError::Storage(format!("S3 GET failed for key {}: {}", key, e))
        })?;

        if response.status().is_success() {
            response.bytes()
                .map(|b| b.to_vec())
                .map_err(|e| VaultError::Storage(format!("S3 GET read body failed: {}", e)))
        } else if response.status().as_u16() == 404 {
            Err(VaultError::SnapshotNotFound(key.to_string()))
        } else {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            Err(VaultError::Storage(format!(
                "S3 GET failed for key {}: {} - {}",
                key, status, body
            )))
        }
    }

    /// DELETE an object from S3.
    fn delete_object(&self, key: &str) -> VaultResult<()> {
        let url = self.object_url(key);
        let mut headers = Vec::new();
        self.sign_request("DELETE", key, &mut headers, &[]);

        let mut request = self.client.delete(&url);
        for (k, v) in &headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let response = request.send().map_err(|e| {
            VaultError::Storage(format!("S3 DELETE failed for key {}: {}", key, e))
        })?;

        if !response.status().is_success() && response.status().as_u16() != 204 {
            let status = response.status();
            return Err(VaultError::Storage(format!(
                "S3 DELETE failed for key {}: {}",
                key, status
            )));
        }

        Ok(())
    }

    /// List objects in S3 with a given prefix.
    fn list_objects(&self, prefix: &str) -> VaultResult<Vec<String>> {
        let endpoint = self.endpoint.trim_end_matches('/');
        let url = format!(
            "{}/{}?list-type=2&prefix={}",
            endpoint,
            self.bucket,
            urlencoding::encode(prefix),
        );

        let mut headers = Vec::new();
        self.sign_request("GET", prefix, &mut headers, &[]);

        let mut request = self.client.get(&url);
        for (k, v) in &headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let response = request.send().map_err(|e| {
            VaultError::Storage(format!("S3 LIST failed: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(VaultError::Storage(format!(
                "S3 LIST failed: {}",
                response.status()
            )));
        }

        let body = response.text().map_err(|e| {
            VaultError::Storage(format!("S3 LIST read body failed: {}", e))
        })?;

        // Parse XML to extract keys (simplified parsing)
        let mut keys = Vec::new();
        for key_start in body.matches("<Key>") {
            if let Some(rest) = key_start.strip_prefix("<Key>") {
                if let Some(end) = rest.find("</Key>") {
                    keys.push(rest[..end].to_string());
                }
            }
        }

        Ok(keys)
    }

    fn snapshot_key(&self, id: &str) -> String {
        self.s3_key(&format!("snapshots/{}.json", id))
    }

    fn data_key(&self, snapshot_id: &str, rel_path: &str) -> String {
        self.s3_key(&format!("data/{}/{}", snapshot_id, rel_path))
    }
}

impl VaultStorage for S3VaultStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> VaultResult<()> {
        let key = self.data_key(snapshot_id, rel_path);
        self.put_object(&key, data)
    }

    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<Vec<u8>> {
        let key = self.data_key(snapshot_id, rel_path);
        self.get_object(&key)
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> VaultResult<()> {
        let key = self.snapshot_key(&snapshot.id);
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| {
            VaultError::Storage(format!("Failed to serialize snapshot: {}", e))
        })?;
        self.put_object(&key, json.as_bytes())
    }

    fn load_snapshot(&self, id: &str) -> VaultResult<Snapshot> {
        let key = self.snapshot_key(id);
        let data = self.get_object(&key)?;
        let json = String::from_utf8(data).map_err(|e| {
            VaultError::Storage(format!("Invalid UTF-8 in snapshot {}: {}", id, e))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            VaultError::Storage(format!("Failed to parse snapshot {}: {}", id, e))
        })
    }

    fn list_snapshots(&self) -> VaultResult<Vec<Snapshot>> {
        let prefix = self.s3_key("snapshots/");
        let keys = self.list_objects(&prefix)?;

        let mut snapshots = Vec::new();
        for key in keys {
            if !key.ends_with(".json") {
                continue;
            }
            let data = self.get_object(&key)?;
            if let Ok(snapshot) = serde_json::from_slice::<Snapshot>(&data) {
                snapshots.push(snapshot);
            }
        }

        snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        Ok(snapshots)
    }

    fn delete_snapshot(&self, id: &str) -> VaultResult<()> {
        // Delete snapshot metadata
        let meta_key = self.snapshot_key(id);
        self.delete_object(&meta_key)?;

        // Delete all data files for this snapshot
        let data_prefix = self.s3_key(&format!("data/{}/", id));
        let data_keys = self.list_objects(&data_prefix)?;
        for key in data_keys {
            self.delete_object(&key)?;
        }

        Ok(())
    }

    fn latest_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots
            .into_iter()
            .filter(|s| s.source == source)
            .max_by_key(|s| s.created_at))
    }

    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots
            .into_iter()
            .filter(|s| s.source == source && s.strategy == BackupStrategy::Full)
            .max_by_key(|s| s.created_at))
    }

    fn backend_name(&self) -> &str {
        "s3"
    }

    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()> {
        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::RestoreFailed(format!(
                "Failed to create target directory {}: {}",
                target.display(),
                e
            ))
        })?;

        for entry in &snapshot.entries {
            let data = self.retrieve_file(&snapshot.id, &entry.path)?;
            let file_path = target.join(&entry.path);

            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(&file_path, &data).map_err(|e| {
                VaultError::RestoreFailed(format!(
                    "Failed to restore file {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SigV4 helpers
// ---------------------------------------------------------------------------

/// Compute SHA-256 hash and return as hex string.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Compute HMAC-SHA256 and return raw bytes.
fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Chain multiple HMAC-SHA256 derivations.
fn hmac_sha256_chain(key: &[u8], messages: &[&[u8]]) -> Vec<u8> {
    let mut current = key.to_vec();
    for msg in messages {
        current = hmac_sha256(&current, msg);
    }
    current
}

/// Compute HMAC-SHA256 and return as hex string.
fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_backend_name() {
        let s3 = S3VaultStorage::new(
            "my-bucket".to_string(),
            "backup/".to_string(),
            Some("https://s3.example.com".to_string()),
            Some("us-east-1".to_string()),
            None,
            None,
        );
        assert_eq!(s3.backend_name(), "s3");
    }

    #[test]
    fn test_s3_key_building() {
        let s3 = S3VaultStorage::new(
            "my-bucket".to_string(),
            "backups/".to_string(),
            Some("https://s3.example.com".to_string()),
            Some("us-east-1".to_string()),
            None,
            None,
        );
        assert_eq!(s3.s3_key("snapshots/test.json"), "backups/snapshots/test.json");
        assert_eq!(s3.snapshot_key("snap-001"), "backups/snapshots/snap-001.json");
    }

    #[test]
    fn test_s3_key_no_prefix() {
        let s3 = S3VaultStorage::new(
            "my-bucket".to_string(),
            "".to_string(),
            None,
            None,
            None,
            None,
        );
        assert_eq!(s3.s3_key("snapshots/test.json"), "snapshots/test.json");
    }

    #[test]
    fn test_minio_factory() {
        let s3 = S3VaultStorage::minio(
            "http://localhost:9000".to_string(),
            "backups".to_string(),
            "vault/".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
        );
        assert_eq!(s3.endpoint, "http://localhost:9000");
        assert_eq!(s3.bucket, "backups");
        assert_eq!(s3.access_key_id, Some("minioadmin".to_string()));
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"");
        assert_eq!(hash.len(), 64);
        // SHA-256 of empty string is well-known
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sigv4_signing() {
        let s3 = S3VaultStorage::new(
            "test-bucket".to_string(),
            "".to_string(),
            Some("https://s3.us-east-1.amazonaws.com".to_string()),
            Some("us-east-1".to_string()),
            Some("AKIAIOSFODNN7EXAMPLE".to_string()),
            Some("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string()),
        );

        let mut headers = Vec::new();
        s3.sign_request("GET", "test-key", &mut headers, b"");

        // Should have Authorization header
        let has_auth = headers.iter().any(|(k, _)| k == "Authorization");
        assert!(has_auth, "SigV4 signing should produce Authorization header");

        // Should have x-amz-date header
        let has_date = headers.iter().any(|(k, _)| k == "x-amz-date");
        assert!(has_date, "SigV4 signing should produce x-amz-date header");
    }
}
