//! Audit logging with tamper-proof hash chain.
//!
//! # Phase 8 Features
//!
//! - **AuditLog** — Append-only audit log with hash-chain integrity
//! - **AuditEntry** — Individual audit record (timestamp, user, operation, target, result, metadata)
//! - **AuditQuery** — Query builder with time/user/operation/target filters, pagination, export

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::error::{VaultError, VaultResult};

// ============================================================================
// Types
// ============================================================================

/// Types of operations that can be audited.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AuditOperation {
    BackupStarted,
    BackupCompleted,
    BackupFailed,
    RestoreStarted,
    RestoreCompleted,
    RestoreFailed,
    DeleteSnapshot,
    PolicyCreated,
    PolicyUpdated,
    PolicyDeleted,
    SelfHealStarted,
    SelfHealCompleted,
    SelfHealFailed,
    TenantCreated,
    TenantUpdated,
    TenantDeleted,
    ComplianceCheck,
    ComplianceViolation,
    QuotaWarning,
    Login,
    Logout,
    Custom(String),
}

impl std::fmt::Display for AuditOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditOperation::Custom(s) => write!(f, "custom:{}", s),
            other => write!(f, "{:?}", other),
        }
    }
}

/// Result status of an audited operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Success,
    Failure,
    Partial,
    Denied,
}

/// A single audit entry in the tamper-proof log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// User or service principal that performed the operation.
    pub user_id: String,
    /// Operation type.
    pub operation: AuditOperation,
    /// Target resource identifier (snapshot id, tenant id, etc.).
    pub target: String,
    /// Outcome of the operation.
    pub result: AuditResult,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, String>,
    /// SHA-256 hash of this entry (including prev_hash for chain integrity).
    pub hash: String,
    /// Hash of the previous entry (empty for genesis entry).
    pub prev_hash: String,
}

impl AuditEntry {
    /// Compute the hash for this entry based on its content and the previous hash.
    fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.seq.to_le_bytes());
        hasher.update(self.timestamp.to_rfc3339().as_bytes());
        hasher.update(self.user_id.as_bytes());
        hasher.update(self.operation.to_string().as_bytes());
        hasher.update(self.target.as_bytes());
        hasher.update(format!("{:?}", self.result).as_bytes());
        // Deterministic metadata ordering
        let mut meta_keys: Vec<&String> = self.metadata.keys().collect();
        meta_keys.sort();
        for k in meta_keys {
            hasher.update(k.as_bytes());
            hasher.update(self.metadata[k].as_bytes());
        }
        hasher.update(self.prev_hash.as_bytes());
        hex::encode(hasher.finalize())
    }
}

// ============================================================================
// Audit Log
// ============================================================================

/// Configuration for log rotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationConfig {
    /// Maximum number of entries before rotation (0 = unlimited).
    pub max_entries: u64,
    /// Maximum age in days before rotation (0 = unlimited).
    pub max_age_days: u32,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_age_days: 365,
        }
    }
}

/// In-memory audit log with hash-chain integrity and rotation support.
#[derive(Debug, Clone)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    archived: Vec<Vec<AuditEntry>>,
    rotation: RotationConfig,
    next_seq: u64,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new(RotationConfig::default())
    }
}

impl AuditLog {
    /// Create a new empty audit log with the given rotation config.
    pub fn new(rotation: RotationConfig) -> Self {
        Self {
            entries: Vec::new(),
            archived: Vec::new(),
            rotation,
            next_seq: 1,
        }
    }

    /// Append a new entry to the audit log.
    pub fn append(
        &mut self,
        user_id: &str,
        operation: AuditOperation,
        target: &str,
        result: AuditResult,
        metadata: HashMap<String, String>,
    ) -> VaultResult<&AuditEntry> {
        let prev_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_default();

        let entry = AuditEntry {
            seq: self.next_seq,
            timestamp: Utc::now(),
            user_id: user_id.to_string(),
            operation,
            target: target.to_string(),
            result,
            metadata,
            hash: String::new(), // placeholder
            prev_hash,
        };

        let mut entry = entry;
        entry.hash = entry.compute_hash();

        self.next_seq += 1;
        self.entries.push(entry);

        // Check rotation
        self.maybe_rotate();

        Ok(self.entries.last().unwrap())
    }

    /// Return the total number of active (non-archived) entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the log empty?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by sequence number.
    pub fn get(&self, seq: u64) -> Option<&AuditEntry> {
        self.entries.iter().find(|e| e.seq == seq)
    }

    /// Get the latest entry.
    pub fn latest(&self) -> Option<&AuditEntry> {
        self.entries.last()
    }

