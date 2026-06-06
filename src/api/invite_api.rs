//! Invite API: create board invites with high-entropy tokens (COLLAB-02, D-13, D-14).
//!
//! Security:
//! - Tokens are 32-char base62 CSPRNG strings (~190 bits entropy, T-02-16)
//! - Tokens stored in plaintext (high-entropy; hashing adds cost without benefit — D-14)
//! - Only board owners can create invites (D-09, T-02-14)
//! - All SQL is parameterized — no format! into SQL (T-02-17)

use leptos::prelude::*;

/// Generate a cryptographically random 32-character base62 token.
/// Base62 charset: 0-9, a-z, A-Z (62 chars). 32 chars → ~190 bits entropy.
/// Uses the OS/thread CSPRNG from rand 0.10 (T-02-16 — not guessable/enumerable).
pub fn generate_invite_token() -> String {
    #[cfg(feature = "ssr")]
    {
        use rand::RngExt;
        const BASE62: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut rng = rand::rng();
        (0..32)
            .map(|_| {
                let idx = rng.random_range(0..62usize);
                BASE62[idx] as char
            })
            .collect()
    }
    #[cfg(not(feature = "ssr"))]
    {
        // Not callable from WASM; token generation is server-side only
        String::new()
    }
}

/// Inner fn: insert an invites row and return the token.
///
/// - `email` is lowercased + trimmed before storage.
/// - `expires_at = now + 7 days` (D-14 single-use 7-day window).
/// - `accepted = 0` (new invite is always unaccepted).
/// - Token is generated via CSPRNG and stored in plaintext (D-14, T-02-16).
/// - Re-inviting the same email creates a fresh row with a new token (D-14).
///
/// All SQL is parameterized — no format! interpolation (T-02-17).
#[cfg(feature = "ssr")]
pub async fn create_invite(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    inviter_id: &str,
    email: &str,
    now: i64,
) -> Result<String, sqlx::Error> {
    use uuid::Uuid;

    let email = email.trim().to_lowercase();
    let id = Uuid::now_v7().to_string();
    let token = generate_invite_token();
    let expires_at = now + 7 * 24 * 3600 * 1000; // 7 days in milliseconds (D-14)
    let accepted: i64 = 0;

    sqlx::query!(
        r#"INSERT INTO invites (id, board_id, inviter_id, email, token, expires_at, accepted)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        id,
        board_id,
        inviter_id,
        email,
        token,
        expires_at,
        accepted,
    )
    .execute(pool)
    .await?;

    Ok(token)
}

/// Create a board invite. Owner-only (D-09, T-02-14).
///
/// Returns the invite URL path `/invite/{token}` so the UI can display it (D-13).
/// The link is returned regardless of whether email delivery succeeded (D-13).
///
/// Security:
/// - `require_board_member` returns a generic "board not found" for non-members (D-12)
/// - Owner-only check rejects non-owner members explicitly (D-09)
/// - Email is validated (trim, lowercase, non-empty, must contain '@')
/// - All DB access is parameterized (T-02-17)
#[server]
pub async fn invite_member(
    board_id: String,
    email: String,
) -> Result<String, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    // Auth + membership gate (D-12 — returns "board not found" for non-members)
    let (user, role) = require_board_member(&board_id, &state.read_pool.0).await?;

    // Owner-only enforcement (D-09, T-02-14)
    if role != "owner" {
        return Err(ServerFnError::new("Only the board owner can invite members"));
    }

    // Validate email
    let email = email.trim().to_lowercase();
    if email.is_empty() {
        return Err(ServerFnError::new("Email address is required"));
    }
    if !email.contains('@') {
        return Err(ServerFnError::new("Invalid email address"));
    }

    let now = crate::server::now_millis()
        .map_err(|e| ServerFnError::new(format!("Clock error: {e}")))?;

    // Create invite row; returns token (D-14 — fresh token each time)
    let token = create_invite(&state.write_pool.0, &board_id, &user.id, &email, now)
        .await
        .map_err(|e| {
            tracing::error!("create_invite DB error: {:?}", e);
            ServerFnError::new("Failed to create invite")
        })?;

    let invite_url = format!("/invite/{token}");

    // Look up board name for the email (parameterized query — T-02-17)
    let board_name: Option<String> = sqlx::query_scalar!(
        "SELECT name FROM boards WHERE id = ?",
        board_id
    )
    .fetch_optional(&state.read_pool.0)
    .await
    .unwrap_or(None);
    let board_name = board_name.as_deref().unwrap_or("your board");

    // Attempt email delivery via the pluggable mailer.
    // On Err: log and CONTINUE — the link is the primary channel (D-13).
    if let Err(e) = state.mailer.send_invite(&email, &invite_url, board_name).await {
        tracing::warn!(
            target: "lanes::invite",
            error = %e,
            %email,
            "Invite email delivery failed; invite link still valid (D-13)"
        );
    }

    // Always return the invite URL (D-13 — link is the primary delivery mechanism)
    Ok(invite_url)
}
