//! Web UI — 内嵌管理面板 (HTMX + Alpine.js)

use axum::response::Html;

const STYLE_CSS: &str = r##"
    :root { --bg: #0f172a; --card: #1e293b; --accent: #22c55e; --text: #e2e8f0; --dim: #94a3b8; --border: #334155; --ok: #22c55e; --warn: #eab308; --err: #ef4444; }
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: var(--bg); color: var(--text); min-height: 100vh; }
    nav { background: var(--card); border-bottom: 1px solid var(--border); padding: 1rem 2rem; display: flex; align-items: center; gap: 2rem; position: sticky; top: 0; z-index: 10; }
    nav .logo { font-size: 1.25rem; font-weight: 700; color: var(--accent); }
    nav a { color: var(--dim); text-decoration: none; font-size: 0.9rem; }
    nav a:hover { color: var(--text); }
    nav a.active { color: var(--accent); font-weight: 600; }
    .container { max-width: 1200px; margin: 2rem auto; padding: 0 2rem; }
    .card { background: var(--card); border: 1px solid var(--border); border-radius: 8px; padding: 1.5rem; margin-bottom: 1rem; }
    .card h2 { font-size: 1.1rem; margin-bottom: 1rem; color: var(--accent); }
    .stats-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 1rem; }
    .stat { text-align: center; padding: 1rem; }
    .stat .number { font-size: 2rem; font-weight: 700; color: var(--accent); }
    .stat .label { font-size: 0.85rem; color: var(--dim); margin-top: 0.3rem; }
    table { width: 100%; border-collapse: collapse; }
    th, td { text-align: left; padding: 0.6rem 1rem; border-bottom: 1px solid var(--border); font-size: 0.85rem; }
    th { color: var(--dim); font-weight: 600; }
    .badge { display: inline-block; padding: 0.15rem 0.5rem; border-radius: 4px; font-size: 0.7rem; font-weight: 600; }
    .badge-ok { background: rgba(34,197,94,0.2); color: var(--ok); }
    .badge-info { background: rgba(59,130,246,0.2); color: #3b82f6; }
    .badge-err { background: rgba(239,68,68,0.2); color: var(--err); }
    .empty { text-align: center; padding: 3rem; color: var(--dim); }
    .empty .icon { font-size: 3rem; margin-bottom: 1rem; }
    .two-col { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
    @media (max-width: 768px) { .two-col { grid-template-columns: 1fr; } }
    pre { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 1rem; overflow-x: auto; font-size: 0.8rem; }
"##;

fn nav_html(active: &str) -> String {
    let items = [
        ("dashboard", "/", "📊 Dashboard"),
        ("devices", "/ui/devices", "📱 Devices"),
        ("policies", "/ui/policies", "📋 Policies"),
        ("snapshots", "/ui/snapshots", "💾 Snapshots"),
        ("api", "/ui/api", "🔌 API"),
    ];
    let links: Vec<String> = items
        .iter()
        .map(|(key, href, label)| {
            let cls = if *key == active {
                " class=\"active\""
            } else {
                ""
            };
            format!("<a href=\"{}\"{}>{}</a>", href, cls, label)
        })
        .collect();
    format!(
        "<nav><span class=\"logo\">🔒 OpenVault</span>{}</nav>",
        links.join("")
    )
}

fn page_shell(title: &str, nav_active: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!DOCTYPE html><html lang=\"zh-CN\"><head>\
        <meta charset=\"UTF-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1.0\">\
        <title>{t} — OpenVault</title>\
        <script src=\"https://unpkg.com/htmx.org@1.9.10\"></script>\
        <script defer src=\"https://unpkg.com/alpinejs@3.13.3\"></script>\
        <style>{css}</style></head>\
        <body>{nav}<div class=\"container\">{body}</div></body></html>",
        t = title,
        css = STYLE_CSS,
        nav = nav_html(nav_active),
        body = body,
    ))
}