    /// Verify the entire hash chain integrity.
    pub fn verify_chain(&self) -> VaultResult<bool> {
        for (i, entry) in self.entries.iter().enumerate() {
            let expected_hash = entry.compute_hash();
            if entry.hash != expected_hash {
                return Err(VaultError::Integrity(format!(
                    "Hash mismatch at seq {}: expected {}, got {}",
                    entry.seq, expected_hash, entry.hash
                )));
            }
            if i > 0 {
                let prev = &self.entries[i - 1];
                if entry.prev_hash != prev.hash {
                    return Err(VaultError::Integrity(format!(
                        "Chain broken at seq {}: prev_hash {} != actual prev hash {}",
                        entry.seq, entry.prev_hash, prev.hash
                    )));
                }
            } else if !entry.prev_hash.is_empty() {
                return Err(VaultError::Integrity(format!(
                    "Genesis entry seq {} should have empty prev_hash, got {}",
                    entry.seq, entry.prev_hash
                )));
            }
        }
        Ok(true)
    }

    /// Return all active entries (most recent first).
    pub fn all_entries(&self) -> Vec<&AuditEntry> {
        self.entries.iter().rev().collect()
    }

    /// Query the audit log with filters.
    pub fn query(&self, q: &AuditQuery) -> AuditQueryResult<'_> {
        let filtered: Vec<&AuditEntry> = self
            .entries
            .iter()
            .rev() // newest first
            .filter(|e| q.matches(e))
            .collect();

        let total = filtered.len();
        let page = q.page.unwrap_or(0);
        let per_page = q.per_page.unwrap_or(50);
        let start = page as usize * per_page as usize;
        let end = std::cmp::min(start + per_page as usize, total);
        let items: Vec<&AuditEntry> = if start < total {
            filtered[start..end].to_vec()
        } else {
            Vec::new()
        };

        AuditQueryResult {
            total,
            page,
            per_page,
            items,
        }
    }

    /// Export all matching entries as JSON string.
    pub fn export_json(&self, q: &AuditQuery) -> VaultResult<String> {
        let filtered: Vec<&AuditEntry> = self.entries.iter().rev().filter(|e| q.matches(e)).collect();
        serde_json::to_string_pretty(&filtered)
            .map_err(|e| VaultError::BackupFailed(format!("JSON export failed: {}", e)))
    }

    /// Export all matching entries as CSV string.
    pub fn export_csv(&self, q: &AuditQuery) -> VaultResult<String> {
        let filtered: Vec<&AuditEntry> = self.entries.iter().rev().filter(|e| q.matches(e)).collect();
        let mut w = String::from("seq,timestamp,user_id,operation,target,result,hash\n");
        for e in &filtered {
            w.push_str(&format!(
                "{},{},{},{},{},{:?},{}\n",
                e.seq,
                e.timestamp.to_rfc3339(),
                e.user_id,
                e.operation,
                e.target,
                e.result,
                e.hash
            ));
        }
        Ok(w)
    }

    /// Get archived log segments.
    pub fn archived_segments(&self) -> usize {
        self.archived.len()
    }

    // ---- internal ----

    fn maybe_rotate(&mut self) {
        if self.rotation.max_entries > 0 && self.entries.len() as u64 > self.rotation.max_entries {
            let half = self.entries.len() / 2;
            let old: Vec<AuditEntry> = self.entries.drain(..half).collect();
            self.archived.push(old);
        }
    }
}

// ============================================================================
// Audit Query
// ============================================================================

/// Query parameters for filtering audit entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    /// Filter by start time (inclusive).
    pub start_time: Option<DateTime<Utc>>,
    /// Filter by end time (inclusive).
    pub end_time: Option<DateTime<Utc>>,
    /// Filter by user id.
    pub user_id: Option<String>,
    /// Filter by operation type.
    pub operation: Option<AuditOperation>,
    /// Filter by target (substring match).
    pub target: Option<String>,
    /// Filter by result.
    pub result: Option<AuditResult>,
    /// Page number (0-based).
    pub page: Option<u32>,
    /// Items per page.
    pub per_page: Option<u32>,
}

impl AuditQuery {
    /// Create a new empty query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set start time filter.
    pub fn start_time(mut self, t: DateTime<Utc>) -> Self {
        self.start_time = Some(t);
        self
    }

    /// Set end time filter.
    pub fn end_time(mut self, t: DateTime<Utc>) -> Self {
        self.end_time = Some(t);
        self
    }

    /// Set user_id filter.
    pub fn user_id(mut self, uid: &str) -> Self {
        self.user_id = Some(uid.to_string());
        self
    }

    /// Set operation filter.
    pub fn operation(mut self, op: AuditOperation) -> Self {
        self.operation = Some(op);
        self
    }

    /// Set target filter (substring).
    pub fn target(mut self, tgt: &str) -> Self {
        self.target = Some(tgt.to_string());
        self
    }

    /// Set result filter.
    pub fn result(mut self, r: AuditResult) -> Self {
        self.result = Some(r);
        self
    }

