//! OpenVault Intelligence Layer
//!
//! AI-powered features for smart backup management:
//!
//! - **FileClassifier**: Automatic file importance classification based on
//!   path/name/extension patterns with support for custom regex rules.
//! - **AnomalyPredictor**: Analyzes verification history trends to predict
//!   backend performance issues and fault risks.
//! - **SmartScheduler**: Optimizes backup scheduling based on network
//!   conditions, device availability, and priority queues.

pub mod anomaly;
pub mod classifier;
pub mod scheduler;

pub use anomaly::{AnomalyPredictor, CheckpointRecord, RiskAssessment, RiskLevel};
pub use classifier::{BackupPriority, ClassificationRule, FileCategory, FileClassifier};
pub use scheduler::{NetworkCondition, ScheduleEntry, ScheduleWindow, SmartScheduler};