pub fn dashboard_page() -> Html<String> {
    page_shell(
        "Dashboard",
        "dashboard",
        r##"
    <div class="stats-grid">
        <div class="card stat">
            <div class="number" x-data="{v:0}" x-init="fetch('/api/v1/status').then(r=>r.json()).then(d=>v=d.total_snapshots||0)"><span x-text="v">-</span></div>
            <div class="label">Snapshots</div>
        </div>
        <div class="card stat">
            <div class="number" x-data="{v:0}" x-init="fetch('/api/v1/status').then(r=>r.json()).then(d=>v=d.connected_devices||0)"><span x-text="v">-</span></div>
            <div class="label">Devices</div>
        </div>
        <div class="card stat">
            <div class="number" x-data="{v:'-'}" x-init="fetch('/api/v1/status').then(r=>r.json()).then(d=>v=d.health||'unknown')"><span x-text="v">-</span></div>
            <div class="label">Health</div>
        </div>
        <div class="card stat">
            <div class="number" x-data="{v:'0 B'}" x-init="fetch('/api/v1/status').then(r=>r.json()).then(d=>{const b=d.total_storage_bytes||0;const u=['B','KB','MB','GB'];let i=0;while(b>=1024&&i<3){b/=1024;i++}v=b.toFixed(1)+' '+u[i]})"><span x-text="v">-</span></div>
            <div class="label">Storage</div>
        </div>
    </div>
    <div class="two-col">
        <div class="card">
            <h2>📱 Registered Devices</h2>
            <div x-data="{devices:[]}" x-init="fetch('/api/v1/devices').then(r=>r.json()).then(d=>devices=Array.isArray(d)?d:d.devices||[])">
                <template x-for="dev in devices.slice(0,5)" :key="dev.device_id">
                    <div style="padding:0.4rem 0;border-bottom:1px solid var(--border)">
                        <strong x-text="dev.device_name||dev.device_id"></strong>
                        <span class="badge badge-ok" x-text="dev.status||'active'" style="margin-left:0.5rem"></span>
                    </div>
                </template>
                <div x-show="devices.length===0" style="color:var(--dim)">No devices registered</div>
            </div>
        </div>
        <div class="card">
            <h2>📋 Backup Policies</h2>
            <div x-data="{policies:[]}" x-init="fetch('/api/v1/policies').then(r=>r.json()).then(d=>policies=Array.isArray(d)?d:d.policies||[])">
                <template x-for="p in policies.slice(0,5)" :key="p.policy_id">
                    <div style="padding:0.4rem 0;border-bottom:1px solid var(--border)">
                        <strong x-text="p.name||p.policy_id"></strong>
                        <span class="badge badge-info" x-text="p.strategy_type||'3-2-1'" style="margin-left:0.5rem"></span>
                    </div>
                </template>
                <div x-show="policies.length===0" style="color:var(--dim)">No policies configured</div>
            </div>
        </div>
    </div>
    "##,
    )
}

pub fn devices_page() -> Html<String> {
    page_shell(
        "Devices",
        "devices",
        r##"
    <div class="card">
        <h2>📱 Registered Devices</h2>
        <div x-data="{devices:[]}" x-init="fetch('/api/v1/devices').then(r=>r.json()).then(d=>devices=Array.isArray(d)?d:d.devices||[])">
            <table>
                <thead><tr><th>ID</th><th>Name</th><th>Status</th><th>Last Heartbeat</th></tr></thead>
                <tbody>
                    <template x-for="dev in devices" :key="dev.device_id">
                        <tr>
                            <td><span class="badge badge-info" x-text="dev.device_id"></span></td>
                            <td x-text="dev.device_name||'-'"></td>
                            <td><span class="badge badge-ok" x-text="dev.status||'active'"></span></td>
                            <td x-text="dev.last_heartbeat?dev.last_heartbeat.slice(0,19):'-'"></td>
                        </tr>
                    </template>
                </tbody>
            </table>
            <div x-show="devices.length===0" class="empty"><div class="icon">📱</div>No devices registered</div>
        </div>
    </div>
    "##,
    )
}

