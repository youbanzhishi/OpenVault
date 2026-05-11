//! Agent Manifest Handler
//!
//! Implements the Agent Discovery Protocol endpoint.
//! Returns a machine-readable manifest at `/.well-known/agent.json`
//! describing OpenVault's capabilities, API endpoints, and auth requirements.

use axum::Json;

/// GET /.well-known/agent.json
///
/// Returns the Agent Discovery Manifest — a structured description of
/// OpenVault's capabilities for AI agents and automated tooling.
pub async fn agent_manifest() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "schema_version": "1.0",
        "name": "OpenVault",
        "description": "安全智能的文件备份与容灾恢复系统 — 狡兔三窟，AI守护，永不丢失",
        "version": "1.0.1",
        "base_url": "http://localhost:8090",
        "auth": {
            "type": "bearer",
            "header": "Authorization",
            "format": "Bearer <jwt>",
            "note": "写操作需要JWT认证，读操作部分开放"
        },
        "core_concepts": {
            "321_strategy": "3份副本、2种介质、1份异地",
            "snapshot": "备份快照，可增量/差异/全量",
            "self_healing": "自动检测并修复损坏文件",
            "compliance": "3-2-1策略合规检查与报告",
            "ai_restore": "自然语言恢复（如'恢复上周的文档'）"
        },
        "capabilities": [
            {
                "name": "trigger_backup",
                "description": "触发备份",
                "method": "POST",
                "path": "/api/v1/backup",
                "params": {
                    "policy_id": "策略ID(可选)",
                    "strategy": "full/incremental/differential"
                }
            },
            {
                "name": "trigger_restore",
                "description": "触发恢复",
                "method": "POST",
                "path": "/api/v1/restore",
                "params": {
                    "snapshot_id": "快照ID",
                    "target_path": "恢复目标路径(可选)"
                }
            },
            {
                "name": "ai_restore",
                "description": "AI自然语言恢复",
                "method": "POST",
                "path": "/api/v1/restore/ai",
                "params": {
                    "query": "自然语言描述，如'恢复上周的税务文件'"
                }
            },
            {
                "name": "search_files",
                "description": "搜索备份文件",
                "method": "POST",
                "path": "/api/v1/search",
                "params": {
                    "query": "搜索关键词",
                    "snapshot_id": "限定快照(可选)"
                }
            },
            {
                "name": "intel_suggestions",
                "description": "获取AI智能建议",
                "method": "GET",
                "path": "/api/v1/intel/suggestions"
            },
            {
                "name": "list_snapshots",
                "description": "列出备份快照",
                "method": "GET",
                "path": "/api/v1/snapshots"
            },
            {
                "name": "compliance_check",
                "description": "3-2-1合规检查",
                "method": "GET",
                "path": "/api/v1/compliance/check"
            },
            {
                "name": "compliance_report",
                "description": "合规报告",
                "method": "GET",
                "path": "/api/v1/compliance/report"
            },
            {
                "name": "verify_integrity",
                "description": "完整性校验(CLI)",
                "method": "CLI",
                "path": "vault verify"
            },
            {
                "name": "heal_scan",
                "description": "扫描损坏(CLI)",
                "method": "CLI",
                "path": "vault heal scan"
            },
            {
                "name": "heal_repair",
                "description": "修复损坏(CLI)",
                "method": "CLI",
                "path": "vault heal repair"
            },
            {
                "name": "manage_devices",
                "description": "设备管理",
                "method": "POST",
                "path": "/api/v1/devices"
            },
            {
                "name": "manage_policies",
                "description": "策略管理",
                "method": "POST",
                "path": "/api/v1/policies"
            },
            {
                "name": "audit_log",
                "description": "审计日志",
                "method": "GET",
                "path": "/api/v1/audit"
            }
        ],
        "storage_backends": {
            "local": {
                "description": "本地文件系统",
                "params": ["path"]
            },
            "s3": {
                "description": "AWS S3",
                "params": ["bucket", "region", "access_key_id", "secret_access_key"]
            },
            "r2": {
                "description": "Cloudflare R2",
                "params": ["account_id", "bucket", "access_key_id", "secret_access_key"]
            }
        },
        "links": {
            "user_guide": "/docs/user-guide.md",
            "agent_guide": "/docs/agent-guide.md",
            "api_reference": "/docs/api-reference.md",
            "backup_strategies": "/docs/backup-strategies.md",
            "source": "https://github.com/youbanzhishi/OpenVault",
            "health": "/api/v1/health"
        }
    }))
}
