//! Bearer token authentication extractor and per-token rate limiter for the REST API.
//!
//! `ApiUser` is an Axum `FromRequestParts` extractor that:
//!   1. Reads `Authorization: Bearer <token>`
//!   2. Computes SHA-256 of the raw token
//!   3. Looks up the hash in `api_tokens JOIN users`
//!   4. Checks the per-token rate bucket (120 req/min, D-21)
//!   5. Fire-and-forgets a `last_used_at` UPDATE
//!
//! The raw Authorization header value is NEVER logged — only `token_id` after the DB lookup
//! (RESEARCH Security Domain / T-07-16).

#[cfg(feature = "ssr")]
use {
    axum::{
        Json,
        extract::FromRequestParts,
        http::{header, request::Parts, StatusCode},
    },
    sha2::{Digest, Sha256},
    std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    },
    crate::{auth::models::AuthUser, server::state::AppState},
};

// ---------------------------------------------------------------------------
// RateLimiter — in-memory per-token fixed window (D-21 / T-07-17)
// ---------------------------------------------------------------------------

/// Per-token request bucket (120 req / 60 s window).
#[cfg(feature = "ssr")]
pub struct RateBucket {
    pub count: u32,
    pub reset_at: Instant,
}

/// In-memory per-token rate limiter held as a field on `AppState`.
///
/// Cloning is cheap — the inner `Arc<Mutex<HashMap>>` is reference-counted.
/// Rate limit: 120 requests per 60-second **fixed** window per token.
///
/// Note: this is a fixed window, not a sliding window — the counter resets when the
/// window boundary elapses, so up to ~2x `LIMIT` requests can pass across a single
/// boundary. This is an accepted trade-off for D-21's coarse abuse-protection goal.
///
/// Memory: stale buckets are reclaimed opportunistically inside `check()` (any bucket
/// whose window has fully elapsed is dropped), so rotated/expired tokens do not leak.
#[cfg(feature = "ssr")]
#[derive(Clone, Default)]
pub struct RateLimiter(pub Arc<Mutex<HashMap<String, RateBucket>>>);

#[cfg(feature = "ssr")]
impl RateLimiter {
    /// Maximum requests per window.
    const LIMIT: u32 = 120;
    /// Window duration.
    const WINDOW: Duration = Duration::from_secs(60);

    /// Returns `true` if the request is within the rate limit (and increments the counter).
    /// Returns `false` if the token has exhausted its budget for the current window.
    pub fn check(&self, token_id: &str) -> bool {
        let mut map = self.0.lock().unwrap();
        let now = Instant::now();

        // Opportunistic pruning: drop buckets whose window has fully elapsed so the map
        // does not grow unbounded as tokens are rotated/retired (prevents the memory leak).
        map.retain(|_, b| now <= b.reset_at);

        let bucket = map
            .entry(token_id.to_string())
            .or_insert_with(|| RateBucket {
                count: 0,
                reset_at: now + Self::WINDOW,
            });

        if now > bucket.reset_at {
            bucket.count = 0;
            bucket.reset_at = now + Self::WINDOW;
        }

        if bucket.count >= Self::LIMIT {
            return false;
        }
        bucket.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// ApiUser — bearer-token authenticated request identity
// ---------------------------------------------------------------------------

/// Wraps `AuthUser` for API-token-authenticated requests.
///
/// Extracting this type in a handler requires a valid `Authorization: Bearer <token>` header
/// whose SHA-256 hash matches a row in `api_tokens`. Missing/malformed/unknown tokens yield 401.
/// Exceeding the per-token rate limit yields 429.
///
/// The resolved `AuthUser` acts as the request owner: board-scoped handlers enforce the same
/// membership check as the Leptos UI (D-16 / T-07-14).
#[cfg(feature = "ssr")]
pub struct ApiUser(pub AuthUser);

/// Inner resolution logic, separated so it can be unit-tested without a live HTTP socket.
///
/// Given a raw header value (the full `Authorization` header), a read pool for the lookup,
/// and a write pool for the `last_used_at` update, returns the resolved `AuthUser` or an HTTP
/// status indicating the failure reason.
///
/// The lookup runs against `read_pool`; the fire-and-forget `last_used_at` UPDATE runs against
/// `write_pool` because the read pool is opened `.read_only(true)` and would reject the write.
///
/// **Security:** the raw header value is never passed to `tracing::*` (T-07-16).
#[cfg(feature = "ssr")]
pub async fn resolve_api_user(
    read_pool: &sqlx::SqlitePool,
    write_pool: &sqlx::SqlitePool,
    rate_limiter: &RateLimiter,
    auth_header_value: &str,
) -> Result<AuthUser, StatusCode> {
    // 1. Parse "Bearer <token>"
    let token = auth_header_value
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 2. SHA-256 hash (high-entropy random token — no KDF needed, Pitfall 5 / A4)
    // sha2 output is a GenericArray<u8, N>; encode bytes as hex string
    let digest_bytes = Sha256::digest(token.as_bytes());
    let hash = digest_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    // 3. DB lookup — join api_tokens with users
    let row: Option<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT t.id, u.id, u.email, u.display_name, u.avatar_color
         FROM api_tokens t JOIN users u ON t.user_id = u.id
         WHERE t.token_hash = ?",
    )
    .bind(&hash)
    .fetch_optional(read_pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (token_id, user_id, email, display_name, avatar_color) =
        row.ok_or(StatusCode::UNAUTHORIZED)?;

    // 4. Rate limit check — AFTER finding the token so unknown tokens don't pollute the map
    if !rate_limiter.check(&token_id) {
        tracing::warn!("rate limit exceeded for token_id={token_id}");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // 5. Fire-and-forget last_used_at update — MUST use the write pool: the read pool is
    //    opened `.read_only(true)`, so this UPDATE would be rejected with SQLITE_READONLY.
    let write = write_pool.clone();
    let tid = token_id.clone();
    tokio::spawn(async move {
        let now = chrono::Utc::now().timestamp_millis();
        if let Err(e) =
            sqlx::query!("UPDATE api_tokens SET last_used_at = ? WHERE id = ?", now, tid)
                .execute(&write)
                .await
        {
            tracing::warn!("last_used_at update failed for token_id={tid}: {e}");
        }
    });

    Ok(AuthUser {
        id: user_id,
        email,
        display_name: display_name.unwrap_or_default(),
        avatar_color: avatar_color.unwrap_or_else(|| "#7c5cff".to_string()),
        password_hash: None,
        auth_provider: "token".to_string(),
        created_at: 0,
    })
}

#[cfg(feature = "ssr")]
impl FromRequestParts<AppState> for ApiUser {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let err = |code: StatusCode, msg: &'static str| {
            (code, Json(serde_json::json!({"error": msg})))
        };

        // Extract Authorization header
        let auth_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "missing Authorization header"))?;

        resolve_api_user(&state.read_pool.0, &state.write_pool.0, &state.rate_limiter, auth_value)
            .await
            .map(ApiUser)
            .map_err(|status| match status {
                StatusCode::UNAUTHORIZED => err(StatusCode::UNAUTHORIZED, "invalid token"),
                StatusCode::TOO_MANY_REQUESTS => {
                    err(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded")
                }
                _ => err(StatusCode::INTERNAL_SERVER_ERROR, "temporarily unavailable"),
            })
    }
}
