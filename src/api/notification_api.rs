//! Notification server functions (RT-04 / INBOX-01).
//!
//! `get_unread_count` seeds the sidebar inbox badge on initial load.
//! Live updates arrive over the per-user WS channel (NotifEvent::UnreadCountUpdated).
//! `insert_notification_inner` is the shared INSERT used by all notification generators.
//! `notify_watchers_inner` fires watch_activity notifications for all watchers except the actor.

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

/// Internal: insert a notification row for a user.
///
/// Called by all notification generators (scheduler, watch_activity, assigned).
/// Security (T-07-02): actor_id is always the server-resolved caller, never client-supplied.
/// Security (T-07-01): user_id is server-resolved board member, never a global broadcast.
#[cfg(feature = "ssr")]
pub async fn insert_notification_inner(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    board_id: &str,
    card_id: Option<&str>,
    kind: &str,
    actor_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;

    let id = Uuid::now_v7().to_string();
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "INSERT INTO notifications (id, user_id, board_id, card_id, kind, actor_id, read, created_at)
         VALUES (?,?,?,?,?,?,0,?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(board_id)
    .bind(card_id)
    .bind(kind)
    .bind(actor_id)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Internal: notify all watchers of a card except the actor (watch_activity generator).
///
/// D-03: fires on card move, archive, member add/remove, new comment.
/// D-07: self-suppressed — actor_id is excluded from watcher recipients.
/// Returns list of user_ids that received a notification row.
///
/// Security (T-07-02): actor_id is the authenticated caller from the server fn, never client.
/// Security (T-07-03): `user_id != actor_id` guard ensures actor self-suppression.
#[cfg(feature = "ssr")]
pub async fn notify_watchers_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    board_id: &str,
    actor_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    // SELECT watchers for this card excluding the actor (D-07 self-suppression)
    let watcher_ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM watchers WHERE card_id = ? AND user_id != ?",
    )
    .bind(card_id)
    .bind(actor_id)
    .fetch_all(pool)
    .await?;

    let mut notified: Vec<String> = Vec::new();

    for uid in watcher_ids {
        insert_notification_inner(pool, &uid, board_id, Some(card_id), "watch_activity", Some(actor_id))
            .await?;
        notified.push(uid);
    }

    Ok(notified)
}
