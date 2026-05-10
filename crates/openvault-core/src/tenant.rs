//! Multi-tenant support with RBAC access control.
//!
//! # Phase 8 Features
//!
//! - **Tenant** — Isolated tenant with quota and policy overrides
//! - **TenantManager** — CRUD for tenants, quota checking, data isolation
//! - **AccessControl** — Role-based access control with resource-level permissions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{VaultError, VaultResult};

// ============================================================================
// Tenant
// ============================================================================

/// Storage quota for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    /// Maximum total storage in bytes (0 = unlimited).
    pub max_storage_bytes: u64,
    /// Maximum number of files (0 = unlimited).
    pub max_files: u64,
    /// Maximum number of backup copies/replicas (0 = unlimited).
    pub max_copies: u64,
}

impl Default for TenantQuota {
    fn default() -> Self {
        Self {
            max_storage_bytes: 0,
            max_files: 0,
            max_copies: 0,
        }
    }
}

/// Current usage statistics for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenantUsage {
    pub storage_bytes: u64,
    pub file_count: u64,
    pub copy_count: u64,
}

/// A tenant in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub tenant_id: String,
    pub name: String,
    pub quota: TenantQuota,
    /// Policy overrides for this tenant (key=setting, value=override).
    pub policy_overrides: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub enabled: bool,
}

impl Tenant {
    /// Create a new tenant with default quota.
    pub fn new(name: &str) -> Self {
        let now = Utc::now();
        Self {
            tenant_id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            quota: TenantQuota::default(),
            policy_overrides: HashMap::new(),
            created_at: now,
            updated_at: now,
            enabled: true,
        }
    }

    /// Check if usage is within quota.
    pub fn check_quota(&self, usage: &TenantUsage) -> QuotaResult {
        let mut violations = Vec::new();
        if self.quota.max_storage_bytes > 0 && usage.storage_bytes > self.quota.max_storage_bytes {
            violations.push(QuotaViolation {
                kind: QuotaKind::Storage,
                used: usage.storage_bytes,
                limit: self.quota.max_storage_bytes,
            });
        }
        if self.quota.max_files > 0 && usage.file_count > self.quota.max_files {
            violations.push(QuotaViolation {
                kind: QuotaKind::Files,
                used: usage.file_count,
                limit: self.quota.max_files,
            });
        }
        if self.quota.max_copies > 0 && usage.copy_count > self.quota.max_copies {
            violations.push(QuotaViolation {
                kind: QuotaKind::Copies,
                used: usage.copy_count,
                limit: self.quota.max_copies,
            });
        }
        QuotaResult {
            within_quota: violations.is_empty(),
            violations,
        }
    }
}

/// Kind of quota violation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaKind {
    Storage,
    Files,
    Copies,
}

/// A single quota violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaViolation {
    pub kind: QuotaKind,
    pub used: u64,
    pub limit: u64,
}

/// Result of a quota check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaResult {
    pub within_quota: bool,
    pub violations: Vec<QuotaViolation>,
}

// ============================================================================
// Tenant Manager
// ============================================================================

/// Manages tenants with quota checking and data isolation.
#[derive(Debug, Clone, Default)]
pub struct TenantManager {
    tenants: HashMap<String, Tenant>,
    usage: HashMap<String, TenantUsage>,
}

impl TenantManager {
    /// Create a new tenant manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new tenant.
    pub fn create_tenant(&mut self, name: &str, quota: TenantQuota) -> VaultResult<Tenant> {
        let mut tenant = Tenant::new(name);
        tenant.quota = quota;
        let id = tenant.tenant_id.clone();
        self.tenants.insert(id.clone(), tenant.clone());
        self.usage.insert(id, TenantUsage::default());
        Ok(tenant)
    }

    /// Get a tenant by id.
    pub fn get_tenant(&self, tenant_id: &str) -> VaultResult<&Tenant> {
        self.tenants
            .get(tenant_id)
            .ok_or_else(|| VaultError::Config(format!("Tenant {} not found", tenant_id)))
    }

