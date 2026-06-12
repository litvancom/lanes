//! Background notification scheduler (INBOX-01 / D-01/D-02/D-05/D-10).
//!
//! `run_due_notification_scheduler`: every 5 min, scans cards with upcoming/past
//!   due dates and inserts `due_soon`/`overdue` notification rows for all board members
//!   (deduped: at most one unread per kind per card — D-05).
//!
//! `run_notif_cleanup`: hourly, prunes read notifications older than 30 days (D-10).
//!
//! `scan_due_notifications_once`: the testable inner function that Task-1 tests target.
//!   This is a stub replaced with the real implementation in Task 2.

/// Run the due-date notification scheduler.
///
/// Spawned once in start_server after the presence sweep block.
/// Loops with a 5-minute interval. Calls scan_due_notifications_once each tick.
/// D-01/D-02: generates due_soon (24h window) and overdue notifications.
/// D-05: dedup via NOT EXISTS guard in scan_due_notifications_once.
///
/// This is a stub — replaced with the real implementation in Task 2.
#[cfg(feature = "ssr")]
pub async fn run_due_notification_scheduler(
    write_pool: sqlx::SqlitePool,
    user_notifs: crate::server::user_notif_registry::UserNotifRegistry,
) {
    todo!("run_due_notification_scheduler — implemented in Task 2 (07-01)")
}

/// Run the read-notification cleanup task.
///
/// Spawned once in start_server.
/// Loops hourly, deleting read notifications older than 30 days (D-10).
/// Security (T-07-04): prevents unbounded table growth.
///
/// This is a stub — replaced with the real implementation in Task 2.
#[cfg(feature = "ssr")]
pub async fn run_notif_cleanup(write_pool: sqlx::SqlitePool) {
    todo!("run_notif_cleanup — implemented in Task 2 (07-01)")
}

/// Testable inner scan: find all due_soon + overdue notifications to insert for a given now_ms.
///
/// Injects `now_ms` deterministically (no wall clock) for unit tests.
/// Returns the list of notification IDs inserted.
///
/// D-05 dedup: NOT EXISTS guard ensures at most one unread due_soon and one overdue per card.
/// T-07-01: only board_members of the card's board receive notifications (JOIN board_members).
///
/// This is a stub — replaced with the real implementation in Task 2.
#[cfg(feature = "ssr")]
pub async fn scan_due_notifications_once(
    pool: &sqlx::SqlitePool,
    now_ms: i64,
) -> Result<Vec<String>, sqlx::Error> {
    todo!("scan_due_notifications_once — implemented in Task 2 (07-01)")
}
