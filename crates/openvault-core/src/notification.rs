//! Notification service with dedup, multiple channels, and history.
//!
//! # Phase 8 Features
//!
//! - **NotificationService** — Dispatch notifications via Webhook/Email(stub)/InApp
//! - **NotificationRule** — Per-type and severity routing rules
//! - **NotificationDedup** — Prevent duplicate notifications within a time window

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::VaultResult;

// ============================================================================
// Notification Types
// ============================================================================

/// Types of notifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    BackupCompleted,
    BackupFailed,
    RestoreCompleted,
    RestoreFailed,
    SelfHealTriggered,
    SelfHealCompleted,
    RiskWarning,
    ComplianceViolation,
    QuotaWarning,
    Custom(String),
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationType::Custom(s) => write!(f, "custom:{}", s),
            other => write!(f, "{:?}", other),
        }
    }
}

/// Severity level of a notification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
            Severity::Critical => write!(f, "critical"),
        }
    }
}

/// Delivery channel for notifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Webhook,
    Email, // stub
    InApp,
}

// ============================================================================
// Notification
// ============================================================================

/// A single notification instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub notification_type: NotificationType,
    pub severity: Severity,
    pub title: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub tenant_id: Option<String>,
    pub metadata: HashMap<String, String>,
    pub read: bool,
}

// ============================================================================
// Notification Rule
// ============================================================================

/// A rule that controls which notifications are sent to which channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRule {
    pub rule_id: String,
    pub name: String,
    /// Which notification types this rule applies to (empty = all).
    pub notification_types: Vec<NotificationType>,
    /// Minimum severity to trigger.
    pub min_severity: Severity,
    /// Channels to send to.
    pub channels: Vec<Channel>,
    /// Deduplication window in minutes (0 = no dedup).
    pub dedup_minutes: u32,
    /// Whether the rule is enabled.
    pub enabled: bool,
    /// Webhook URL (if Channel::Webhook is configured).
    pub webhook_url: Option<String>,
}

impl Default for NotificationRule {
    fn default() -> Self {
        Self {
            rule_id: "default".to_string(),
            name: "Default Rule".to_string(),
            notification_types: vec![],
            min_severity: Severity::Info,
            channels: vec![Channel::InApp],
            dedup_minutes: 5,
            enabled: true,
            webhook_url: None,
        }
    }
}

impl NotificationRule {
    /// Does a notification type + severity match this rule?
    pub fn matches(&self, ntype: &NotificationType, severity: &Severity) -> bool {
        if !self.enabled {
            return false;
        }
        if severity < &self.min_severity {
            return false;
        }
        if self.notification_types.is_empty() {
            return true;
        }
        self.notification_types.contains(ntype)
    }
}

// ============================================================================
// Notification Service
// ============================================================================

/// In-memory notification service with dedup and history.
#[derive(Debug, Clone)]
pub struct NotificationSvc {
    rules: Vec<NotificationRule>,
    history: Vec<Notification>,
    /// Dedup key → last sent timestamp
    dedup: HashMap<String, DateTime<Utc>>,
}

impl Default for NotificationSvc {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationSvc {
    /// Create a new notification service.
    pub fn new() -> Self {
        Self {
            rules: vec![NotificationRule::default()],
            history: Vec::new(),
            dedup: HashMap::new(),
        }
    }

    /// Add or update a notification rule.
    pub fn set_rule(&mut self, rule: NotificationRule) {
        if let Some(existing) = self.rules.iter_mut().find(|r| r.rule_id == rule.rule_id) {
            *existing = rule;
        } else {
            self.rules.push(rule);
        }
    }

    /// Remove a rule by id.
    pub fn remove_rule(&mut self, rule_id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.rule_id != rule_id);
        self.rules.len() < before
    }

    /// Get all rules.
    pub fn rules(&self) -> &[NotificationRule] {
        &self.rules
    }

    /// Send a notification.
    pub fn send(
        &mut self,
        ntype: NotificationType,
        severity: Severity,
        title: &str,
        message: &str,
        tenant_id: Option<&str>,
        metadata: HashMap<String, String>,
    ) -> VaultResult<Vec<Channel>> {
        let mut sent_channels = Vec::new();

        // Check dedup
        let dedup_key = format!("{}:{}:{:?}", title, ntype, tenant_id.unwrap_or("global"));

        for rule in &self.rules {
            if !rule.matches(&ntype, &severity) {
                continue;
            }

            // Dedup check
            if rule.dedup_minutes > 0 {
                if let Some(last_sent) = self.dedup.get(&dedup_key) {
                    let elapsed = Utc::now().signed_duration_since(*last_sent);
                    if elapsed < Duration::minutes(rule.dedup_minutes as i64) {
                        continue; // Skip, within dedup window
                    }
                }
            }

            // Record in history (InApp is always "sent")
            let notification = Notification {
                id: uuid::Uuid::new_v4().to_string(),
                notification_type: ntype.clone(),
                severity: severity.clone(),
                title: title.to_string(),
                message: message.to_string(),
                timestamp: Utc::now(),
                tenant_id: tenant_id.map(|s| s.to_string()),
                metadata: metadata.clone(),
                read: false,
            };

            // For Webhook, we'd fire an HTTP request (stub here)
            // For Email, stub
            // For InApp, just store in history

            self.history.push(notification);
            self.dedup.insert(dedup_key.clone(), Utc::now());

            for ch in &rule.channels {
                if !sent_channels.contains(ch) {
                    sent_channels.push(ch.clone());
                }
            }
        }

        Ok(sent_channels)
    }

