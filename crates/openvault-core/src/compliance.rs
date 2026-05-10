//! Compliance rules, checking, and retention management.
//!
//! # Phase 8 Features
//!
//! - **ComplianceRule** — Data classification, retention, and geo-compliance rules
//! - **ComplianceChecker** — Verify backup policies and storage locations against rules
//! - **RetentionManager** — Automatic data expiry based on retention policies

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};




// ============================================================================
// Data Classification
// ============================================================================

/// Data classification levels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum DataClassification {
    Public,
    Internal,
    Confidential,
    Restricted,
}

impl DataClassification {
    /// Infer classification from a file path.
    pub fn from_path(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.contains("/secret/") || lower.contains("/classified/") || lower.contains("/restricted/") {
            DataClassification::Restricted
        } else if lower.contains("/confidential/") || lower.contains("/private/") {
            DataClassification::Confidential
        } else if lower.contains("/internal/") || lower.contains("/staff/") {
            DataClassification::Internal
        } else {
            DataClassification::Public
        }
    }
}

// ============================================================================
// Compliance Rule
// ============================================================================

/// Retention policy for a compliance rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    /// Keep all data indefinitely.
    KeepAll,
    /// Keep data for N years.
    KeepYears(u32),
    /// Keep data until a specific date.
    KeepUntil(String), // ISO 8601 date string
    /// Custom retention expression.
    Custom(String),
}

/// A single compliance rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceRule {
    pub rule_id: String,
    pub name: String,
    pub description: String,
    /// Data classification this rule applies to.
    pub classification: DataClassification,
    /// Required retention policy.
    pub retention: RetentionPolicy,
    /// Allowed geographic regions (empty = any).
    pub allowed_regions: Vec<String>,
    /// Path patterns this rule matches (glob-style).
    pub path_patterns: Vec<String>,
    /// Whether the rule is enabled.
    pub enabled: bool,
}

impl ComplianceRule {
    /// Check if a path matches this rule's patterns.
    pub fn matches_path(&self, path: &str) -> bool {
        if self.path_patterns.is_empty() {
            return true;
        }
        for pat in &self.path_patterns {
            if glob_match(pat, path) {
                return true;
            }
        }
        false
    }

    /// Check if a region is compliant with this rule.
    pub fn is_region_compliant(&self, region: &str) -> bool {
        if self.allowed_regions.is_empty() {
            return true;
        }
        self.allowed_regions.iter().any(|r| r.eq_ignore_ascii_case(region))
    }
}

/// Simple glob pattern match (supports * and **).
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    // Simple implementation: treat * as any substring
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut idx = 0;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if let Some(pos) = text[idx..].find(part) {
                idx += pos + part.len();
            } else {
                return false;
            }
            // First part must match from the start
            if i == 0 && !text.starts_with(part) {
                return false;
            }
        }
        // Last part must match the end if pattern doesn't end with *
        if !pattern.ends_with('*') {
            let last = parts.last().unwrap();
            if !text.ends_with(last) {
                return false;
            }
        }
        return true;
    }
    pattern == text
}

// ============================================================================
// Compliance Check Result
// ============================================================================

/// Severity of a compliance finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindingSeverity::Info => write!(f, "info"),
            FindingSeverity::Warning => write!(f, "warning"),
            FindingSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// A single compliance finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceFinding {
    pub rule_id: String,
    pub rule_name: String,
    pub severity: FindingSeverity,
    pub resource: String,
    pub message: String,
    pub detail: String,
}

/// Overall compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub timestamp: DateTime<Utc>,
    pub overall_status: ComplianceStatus,
    pub findings: Vec<ComplianceFinding>,
    pub rules_checked: u32,
    pub rules_passed: u32,
    pub rules_failed: u32,
}

impl ComplianceReport {
    /// Short summary string.
    pub fn summary(&self) -> String {
        format!(
            "Compliance {}: {}/{} rules passed, {} findings",
            match self.overall_status {
                ComplianceStatus::Pass => "✅ PASS",
                ComplianceStatus::Fail => "❌ FAIL",
            },
            self.rules_passed,
            self.rules_checked,
            self.findings.len()
        )
    }
}

