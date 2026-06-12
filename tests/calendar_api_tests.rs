//! Tests for calendar data layer (CAL-01, Plan 07-03 Task 1).
//!
//! Covers:
//!   - `calendar_month_filter`: a card due in the target month is returned;
//!     a card due in the next month is excluded.
//!   - `calendar_monday_start_grid`: `leading_days` / month-bounds math is correct
//!     for a month whose first day is Wednesday (expect 2 leading pad days).
//!
//! Run: cargo test --features ssr --test calendar_api_tests

#[cfg(feature = "ssr")]
mod calendar_api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::derive_key_prefix;
    use tempfile::NamedTempFile;

    // -------------------------------------------------------------------------
    // Shared fixtures
    // -------------------------------------------------------------------------

    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool, read_pool)
    }

    async fn insert_user(pool: &sqlx::SqlitePool, email: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO users (id, email, display_name, avatar_color, auth_provider, created_at)
               VALUES (?, ?, ?, '#7c5cff', 'password', ?)"#,
            id, email, email, now
        )
        .execute(pool)
        .await
        .expect("insert user");
        id
    }

    async fn insert_board(pool: &sqlx::SqlitePool, name: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let key_prefix = derive_key_prefix(name);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
               VALUES (?, ?, ?, '#6366f1', 0, 0, ?, ?)"#,
            id, name, key_prefix, now, now
        )
        .execute(pool)
        .await
        .expect("insert board");
        id
    }

    async fn insert_member(pool: &sqlx::SqlitePool, board_id: &str, user_id: &str) {
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'member')",
            board_id, user_id
        )
        .execute(pool)
        .await
        .expect("insert member");
    }

    async fn insert_list(pool: &sqlx::SqlitePool, board_id: &str, name: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, '0|a', 0)",
            id, board_id, name
        )
        .execute(pool)
        .await
        .expect("insert list");
        id
    }

    async fn insert_card_with_due(
        pool: &sqlx::SqlitePool,
        board_id: &str,
        list_id: &str,
        title: &str,
        card_num: i64,
        due_at: i64,
    ) -> String {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        let id = Uuid::now_v7().to_string();
        // Give each card a unique position using card_num as a seed (simple sequential positions)
        let pos = format!("0|{:0>4}", card_num);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let _ = FractionalIndex::default(); // ensure dep is used
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, checklist_done, checklist_total, comment_count, attachment_count,
               due_at, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, ?, ?, ?)"#,
            id, list_id, board_id, card_num, title, pos, due_at, now, now
        )
        .execute(pool)
        .await
        .expect("insert card with due");
        id
    }

    // -------------------------------------------------------------------------
    // Test: calendar_month_filter
    // A card due within the target month (June 2026) is returned.
    // A card due in the following month (July 2026) is excluded.
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn calendar_month_filter() {
        use lanes::api::calendar_api::{get_calendar_cards_inner, month_bounds_ms};

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user(&write_pool, "alice@test.com").await;
        let board_id = insert_board(&write_pool, "My Board").await;
        insert_member(&write_pool, &board_id, &user_id).await;
        let list_id = insert_list(&write_pool, &board_id, "To Do").await;

        // June 2026 bounds
        let (start_ms, end_ms) = month_bounds_ms(2026, 6);

        // Card due mid-June 2026 (2026-06-15 00:00:00 UTC)
        let june_due_ms: i64 = 1_781_481_600_000; // 2026-06-15 00:00:00 UTC
        assert!(
            june_due_ms >= start_ms && june_due_ms <= end_ms,
            "june_due_ms={june_due_ms} should be within [{start_ms}, {end_ms}]"
        );
        insert_card_with_due(&write_pool, &board_id, &list_id, "June Card", 1, june_due_ms).await;

        // Card due July 2026 (after end_ms)
        let july_due_ms: i64 = end_ms + 60_000; // 1 minute after June ends
        insert_card_with_due(&write_pool, &board_id, &list_id, "July Card", 2, july_due_ms).await;

        let cards = get_calendar_cards_inner(&write_pool, &user_id, start_ms, end_ms)
            .await
            .expect("query should succeed");

        assert_eq!(cards.len(), 1, "only the June card should be returned");
        assert_eq!(cards[0].title, "June Card");
        assert_eq!(cards[0].board_id, board_id);
        assert!(cards[0].due_at.is_some());
    }

    // -------------------------------------------------------------------------
    // Test: calendar_monday_start_grid
    // Verify that leading_days() returns 2 for June 2026 (June 1 is a Monday → 0 pads),
    // and verify another month that starts on Wednesday (e.g., April 2026, starts Wed → 2 pads).
    // -------------------------------------------------------------------------

    #[test]
    fn calendar_monday_start_grid() {
        use lanes::api::calendar_api::leading_days;

        // April 2026 starts on Wednesday (Wed = 2 days from Monday)
        // Mon=0, Tue=1, Wed=2, Thu=3, Fri=4, Sat=5, Sun=6
        let pads_april_2026 = leading_days(2026, 4);
        assert_eq!(
            pads_april_2026, 2,
            "April 2026 starts on Wednesday — expect 2 leading pad days, got {pads_april_2026}"
        );

        // June 2026 starts on Monday (Mon = 0 pads)
        let pads_june_2026 = leading_days(2026, 6);
        assert_eq!(
            pads_june_2026, 0,
            "June 2026 starts on Monday — expect 0 leading pad days, got {pads_june_2026}"
        );

        // January 2026 starts on Thursday (Thu = 3 pads)
        let pads_jan_2026 = leading_days(2026, 1);
        assert_eq!(
            pads_jan_2026, 3,
            "January 2026 starts on Thursday — expect 3 leading pad days, got {pads_jan_2026}"
        );

        // February 2024 (leap year) starts on Thursday (Thu = 3 pads)
        let pads_feb_2024 = leading_days(2024, 2);
        assert_eq!(
            pads_feb_2024, 3,
            "February 2024 starts on Thursday — expect 3 leading pad days, got {pads_feb_2024}"
        );
    }

    // -------------------------------------------------------------------------
    // Test: month_bounds_ms edge cases (December → January wrap, leap year)
    // -------------------------------------------------------------------------

    #[test]
    fn month_bounds_december_wrap() {
        use lanes::api::calendar_api::month_bounds_ms;
        use chrono::{TimeZone, Utc};

        // December 2026 — ensure end_ms is December 31, not January 1
        let (start_ms, end_ms) = month_bounds_ms(2026, 12);

        let start_dt = Utc.timestamp_millis_opt(start_ms).unwrap();
        let end_dt = Utc.timestamp_millis_opt(end_ms).unwrap();

        assert_eq!(start_dt.format("%Y-%m-%d").to_string(), "2026-12-01");
        assert_eq!(end_dt.format("%Y-%m-%d").to_string(), "2026-12-31");
    }

    #[test]
    fn month_bounds_feb_leap_year() {
        use lanes::api::calendar_api::month_bounds_ms;
        use chrono::{TimeZone, Utc};

        // February 2024 — leap year, 29 days
        let (start_ms, end_ms) = month_bounds_ms(2024, 2);

        let start_dt = Utc.timestamp_millis_opt(start_ms).unwrap();
        let end_dt = Utc.timestamp_millis_opt(end_ms).unwrap();

        assert_eq!(start_dt.format("%Y-%m-%d").to_string(), "2024-02-01");
        assert_eq!(end_dt.format("%Y-%m-%d").to_string(), "2024-02-29");
    }
}
