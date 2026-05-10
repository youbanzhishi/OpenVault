//! Anomaly prediction for backup health.
//!
//! Analyzes historical checkpoint/verification records to detect trends
//! that may indicate impending backend failures or performance degradation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Risk level for an anomaly assessment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "Low"),
            RiskLevel::Medium => write!(f, "Medium"),
            RiskLevel::High => write!(f, "High"),
            RiskLevel::Critical => write!(f, "Critical"),
        }
    }
}

/// A single checkpoint/verification record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    /// Timestamp of the checkpoint.
    pub timestamp: DateTime<Utc>,
    /// Duration of the verification in seconds.
    pub duration_secs: f64,
    /// Number of files checked.
    pub files_checked: u64,
    /// Number of errors encountered.
    pub errors: u64,
    /// Backend identifier (e.g., "s3", "local").
    pub backend: String,
    /// Whether the checkpoint passed.
    pub passed: bool,
}

impl CheckpointRecord {
    /// Create a new checkpoint record.
    pub fn new(
        duration_secs: f64,
        files_checked: u64,
        errors: u64,
        backend: impl Into<String>,
        passed: bool,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            duration_secs,
            files_checked,
            errors,
            backend: backend.into(),
            passed,
        }
    }

    /// Error rate for this checkpoint.
    pub fn error_rate(&self) -> f64 {
        if self.files_checked == 0 {
            return 0.0;
        }
        self.errors as f64 / self.files_checked as f64
    }

    /// Average duration per file.
    pub fn avg_duration_per_file(&self) -> f64 {
        if self.files_checked == 0 {
            return 0.0;
        }
        self.duration_secs / self.files_checked as f64
    }
}

/// Full risk assessment result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Overall risk level.
    pub risk_level: RiskLevel,
    /// Duration trend (seconds per checkpoint, positive = slowing down).
    pub duration_trend: f64,
    /// Error rate trend (per checkpoint, positive = more errors).
    pub error_rate_trend: f64,
    /// Recent average error rate.
    pub recent_error_rate: f64,
    /// Recent average duration per file (seconds).
    pub recent_avg_duration: f64,
    /// Number of consecutive failures.
    pub consecutive_failures: u64,
    /// Human-readable recommendation.
    pub recommendation: String,
    /// Breakdown of individual risk factors.
    pub factors: Vec<RiskFactor>,
}

/// A single risk factor contributing to the assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    /// Name of the risk factor.
    pub name: String,
    /// Contribution level.
    pub severity: RiskLevel,
    /// Description.
    pub description: String,
}

/// The anomaly predictor engine.
pub struct AnomalyPredictor {
    /// Number of recent records to consider for trend analysis.
    window_size: usize,
    /// Duration increase threshold for Medium risk (ratio, e.g., 0.3 = 30% slower).
    duration_medium_threshold: f64,
    /// Duration increase threshold for High risk.
    duration_high_threshold: f64,
    /// Error rate threshold for Medium risk.
    error_rate_medium_threshold: f64,
    /// Error rate threshold for High risk.
    error_rate_high_threshold: f64,
    /// Consecutive failure count for Critical risk.
    critical_failure_count: u64,
}

impl Default for AnomalyPredictor {
    fn default() -> Self {
        Self::new()
    }
}

impl AnomalyPredictor {
    /// Create a new predictor with default thresholds.
    pub fn new() -> Self {
        Self {
            window_size: 10,
            duration_medium_threshold: 0.3,
            duration_high_threshold: 0.8,
            error_rate_medium_threshold: 0.05,
            error_rate_high_threshold: 0.15,
            critical_failure_count: 3,
        }
    }

    /// Create a predictor with custom thresholds.
    pub fn with_thresholds(
        window_size: usize,
        duration_medium: f64,
        duration_high: f64,
        error_rate_medium: f64,
        error_rate_high: f64,
        critical_failures: u64,
    ) -> Self {
        Self {
            window_size,
            duration_medium_threshold: duration_medium,
            duration_high_threshold: duration_high,
            error_rate_medium_threshold: error_rate_medium,
            error_rate_high_threshold: error_rate_high,
            critical_failure_count: critical_failures,
        }
    }