/// Overall compliance pass/fail status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceStatus {
    Pass,
    Fail,
}

// ============================================================================
// Compliance Checker
// ============================================================================

/// Checks backup policies and storage against compliance rules.
#[derive(Debug, Clone, Default)]
pub struct ComplianceChecker {
    rules: Vec<ComplianceRule>,
}

impl ComplianceChecker {
    /// Create a new empty checker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a compliance rule.
    pub fn add_rule(&mut self, rule: ComplianceRule) {
        self.rules.push(rule);
    }

    /// Remove a rule by id.
    pub fn remove_rule(&mut self, rule_id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.rule_id != rule_id);
        self.rules.len() < before
    }

    /// Get all rules.
    pub fn rules(&self) -> &[ComplianceRule] {
        &self.rules
    }

    /// Check compliance for a given path and region.
    pub fn check(&self, path: &str, region: &str, policy_retention_days: u32) -> ComplianceReport {
        let mut findings = Vec::new();
        let mut rules_checked = 0u32;
        let mut rules_passed = 0u32;
        let mut rules_failed = 0u32;

        let inferred = DataClassification::from_path(path);

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }
            if !rule.matches_path(path) {
                continue;
            }

            rules_checked += 1;
            let mut passed = true;

            // Check classification match
            if rule.classification > inferred {
                // Path has lower classification than rule expects — not a violation
                rules_passed += 1;
                continue;
            }

            // Check region compliance
            if !rule.is_region_compliant(region) {
                findings.push(ComplianceFinding {
                    rule_id: rule.rule_id.clone(),
                    rule_name: rule.name.clone(),
                    severity: FindingSeverity::Critical,
                    resource: path.to_string(),
                    message: "Region not compliant".to_string(),
                    detail: format!(
                        "Data in region '{}' but rule '{}' requires one of: {}",
                        region,
                        rule.name,
                        rule.allowed_regions.join(", ")
                    ),
                });
                passed = false;
            }

            // Check retention compliance
            match &rule.retention {
                RetentionPolicy::KeepAll => {
                    // No minimum retention — always passes
                }
                RetentionPolicy::KeepYears(years) => {
                    let required_days = years * 365;
                    if policy_retention_days < required_days {
                        findings.push(ComplianceFinding {
                            rule_id: rule.rule_id.clone(),
                            rule_name: rule.name.clone(),
                            severity: FindingSeverity::Warning,
                            resource: path.to_string(),
                            message: "Retention too short".to_string(),
                            detail: format!(
                                "Policy retention {} days < required {} days ({} years)",
                                policy_retention_days, required_days, years
                            ),
                        });
                        passed = false;
                    }
                }
                RetentionPolicy::KeepUntil(date_str) => {
                    // Parse the date and check
                    if let Ok(until) = date_str.parse::<chrono::NaiveDate>() {
                        let now = Utc::now().date_naive();
                        if now > until {
                            findings.push(ComplianceFinding {
                                rule_id: rule.rule_id.clone(),
                                rule_name: rule.name.clone(),
                                severity: FindingSeverity::Info,
                                resource: path.to_string(),
                                message: "Retention period expired".to_string(),
                                detail: format!("KeepUntil date {} has passed", date_str),
                            });
                            // This is informational, not a failure
                        }
                    }
                }
                RetentionPolicy::Custom(_) => {
                    // Custom rules need manual review
                }
            }

            if passed {
                rules_passed += 1;
            } else {
                rules_failed += 1;
            }
        }

        // If no rules were checked, it's a pass
        let overall_status = if rules_failed > 0 {
            ComplianceStatus::Fail
        } else {
            ComplianceStatus::Pass
        };

        ComplianceReport {
            timestamp: Utc::now(),
            overall_status,
            findings,
            rules_checked,
            rules_passed,
            rules_failed,
        }
    }

    /// Auto-classify a path.
    pub fn classify_path(&self, path: &str) -> DataClassification {
        DataClassification::from_path(path)
    }
}

// ============================================================================
// Retention Manager
// ============================================================================

/// A record of a backup or snapshot that may be subject to retention rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionRecord {
    pub id: String,
    pub path: String,
    pub created_at: DateTime<Utc>,
    pub classification: DataClassification,
    pub size_bytes: u64,
}

