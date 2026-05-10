//! Integrity verification layer for OpenVault.
//!
//! Provides checksums for data integrity verification using SHA-256.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

use crate::error::{VaultError, VaultResult};

/// Supported hash algorithms for integrity verification.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    /// SHA-256 - cryptographic security, widely compatible.
    #[default]
    Sha256,
}

impl HashAlgorithm {
    /// Get the name of this algorithm.
    pub fn name(&self) -> &'static str {
        match self {
            HashAlgorithm::Sha256 => "SHA-256",
        }
    }
}

/// Checksum with algorithm and value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checksum {
    /// The algorithm used for hashing.
    pub algorithm: HashAlgorithm,
    /// Hex-encoded hash value.
    pub value: String,
}

impl Checksum {
    /// Create a new checksum.
    pub fn new(algorithm: HashAlgorithm, value: String) -> Self {
        Self { algorithm, value }
    }

    /// Compute checksum of data using the specified algorithm.
    pub fn compute(data: &[u8], algorithm: HashAlgorithm) -> Self {
        let value = match algorithm {
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                hex::encode(hasher.finalize())
            }
        };
        Self { algorithm, value }
    }

    /// Verify data against this checksum.
    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::compute(data, self.algorithm);
        self.value == computed.value
    }

    /// Verify with detailed result.
    pub fn verify_with_result(&self, data: &[u8]) -> VaultResult<()> {
        if self.verify(data) {
            Ok(())
        } else {
            Err(VaultError::ChecksumMismatch {
                path: String::new(),
                expected: self.value.clone(),
                actual: Self::compute(data, self.algorithm).value,
            })
        }
    }

    /// Get the hex value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Hasher trait for streaming checksum computation.
pub trait Hasher: Write {
    /// Finalize and return the checksum.
    fn finalize(self: Box<Self>) -> Checksum;

    /// Get the algorithm being used.
    fn algorithm(&self) -> HashAlgorithm;
}

/// SHA-256 hasher for streaming operations.
pub struct Sha256Hasher {
    hasher: Sha256,
}

impl Sha256Hasher {
    /// Create a new SHA-256 hasher.
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
        }
    }

    /// Reset the hasher to a fresh state.
    pub fn reset(&mut self) {
        self.hasher = Sha256::new();
    }
}

impl Default for Sha256Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for Sha256Hasher {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Hasher for Sha256Hasher {
    fn finalize(self: Box<Self>) -> Checksum {
        let hash = self.hasher.finalize();
        Checksum::new(HashAlgorithm::Sha256, hex::encode(hash))
    }

    fn algorithm(&self) -> HashAlgorithm {
        HashAlgorithm::Sha256
    }
}

/// Factory for creating hashers.
pub struct HasherFactory;

impl HasherFactory {
    /// Create a hasher for the specified algorithm.
    pub fn create(algorithm: HashAlgorithm) -> Box<dyn Hasher> {
        match algorithm {
            HashAlgorithm::Sha256 => Box::new(Sha256Hasher::new()),
        }
    }
}

/// Compute checksum of a file.
pub fn compute_file_checksum(
    path: &std::path::Path,
    algorithm: HashAlgorithm,
) -> VaultResult<Checksum> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = HasherFactory::create(algorithm);
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize())
}

/// Compute checksum of a reader.
pub fn compute_reader_checksum<R: Read>(
    reader: &mut R,
    algorithm: HashAlgorithm,
) -> VaultResult<Checksum> {
    let mut hasher = HasherFactory::create(algorithm);
    std::io::copy(reader, &mut hasher)?;
    Ok(hasher.finalize())
}

/// Verification level for integrity checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyLevel {
    /// Quick check: verify file exists and size matches.
    Quick,
    /// Standard check: verify checksum.
    Standard,
    /// Deep check: verify all bytes match.
    Deep,
}

/// Result of a file integrity verification.
#[derive(Debug, Clone)]
pub struct IntegrityCheck {
    /// File path.
    pub path: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Expected checksum (if applicable).
    pub expected: Option<String>,
    /// Actual checksum (if computed).
    pub actual: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Snapshot-level integrity report.
#[derive(Debug, Clone, Default)]
pub struct IntegrityReport {
    /// Number of files that passed verification.
    pub files_ok: u32,
    /// Number of files that failed verification.
    pub files_failed: u32,
    /// Total bytes checked.
    pub bytes_checked: u64,
    /// Individual file results.
    pub checks: Vec<IntegrityCheck>,
}

impl IntegrityReport {
    /// Check if all files passed.
    pub fn is_all_ok(&self) -> bool {
        self.files_failed == 0
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Integrity check: {}/{} files OK, {} bytes checked",
            self.files_ok,
            self.files_ok + self.files_failed,
            self.bytes_checked
        )
    }

