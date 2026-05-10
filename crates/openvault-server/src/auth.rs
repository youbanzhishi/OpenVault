//! Authentication and authorization for OpenVault API

use crate::error::{ServerError, ServerResult};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// JWT claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (usually device_id or user_id)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: usize,
    /// Issued at (Unix timestamp)
    pub iat: usize,
    /// Token type (e.g., "access", "refresh")
    pub token_type: String,
    /// Allowed scopes
    pub scopes: Vec<String>,
}

impl Claims {
    /// Create new access token claims
    pub fn new_access(subject: &str, scopes: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            sub: subject.to_string(),
            exp: (now + Duration::hours(24)).timestamp() as usize,
            iat: now.timestamp() as usize,
            token_type: "access".to_string(),
            scopes,
        }
    }

    /// Create new refresh token claims
    pub fn new_refresh(subject: &str) -> Self {
        let now = Utc::now();
        Self {
            sub: subject.to_string(),
            exp: (now + Duration::days(30)).timestamp() as usize,
            iat: now.timestamp() as usize,
            token_type: "refresh".to_string(),
            scopes: vec!["refresh".to_string()],
        }
    }

    /// Check if token has a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.contains(&scope.to_string()) || self.scopes.contains(&"*".to_string())
    }
}

/// Authentication manager
pub struct AuthManager {
    secret: String,
    #[allow(dead_code)]
    issuer: String,
}

impl AuthManager {
    /// Create a new authentication manager
    pub fn new(secret: String, issuer: String) -> Self {
        Self { secret, issuer }
    }

    /// Generate an access token
    pub fn generate_token(&self, claims: &Claims) -> ServerResult<String> {
        let key = EncodingKey::from_secret(self.secret.as_bytes());
        let header = Header::default();
        
        encode(&header, claims, &key)
            .map_err(|e| ServerError::Internal(format!("Failed to generate token: {}", e)))
    }

    /// Validate and decode a token
    pub fn validate_token(&self, token: &str) -> ServerResult<Claims> {
        let key = DecodingKey::from_secret(self.secret.as_bytes());
        let validation = Validation::default();
        
        let token_data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| {
                match e.kind() {
                    jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                        ServerError::Unauthorized("Token expired".to_string())
                    }
                    jsonwebtoken::errors::ErrorKind::InvalidToken => {
                        ServerError::Unauthorized("Invalid token".to_string())
                    }
                    _ => ServerError::Unauthorized(format!("Token validation failed: {}", e)),
                }
            })?;
        
        Ok(token_data.claims)
    }

    /// Extract token from Authorization header
    pub fn extract_token(auth_header: &str) -> ServerResult<&str> {
        if let Some(stripped) = auth_header.strip_prefix("Bearer ") {
            Ok(stripped)
        } else {
            Err(ServerError::Unauthorized(
                "Invalid Authorization header format".to_string(),
            ))
        }
    }

    /// Generate a device token
    pub fn generate_device_token(&self, device_id: &str) -> ServerResult<String> {
        let claims = Claims::new_access(
            device_id,
            vec![
                "backup:read".to_string(),
                "backup:write".to_string(),
                "status:read".to_string(),
            ],
        );
        self.generate_token(&claims)
    }

    /// Generate an admin token
    pub fn generate_admin_token(&self, user_id: &str) -> ServerResult<String> {
        let claims = Claims::new_access(
            user_id,
            vec![
                "*".to_string(), // Full access
            ],
        );
        self.generate_token(&claims)
    }
}

/// Middleware-like function to extract and validate claims from request
pub async fn authenticate(
    auth_manager: &AuthManager,
    authorization: Option<String>,
    required_scope: Option<&str>,
) -> ServerResult<Claims> {
    let auth_header = authorization.ok_or_else(|| {
        ServerError::Unauthorized("Missing Authorization header".to_string())
    })?;

    let token = AuthManager::extract_token(&auth_header)?;
    let claims = auth_manager.validate_token(token)?;

    if let Some(scope) = required_scope {
        if !claims.has_scope(scope) {
            return Err(ServerError::Unauthorized(format!(
                "Missing required scope: {}",
                scope
            )));
        }
    }

    Ok(claims)
}

/// Scope constants
pub mod scopes {
    pub const BACKUP_READ: &str = "backup:read";
    pub const BACKUP_WRITE: &str = "backup:write";
    pub const STATUS_READ: &str = "status:read";
    pub const POLICY_READ: &str = "policy:read";
    pub const POLICY_WRITE: &str = "policy:write";
    pub const DEVICE_READ: &str = "device:read";
    pub const DEVICE_WRITE: &str = "device:write";
    pub const ADMIN: &str = "*";
}
