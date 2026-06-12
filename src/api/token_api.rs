//! Personal API-token management server functions (API-03).
//!
//! Security constraints (D-17 / D-18):
//! - Only a SHA-256 hash of the raw token is stored in `api_tokens.token_hash`.
//! - The raw token is shown once at creation and never retrievable.
//! - `list_api_tokens` returns `ApiTokenMeta` — no `token_hash` field.
//! - `revoke_api_token` is scoped to the authenticated user (no IDOR).
//!
//! Threat mitigations:
//! - T-07-16: raw token value never logged; only token_id after DB insert.
//! - T-07-15: token creation rate-limited via Leptos session (normal UI path).

use leptos::prelude::*;

// ---------------------------------------------------------------------------
// Inner (unit-testable, no Leptos context) — SSR only
// ---------------------------------------------------------------------------

/// Generate a new API token for `user_id`, store only its SHA-256 hash, and return
/// the raw token (to be shown once).
///
/// Token format: 32 random bytes → 64-char lowercase hex string.
/// Hash: `sha2::Sha256` of the raw hex string (high-entropy input → no KDF needed).
#[cfg(feature = "ssr")]
pub async fn create_api_token_inner(
    name: String,
    user_id: &str,
    pool: &sqlx::SqlitePool,
) -> Result<crate::models::CreatedToken, sqlx::Error> {
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    // Validate token name
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(sqlx::Error::Protocol("token name cannot be empty".into()));
    }
    if name.len() > 80 {
        return Err(sqlx::Error::Protocol("token name must be 80 characters or fewer".into()));
    }

    // Generate 32 random bytes → 64-char hex raw token.
    // The rng is dropped before any `.await` so the future remains `Send`.
    let raw_token: String = {
        use rand::RngExt;
        let mut rng = rand::rng();
        (0..32)
            .map(|_| format!("{:02x}", rng.random_range(0u32..=255u32) as u8))
            .collect()
    };

    // SHA-256 hash of the raw token — only this is stored (D-17)
    let hash_bytes = Sha256::digest(raw_token.as_bytes());
    let token_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    let id = Uuid::now_v7().to_string();
    let now = crate::server::now_millis().map_err(|_| sqlx::Error::Protocol("clock error".into()))?;

    sqlx::query!(
        "INSERT INTO api_tokens (id, user_id, name, token_hash, created_at) VALUES (?, ?, ?, ?, ?)",
        id,
        user_id,
        name,
        token_hash,
        now,
    )
    .execute(pool)
    .await?;

    // T-07-16: log token_id only — never log raw_token or token_hash
    tracing::info!("api_token created token_id={id} user_id={user_id}");

    Ok(crate::models::CreatedToken {
        id,
        name,
        raw_token,
    })
}

// ---------------------------------------------------------------------------
// Server functions (called from Leptos components via the UI)
// ---------------------------------------------------------------------------

/// Create a new personal API token.  Returns `CreatedToken` containing the raw token
/// (shown once) and token metadata.  The raw token is never stored or retrievable again.
#[server(CreateApiToken, "/api")]
pub async fn create_api_token(name: String) -> Result<crate::models::CreatedToken, ServerFnError> {
    use crate::server::state::AppState;
    use crate::auth::helpers::require_user;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    create_api_token_inner(name, &user.id, &state.write_pool.0)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("cannot be empty") || msg.contains("or fewer") {
                ServerFnError::new(msg)
            } else {
                tracing::error!("create_api_token DB error: {e}");
                ServerFnError::new("internal error")
            }
        })
}

/// List all tokens for the current user.  Returns metadata only — no token hashes.
#[server(ListApiTokens, "/api")]
pub async fn list_api_tokens() -> Result<Vec<crate::models::ApiTokenMeta>, ServerFnError> {
    use crate::server::state::AppState;
    use crate::auth::helpers::require_user;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    let rows: Vec<(String, String, i64, Option<i64>)> = sqlx::query_as(
        r#"SELECT id, name, created_at, last_used_at
           FROM api_tokens
           WHERE user_id = ?
           ORDER BY created_at DESC"#,
    )
    .bind(&user.id)
    .fetch_all(&state.read_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("list_api_tokens DB error: {e}");
        ServerFnError::new("internal error")
    })?;

    Ok(rows
        .into_iter()
        .map(|(id, name, created_at, last_used_at)| crate::models::ApiTokenMeta {
            id,
            name,
            created_at,
            last_used_at,
        })
        .collect())
}

/// Revoke (delete) an API token.  Scoped to the authenticated user — cannot revoke
/// another user's token (no IDOR).
#[server(RevokeApiToken, "/api")]
pub async fn revoke_api_token(token_id: String) -> Result<(), ServerFnError> {
    use crate::server::state::AppState;
    use crate::auth::helpers::require_user;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    sqlx::query!(
        "DELETE FROM api_tokens WHERE id = ? AND user_id = ?",
        token_id,
        user.id,
    )
    .execute(&state.write_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("revoke_api_token DB error: {e}");
        ServerFnError::new("internal error")
    })?;

    Ok(())
}
