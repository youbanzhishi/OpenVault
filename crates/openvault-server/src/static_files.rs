//! Static File Serving for Web Panel
//!
//! Provides embedded static file service for the OpenVault web management panel.
//! Implements SPA routing (all non-API requests return index.html) and
//! serves a placeholder HTML page with API endpoint documentation.

use axum::{
    body::Body,
    http::{Response, StatusCode, Uri},
    response::IntoResponse,
};

/// The placeholder index.html for the web panel.
const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>OpenVault Web Panel</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
            color: #e0e0e0;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 2rem;
        }
        h1 {
            font-size: 2.5rem;
            margin-bottom: 0.5rem;
            background: linear-gradient(90deg, #00d2ff, #3a7bd5);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .subtitle {
            font-size: 1.2rem;
            color: #aaa;
            margin-bottom: 2rem;
        }
        .api-section {
            width: 100%;
            max-width: 800px;
            background: rgba(255,255,255,0.05);
            border-radius: 12px;
            padding: 1.5rem;
            margin-bottom: 1rem;
        }
        .api-section h2 {
            color: #00d2ff;
            margin-bottom: 0.8rem;
            font-size: 1.3rem;
        }
        .api-endpoint {
            display: flex;
            align-items: center;
            padding: 0.5rem 0;
            border-bottom: 1px solid rgba(255,255,255,0.05);
        }
        .api-endpoint:last-child { border-bottom: none; }
        .method {
            font-weight: bold;
            padding: 0.2rem 0.6rem;
            border-radius: 4px;
            font-size: 0.8rem;
            margin-right: 1rem;
            min-width: 50px;
            text-align: center;
        }
        .method-get { background: #2e7d32; color: #fff; }
        .method-post { background: #1565c0; color: #fff; }
        .path { font-family: monospace; color: #b0bec5; }
        .desc { margin-left: auto; color: #78909c; font-size: 0.85rem; }
        .coming-soon {
            margin-top: 2rem;
            padding: 1rem 2rem;
            background: rgba(0,210,255,0.1);
            border: 1px solid rgba(0,210,255,0.3);
            border-radius: 8px;
            font-size: 1.1rem;
        }
    </style>
</head>
<body>
    <h1>OpenVault Web Panel</h1>
    <p class="subtitle">Intelligent Backup & Disaster Recovery Management</p>
    <div class="coming-soon">
        🚧 Full Web Panel — Coming Soon 🚧
    </div>

    <div class="api-section">
        <h2>📊 Dashboard API</h2>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/overview</span>
            <span class="desc">Dashboard overview stats</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/policies</span>
            <span class="desc">List backup policies</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/files</span>
            <span class="desc">List files (paginated)</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/replicas/:file_id</span>
            <span class="desc">File replica details</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/alerts</span>
            <span class="desc">Alert list</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-post">POST</span>
            <span class="path">/api/dashboard/restore</span>
            <span class="desc">One-click restore</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-post">POST</span>
            <span class="path">/api/dashboard/policy</span>
            <span class="desc">Create policy</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/dashboard/stats</span>
            <span class="desc">Statistics & charts</span>
        </div>
    </div>

    <div class="api-section">
        <h2>🤖 Agent API</h2>
        <div class="api-endpoint">
            <span class="method method-post">POST</span>
            <span class="path">/api/agent/command</span>
            <span class="desc">Execute agent command</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/agent/:agent_id/profile</span>
            <span class="desc">Get agent profile</span>
        </div>
    </div>

    <div class="api-section">
        <h2>📱 Device Management</h2>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/devices/registry</span>
            <span class="desc">List all devices</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/devices/heartbeat</span>
            <span class="desc">Heartbeat report</span>
        </div>
        <div class="api-endpoint">
            <span class="method method-get">GET</span>
            <span class="path">/api/devices/:id/policies</span>
            <span class="desc">Device policy config</span>
        </div>
    </div>
</body>
</html>"#;

/// Serve the index.html placeholder page.
pub fn serve_index() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(INDEX_HTML))
        .unwrap()
}

/// SPA router: for any request that is NOT an API route (doesn't start with /api/),
/// return the index.html. API routes fall through to the regular router.
pub async fn spa_fallback(uri: Uri) -> impl IntoResponse {
    let path = uri.path();
    if path.starts_with("/api/") {
        // Let API routes handle this — return 404
        (
            StatusCode::NOT_FOUND,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"API endpoint not found"}"#,
        )
            .into_response()
    } else {
        serve_index().into_response()
    }
}

/// Check if a URI path is an API route.
pub fn is_api_route(path: &str) -> bool {
    path.starts_with("/api/")
}

/// Get the raw index HTML content.
pub fn get_index_html() -> &'static str {
    INDEX_HTML
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serve_index_returns_html() {
        let response = serve_index();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_spa_fallback_non_api_returns_index() {
        // We test the logic by checking is_api_route
        assert!(!is_api_route("/"));
        assert!(!is_api_route("/dashboard"));
        assert!(!is_api_route("/files/backup"));
    }

    #[test]
    fn test_is_api_route() {
        assert!(is_api_route("/api/v1/health"));
        assert!(is_api_route("/api/dashboard/overview"));
        assert!(is_api_route("/api/agent/command"));
        assert!(!is_api_route("/"));
        assert!(!is_api_route("/about"));
        assert!(!is_api_route("/static/style.css"));
    }

    #[test]
    fn test_index_html_contains_title() {
        let html = get_index_html();
        assert!(html.contains("OpenVault Web Panel"));
        assert!(html.contains("Coming Soon"));
        assert!(html.contains("/api/dashboard/overview"));
    }
}