pub fn policies_page() -> Html<String> {
    page_shell(
        "Policies",
        "policies",
        r##"
    <div class="card">
        <h2>📋 Backup Policies</h2>
        <div x-data="{policies:[]}" x-init="fetch('/api/v1/policies').then(r=>r.json()).then(d=>policies=Array.isArray(d)?d:d.policies||[])">
            <table>
                <thead><tr><th>ID</th><th>Name</th><th>Strategy</th><th>Retention</th></tr></thead>
                <tbody>
                    <template x-for="p in policies" :key="p.policy_id">
                        <tr>
                            <td><span class="badge badge-info" x-text="p.policy_id"></span></td>
                            <td x-text="p.name||'-'"></td>
                            <td><span class="badge badge-ok" x-text="p.strategy_type||'3-2-1'"></span></td>
                            <td x-text="p.retention_days||'∞'"></td>
                        </tr>
                    </template>
                </tbody>
            </table>
            <div x-show="policies.length===0" class="empty"><div class="icon">📋</div>No policies configured</div>
        </div>
    </div>
    "##,
    )
}

pub fn snapshots_page() -> Html<String> {
    page_shell(
        "Snapshots",
        "snapshots",
        r##"
    <div class="card">
        <h2>💾 Backup Snapshots</h2>
        <div x-data="{snaps:[]}" x-init="fetch('/api/v1/snapshots').then(r=>r.json()).then(d=>snaps=Array.isArray(d)?d:d.snapshots||[])">
            <table>
                <thead><tr><th>ID</th><th>Device</th><th>Type</th><th>Size</th><th>Created</th></tr></thead>
                <tbody>
                    <template x-for="s in snaps" :key="s.snapshot_id">
                        <tr>
                            <td><span class="badge badge-info" x-text="s.snapshot_id"></span></td>
                            <td x-text="s.device_id||'-'"></td>
                            <td><span class="badge badge-ok" x-text="s.backup_type||'full'"></span></td>
                            <td x-text="s.size_bytes?(s.size_bytes/1024/1024).toFixed(1)+'MB':'-'"></td>
                            <td x-text="s.created_at?s.created_at.slice(0,19):'-'"></td>
                        </tr>
                    </template>
                </tbody>
            </table>
            <div x-show="snaps.length===0" class="empty"><div class="icon">💾</div>No snapshots available</div>
        </div>
    </div>
    "##,
    )
}

pub fn api_page() -> Html<String> {
    page_shell(
        "API",
        "api",
        r##"
    <div class="card">
        <h2>🔌 API Endpoints</h2>
        <table>
            <thead><tr><th>Method</th><th>Path</th><th>Description</th></tr></thead>
            <tbody>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/api/v1/health</td><td>Health check</td></tr>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/api/v1/status</td><td>System status</td></tr>
                <tr><td><span class="badge badge-info">POST</span></td><td>/api/v1/devices</td><td>Register device</td></tr>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/api/v1/devices</td><td>List devices</td></tr>
                <tr><td><span class="badge badge-info">POST</span></td><td>/api/v1/policies</td><td>Create policy</td></tr>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/api/v1/policies</td><td>List policies</td></tr>
                <tr><td><span class="badge badge-info">POST</span></td><td>/api/v1/backup</td><td>Trigger backup</td></tr>
                <tr><td><span class="badge badge-info">POST</span></td><td>/api/v1/restore</td><td>Trigger restore</td></tr>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/api/v1/snapshots</td><td>List snapshots</td></tr>
                <tr><td><span class="badge badge-ok">GET</span></td><td>/.well-known/agent.json</td><td>Agent discovery</td></tr>
            </tbody>
        </table>
    </div>
    "##,
    )
}
