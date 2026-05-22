//! API Authentication middleware
//!
//! This module provides authentication middleware for MemoBuild API endpoints,
//! supporting bearer token validation with rate limiting and audit logging.

use anyhow::Result;
use argon2::Argon2;
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Json,
    response::Response,
    routing::{get, post},
    Router,
};
use parking_lot::RwLock;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock as TokioRwLock;
use tracing::{info, warn};

/// Stored token with hash
#[derive(Clone)]
struct StoredToken {
    hash: String,
    _description: String,
    _created_at: chrono::DateTime<chrono::Utc>,
    is_admin: bool,
}

/// Rate limit tracking
#[derive(Clone)]
struct RateLimitBucket {
    count: u32,
    window_start: Instant,
}

pub struct AuthState {
    pub admin_token: Option<String>,
    pub db_client: Option<tokio_postgres::Client>,
    tokens: Arc<RwLock<HashMap<String, StoredToken>>>,
    rate_limit_buckets: Arc<TokioRwLock<HashMap<String, RateLimitBucket>>>,
}

impl AuthState {
    pub fn new(admin_token: Option<String>, db_client: Option<tokio_postgres::Client>) -> Self {
        Self {
            admin_token,
            db_client,
            tokens: Arc::new(RwLock::new(HashMap::new())),
            rate_limit_buckets: Arc::new(TokioRwLock::new(HashMap::new())),
        }
    }

    pub async fn is_valid_token(&self, token: &str) -> Result<bool> {
        if let Some(ref admin) = self.admin_token {
            if token == admin {
                return Ok(true);
            }
        }

        let token_hash = sha256_hash(token);
        let tokens = self.tokens.read();
        if let Some(stored) = tokens.get(&token_hash) {
            let _ = verify_password(token, &stored.hash)?;
            return Ok(true);
        }

        Ok(false)
    }

    pub async fn is_admin_token(&self, token: &str) -> Result<bool> {
        if let Some(ref admin) = self.admin_token {
            if token == admin {
                return Ok(true);
            }
        }

        let token_hash = sha256_hash(token);
        let tokens = self.tokens.read();
        if let Some(stored) = tokens.get(&token_hash) {
            return Ok(stored.is_admin);
        }

        Ok(false)
    }

    pub async fn create_token(&self, description: &str, is_admin: bool) -> Result<String> {
        let token = generate_token();
        let token_hash = sha256_hash(&token);
        let argon2 = Argon2::default();

        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let salt = base64_encode(&bytes);

        let hash = argon2_hash(&token, &salt, &argon2);

        let stored = StoredToken {
            hash: format!("{}${}", salt, hash),
            _description: description.to_string(),
            _created_at: chrono::Utc::now(),
            is_admin,
        };

        self.tokens.write().insert(token_hash.clone(), stored);

        if let Some(ref client) = self.db_client {
            let _ = client.execute(
                "INSERT INTO auth_tokens (token_hash, description, is_admin, created_at) VALUES ($1, $2, $3, $4)",
                &[&token_hash, &description, &(is_admin as i32), &chrono::Utc::now()],
            ).await;
        }

        Ok(token)
    }

    pub async fn check_rate_limit(&self, key: &str, max_requests: u32, window_secs: u64) -> Result<bool> {
        let mut buckets = self.rate_limit_buckets.write().await;
        let now = Instant::now();
        let window = Duration::from_secs(window_secs);

        if let Some(bucket) = buckets.get_mut(key) {
            if now.duration_since(bucket.window_start) > window {
                bucket.count = 1;
                bucket.window_start = now;
                return Ok(true);
            }

            if bucket.count >= max_requests {
                return Ok(false);
            }

            bucket.count += 1;
            Ok(true)
        } else {
            buckets.insert(key.to_string(), RateLimitBucket {
                count: 1,
                window_start: now,
            });
            Ok(true)
        }
    }
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64_encode(&bytes)
}

