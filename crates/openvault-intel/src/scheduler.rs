//! Smart backup scheduler.
//!
//! Optimizes backup scheduling based on network conditions, device online
//! status, priority queues, and configurable backup windows.

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// Current network condition at a device.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NetworkCondition {
    /// Available bandwidth in Mbps.
    pub bandwidth_mbps: f64,
    /// Latency in milliseconds.
    pub latency_ms: f64,
    /// Packet loss ratio (0.0–1.0).
    pub packet_loss: f64,
}

impl Default for NetworkCondition {
    fn default() -> Self {
        Self {
            bandwidth_mbps: 100.0,
            latency_ms: 10.0,
            packet_loss: 0.0,
        }
    }
}

impl NetworkCondition {
    /// Create an excellent network condition.
    pub fn excellent() -> Self {
        Self {
            bandwidth_mbps: 1000.0,
            latency_ms: 1.0,
            packet_loss: 0.0,
        }
    }

    /// Create a poor network condition.
    pub fn poor() -> Self {
        Self {
            bandwidth_mbps: 5.0,
            latency_ms: 200.0,
            packet_loss: 0.05,
        }
    }

    /// Score from 0–100 indicating network quality (higher is better).
    pub fn quality_score(&self) -> f64 {
        let bw_score = (self.bandwidth_mbps / 100.0).min(1.0) * 40.0;
        let lat_score = (1.0 - (self.latency_ms / 500.0).min(1.0)) * 30.0;
        let loss_score = (1.0 - self.packet_loss) * 30.0;
        bw_score + lat_score + loss_score
    }
}

/// A time window during which backups are allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleWindow {
    /// Start hour (0–23) in local time.
    pub start_hour: u32,
    /// End hour (0–23) in local time.
    pub end_hour: u32,
    /// Days of the week (0 = Monday, 6 = Sunday). Empty = every day.
    pub days: Vec<u32>,
}

impl Default for ScheduleWindow {
    fn default() -> Self {
        Self {
            start_hour: 0,
            end_hour: 24,
            days: Vec::new(),
        }
    }
}

impl ScheduleWindow {
    /// Create a new schedule window.
    pub fn new(start_hour: u32, end_hour: u32) -> Self {
        Self {
            start_hour,
            end_hour,
            days: Vec::new(),
        }
    }

    /// Create a schedule window for specific days.
    pub fn with_days(start_hour: u32, end_hour: u32, days: Vec<u32>) -> Self {
        Self {
            start_hour,
            end_hour,
            days,
        }
    }

    /// Check if a given datetime falls within this window.
    pub fn is_within(&self, dt: &DateTime<Utc>) -> bool {
        let hour = dt.hour();
        // Check day of week if specified
        if !self.days.is_empty() {
            let weekday = dt.weekday().num_days_from_monday();
            if !self.days.contains(&weekday) {
                return false;
            }
        }
        if self.start_hour <= self.end_hour {
            hour >= self.start_hour && hour < self.end_hour
        } else {
            // Wraps midnight, e.g. 22:00–06:00
            hour >= self.start_hour || hour < self.end_hour
        }
    }
}

/// A single backup task in the scheduling queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    /// Unique task identifier.
    pub id: String,
    /// Device to back up from.
    pub device_id: String,
    /// Source path on the device.
    pub source_path: String,
    /// Priority (higher = more urgent).
    pub priority: u32,
    /// Estimated size in bytes.
    pub estimated_size: u64,
    /// Whether the device is currently online.
    pub device_online: bool,
    /// Current network condition at the device.
    pub network: NetworkCondition,
    /// Whether this task has been scheduled.
    pub scheduled: bool,
}

impl PartialEq for ScheduleEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ScheduleEntry {}

impl PartialOrd for ScheduleEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduleEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first
        other.priority.cmp(&self.priority)
            .then_with(|| other.network.quality_score().partial_cmp(&self.network.quality_score()).unwrap_or(Ordering::Equal))
    }
}

/// The smart scheduler engine.
pub struct SmartScheduler {
    /// Allowed backup window.
    pub window: ScheduleWindow,
    /// Pending tasks (priority queue).
    queue: BinaryHeap<ScheduleEntry>,
    /// Minimum network quality score to schedule a backup.
    min_network_score: f64,
}