    /// Analyze a sequence of checkpoint records and produce a risk assessment.
    pub fn assess(&self, records: &[CheckpointRecord]) -> RiskAssessment {
        let mut factors = Vec::new();

        if records.is_empty() {
            return RiskAssessment {
                risk_level: RiskLevel::Low,
                duration_trend: 0.0,
                error_rate_trend: 0.0,
                recent_error_rate: 0.0,
                recent_avg_duration: 0.0,
                consecutive_failures: 0,
                recommendation: "No checkpoint data available. Run verification to establish baseline.".to_string(),
                factors,
            };
        }

        // Split records into two halves for trend analysis
        let mid = records.len() / 2;
        let (older, newer) = if records.len() >= 2 {
            (&records[..mid], &records[mid..])
        } else {
            (&records[..0], records)
        };

        // Duration trend
        let older_avg_dur = if older.is_empty() {
            0.0
        } else {
            older.iter().map(|r| r.avg_duration_per_file()).sum::<f64>() / older.len() as f64
        };
        let newer_avg_dur = if newer.is_empty() {
            0.0
        } else {
            newer.iter().map(|r| r.avg_duration_per_file()).sum::<f64>() / newer.len() as f64
        };

        let duration_trend = if older_avg_dur > 0.0 {
            (newer_avg_dur - older_avg_dur) / older_avg_dur
        } else {
            0.0
        };

        // Duration risk factor
        if duration_trend > self.duration_high_threshold {
            factors.push(RiskFactor {
                name: "duration_spike".to_string(),
                severity: RiskLevel::High,
                description: format!(
                    "Verification duration increased by {:.1}% — backend may be degraded",
                    duration_trend * 100.0
                ),
            });
        } else if duration_trend > self.duration_medium_threshold {
            factors.push(RiskFactor {
                name: "duration_increase".to_string(),
                severity: RiskLevel::Medium,
                description: format!(
                    "Verification duration increased by {:.1}% — monitor for further degradation",
                    duration_trend * 100.0
                ),
            });
        }

        // Error rate trend
        let older_avg_err = if older.is_empty() {
            0.0
        } else {
            older.iter().map(|r| r.error_rate()).sum::<f64>() / older.len() as f64
        };
        let newer_avg_err = if newer.is_empty() {
            0.0
        } else {
            newer.iter().map(|r| r.error_rate()).sum::<f64>() / newer.len() as f64
        };
        let error_rate_trend = newer_avg_err - older_avg_err;

        if newer_avg_err > self.error_rate_high_threshold {
            factors.push(RiskFactor {
                name: "high_error_rate".to_string(),
                severity: RiskLevel::High,
                description: format!(
                    "Error rate {:.1}% exceeds high threshold — investigate backend health",
                    newer_avg_err * 100.0
                ),
            });
        } else if newer_avg_err > self.error_rate_medium_threshold {
            factors.push(RiskFactor {
                name: "elevated_error_rate".to_string(),
                severity: RiskLevel::Medium,
                description: format!(
                    "Error rate {:.1}% above normal — keep monitoring",
                    newer_avg_err * 100.0
                ),
            });
        }

        // Consecutive failures
        let consecutive_failures = records
            .iter()
            .rev()
            .take_while(|r| !r.passed)
            .count() as u64;

        if consecutive_failures >= self.critical_failure_count {
            factors.push(RiskFactor {
                name: "consecutive_failures".to_string(),
                severity: RiskLevel::Critical,
                description: format!(
                    "{} consecutive verification failures — backend may be down",
                    consecutive_failures
                ),
            });
        } else if consecutive_failures > 0 {
            factors.push(RiskFactor {
                name: "recent_failure".to_string(),
                severity: RiskLevel::Medium,
                description: format!(
                    "{} recent verification failure(s)",
                    consecutive_failures
                ),
            });
        }

        // Recent stats
        let recent: Vec<_> = records.iter().rev().take(self.window_size).collect();
        let recent_error_rate = if recent.is_empty() {
            0.0
        } else {
            recent.iter().map(|r| r.error_rate()).sum::<f64>() / recent.len() as f64
        };
        let recent_avg_duration = if recent.is_empty() {
            0.0
        } else {
            recent.iter().map(|r| r.avg_duration_per_file()).sum::<f64>() / recent.len() as f64
        };

        // Overall risk = max of all factor severities
        let risk_level = factors
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(RiskLevel::Low);

        let recommendation = match risk_level {
            RiskLevel::Low => "All systems healthy. Continue normal operations.".to_string(),
            RiskLevel::Medium => "Some concerning trends detected. Increase monitoring frequency and review recent logs.".to_string(),
            RiskLevel::High => "Significant degradation detected. Consider switching to a backup backend and investigate root cause.".to_string(),
            RiskLevel::Critical => "Backend may be failing. Switch to alternate storage immediately and perform full integrity check.".to_string(),
        };

        RiskAssessment {
            risk_level,
            duration_trend,
            error_rate_trend,
            recent_error_rate,
            recent_avg_duration,
            consecutive_failures,
            recommendation,
            factors,
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
    fn test_empty_records_low_risk() {
        let predictor = AnomalyPredictor::new();
        let assessment = predictor.assess(&[]);
        assert_eq!(assessment.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_healthy_records_low_risk() {
        let predictor = AnomalyPredictor::new();
        let records: Vec<CheckpointRecord> = (0..10)
            .map(|_| CheckpointRecord::new(1.0, 1000, 0, "local", true))
            .collect();
        let assessment = predictor.assess(&records);
        assert_eq!(assessment.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_consecutive_failures_critical() {
        let predictor = AnomalyPredictor::new();
        let records: Vec<CheckpointRecord> = (0..5)
            .map(|_| CheckpointRecord::new(1.0, 1000, 50, "s3", false))
            .collect();
        let assessment = predictor.assess(&records);
        assert!(assessment.risk_level >= RiskLevel::Critical);
        assert!(assessment.consecutive_failures >= 3);
    }

    #[test]
    fn test_duration_increase_medium_risk() {
        let predictor = AnomalyPredictor::new();
        let mut records = Vec::new();
        // Older records: fast
        for _ in 0..5 {
            records.push(CheckpointRecord::new(1.0, 1000, 0, "local", true));
        }
        // Newer records: slow (>30% increase → 1.0 → 1.5 = 50% increase)
        for _ in 0..5 {
            records.push(CheckpointRecord::new(1.5, 1000, 0, "local", true));
        }
        let assessment = predictor.assess(&records);
        assert!(assessment.duration_trend > 0.3);
        assert!(assessment.risk_level >= RiskLevel::Medium);
    }

    #[test]
    fn test_error_rate_high_risk() {
        let predictor = AnomalyPredictor::new();
        let mut records = Vec::new();
        // Older: no errors
        for _ in 0..5 {
            records.push(CheckpointRecord::new(1.0, 1000, 0, "s3", true));
        }
        // Newer: high error rate (200/1000 = 20%)
        for _ in 0..5 {
            records.push(CheckpointRecord::new(1.0, 1000, 200, "s3", false));
        }
        let assessment = predictor.assess(&records);
        assert!(assessment.risk_level >= RiskLevel::High);
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Critical > RiskLevel::High);
        assert!(RiskLevel::High > RiskLevel::Medium);
        assert!(RiskLevel::Medium > RiskLevel::Low);
    }

    #[test]
    fn test_checkpoint_record_error_rate() {
        let record = CheckpointRecord::new(10.0, 1000, 50, "local", true);
        let rate = record.error_rate();
        assert!((rate - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_checkpoint_record_avg_duration() {
        let record = CheckpointRecord::new(10.0, 100, 0, "local", true);
        let avg = record.avg_duration_per_file();
        assert!((avg - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_assessment_recommendation() {
        let predictor = AnomalyPredictor::new();
        let records: Vec<CheckpointRecord> = (0..5)
            .map(|_| CheckpointRecord::new(1.0, 1000, 0, "local", true))
            .collect();
        let assessment = predictor.assess(&records);
        assert!(!assessment.recommendation.is_empty());
    }

    #[test]
    fn test_risk_factor_present() {
        let predictor = AnomalyPredictor::new();
        let mut records = Vec::new();
        for _ in 0..3 {
            records.push(CheckpointRecord::new(1.0, 1000, 0, "local", true));
        }
        for _ in 0..3 {
            records.push(CheckpointRecord::new(1.0, 1000, 0, "local", false));
        }
        let assessment = predictor.assess(&records);
        assert!(!assessment.factors.is_empty());
    }
}
