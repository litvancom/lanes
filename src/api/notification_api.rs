//! Notification server functions (RT-04 / INBOX-01 / INBOX-02).
//!
//! `get_unread_count` seeds the sidebar inbox badge on initial load.
//! Live updates arrive over the per-user WS channel (NotifEvent::UnreadCountUpdated).
//! `insert_notification_inner` is the shared INSERT used by all notification generators.
//! `notify_watchers_inner` fires watch_activity notifications for all watchers except the actor.
//! `list_notifications` returns paginated rows for the inbox UI.
//! `mark_notification_read` / `mark_all_notifications_read` implement INBOX-02 mark-read.

use leptos::prelude::*;
use crate::models::NotificationRow;

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

/// List notifications for the current user (INBOX-01).
///
/// Returns up to `limit` rows ordered by creation time descending, starting at `offset`.
/// Joins cards/boards for title/name/card_num and users for actor display info.
///
/// Auth gate: require_user() rejects unauthenticated requests.
/// T-07-07: WHERE n.user_id = ? ensures cross-user isolation.
#[server]
pub async fn list_notifications(limit: i64, offset: i64) -> Result<Vec<NotificationRow>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    // sqlx::query_as! requires compile-time schema verification via DATABASE_URL.
    // Use sqlx::query_as with manual FromRow mapping for the complex multi-join query.
    let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, Option<String>, i64, i64)>(
        r#"SELECT n.id, n.kind,
                  n.card_id, c.title as card_title,
                  n.board_id, b.name as board_name,
                  c.card_num,
                  u.display_name as actor_name,
                  u.avatar_color as actor_color,
                  n.read,
                  n.created_at
           FROM notifications n
           LEFT JOIN cards c ON c.id = n.card_id
           LEFT JOIN boards b ON b.id = n.board_id
           LEFT JOIN users u ON u.id = n.actor_id
           WHERE n.user_id = ?
           ORDER BY n.created_at DESC
           LIMIT ? OFFSET ?"#,
    )
    .bind(&user.id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.read_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("list_notifications error: {e}");
        ServerFnError::new("Failed to load notifications")
    })?;

    let notifications = rows
        .into_iter()
        .map(|(id, kind, card_id, card_title, board_id, board_name, card_num, actor_name, actor_color, read_int, created_at)| {
            NotificationRow {
                id,
                kind,
                card_id,
                card_title,
                board_id,
                board_name,
                card_num,
                actor_name,
                actor_color,
                read: read_int != 0,
                created_at,
            }
        })
        .collect();

    Ok(notifications)
}

/// Mark a single notification as read (INBOX-02).
///
/// Security (T-07-06): UPDATE is scoped `WHERE id=? AND user_id=?` — a user
/// cannot mark another user's notification read even with a guessed id.
#[server]
pub async fn mark_notification_read(notif_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    mark_notification_read_inner(&state.write_pool.0, &notif_id, &user.id)
        .await
        .map_err(|e| {
            tracing::error!("mark_notification_read error: {e}");
            ServerFnError::new("Failed to mark notification read")
        })
}

/// Inner fn for mark_notification_read — testable (no Leptos context).
///
/// Security (T-07-06): scoped to `user_id` so a user cannot clear another's notification.
#[cfg(feature = "ssr")]
pub async fn mark_notification_read_inner(
    pool: &sqlx::SqlitePool,
    notif_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notifications SET read=1 WHERE id=? AND user_id=?",
    )
    .bind(notif_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark all notifications for the current user as read (INBOX-02).
///
/// Security (T-07-06): UPDATE is scoped `WHERE user_id=?` — only clears the
/// calling user's notifications.
#[server]
pub async fn mark_all_notifications_read() -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    mark_all_notifications_read_inner(&state.write_pool.0, &user.id)
        .await
        .map_err(|e| {
            tracing::error!("mark_all_notifications_read error: {e}");
            ServerFnError::new("Failed to mark all notifications read")
        })
}

/// Inner fn for mark_all_notifications_read — testable (no Leptos context).
#[cfg(feature = "ssr")]
pub async fn mark_all_notifications_read_inner(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notifications SET read=1 WHERE user_id=?",
    )
    .bind(user_id)
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