impl Default for SmartScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartScheduler {
    /// Create a new scheduler with default settings.
    pub fn new() -> Self {
        Self {
            window: ScheduleWindow::default(),
            queue: BinaryHeap::new(),
            min_network_score: 30.0,
        }
    }

    /// Create a scheduler with a specific backup window.
    pub fn with_window(window: ScheduleWindow) -> Self {
        Self {
            window,
            queue: BinaryHeap::new(),
            min_network_score: 30.0,
        }
    }

    /// Set minimum network quality score for scheduling.
    pub fn with_min_network_score(mut self, score: f64) -> Self {
        self.min_network_score = score;
        self
    }

    /// Add a task to the scheduling queue.
    pub fn enqueue(&mut self, entry: ScheduleEntry) {
        self.queue.push(entry);
    }

    /// Remove a task by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let original_len = self.queue.len();
        let remaining: BinaryHeap<ScheduleEntry> = self.queue
            .drain()
            .filter(|e| e.id != id)
            .collect();
        self.queue = remaining;
        self.queue.len() < original_len
    }
    /// Get the number of pending tasks.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Peek at the highest-priority task without removing it.
    pub fn peek(&self) -> Option<&ScheduleEntry> {
        self.queue.peek()
    }

    /// Schedule the next eligible task.
    ///
    /// Returns the next task that:
    /// 1. Falls within the backup window
    /// 2. Has its device online
    /// 3. Meets minimum network quality
    pub fn schedule_next(&mut self) -> Option<ScheduleEntry> {
        let now = Utc::now();
        let mut skipped = Vec::new();
        let mut result = None;

        while let Some(entry) = self.queue.pop() {
            if !self.window.is_within(&now) {
                // Outside window — put back and stop
                skipped.push(entry);
                break;
            }
            if !entry.device_online {
                skipped.push(entry);
                continue;
            }
            if entry.network.quality_score() < self.min_network_score {
                skipped.push(entry);
                continue;
            }
            // Eligible!
            result = Some(entry);
            break;
        }

        // Put skipped entries back
        for entry in skipped {
            self.queue.push(entry);
        }

        result
    }

    /// Schedule all currently eligible tasks.
    pub fn schedule_all_eligible(&mut self) -> Vec<ScheduleEntry> {
        let mut results = Vec::new();
        while let Some(entry) = self.schedule_next() {
            results.push(entry);
        }
        results
    }

    /// Get scheduling suggestions for the current state.
    pub fn suggestions(&self) -> Vec<String> {
        let mut tips = Vec::new();

        let now = Utc::now();
        if !self.window.is_within(&now) {
            tips.push(format!(
                "Outside backup window ({}:00–{}:00). Next window starts at {}:00.",
                self.window.start_hour, self.window.end_hour, self.window.start_hour
            ));
        }

        let offline_count = self.queue.iter().filter(|e| !e.device_online).count();
        if offline_count > 0 {
            tips.push(format!(
                "{} task(s) waiting for devices to come online.",
                offline_count
            ));
        }

        let poor_network = self.queue.iter().filter(|e| e.network.quality_score() < self.min_network_score).count();
        if poor_network > 0 {
            tips.push(format!(
                "{} task(s) delayed due to poor network conditions.",
                poor_network
            ));
        }

        let high_priority = self.queue.iter().filter(|e| e.priority >= 3).count();
        if high_priority > 0 {
            tips.push(format!(
                "{} high-priority task(s) in queue.",
                high_priority
            ));
        }

        if tips.is_empty() {
            tips.push("All systems ready. No scheduling issues detected.".to_string());
        }

        tips
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_quality_score() {
        let excellent = NetworkCondition::excellent();
        assert!(excellent.quality_score() > 80.0);

        let poor = NetworkCondition::poor();
        assert!(poor.quality_score() < 50.0);
    }

    #[test]
    fn test_schedule_window_within() {
        let window = ScheduleWindow::new(22, 6);
        // We can't easily test specific hours without mocking, so test basic logic
        let window_always = ScheduleWindow::new(0, 24);
        let now = Utc::now();
        assert!(window_always.is_within(&now));
    }

    #[test]
    fn test_schedule_window_day_restriction() {
        let window = ScheduleWindow::with_days(0, 24, vec![0]); // Monday only
        let now = Utc::now();
        let is_monday = now.weekday().num_days_from_monday() == 0;
        assert_eq!(window.is_within(&now), is_monday);
    }

    #[test]
    fn test_priority_queue_ordering() {
        let mut scheduler = SmartScheduler::new();
        scheduler.enqueue(ScheduleEntry {
            id: "low".into(),
            device_id: "dev1".into(),
            source_path: "/data".into(),
            priority: 1,
            estimated_size: 1000,
            device_online: true,
            network: NetworkCondition::default(),
            scheduled: false,
        });
        scheduler.enqueue(ScheduleEntry {
            id: "high".into(),
            device_id: "dev1".into(),
            source_path: "/src".into(),
            priority: 3,
            estimated_size: 500,
            device_online: true,
            network: NetworkCondition::default(),
            scheduled: false,
        });

        let next = scheduler.schedule_next();
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, "high");
    }

    #[test]
    fn test_offline_device_not_scheduled() {
        let mut scheduler = SmartScheduler::new();
        scheduler.enqueue(ScheduleEntry {
            id: "offline_task".into(),
            device_id: "dev1".into(),
            source_path: "/data".into(),
            priority: 3,
            estimated_size: 1000,
            device_online: false,
            network: NetworkCondition::default(),
            scheduled: false,
        });

        let result = scheduler.schedule_next();
        assert!(result.is_none());
    }

    #[test]
    fn test_poor_network_delayed() {
        let mut scheduler = SmartScheduler::new().with_min_network_score(50.0);
        scheduler.enqueue(ScheduleEntry {
            id: "poor_net".into(),
            device_id: "dev1".into(),
            source_path: "/data".into(),
            priority: 3,
            estimated_size: 1000,
            device_online: true,
            network: NetworkCondition::poor(),
            scheduled: false,
        });

        let result = scheduler.schedule_next();
        assert!(result.is_none());
    }

    #[test]
    fn test_suggestions_output() {
        let mut scheduler = SmartScheduler::new();
        scheduler.enqueue(ScheduleEntry {
            id: "t1".into(),
            device_id: "dev1".into(),
            source_path: "/data".into(),
            priority: 3,
            estimated_size: 1000,
            device_online: false,
            network: NetworkCondition::default(),
            scheduled: false,
        });
        let tips = scheduler.suggestions();
        assert!(!tips.is_empty());
    }

    #[test]
    fn test_schedule_all_eligible() {
        let mut scheduler = SmartScheduler::new();
        for i in 0..3 {
            scheduler.enqueue(ScheduleEntry {
                id: format!("task_{}", i),
                device_id: "dev1".into(),
                source_path: format!("/data/{}", i),
                priority: 1,
                estimated_size: 100,
                device_online: true,
                network: NetworkCondition::excellent(),
                scheduled: false,
            });
        }
        let results = scheduler.schedule_all_eligible();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_pending_count() {
        let mut scheduler = SmartScheduler::new();
        assert_eq!(scheduler.pending_count(), 0);
        scheduler.enqueue(ScheduleEntry {
            id: "t1".into(),
            device_id: "dev1".into(),
            source_path: "/data".into(),
            priority: 1,
            estimated_size: 100,
            device_online: true,
            network: NetworkCondition::default(),
            scheduled: false,
        });
        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_peek() {
        let mut scheduler = SmartScheduler::new();
        scheduler.enqueue(ScheduleEntry {
            id: "high_prio".into(),
            device_id: "dev1".into(),
            source_path: "/src".into(),
            priority: 5,
            estimated_size: 100,
            device_online: true,
            network: NetworkCondition::default(),
            scheduled: false,
        });
        let peeked = scheduler.peek();
        assert!(peeked.is_some());
        assert_eq!(peeked.unwrap().id, "high_prio");
        // Peek doesn't remove
        assert_eq!(scheduler.pending_count(), 1);
    }
}
