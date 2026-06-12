//! UserNotifRegistry tests (RT-04 / 06-06).
//!
//! Tests for per-user notification channel delivery, isolation, and connection-scoped
//! teardown (CR-01: remove_if_current preserves sibling connections under fan-out).

#[cfg(test)]
mod tests {
    use lanes::server::user_notif_registry::UserNotifRegistry;
    use lanes::models::events::NotifEvent;
    use tokio::sync::mpsc::error::TryRecvError;

    /// A published event reaches the subscribed user (delivery).
    #[tokio::test]
    async fn test_mention_notif_delivery() {
        let reg = UserNotifRegistry::new();
        let (_channel_id, _tx, mut rx) = reg.subscribe("userB");

        let event = NotifEvent::UnreadCountUpdated { count: 1 };
        reg.publish("userB", event.clone());

        match rx.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 1, "delivered count should be 1");
            }
            Ok(other) => panic!("expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("expected event, got {e:?}"),
        }
    }

    /// Publishing to userB does NOT deliver to userA (isolation).
    #[tokio::test]
    async fn test_mention_notif_isolation() {
        let reg = UserNotifRegistry::new();
        let (_channel_id_a, _tx_a, mut rx_a) = reg.subscribe("userA");
        let (_channel_id_b, _tx_b, mut rx_b) = reg.subscribe("userB");

        // Publish only to userB.
        let event = NotifEvent::UnreadCountUpdated { count: 3 };
        reg.publish("userB", event);

        // userA should receive nothing.
        match rx_a.try_recv() {
            Err(TryRecvError::Empty) => {} // correct — userA is isolated
            Ok(ev) => panic!("userA should NOT receive anything, but got {ev:?}"),
            Err(e) => panic!("unexpected error on userA rx: {e:?}"),
        }

        // userB should receive the event.
        match rx_b.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 3);
            }
            Ok(other) => panic!("expected UnreadCountUpdated for userB, got {other:?}"),
            Err(e) => panic!("userB should receive event, got {e:?}"),
        }
    }

    /// After remove_if_current with the connection's own channel_id, publishing no longer delivers.
    #[tokio::test]
    async fn test_notif_cleanup_on_remove_if_current() {
        let reg = UserNotifRegistry::new();
        let (channel_id, _tx, _rx) = reg.subscribe("userB");

        // Remove using the exact channel_id we installed — should succeed.
        reg.remove_if_current("userB", &channel_id);

        // Publishing to a removed user should NOT panic — it silently drops.
        reg.publish("userB", NotifEvent::UnreadCountUpdated { count: 99 });
        // If we got here without panic, the test passes.
    }

    /// CR-01: remove_if_current with tab A's channel_id does NOT wipe tab B's entry.
    ///
    /// Scenario: tab A subscribes, tab B subscribes (both same user); tab A disconnects.
    /// Tab B should still receive events.
    #[tokio::test]
    async fn test_remove_if_current_preserves_newer_tab() {
        let reg = UserNotifRegistry::new();

        // Tab A subscribes first.
        let (channel_id_a, _tx_a, _rx_a) = reg.subscribe("userC");
        // Tab B subscribes second — appended alongside tab A (fan-out, NOT overwrite).
        let (channel_id_b, _tx_b, mut rx_b) = reg.subscribe("userC");

        // Tab A closes — remove_if_current should remove only tab A's entry.
        reg.remove_if_current("userC", &channel_id_a);

        // Tab B should still receive events.
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 7 });
        match rx_b.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 7, "tab B should still receive after tab A's remove_if_current");
            }
            Ok(other) => panic!("expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("tab B should receive event, got {e:?}"),
        }

        // Tab B closes — remove_if_current with tab B's channel_id removes the last entry.
        reg.remove_if_current("userC", &channel_id_b);
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 99 });
        // No panic — silently dropped (user key removed since Vec is empty).
    }

    /// Fan-out: two concurrent connections for the same user BOTH receive a single publish.
    ///
    /// This is the regression that the old one-sender registry would fail:
    /// the second subscribe() used to overwrite the first Sender, so tab A would go stale.
    #[tokio::test]
    async fn test_notif_fanout_two_connections_same_user() {
        let reg = UserNotifRegistry::new();

        // Two connections open concurrently for the same user (e.g. dashboard + board tab).
        let (_channel_id_1, _tx_1, mut rx_1) = reg.subscribe("userD");
        let (_channel_id_2, _tx_2, mut rx_2) = reg.subscribe("userD");

        // One publish should fan out to BOTH receivers.
        reg.publish("userD", NotifEvent::UnreadCountUpdated { count: 5 });

        // Connection 1 must receive the event.
        match rx_1.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 5, "connection 1 should receive the fan-out event");
            }
            Ok(other) => panic!("connection 1: expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("connection 1 should receive event, got {e:?}"),
        }

        // Connection 2 must also receive the event.
        match rx_2.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 5, "connection 2 should receive the fan-out event");
            }
            Ok(other) => panic!("connection 2: expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("connection 2 should receive event, got {e:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Wave-0 RED tests for notification generators (INBOX-01 / 07-01)
// ---------------------------------------------------------------------------
//
// RED-STATE NOTE: These tests reference functions that are stubs (`todo!()`)
// in the Task-1 codebase. They will panic if run now — this is expected RED.
// Task-1 gate is COMPILE-ONLY by design (cargo test --features ssr --no-run).
// Task 2 makes due_soon_generates_notification + due_soon_dedup GREEN.
// Task 3 makes watch_activity_notification + assigned_notification GREEN.

#[cfg(all(test, feature = "ssr"))]
mod generator_tests {
    use lanes::server::db::run_migrations;
    use lanes::server::scheduler::scan_due_notifications_once;
    use lanes::api::notification_api::{insert_notification_inner, notify_watchers_inner};
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    /// Create a temp DB with all migrations applied; return (file guard, write_pool).
    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let write_pool = lanes::server::db::make_write_pool(&url)
            .await
            .expect("make_write_pool");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool)
    }

    /// Insert a minimal user row; returns user_id.
    async fn insert_user(pool: &sqlx::SqlitePool, email: &str) -> String {
        let id = Uuid::now_v7().to_string();
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
             VALUES (?, ?, 'x', ?, '#7c5cff', 'password', ?)",
        )
        .bind(&id)
        .bind(email)
        .bind(email)
        .bind(now)
        .execute(pool)
        .await
        .expect("insert user");
        id
    }

    /// Insert a minimal board; returns board_id.
    async fn insert_board(pool: &sqlx::SqlitePool, owner_id: &str) -> String {
        let id = Uuid::now_v7().to_string();
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        sqlx::query(
            "INSERT INTO boards (id, name, key_prefix, color, next_card_num, starred, archived, created_at, updated_at) \
             VALUES (?, 'board', 'TST', '#ff0', 1, 0, 0, ?, ?)",
        )
        .bind(&id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("insert board");

        // owner is a member
        sqlx::query(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
        )
        .bind(&id)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("insert board_member");

        id
    }

    /// Insert a minimal card with a due_at timestamp; returns card_id.
    async fn insert_card_with_due(
        pool: &sqlx::SqlitePool,
        board_id: &str,
        due_at: i64,
    ) -> String {
        let card_id = Uuid::now_v7().to_string();
        let list_id = Uuid::now_v7().to_string();
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        sqlx::query(
            "INSERT INTO lists (id, board_id, name, position, archived) \
             VALUES (?, ?, 'list', 'a', 0)",
        )
        .bind(&list_id)
        .bind(board_id)
        .execute(pool)
        .await
        .expect("insert list");

        sqlx::query(
            "INSERT INTO cards (id, list_id, board_id, card_num, title, position, done, archived, created_at, updated_at, due_at) \
             VALUES (?, ?, ?, 1, 'card', 'a', 0, 0, ?, ?, ?)",
        )
        .bind(&card_id)
        .bind(&list_id)
        .bind(board_id)
        .bind(now)
        .bind(now)
        .bind(due_at)
        .execute(pool)
        .await
        .expect("insert card");

        card_id
    }

    /// INBOX-01: a card due within 24h generates a due_soon notification for each board member.
    ///
    /// RED at Task 1: scan_due_notifications_once is todo!() — panics if run.
    /// GREEN at Task 2 when the real implementation replaces the stub.
    #[tokio::test]
    async fn due_soon_generates_notification() {
        let (_file, pool) = test_db().await;

        let user_id = insert_user(&pool, "alice@test.com").await;
        let board_id = insert_board(&pool, &user_id).await;

        // Card due 12h from now (within 24h window)
        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let due_at = now_ms + 12 * 60 * 60 * 1000; // +12h
        let _card_id = insert_card_with_due(&pool, &board_id, due_at).await;

        let inserted = scan_due_notifications_once(&pool, now_ms)
            .await
            .expect("scan_due_notifications_once failed");

        assert!(
            !inserted.is_empty(),
            "expected at least one due_soon notification to be inserted"
        );

        // Verify the notification row is in the DB
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'due_soon' AND read = 0",
        )
        .bind(&user_id)
        .fetch_one(&pool)
        .await
        .expect("count query");
        assert_eq!(count, 1, "expected exactly one due_soon notification");
    }

    /// INBOX-01 D-05: a second scan with the same now_ms inserts zero additional rows (dedup).
    ///
    /// RED at Task 1: scan_due_notifications_once is todo!() — panics if run.
    /// GREEN at Task 2.
    #[tokio::test]
    async fn due_soon_dedup() {
        let (_file, pool) = test_db().await;

        let user_id = insert_user(&pool, "bob@test.com").await;
        let board_id = insert_board(&pool, &user_id).await;

        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let due_at = now_ms + 12 * 60 * 60 * 1000;
        let _card_id = insert_card_with_due(&pool, &board_id, due_at).await;

        // First scan: inserts one row
        let first = scan_due_notifications_once(&pool, now_ms)
            .await
            .expect("first scan failed");
        assert!(!first.is_empty(), "first scan should insert a notification");

        // Second scan with same now_ms: dedup — zero new rows
        let second = scan_due_notifications_once(&pool, now_ms)
            .await
            .expect("second scan failed");
        assert!(
            second.is_empty(),
            "second scan should insert zero rows (dedup D-05), got: {second:?}"
        );
    }

    /// INBOX-01 D-03/D-07: watch_activity notification fires for watchers except the actor.
    ///
    /// RED at Task 1: notify_watchers_inner is todo!() — panics if run.
    /// GREEN at Task 3.
    #[tokio::test]
    async fn watch_activity_notification() {
        let (_file, pool) = test_db().await;

        let actor_id = insert_user(&pool, "carol@test.com").await;
        let watcher_id = insert_user(&pool, "dave@test.com").await;
        let board_id = insert_board(&pool, &actor_id).await;

        // Add watcher as board member too
        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'member')",
        )
        .bind(&board_id)
        .bind(&watcher_id)
        .execute(&pool)
        .await
        .expect("add watcher as board member");

        let card_id = insert_card_with_due(&pool, &board_id, now_ms + 1000).await;

        // Add watcher (not the actor)
        sqlx::query("INSERT OR IGNORE INTO watchers (card_id, user_id) VALUES (?, ?)")
            .bind(&card_id)
            .bind(&watcher_id)
            .execute(&pool)
            .await
            .expect("insert watcher");

        // Actor also watches (should be self-suppressed — D-07)
        sqlx::query("INSERT OR IGNORE INTO watchers (card_id, user_id) VALUES (?, ?)")
            .bind(&card_id)
            .bind(&actor_id)
            .execute(&pool)
            .await
            .expect("insert actor watcher");

        // Notify watchers — actor should be excluded
        let notified = notify_watchers_inner(&pool, &card_id, &board_id, &actor_id)
            .await
            .expect("notify_watchers_inner failed");

        // watcher_id should receive a notification; actor should not
        assert!(
            notified.contains(&watcher_id),
            "watcher should receive watch_activity notification"
        );
        assert!(
            !notified.contains(&actor_id),
            "actor should NOT receive self-notification (D-07)"
        );

        // Verify DB row
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'watch_activity' AND read = 0",
        )
        .bind(&watcher_id)
        .fetch_one(&pool)
        .await
        .expect("count query");
        assert_eq!(count, 1, "expected one watch_activity notification for watcher");

        let actor_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'watch_activity'",
        )
        .bind(&actor_id)
        .fetch_one(&pool)
        .await
        .expect("actor count query");
        assert_eq!(actor_count, 0, "actor must not receive watch_activity for own action");
    }

    /// INBOX-01 D-04/D-07: assigned notification fires when user is added as card member.
    ///
    /// Self-add (adding actor as themselves) must NOT produce an assigned notification.
    ///
    /// RED at Task 1: insert_notification_inner + assign_member_inner integration is pending.
    /// GREEN at Task 3 when notify_watchers_inner is wired and assigned hook is added.
    #[tokio::test]
    async fn assigned_notification() {
        let (_file, pool) = test_db().await;

        let actor_id = insert_user(&pool, "eve@test.com").await;
        let assignee_id = insert_user(&pool, "frank@test.com").await;
        let board_id = insert_board(&pool, &actor_id).await;

        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Add assignee as board member
        sqlx::query(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'member')",
        )
        .bind(&board_id)
        .bind(&assignee_id)
        .execute(&pool)
        .await
        .expect("add assignee as board member");

        let card_id = insert_card_with_due(&pool, &board_id, now_ms + 1000).await;

        // Assign a different user — should produce an assigned notification for that user
        insert_notification_inner(&pool, &assignee_id, &board_id, Some(&card_id), "assigned", Some(&actor_id))
            .await
            .expect("insert_notification_inner for assigned");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'assigned' AND read = 0",
        )
        .bind(&assignee_id)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 1, "assignee should have one assigned notification");

        // Self-assign: actor adds themselves — must NOT create an assigned notification
        // (This validates the D-07 suppression logic in assign_member in Task 3.)
        // For now we assert there's no assigned notification for actor_id.
        let actor_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'assigned'",
        )
        .bind(&actor_id)
        .fetch_one(&pool)
        .await
        .expect("actor count");
        assert_eq!(actor_count, 0, "actor must not receive assigned notification for self-add");
    }
}
