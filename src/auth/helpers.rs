use leptos::prelude::ServerFnError;
use crate::auth::backend::EmailPasswordBackend;
use crate::auth::models::AuthUser;

/// Type alias hiding the backend type from calling code (D-04).
pub type AuthSession = axum_login::AuthSession<EmailPasswordBackend>;

/// Ten-color avatar palette from design handoff (D-21).
const AVATAR_COLORS: &[&str] = &[
    "#7c5cff", "#ff5c87", "#ff8c42", "#ffd166", "#06d6a0",
    "#118ab2", "#e76f51", "#a8dadc", "#457b9d", "#e63946",
];

/// Deterministically assign an avatar color from the design palette based on email (D-21).
/// Hash is a simple byte-sum fold — deterministic and collision-resistant enough for color assignment.
pub fn derive_avatar_color(email: &str) -> &'static str {
    let hash = email.bytes().fold(0usize, |acc, b| acc.wrapping_add(b as usize));
    AVATAR_COLORS[hash % AVATAR_COLORS.len()]
}

/// Extract the current authenticated user from the request's AuthSession.
/// Returns Err("unauthenticated") if no session or session has no user (D-04).
/// Callers translate this to a /login redirect on the client side.
#[cfg(feature = "ssr")]
pub async fn require_user() -> Result<AuthUser, ServerFnError> {
    use leptos_axum::extract;
    let auth_session: AuthSession = extract()
        .await
        .map_err(|_| ServerFnError::new("Session unavailable"))?;
    auth_session.user.ok_or_else(|| ServerFnError::new("unauthenticated"))
}

/// Assert that the current user is a member of the given board, returning their role.
/// Returns Err("board not found") for BOTH non-members and deleted boards (D-12).
/// Generic error prevents board existence disclosure — does not reveal whether board exists.
#[cfg(feature = "ssr")]
pub async fn require_board_member(
    board_id: &str,
    pool: &sqlx::SqlitePool,
) -> Result<(AuthUser, String), ServerFnError> {
    let user = require_user().await?;
    let role: Option<String> = sqlx::query_scalar!(
        "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
        board_id,
        user.id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("require_board_member DB error: {e}");
        ServerFnError::new("board not found") // D-12: never reveal existence
    })?;

    match role {
        Some(r) => Ok((user, r)),
        None => Err(ServerFnError::new("board not found")), // D-12
    }
}
