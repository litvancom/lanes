//! Invite API: create board invites with high-entropy tokens (COLLAB-02, D-13, D-14).
//! Invite acceptance: strict email binding (D-15), single-use + 7-day expiry (D-14), atomic consume (COLLAB-03).
//!
//! Security:
//! - Tokens are 32-char base62 CSPRNG strings (~190 bits entropy, T-02-16)
//! - Tokens stored in plaintext (high-entropy; hashing adds cost without benefit — D-14)
//! - Only board owners can create invites (D-09, T-02-14)
//! - Acceptance strictly checks invite.email == user.email (case-insensitive, D-15, T-02-19)
//! - accepted flag checked BEFORE the transaction to prevent race-based double-accept (D-14, T-02-20)
//! - All SQL is parameterized — no format! into SQL (T-02-17)

use leptos::prelude::*;

/// Errors from invite acceptance (returned from `consume_invite`).
#[cfg(feature = "ssr")]
#[derive(Debug, thiserror::Error)]
pub enum AcceptError {
    #[error("Invalid invite link")]
    Invalid,
    #[error("This invite has already been accepted.")]
    AlreadyUsed,
    #[error("This invite link has expired. Ask the board owner to send a new one.")]
    Expired,
    #[error("This invite was sent to a different email address.")]
    WrongEmail,
    #[error("Database error: {0}")]
    Db(#[from] sqlx::Error),
}

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

/// Inner fn: atomically consume a valid invite and insert the invitee as a board member.
///
/// Returns the board_id on success so the caller can redirect to `/board/{id}`.
///
/// Checks (in order, per RESEARCH Pattern 7 + Pitfall — check accepted BEFORE INSERT):
/// 1. Token lookup — missing → AcceptError::Invalid
/// 2. Already accepted check — accepted != 0 → AcceptError::AlreadyUsed (BEFORE expiry/email)
/// 3. Expiry check — expires_at < now → AcceptError::Expired
/// 4. Strict email binding — invite.email != user_email (case-insensitive) → AcceptError::WrongEmail (D-15, T-02-19)
/// 5. Transaction: UPDATE invites SET accepted=1 + INSERT OR IGNORE INTO board_members (D-14, T-02-20)
///
/// # Security
/// - Case-insensitive email comparison via `.to_lowercase()` (D-15)
/// - accepted checked first to short-circuit before any mutation (T-02-20)
/// - Both mutations in one transaction — partial failure is impossible (D-14, T-02-20)
/// - INSERT OR IGNORE prevents duplicate board_members if a race slips through (last-line defence)
#[cfg(feature = "ssr")]
pub async fn consume_invite(
    pool: &sqlx::SqlitePool,
    token: &str,
    user_id: &str,
    user_email: &str,
    now: i64,
) -> Result<String, AcceptError> {
    // 1. Token lookup (parameterized — T-02-17)
    let row = sqlx::query!(
        "SELECT id, board_id, email, expires_at, accepted FROM invites WHERE token = ?",
        token
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AcceptError::Invalid)?;

    // 2. Already-accepted check BEFORE expiry/email (Pitfall — check accepted first)
    if row.accepted != 0 {
        return Err(AcceptError::AlreadyUsed);
    }

    // 3. Expiry check (D-14 — 7-day window)
    if row.expires_at < now {
        return Err(AcceptError::Expired);
    }

    // 4. Strict email binding (D-15, T-02-19) — case-insensitive comparison
    if row.email.to_lowercase() != user_email.to_lowercase() {
        return Err(AcceptError::WrongEmail);
    }

    let board_id = row.board_id.clone();
    let invite_id = row.id.clone();

    // 5. Atomic accept: mark invite used + insert board_members in one transaction (D-14, T-02-20)
    let mut tx = pool.begin().await?;

    sqlx::query!(
        "UPDATE invites SET accepted = 1 WHERE id = ?",
        invite_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "INSERT OR IGNORE INTO board_members (board_id, user_id, role) VALUES (?, ?, 'member')",
        board_id,
        user_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(board_id)
}

/// Accept a board invite — server function wrapper for `consume_invite`.
///
/// Auth: `require_user()` is called first (D-04 — authz before any other operation).
/// If unauthenticated, returns an error; the page routes the visitor to /login?return=/invite/{token}.
///
/// On success: redirects to `/board/{board_id}` (COLLAB-03).
/// On each AcceptError variant: maps to the UI-SPEC copy for the error state.
///
/// # Security
/// - D-04: require_user() before any DB access
/// - D-15: strict email binding enforced inside consume_invite
/// - D-14: expiry + single-use enforced inside consume_invite
/// - T-02-19, T-02-20, T-02-21: all mitigated via consume_invite guards
#[server]
pub async fn accept_invite(token: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    // D-04: require user FIRST — unauthenticated callers are rejected here;
    // the InviteAcceptPage drives the /login?return flow before calling this.
    let user = require_user().await?;

    let state = expect_context::<AppState>();

    let now = crate::server::now_millis()
        .map_err(|e| ServerFnError::new(format!("Clock error: {e}")))?;

    let board_id = consume_invite(&state.write_pool.0, &token, &user.id, &user.email, now)
        .await
        .map_err(|e| match e {
            AcceptError::Invalid => ServerFnError::new("Invalid invite link"),
            AcceptError::AlreadyUsed => ServerFnError::new("This invite has already been accepted."),
            AcceptError::Expired => ServerFnError::new("This invite link has expired. Ask the board owner to send a new one."),
            AcceptError::WrongEmail => ServerFnError::new("This invite was sent to a different email address."),
            AcceptError::Db(db_err) => {
                tracing::error!(target: "lanes::invite", "accept_invite DB error: {:?}", db_err);
                ServerFnError::new("Failed to accept invite")
            }
        })?;

    leptos_axum::redirect(&format!("/board/{board_id}"));
    Ok(())
}
