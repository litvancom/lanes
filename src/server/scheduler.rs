//! Background notification scheduler (INBOX-01 / D-01/D-02/D-05/D-10).
//!
//! `run_due_notification_scheduler`: every 5 min, scans cards with upcoming/past
//!   due dates and inserts `due_soon`/`overdue` notification rows for all board members
//!   (deduped: at most one unread per kind per card — D-05).
//!
//! `run_notif_cleanup`: hourly, prunes read notifications older than 30 days (D-10).
//!
//! `scan_due_notifications_once`: the testable inner function that injects `now_ms`
//!   deterministically for unit tests.

/// Testable inner scan: find all due_soon + overdue notifications to insert for a given now_ms.
///
/// Injects `now_ms` deterministically (no wall clock) for unit tests.
/// Returns the list of notification IDs inserted.
///
/// D-05 dedup: NOT EXISTS guard ensures at most one unread due_soon and one overdue per card.
/// T-07-01: only board_members of the card's board receive notifications (JOIN board_members).
/// Pitfall 3 / A1: ticks run sequentially in a single loop — no concurrent-insert race.
#[cfg(feature = "ssr")]
pub async fn scan_due_notifications_once(
    pool: &sqlx::SqlitePool,
    now_ms: i64,
) -> Result<Vec<String>, sqlx::Error> {
    use crate::api::notification_api::insert_notification_inner;

    let soon_ms = now_ms + 24 * 60 * 60 * 1000; // 24h window (D-02)
    let mut inserted_ids: Vec<String> = Vec::new();

    // --- due_soon: archived=0, done=0, due_at in (now_ms, soon_ms] ---
    // Dedup: NOT EXISTS unread due_soon for this (user_id, card_id) pair.
    let due_soon_rows: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT c.id, bm.user_id, c.board_id
           FROM cards c
           JOIN board_members bm ON bm.board_id = c.board_id
           WHERE c.archived = 0
             AND c.done = 0
             AND c.due_at > ?
             AND c.due_at <= ?
             AND NOT EXISTS (
               SELECT 1 FROM notifications n
               WHERE n.card_id = c.id
                 AND n.user_id = bm.user_id
                 AND n.kind = 'due_soon'
                 AND n.read = 0
             )"#,
    )
    .bind(now_ms)
    .bind(soon_ms)
    .fetch_all(pool)
    .await?;

    for (card_id, user_id, board_id) in due_soon_rows {
        // Re-check dedup within the same tick (Pitfall 3 — sequential is sufficient).
        let notif_id = uuid::Uuid::now_v7().to_string();

        sqlx::query(
            "INSERT INTO notifications (id, user_id, board_id, card_id, kind, actor_id, read, created_at)
             VALUES (?,?,?,?,?,?,0,?)",
        )
        .bind(&notif_id)
        .bind(&user_id)
        .bind(&board_id)
        .bind(&card_id)
        .bind("due_soon")
        .bind(Option::<String>::None)
        // Use the injected scan time so the tick is internally consistent and deterministic
        // for tests (rather than a fresh per-row wall-clock read).
        .bind(now_ms)
        .execute(pool)
        .await?;

        inserted_ids.push(notif_id);
    }

    // --- overdue: archived=0, done=0, due_at IS NOT NULL AND due_at < now_ms ---
    let overdue_rows: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT c.id, bm.user_id, c.board_id
           FROM cards c
           JOIN board_members bm ON bm.board_id = c.board_id
           WHERE c.archived = 0
             AND c.done = 0
             AND c.due_at IS NOT NULL
             AND c.due_at < ?
             AND NOT EXISTS (
               SELECT 1 FROM notifications n
               WHERE n.card_id = c.id
                 AND n.user_id = bm.user_id
                 AND n.kind = 'overdue'
                 AND n.read = 0
             )"#,
    )
    .bind(now_ms)
    .fetch_all(pool)
    .await?;

    for (card_id, user_id, board_id) in overdue_rows {
        let notif_id = uuid::Uuid::now_v7().to_string();

        sqlx::query(
            "INSERT INTO notifications (id, user_id, board_id, card_id, kind, actor_id, read, created_at)
             VALUES (?,?,?,?,?,?,0,?)",
        )
        .bind(&notif_id)
        .bind(&user_id)
        .bind(&board_id)
        .bind(&card_id)
        .bind("overdue")
        .bind(Option::<String>::None)
        // Use the injected scan time so the tick is internally consistent and deterministic.
        .bind(now_ms)
        .execute(pool)
        .await?;

        inserted_ids.push(notif_id);
    }

    Ok(inserted_ids)
}

/// Run the due-date notification scheduler.
///
/// Spawned once in start_server after the presence sweep block.
/// Loops with a 5-minute interval (skips the first tick so server boot is clean).
/// D-01/D-02: generates due_soon (24h window) and overdue notifications.
/// D-05: dedup via NOT EXISTS guard in scan_due_notifications_once.
/// T-07-01: board members only (JOIN board_members in scan).
#[cfg(feature = "ssr")]
pub async fn run_due_notification_scheduler(
    write_pool: sqlx::SqlitePool,
    user_notifs: crate::server::user_notif_registry::UserNotifRegistry,
) {
    use std::time::Duration;
    use crate::models::events::NotifEvent;
    use crate::server::now_millis;

    loop {
        tokio::time::sleep(Duration::from_secs(5 * 60)).await;

        let now_ms = match now_millis() {
            Ok(ms) => ms,
            Err(e) => {
                tracing::error!("scheduler: clock error: {e}");
                continue;
            }
        };

        let inserted = match scan_due_notifications_once(&write_pool, now_ms).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!("scan_due_notifications_once error: {e}");
                continue;
            }
        };

        if inserted.is_empty() {
            continue;
        }

        tracing::info!("scheduler: inserted {} due notifications", inserted.len());

        // For each inserted notification, publish a live UnreadCountUpdated to the user.
        for notif_id in &inserted {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT user_id FROM notifications WHERE id = ?",
            )
            .bind(notif_id)
            .fetch_optional(&write_pool)
            .await
            .unwrap_or(None);

            if let Some((uid,)) = row {
                let count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read = 0",
                )
                .bind(&uid)
                .fetch_one(&write_pool)
                .await
                .unwrap_or(0);

                user_notifs.publish(&uid, NotifEvent::UnreadCountUpdated { count });
            }
        }
    }
}

/// Run the read-notification cleanup task.
///
/// Spawned once in start_server.
/// Loops hourly, deleting read notifications older than 30 days (D-10).
/// Security (T-07-04): prevents unbounded table growth.
#[cfg(feature = "ssr")]
pub async fn run_notif_cleanup(write_pool: sqlx::SqlitePool) {
    use std::time::Duration;
    use crate::server::now_millis;

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;

        let now_ms = match now_millis() {
            Ok(ms) => ms,
            Err(e) => {
                tracing::error!("notif_cleanup: clock error: {e}");
                continue;
            }
        };

        // Prune read notifications older than 30 days
        let cutoff_ms = now_ms - 30 * 24 * 60 * 60 * 1000_i64;

        match sqlx::query(
            "DELETE FROM notifications WHERE read = 1 AND created_at < ?",
        )
        .bind(cutoff_ms)
        .execute(&write_pool)
        .await
        {
            Ok(result) => {
                let rows = result.rows_affected();
                if rows > 0 {
                    tracing::info!("notif_cleanup: pruned {} read notifications", rows);
                }
            }
            Err(e) => {
                tracing::error!("notif_cleanup error: {e}");
            }
        }
    }
}