    /// Add a check result.
    pub fn add_check(&mut self, check: IntegrityCheck) {
        if check.passed {
            self.files_ok += 1;
        } else {
            self.files_failed += 1;
        }
        self.checks.push(check);
    }
}

/// Multi-level integrity verification engine.
pub struct IntegrityEngine;

impl IntegrityEngine {
    /// Verify a single file against expected checksum.
    pub fn verify_file(path: &std::path::Path, expected: &str) -> VaultResult<IntegrityCheck> {
        let path_str = path.display().to_string();

        let checksum = compute_file_checksum(path, HashAlgorithm::Sha256)?;

        if checksum.value == expected {
            Ok(IntegrityCheck {
                path: path_str,
                passed: true,
                expected: Some(expected.to_string()),
                actual: Some(checksum.value),
                error: None,
            })
        } else {
            Ok(IntegrityCheck {
                path: path_str,
                passed: false,
                expected: Some(expected.to_string()),
                actual: Some(checksum.value),
                error: Some("Checksum mismatch".to_string()),
            })
        }
    }

    /// Verify multiple files from a list of entries.
    pub fn verify_files<'a>(
        entries: impl Iterator<Item = (&'a str, &'a str, u64)>,
        storage_base: &std::path::Path,
    ) -> IntegrityReport {
        let mut report = IntegrityReport::default();

        for (path, expected_checksum, expected_size) in entries {
            let full_path = storage_base.join(path);
            let path_str = path.to_string();

            // Check file exists
            let metadata = match std::fs::metadata(&full_path) {
                Ok(m) => m,
                Err(e) => {
                    report.add_check(IntegrityCheck {
                        path: path_str,
                        passed: false,
                        expected: Some(expected_checksum.to_string()),
                        actual: None,
                        error: Some(format!("File not accessible: {}", e)),
                    });
                    continue;
                }
            };

            // Check size first (quick check)
            if metadata.len() != expected_size {
                report.add_check(IntegrityCheck {
                    path: path_str,
                    passed: false,
                    expected: Some(expected_checksum.to_string()),
                    actual: Some(format!("size={}", metadata.len())),
                    error: Some(format!(
                        "Size mismatch: expected {}, got {}",
                        expected_size,
                        metadata.len()
                    )),
                });
                report.bytes_checked += metadata.len();
                continue;
            }

            // Full checksum verification
            match compute_file_checksum(&full_path, HashAlgorithm::Sha256) {
                Ok(checksum) => {
                    report.bytes_checked += metadata.len();
                    if checksum.value == expected_checksum {
                        report.add_check(IntegrityCheck {
                            path: path_str,
                            passed: true,
                            expected: Some(expected_checksum.to_string()),
                            actual: Some(checksum.value),
                            error: None,
                        });
                    } else {
                        report.add_check(IntegrityCheck {
                            path: path_str,
                            passed: false,
                            expected: Some(expected_checksum.to_string()),
                            actual: Some(checksum.value),
                            error: Some("Checksum mismatch".to_string()),
                        });
                    }
                }
                Err(e) => {
                    report.add_check(IntegrityCheck {
                        path: path_str,
                        passed: false,
                        expected: Some(expected_checksum.to_string()),
                        actual: None,
                        error: Some(format!("Checksum computation failed: {}", e)),
                    });
                }
            }
        }

        report
    }

    /// Compute aggregate checksum for multiple files (for snapshot-level verification).
    pub fn compute_aggregate_checksum(checksums: &[Checksum]) -> VaultResult<String> {
        if checksums.is_empty() {
            return Ok(String::new());
        }

        let mut hasher = Sha256::new();
        for checksum in checksums {
            hasher.update(checksum.value.as_bytes());
        }
        Ok(hex::encode(hasher.finalize()))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_checksum() {
        let data = b"SHA-256 test data for OpenVault";
        let checksum = Checksum::compute(data, HashAlgorithm::Sha256);

        assert_eq!(checksum.algorithm, HashAlgorithm::Sha256);
        assert!(checksum.verify(data));
        assert_eq!(checksum.value.len(), 64); // SHA-256 produces 64 hex chars
    }

    #[test]
    fn test_default_algorithm() {
        let data = b"Test";
        let checksum = Checksum::compute(data, HashAlgorithm::default());
        assert_eq!(checksum.algorithm, HashAlgorithm::Sha256);
    }

    #[test]
    fn test_checksum_mismatch() {
        let data = b"Original data";
        let checksum = Checksum::compute(data, HashAlgorithm::Sha256);

        let tampered = b"Tampered data";
        assert!(!checksum.verify(tampered));
    }

    #[test]
    fn test_checksum_verify_with_result() {
        let data = b"Test data";
        let checksum = Checksum::compute(data, HashAlgorithm::Sha256);

        assert!(checksum.verify_with_result(data).is_ok());

        let tampered = b"Modified data";
        let result = checksum.verify_with_result(tampered);
        assert!(result.is_err());
    }

    #[test]
    fn test_streaming_hasher() {
        let mut hasher = Sha256Hasher::new();

        hasher.write_all(b"Hello, ").unwrap();
        hasher.write_all(b"OpenVault!").unwrap();

        let checksum = Box::new(hasher).finalize();

        // Verify it matches direct computation
        let direct = Checksum::compute(b"Hello, OpenVault!", HashAlgorithm::Sha256);
        assert_eq!(checksum.value, direct.value);
    }

    #[test]
    fn test_hasher_factory() {
        let hasher: Box<dyn Hasher> = HasherFactory::create(HashAlgorithm::Sha256);
        assert_eq!(hasher.algorithm(), HashAlgorithm::Sha256);
    }

    #[test]
    fn test_integrity_check_passed() {
        let check = IntegrityCheck {
            path: "/test/file.txt".to_string(),
            passed: true,
            expected: Some("abc123".to_string()),
            actual: Some("abc123".to_string()),
            error: None,
        };

        assert!(check.passed);
        assert!(check.error.is_none());
    }

    #[test]
    fn test_integrity_check_failed() {
        let check = IntegrityCheck {
            path: "/test/file.txt".to_string(),
            passed: false,
            expected: Some("abc123".to_string()),
            actual: Some("def456".to_string()),
            error: Some("Checksum mismatch".to_string()),
        };

        assert!(!check.passed);
        assert!(check.error.is_some());
    }

    #[test]
    fn test_integrity_report() {
        let mut report = IntegrityReport::default();

        report.add_check(IntegrityCheck {
            path: "file1.txt".to_string(),
            passed: true,
            expected: None,
            actual: None,
            error: None,
        });
        report.add_check(IntegrityCheck {
            path: "file2.txt".to_string(),
            passed: false,
            expected: Some("abc".to_string()),
            actual: Some("def".to_string()),
            error: Some("Mismatch".to_string()),
        });

        assert_eq!(report.files_ok, 1);
        assert_eq!(report.files_failed, 1);
        assert!(!report.is_all_ok());
    }

    #[test]
    fn test_aggregate_checksum() {
        let checksums = vec![
            Checksum::new(HashAlgorithm::Sha256, "aaa".to_string()),
            Checksum::new(HashAlgorithm::Sha256, "bbb".to_string()),
            Checksum::new(HashAlgorithm::Sha256, "ccc".to_string()),
        ];

        let aggregate = IntegrityEngine::compute_aggregate_checksum(&checksums).unwrap();
        assert_eq!(aggregate.len(), 64); // SHA-256 hash of concatenated values

        // Empty list should return empty string
        let empty_aggregate = IntegrityEngine::compute_aggregate_checksum(&[]).unwrap();
        assert_eq!(empty_aggregate, "");
    }

    #[test]
    fn test_file_checksum_computation() {
        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("openvault_test_checksum.txt");

        std::fs::write(&temp_file, b"Test content for integrity check").unwrap();

        let checksum = compute_file_checksum(&temp_file, HashAlgorithm::Sha256).unwrap();

        // Verify the checksum
        assert!(checksum.verify(b"Test content for integrity check"));

        // Cleanup
        std::fs::remove_file(&temp_file).ok();
    }

    #[test]
    fn test_hash_algorithm_name() {
        assert_eq!(HashAlgorithm::Sha256.name(), "SHA-256");
    }
}