    /// Get notification history.
    pub fn history(&self) -> &[Notification] {
        &self.history
    }

    /// Get unread notifications.
    pub fn unread(&self) -> Vec<&Notification> {
        self.history.iter().filter(|n| !n.read).collect()
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, notification_id: &str) -> bool {
        if let Some(n) = self.history.iter_mut().find(|n| n.id == notification_id) {
            n.read = true;
            true
        } else {
            false
        }
    }

    /// Mark all as read.
    pub fn mark_all_read(&mut self) {
        for n in &mut self.history {
            n.read = true;
        }
    }

    /// Clear notification history.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.dedup.clear();
    }

    /// Count notifications by type.
    pub fn count_by_type(&self, ntype: &NotificationType) -> usize {
        self.history
            .iter()
            .filter(|n| n.notification_type == *ntype)
            .count()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_notification() {
        let mut svc = NotificationSvc::new();
        let channels = svc
            .send(
                NotificationType::BackupCompleted,
                Severity::Info,
                "Backup done",
                "Backup completed successfully",
                None,
                HashMap::new(),
            )
            .unwrap();
        assert!(!channels.is_empty());
        assert_eq!(svc.history().len(), 1);
    }

    #[test]
    fn test_dedup() {
        let mut svc = NotificationSvc::new();
        // Send same notification twice quickly
        svc.send(
            NotificationType::BackupFailed,
            Severity::Error,
            "Backup fail",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        svc.send(
            NotificationType::BackupFailed,
            Severity::Error,
            "Backup fail",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        // Second should be deduped
        assert_eq!(svc.history().len(), 1);
    }

    #[test]
    fn test_severity_filter() {
        let mut svc = NotificationSvc::new();
        // Override rule to require Warning
        svc.set_rule(NotificationRule {
            min_severity: Severity::Warning,
            ..NotificationRule::default()
        });
        let channels = svc
            .send(
                NotificationType::BackupCompleted,
                Severity::Info,
                "Low sev",
                "msg",
                None,
                HashMap::new(),
            )
            .unwrap();
        assert!(channels.is_empty());
    }

    #[test]
    fn test_type_filter() {
        let mut svc = NotificationSvc::new();
        svc.set_rule(NotificationRule {
            notification_types: vec![NotificationType::ComplianceViolation],
            ..NotificationRule::default()
        });
        // Compliance violation should go through
        let ch = svc
            .send(
                NotificationType::ComplianceViolation,
                Severity::Error,
                "Violation",
                "msg",
                None,
                HashMap::new(),
            )
            .unwrap();
        assert!(!ch.is_empty());
        // Backup completed should not match
        let ch2 = svc
            .send(
                NotificationType::BackupCompleted,
                Severity::Error,
                "Backup",
                "msg",
                None,
                HashMap::new(),
            )
            .unwrap();
        assert!(ch2.is_empty());
    }

    #[test]
    fn test_mark_read() {
        let mut svc = NotificationSvc::new();
        svc.send(
            NotificationType::BackupCompleted,
            Severity::Info,
            "Test",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        let id = svc.history()[0].id.clone();
        assert!(!svc.history()[0].read);
        assert!(svc.mark_read(&id));
        assert!(svc.history()[0].read);
    }

    #[test]
    fn test_unread() {
        let mut svc = NotificationSvc::new();
        svc.send(
            NotificationType::BackupCompleted,
            Severity::Info,
            "A",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        svc.send(
            NotificationType::BackupFailed,
            Severity::Error,
            "B",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        assert_eq!(svc.unread().len(), 2);
        svc.mark_all_read();
        assert_eq!(svc.unread().len(), 0);
    }

    #[test]
    fn test_count_by_type() {
        let mut svc = NotificationSvc::new();
        svc.send(
            NotificationType::BackupCompleted,
            Severity::Info,
            "A",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        svc.send(
            NotificationType::BackupCompleted,
            Severity::Info,
            "B",
            "msg",
            None,
            HashMap::new(),
        )
        .unwrap();
        // The second "B" has different title but same type — dedup key is different (includes title)
        // Actually dedup key includes title, so they won't be deduped
        assert_eq!(svc.count_by_type(&NotificationType::BackupCompleted), 2);
    }

    #[test]
    fn test_remove_rule() {
        let mut svc = NotificationSvc::new();
        let rule_id = svc.rules()[0].rule_id.clone();
        assert!(svc.remove_rule(&rule_id));
        assert!(svc.rules().is_empty());
    }
}