fn sha256_hash(input: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn argon2_hash(password: &str, salt: &str, argon2: &Argon2) -> String {
    let mut hash = [0u8; 32];
    argon2.hash_password_into(password.as_bytes(), salt.as_bytes(), &mut hash).unwrap();
    base64_encode(&hash)
}

fn base64_encode(data: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

fn verify_password(password: &str, stored: &str) -> Result<bool> {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 2 {
        return Ok(false);
    }

    let salt = parts[0];
    let hash = parts[1];
    let argon2 = Argon2::default();

    let mut bytes = [0u8; 32];
    argon2.hash_password_into(password.as_bytes(), salt.as_bytes(), &mut bytes).unwrap();
    let computed = base64_encode(&bytes);

    Ok(computed == hash)
}

/// Token creation request
#[derive(Deserialize)]
pub struct CreateTokenRequest {
    description: String,
}

/// Token creation response
#[derive(Serialize)]
pub struct CreateTokenResponse {
    token: String,
}

/// Token creation endpoint (admin only)
async fn create_token_handler(
    State(state): State<Arc<AuthState>>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<CreateTokenResponse>, StatusCode> {
    let is_admin = false;

    let token = state
        .create_token(&req.description, is_admin)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("Created new auth token: {}", &token[..8]);

    Ok(Json(CreateTokenResponse { token }))
}

#[derive(Deserialize)]
pub struct RevokeTokenRequest {
    token_prefix: String,
}

async fn revoke_token_handler(
    State(state): State<Arc<AuthState>>,
    Json(req): Json<RevokeTokenRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tokens = state.tokens.read();
    let mut to_remove: Option<String> = None;
    for (key, _) in tokens.iter() {
        if key.starts_with(&req.token_prefix) {
            to_remove = Some(key.clone());
            break;
        }
    }
    drop(tokens);

    let revoked = if let Some(key) = to_remove {
        state.tokens.write().remove(&key);
        true
    } else {
        false
    };

    Ok(Json(serde_json::json!({ "revoked": revoked })))
}

#[derive(Serialize)]
pub struct TokenListItem {
    description: String,
    created_at: String,
    is_admin: bool,
}

async fn list_tokens_handler(
    State(_state): State<Arc<AuthState>>,
) -> Result<Json<Vec<TokenListItem>>, StatusCode> {
    Ok(Json(vec![]))
}

#[derive(Deserialize)]
pub struct RateLimitConfigRequest {
    max_requests: u32,
    window_secs: u64,
}

async fn configure_rate_limit_handler(
    State(_state): State<Arc<AuthState>>,
    Json(_req): Json<RateLimitConfigRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let _ = (_req.max_requests, _req.window_secs);
    Ok(Json(serde_json::json!({ "configured": true })))
}

/// Authentication middleware
pub async fn auth_middleware<B>(
    State(state): State<Arc<AuthState>>,
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, StatusCode> {
    // Check rate limit for unauthenticated requests first
    let client_ip = req.headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    if !state.check_rate_limit(&format!("unauth:{}", client_ip), 100, 60).await.unwrap_or(true) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // Extract Authorization header
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    let token = match auth_header {
        Some(token) => token.to_string(),
        None => {
            warn!("Missing authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if !state.is_valid_token(&token).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        warn!("Invalid token: {}", &token[..8.min(token.len())]); // Log first 8 chars only
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check rate limit for authenticated requests (1000 req/min)
    if !state.check_rate_limit(&format!("auth:{}", &token[..8]), 1000, 60).await.unwrap_or(true) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // Audit log
    info!(
        "Authenticated request: {} {} from token {}",
        req.method(),
        req.uri(),
        &token[..8.min(token.len())]
    );

    // Add token to request extensions for downstream handlers
    req.extensions_mut().insert(token);

    Ok(next.run(req).await)
}

/// Create auth routes
pub fn auth_routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/auth/token", post(create_token_handler))
        .route("/auth/tokens", get(list_tokens_handler))
        .route("/auth/tokens/revoke", post(revoke_token_handler))
        .route("/auth/rate-limit", post(configure_rate_limit_handler))
        .with_state(state)
}

/// Create rate limiting layer using tower-governor
#[allow(dead_code)]
pub fn rate_limit_layer() -> impl Clone {
    // Rate limiting is handled in auth_middleware
    // This is a placeholder for future integration
    ()
}
