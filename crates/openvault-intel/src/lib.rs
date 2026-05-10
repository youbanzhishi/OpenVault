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

pub mod classifier;
pub mod anomaly;
pub mod scheduler;

pub use classifier::{FileClassifier, ClassificationRule, FileCategory, BackupPriority};
pub use anomaly::{AnomalyPredictor, RiskLevel, RiskAssessment, CheckpointRecord};
pub use scheduler::{SmartScheduler, ScheduleWindow, ScheduleEntry, NetworkCondition};