/// Result of a retention cleanup sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionSweepResult {
    pub scanned: u32,
    pub expired: u32,
    pub retained: u32,
    pub freed_bytes: u64,
    pub details: Vec<String>,
}

/// Manages data retention based on compliance rules.
#[derive(Debug, Clone)]
pub struct RetentionManager {
    rules: Vec<ComplianceRule>,
}

impl RetentionManager {
    /// Create a new retention manager.
    pub fn new(rules: Vec<ComplianceRule>) -> Self {
        Self { rules }
    }

    /// Determine if a record has expired according to applicable rules.
    pub fn is_expired(&self, record: &RetentionRecord) -> bool {
        let applicable: Vec<&ComplianceRule> = self
            .rules
            .iter()
            .filter(|r| r.enabled && r.matches_path(&record.path))
            .collect();

        if applicable.is_empty() {
            return false; // No rules = keep
        }

        let now = Utc::now();
        for rule in &applicable {
            match &rule.retention {
                RetentionPolicy::KeepAll => return false,
                RetentionPolicy::KeepYears(years) => {
                    let keep_until = record.created_at + chrono::Duration::days((*years as i64) * 365);
                    if now < keep_until {
                        return false;
                    }
                }
                RetentionPolicy::KeepUntil(date_str) => {
                    if let Ok(until) = date_str.parse::<chrono::NaiveDate>() {
                        let keep_until = until.and_hms_opt(23, 59, 59).unwrap();
                        let keep_until_utc = DateTime::<Utc>::from_naive_utc_and_offset(keep_until, Utc);
                        if now < keep_until_utc {
                            return false;
                        }
                    }
                }
                RetentionPolicy::Custom(_) => return false, // Don't auto-expire custom rules
            }
        }
        true
    }