    /// Update a tenant.
    pub fn update_tenant(&mut self, tenant_id: &str, name: Option<&str>, quota: Option<TenantQuota>) -> VaultResult<Tenant> {
        let tenant = self
            .tenants
            .get_mut(tenant_id)
            .ok_or_else(|| VaultError::Config(format!("Tenant {} not found", tenant_id)))?;
        if let Some(n) = name {
            tenant.name = n.to_string();
        }
        if let Some(q) = quota {
            tenant.quota = q;
        }
        tenant.updated_at = Utc::now();
        Ok(tenant.clone())
    }

    /// Delete a tenant.
    pub fn delete_tenant(&mut self, tenant_id: &str) -> VaultResult<()> {
        self.tenants
            .remove(tenant_id)
            .ok_or_else(|| VaultError::Config(format!("Tenant {} not found", tenant_id)))?;
        self.usage.remove(tenant_id);
        Ok(())
    }

    /// List all tenants.
    pub fn list_tenants(&self) -> Vec<&Tenant> {
        self.tenants.values().collect()
    }

    /// Get usage for a tenant.
    pub fn get_usage(&self, tenant_id: &str) -> VaultResult<&TenantUsage> {
        self.usage
            .get(tenant_id)
            .ok_or_else(|| VaultError::Config(format!("Tenant {} not found", tenant_id)))
    }

    /// Update usage for a tenant.
    pub fn update_usage(&mut self, tenant_id: &str, usage: TenantUsage) -> VaultResult<()> {
        if !self.tenants.contains_key(tenant_id) {
            return Err(VaultError::Config(format!("Tenant {} not found", tenant_id)));
        }
        self.usage.insert(tenant_id.to_string(), usage);
        Ok(())
    }

    /// Check quota for a tenant.
    pub fn check_quota(&self, tenant_id: &str) -> VaultResult<QuotaResult> {
        let tenant = self.get_tenant(tenant_id)?;
        let usage = self.get_usage(tenant_id)?;
        Ok(tenant.check_quota(usage))
    }

    /// Check if a path belongs to a tenant (data isolation by prefix convention).
    pub fn is_path_owned(&self, tenant_id: &str, path: &str) -> bool {
        // Convention: /tenants/{tenant_id}/... belongs to that tenant
        let prefix = format!("/tenants/{}/", tenant_id);
        path.starts_with(&prefix) || !path.contains("/tenants/")
    }
}

// ============================================================================
// Access Control (RBAC)
// ============================================================================

/// Roles in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

/// Permissions that can be granted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Backup,
    Restore,
    Delete,
    Policy,
    Admin,
}

/// Maps roles to their default permissions.
pub fn role_permissions(role: &Role) -> Vec<Permission> {
    match role {
        Role::Admin => vec![
            Permission::Backup,
            Permission::Restore,
            Permission::Delete,
            Permission::Policy,
            Permission::Admin,
        ],
        Role::Operator => vec![
            Permission::Backup,
            Permission::Restore,
            Permission::Delete,
        ],
        Role::Viewer => vec![Permission::Restore],
    }
}

/// A user's access control entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccess {
    pub user_id: String,
    pub tenant_id: String,
    pub role: Role,
    /// Optional resource path restriction (user can only access this path prefix).
    pub path_restriction: Option<String>,
}

impl UserAccess {
    /// Create a new user access entry.
    pub fn new(user_id: &str, tenant_id: &str, role: Role) -> Self {
        Self {
            user_id: user_id.to_string(),
            tenant_id: tenant_id.to_string(),
            role,
            path_restriction: None,
        }
    }

    /// With a path restriction.
    pub fn with_path_restriction(mut self, path: &str) -> Self {
        self.path_restriction = Some(path.to_string());
        self
    }

    /// Check if this user has a specific permission.
    pub fn has_permission(&self, perm: &Permission) -> bool {
        role_permissions(&self.role).contains(perm)
    }

    /// Check if this user can access a given path.
    pub fn can_access_path(&self, path: &str) -> bool {
        match &self.path_restriction {
            Some(prefix) => path.starts_with(prefix),
            None => true,
        }
    }
}

/// Access control manager.
#[derive(Debug, Clone, Default)]
pub struct AccessControl {
    entries: HashMap<String, UserAccess>,
}

