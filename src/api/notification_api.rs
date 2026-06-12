//! Notification server functions (RT-04).
//!
//! `get_unread_count` seeds the sidebar inbox badge on initial load.
//! Live updates arrive over the per-user WS channel (NotifEvent::UnreadCountUpdated).

use leptos::prelude::*;

/// Return the unread notification count for the currently authenticated user.
///
/// Used to seed the sidebar inbox badge on page load.
/// Live increments arrive over the per-user WS channel (RT-04).
///
/// Auth gate: require_user() rejects unauthenticated requests.
/// T-6-22: count computed server-side via SELECT COUNT(*) WHERE read=0; not client-supplied.
#[server]
pub async fn get_unread_count() -> Result<i64, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read = 0",
    )
    .bind(&user.id)
    .fetch_one(&state.read_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("get_unread_count error: {e}");
        ServerFnError::new("Failed to load notification count")
    })?;

    Ok(count)
}