    /// Run a sweep over records, returning which ones have expired.
    pub fn sweep(&self, records: &[RetentionRecord]) -> RetentionSweepResult {
        let mut expired = Vec::new();
        let mut retained = 0u32;
        let mut freed_bytes = 0u64;
        let mut details = Vec::new();

        for record in records {
            if self.is_expired(record) {
                freed_bytes += record.size_bytes;
                details.push(format!(
                    "EXPIRED: {} ({}, {}) — {} bytes",
                    record.id, record.path, record.created_at.format("%Y-%m-%d"), record.size_bytes
                ));
                expired.push(record.id.clone());
            } else {
                retained += 1;
            }
        }

        RetentionSweepResult {
            scanned: records.len() as u32,
            expired: expired.len() as u32,
            retained,
            freed_bytes,
            details,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_classification_from_path() {
        assert_eq!(DataClassification::from_path("/public/readme.md"), DataClassification::Public);
        assert_eq!(DataClassification::from_path("/internal/docs"), DataClassification::Internal);
        assert_eq!(DataClassification::from_path("/confidential/secret.pdf"), DataClassification::Confidential);
        assert_eq!(DataClassification::from_path("/restricted/nuclear.doc"), DataClassification::Restricted);
    }

    #[test]
    fn test_compliance_rule_matches_path() {
        let rule = ComplianceRule {
            rule_id: "r1".into(),
            name: "test".into(),
            description: "test".into(),
            classification: DataClassification::Confidential,
            retention: RetentionPolicy::KeepYears(7),
            allowed_regions: vec!["EU".into()],
            path_patterns: vec!["**/confidential/**".into()],
            enabled: true,
        };
        assert!(rule.matches_path("/data/confidential/report.pdf"));
        assert!(!rule.matches_path("/data/public/readme.md"));
    }

    #[test]
    fn test_region_compliance() {
        let rule = ComplianceRule {
            rule_id: "r1".into(),
            name: "GDPR".into(),
            description: "EU data".into(),
            classification: DataClassification::Restricted,
            retention: RetentionPolicy::KeepYears(10),
            allowed_regions: vec!["EU".into()],
            path_patterns: vec![],
            enabled: true,
        };
        assert!(rule.is_region_compliant("EU"));
        assert!(!rule.is_region_compliant("US"));
    }

    #[test]
    fn test_compliance_check_passes() {
        let mut checker = ComplianceChecker::new();
        checker.add_rule(ComplianceRule {
            rule_id: "r1".into(),
            name: "GDPR".into(),
            description: "EU data".into(),
            classification: DataClassification::Confidential,
            retention: RetentionPolicy::KeepYears(7),
            allowed_regions: vec!["EU".into()],
            path_patterns: vec!["**/confidential/**".into()],
            enabled: true,
        });
        let report = checker.check("/data/confidential/report.pdf", "EU", 365 * 7 + 1);
        assert_eq!(report.overall_status, ComplianceStatus::Pass);
    }

    #[test]
    fn test_compliance_check_fails_region() {
        let mut checker = ComplianceChecker::new();
        checker.add_rule(ComplianceRule {
            rule_id: "r1".into(),
            name: "GDPR".into(),
            description: "EU data".into(),
            classification: DataClassification::Confidential,
            retention: RetentionPolicy::KeepYears(7),
            allowed_regions: vec!["EU".into()],
            path_patterns: vec!["**/confidential/**".into()],
            enabled: true,
        });
        let report = checker.check("/data/confidential/report.pdf", "US", 365 * 8);
        assert_eq!(report.overall_status, ComplianceStatus::Fail);
        assert!(report.findings.iter().any(|f| f.message.contains("Region")));
    }

    #[test]
    fn test_compliance_check_fails_retention() {
        let mut checker = ComplianceChecker::new();
        checker.add_rule(ComplianceRule {
            rule_id: "r1".into(),
            name: "7yr retention".into(),
            description: "Must keep 7 years".into(),
            classification: DataClassification::Confidential,
            retention: RetentionPolicy::KeepYears(7),
            allowed_regions: vec![],
            path_patterns: vec!["**/confidential/**".into()],
            enabled: true,
        });
        let report = checker.check("/data/confidential/report.pdf", "US", 30);
        assert_eq!(report.overall_status, ComplianceStatus::Fail);
        assert!(report.findings.iter().any(|f| f.message.contains("Retention")));
    }

    #[test]
    fn test_retention_manager_expired() {
        let rules = vec![ComplianceRule {
            rule_id: "r1".into(),
            name: "short retention".into(),
            description: "test".into(),
            classification: DataClassification::Public,
            retention: RetentionPolicy::KeepYears(0), // 0 years = expire immediately
            allowed_regions: vec![],
            path_patterns: vec![],
            enabled: true,
        }];
        let mgr = RetentionManager::new(rules);
        let record = RetentionRecord {
            id: "snap1".into(),
            path: "/data/file.txt".into(),
            created_at: Utc::now() - chrono::Duration::days(1),
            classification: DataClassification::Public,
            size_bytes: 1024,
        };
        assert!(mgr.is_expired(&record));
    }

    #[test]
    fn test_retention_manager_sweep() {
        let rules = vec![ComplianceRule {
            rule_id: "r1".into(),
            name: "keep 1 year".into(),
            description: "test".into(),
            classification: DataClassification::Public,
            retention: RetentionPolicy::KeepYears(1),
            allowed_regions: vec![],
            path_patterns: vec![],
            enabled: true,
        }];
        let mgr = RetentionManager::new(rules);
        let records = vec![
            RetentionRecord {
                id: "snap1".into(),
                path: "/data/file.txt".into(),
                created_at: Utc::now() - chrono::Duration::days(400),
                classification: DataClassification::Public,
                size_bytes: 1024,
            },
            RetentionRecord {
                id: "snap2".into(),
                path: "/data/file2.txt".into(),
                created_at: Utc::now(),
                classification: DataClassification::Public,
                size_bytes: 2048,
            },
        ];
        let result = mgr.sweep(&records);
        assert_eq!(result.expired, 1);
        assert_eq!(result.retained, 1);
        assert_eq!(result.freed_bytes, 1024);
    }

    #[test]
    fn test_report_summary() {
        let report = ComplianceReport {
            timestamp: Utc::now(),
            overall_status: ComplianceStatus::Pass,
            findings: vec![],
            rules_checked: 5,
            rules_passed: 5,
            rules_failed: 0,
        };
        assert!(report.summary().contains("PASS"));
    }
}