impl AccessControl {
    /// Create a new access control.
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant a user access.
    pub fn grant(&mut self, access: UserAccess) {
        self.entries.insert(access.user_id.clone(), access);
    }

    /// Revoke a user's access.
    pub fn revoke(&mut self, user_id: &str) -> bool {
        self.entries.remove(user_id).is_some()
    }

    /// Check if a user has a permission on a path within a tenant.
    pub fn check(&self, user_id: &str, perm: &Permission, path: &str, tenant_id: &str) -> bool {
        match self.entries.get(user_id) {
            Some(access) => {
                access.tenant_id == tenant_id
                    && access.has_permission(perm)
                    && access.can_access_path(path)
            }
            None => false,
        }
    }

    /// Get a user's access entry.
    pub fn get_user(&self, user_id: &str) -> Option<&UserAccess> {
        self.entries.get(user_id)
    }

    /// List all users for a tenant.
    pub fn list_tenant_users(&self, tenant_id: &str) -> Vec<&UserAccess> {
        self.entries
            .values()
            .filter(|a| a.tenant_id == tenant_id)
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tenant() {
        let mut mgr = TenantManager::new();
        let t = mgr.create_tenant("Acme Corp", TenantQuota::default()).unwrap();
        assert_eq!(t.name, "Acme Corp");
        assert!(t.enabled);
    }

    #[test]
    fn test_tenant_quota_check() {
        let mut mgr = TenantManager::new();
        let t = mgr.create_tenant("test", TenantQuota {
            max_storage_bytes: 1000,
            max_files: 10,
            max_copies: 0,
        }).unwrap();
        let result = mgr.check_quota(&t.tenant_id).unwrap();
        assert!(result.within_quota);
    }

    #[test]
    fn test_tenant_quota_violation() {
        let mut mgr = TenantManager::new();
        let t = mgr.create_tenant("test", TenantQuota {
            max_storage_bytes: 100,
            max_files: 0,
            max_copies: 0,
        }).unwrap();
        mgr.update_usage(&t.tenant_id, TenantUsage {
            storage_bytes: 200,
            file_count: 5,
            copy_count: 1,
        }).unwrap();
        let result = mgr.check_quota(&t.tenant_id).unwrap();
        assert!(!result.within_quota);
        assert!(result.violations.iter().any(|v| v.kind == QuotaKind::Storage));
    }

    #[test]
    fn test_role_permissions() {
        let admin_perms = role_permissions(&Role::Admin);
        assert!(admin_perms.contains(&Permission::Admin));
        let viewer_perms = role_permissions(&Role::Viewer);
        assert!(!viewer_perms.contains(&Permission::Delete));
    }

    #[test]
    fn test_user_access_path_restriction() {
        let access = UserAccess::new("user1", "t1", Role::Operator)
            .with_path_restriction("/tenants/t1/project-a/");
        assert!(access.can_access_path("/tenants/t1/project-a/file.txt"));
        assert!(!access.can_access_path("/tenants/t1/project-b/file.txt"));
    }

    #[test]
    fn test_access_control_check() {
        let mut ac = AccessControl::new();
        ac.grant(UserAccess::new("alice", "t1", Role::Admin));
        ac.grant(UserAccess::new("bob", "t1", Role::Viewer));
        assert!(ac.check("alice", &Permission::Delete, "/data", "t1"));
        assert!(!ac.check("bob", &Permission::Delete, "/data", "t1"));
    }

    #[test]
    fn test_access_control_tenant_isolation() {
        let mut ac = AccessControl::new();
        ac.grant(UserAccess::new("alice", "t1", Role::Admin));
        // Alice cannot access t2
        assert!(!ac.check("alice", &Permission::Backup, "/data", "t2"));
    }

    #[test]
    fn test_delete_tenant() {
        let mut mgr = TenantManager::new();
        let t = mgr.create_tenant("to-delete", TenantQuota::default()).unwrap();
        assert!(mgr.delete_tenant(&t.tenant_id).is_ok());
        assert!(mgr.get_tenant(&t.tenant_id).is_err());
    }
}