    /// Set pagination.
    pub fn paginate(mut self, page: u32, per_page: u32) -> Self {
        self.page = Some(page);
        self.per_page = Some(per_page);
        self
    }

    /// Does an entry match this query?
    fn matches(&self, e: &AuditEntry) -> bool {
        if let Some(st) = self.start_time {
            if e.timestamp < st {
                return false;
            }
        }
        if let Some(et) = self.end_time {
            if e.timestamp > et {
                return false;
            }
        }
        if let Some(ref uid) = self.user_id {
            if e.user_id != *uid {
                return false;
            }
        }
        if let Some(ref op) = self.operation {
            if e.operation != *op {
                return false;
            }
        }
        if let Some(ref tgt) = self.target {
            if !e.target.contains(tgt) {
                return false;
            }
        }
        if let Some(ref res) = self.result {
            if e.result != *res {
                return false;
            }
        }
        true
    }
}

/// Paginated result of an audit query.
#[derive(Debug, Clone, Serialize)]
pub struct AuditQueryResult<'a> {
    pub total: usize,
    pub page: u32,
    pub per_page: u32,
    pub items: Vec<&'a AuditEntry>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_and_len() {
        let mut log = AuditLog::default();
        assert!(log.is_empty());
        log.append("user1", AuditOperation::BackupStarted, "snap1", AuditResult::Success, HashMap::new()).unwrap();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_hash_chain_integrity() {
        let mut log = AuditLog::default();
        log.append("user1", AuditOperation::BackupStarted, "snap1", AuditResult::Success, HashMap::new()).unwrap();
        log.append("user1", AuditOperation::BackupCompleted, "snap1", AuditResult::Success, HashMap::new()).unwrap();
        log.append("user2", AuditOperation::DeleteSnapshot, "snap1", AuditResult::Success, HashMap::new()).unwrap();
        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_tamper_detection() {
        let mut log = AuditLog::default();
        log.append("user1", AuditOperation::BackupStarted, "snap1", AuditResult::Success, HashMap::new()).unwrap();
        // Tamper
        log.entries[0].user_id = "hacker".to_string();
        assert!(log.verify_chain().is_err());
    }

    #[test]
    fn test_query_by_user() {
        let mut log = AuditLog::default();
        log.append("alice", AuditOperation::BackupStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        log.append("bob", AuditOperation::BackupCompleted, "s2", AuditResult::Success, HashMap::new()).unwrap();
        log.append("alice", AuditOperation::RestoreStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();

        let q = AuditQuery::new().user_id("alice");
        let res = log.query(&q);
        assert_eq!(res.total, 2);
    }

    #[test]
    fn test_query_by_operation() {
        let mut log = AuditLog::default();
        log.append("alice", AuditOperation::BackupStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        log.append("alice", AuditOperation::BackupCompleted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        log.append("bob", AuditOperation::RestoreStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();

        let q = AuditQuery::new().operation(AuditOperation::BackupStarted);
        let res = log.query(&q);
        assert_eq!(res.total, 1);
    }

    #[test]
    fn test_query_pagination() {
        let mut log = AuditLog::default();
        for i in 0..10u64 {
            log.append("user", AuditOperation::BackupStarted, &format!("s{}", i), AuditResult::Success, HashMap::new()).unwrap();
        }
        let q = AuditQuery::new().paginate(0, 3);
        let res = log.query(&q);
        assert_eq!(res.total, 10);
        assert_eq!(res.items.len(), 3);
    }

    #[test]
    fn test_export_csv() {
        let mut log = AuditLog::default();
        log.append("user1", AuditOperation::BackupStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        let csv = log.export_csv(&AuditQuery::new()).unwrap();
        assert!(csv.starts_with("seq,timestamp"));
        assert!(csv.contains("user1"));
    }

    #[test]
    fn test_rotation() {
        let mut rot = RotationConfig::default();
        rot.max_entries = 5;
        let mut log = AuditLog::new(rot);
        for i in 0..10u64 {
            log.append("user", AuditOperation::Custom(format!("op{}", i)), &format!("t{}", i), AuditResult::Success, HashMap::new()).unwrap();
        }
        // After exceeding max_entries (5), half should be archived
        assert!(log.archived_segments() > 0);
        assert!(log.len() <= 5);
    }

    #[test]
    fn test_prev_hash_chain() {
        let mut log = AuditLog::default();
        let e1 = log.append("u1", AuditOperation::BackupStarted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        let e1_hash = e1.hash.clone();
        let e2 = log.append("u1", AuditOperation::BackupCompleted, "s1", AuditResult::Success, HashMap::new()).unwrap();
        assert_eq!(e2.prev_hash, e1_hash);
        // Genesis entry has empty prev_hash
        assert!(log.get(1).unwrap().prev_hash.is_empty());
    }
}
