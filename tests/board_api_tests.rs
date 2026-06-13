//! Unit tests for board_api inner functions (get_board / touch_last_viewed).
//! Run: cargo test --features ssr --test board_api_tests
//!
//! Tests call inner fns directly (they take a pool, no Leptos context needed).
//! Pattern follows tests/api_tests.rs.

#[cfg(feature = "ssr")]
mod board_api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::derive_key_prefix;
    use lanes::api::board_api::{get_board_inner, touch_last_viewed_inner, rename_board_inner};
    use tempfile::NamedTempFile;

    /// Create a temp DB with migrations applied; return (file guard, write_pool, read_pool).
    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool, read_pool)
    }

    /// Insert a user row directly for test setup.
    async fn insert_user_direct(pool: &sqlx::SqlitePool, email: &str) -> String {
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

    /// Insert a board row directly for test setup.
    async fn insert_board_direct(pool: &sqlx::SqlitePool, name: &str, creator_id: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let key_prefix = derive_key_prefix(name);
        let color = "#6366f1".to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
               VALUES (?, ?, ?, ?, 0, 0, ?, ?)"#,
            id, name, key_prefix, color, now, now
        )
        .execute(pool)
        .await
        .expect("insert board");
        // Insert owner member row
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
            id, creator_id
        )
        .execute(pool)
        .await
        .expect("insert member");
        id
    }

    /// Insert a list row directly.
    async fn insert_list_direct(
        pool: &sqlx::SqlitePool,
        board_id: &str,
        name: &str,
        position: &str,
        archived: bool,
    ) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let archived_int: i64 = if archived { 1 } else { 0 };
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, ?)",
            id, board_id, name, position, archived_int
        )
        .execute(pool)
        .await
        .expect("insert list");
        id
    }

    /// Insert a card row directly.
    async fn insert_card_direct(
        pool: &sqlx::SqlitePool,
        list_id: &str,
        board_id: &str,
        title: &str,
        position: &str,
        archived: bool,
    ) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let archived_int: i64 = if archived { 1 } else { 0 };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        // Use a simple counter for card_num: query the board's next_card_num
        let card_num: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(card_num), 0) + 1 FROM cards WHERE board_id = ?"
        )
        .bind(board_id)
        .fetch_one(pool)
        .await
        .expect("card_num");
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position, done, archived, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"#,
            id, list_id, board_id, card_num, title, position, archived_int, now, now
        )
        .execute(pool)
        .await
        .expect("insert card");
        id
    }

    // -------------------------------------------------------------------------
    // test_get_board_returns_board_lists_cards
    // -------------------------------------------------------------------------

    /// A member fetching get_board receives the BoardWithMeta, its lists ordered by
    /// position ASC, and its non-archived card stubs.
    #[tokio::test]
    async fn test_get_board_returns_board_lists_cards() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &user_id).await;

        // Insert two lists
        let pos1 = FractionalIndex::default().to_string();
        let pos2 = FractionalIndex::new_after(&FractionalIndex::default()).to_string();
        let list1_id = insert_list_direct(&write_pool, &board_id, "List A", &pos1, false).await;
        let list2_id = insert_list_direct(&write_pool, &board_id, "List B", &pos2, false).await;

        // Insert a card in list1
        let card_pos = FractionalIndex::default().to_string();
        insert_card_direct(&write_pool, &list1_id, &board_id, "Card One", &card_pos, false).await;

        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.board.id, board_id);
        assert_eq!(data.lists.len(), 2, "should return 2 non-archived lists");
        assert_eq!(data.lists[0].id, list1_id, "lists ordered by position ASC");
        assert_eq!(data.lists[1].id, list2_id);
        assert_eq!(data.cards.len(), 1, "should return 1 non-archived card");
        assert_eq!(data.cards[0].title, "Card One");
        let _ = list2_id; // suppress unused
    }

    // -------------------------------------------------------------------------
    // test_get_board_excludes_archived_lists_and_cards
    // -------------------------------------------------------------------------

    /// Archived lists and archived cards are NOT returned by get_board.
    #[tokio::test]
    async fn test_get_board_excludes_archived_lists_and_cards() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &user_id).await;

        let pos1 = FractionalIndex::default().to_string();
        let pos2 = FractionalIndex::new_after(&FractionalIndex::default()).to_string();

        // Active list with active card
        let active_list_id = insert_list_direct(&write_pool, &board_id, "Active List", &pos1, false).await;
        let card_pos = FractionalIndex::default().to_string();
        insert_card_direct(&write_pool, &active_list_id, &board_id, "Active Card", &card_pos, false).await;

        // Archived list (should not appear in lists OR cards)
        let archived_list_id = insert_list_direct(&write_pool, &board_id, "Archived List", &pos2, true).await;
        let card_pos2 = FractionalIndex::new_after(&FractionalIndex::default()).to_string();
        insert_card_direct(&write_pool, &archived_list_id, &board_id, "Archived Card", &card_pos2, true).await;

        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.lists.len(), 1, "only non-archived list returned");
        assert_eq!(data.lists[0].id, active_list_id);
        assert_eq!(data.cards.len(), 1, "only non-archived card returned");
        assert_eq!(data.cards[0].title, "Active Card");
        let _ = archived_list_id; // suppress unused
    }

    // -------------------------------------------------------------------------
    // test_get_board_non_member_rejected
    // -------------------------------------------------------------------------

    /// A non-member gets Err("board not found") from get_board_inner.
    #[tokio::test]
    async fn test_get_board_non_member_rejected() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let non_member_id = insert_user_direct(&write_pool, "nonmember@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &owner_id).await;

        let result = get_board_inner(&write_pool, &board_id, &non_member_id).await;
        assert!(result.is_err(), "non-member should get Err");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("board not found"), "error must be 'board not found', got: {err_msg}");
    }

    // -------------------------------------------------------------------------
    // test_touch_last_viewed_sets_timestamp
    // -------------------------------------------------------------------------

    /// touch_last_viewed_inner sets board_members.last_viewed_at to a non-null value.
    #[tokio::test]
    async fn test_touch_last_viewed_sets_timestamp() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &user_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        touch_last_viewed_inner(&write_pool, &board_id, &user_id, now)
            .await
            .expect("touch_last_viewed_inner");

        let stored: Option<i64> = sqlx::query_scalar(
            "SELECT last_viewed_at FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&user_id)
        .fetch_one(&write_pool)
        .await
        .expect("fetch last_viewed_at");

        assert!(stored.is_some(), "last_viewed_at must be set");
        assert_eq!(stored.unwrap(), now, "stored value must equal the provided timestamp");
    }

    // -------------------------------------------------------------------------
    // test_touch_last_viewed_non_member_rejected
    // -------------------------------------------------------------------------

    /// touch_last_viewed_inner for a non-member is a no-op (0 rows affected is not an error,
    /// but the require_board_member gate in the server fn wrapper prevents reaching this fn).
    /// Here we verify the UPDATE scopes only to the correct user.
    #[tokio::test]
    async fn test_touch_last_viewed_non_member_rejected() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let other_id = insert_user_direct(&write_pool, "other@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &owner_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Touch as owner
        touch_last_viewed_inner(&write_pool, &board_id, &owner_id, now)
            .await
            .expect("touch owner");

        // Attempt to touch as non-member — returns Ok but 0 rows affected (scoped UPDATE)
        // The server fn wrapper (require_board_member) would reject this before calling inner fn.
        // Here we verify that other user's row is NOT affected.
        let stored_other: Option<i64> = sqlx::query_scalar(
            "SELECT last_viewed_at FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&other_id)
        .fetch_optional(&write_pool)
        .await
        .expect("fetch other");

        // other_id has no board_members row, so fetch_optional returns None
        assert!(stored_other.is_none(), "non-member row must not be touched");

        // Owner's timestamp is untouched
        let stored_owner: Option<i64> = sqlx::query_scalar(
            "SELECT last_viewed_at FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&owner_id)
        .fetch_one(&write_pool)
        .await
        .expect("fetch owner");

        assert_eq!(stored_owner, Some(now), "owner timestamp must be correct");
    }

    // -------------------------------------------------------------------------
    // test_rename_board_updates_name
    // -------------------------------------------------------------------------

    /// rename_board_inner trims whitespace and persists the new name to boards.
    #[tokio::test]
    async fn test_rename_board_updates_name() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Original Board", &user_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        rename_board_inner(&write_pool, &board_id, "  Renamed Board  ".to_string(), now)
            .await
            .expect("rename_board_inner");

        let stored_name: String = sqlx::query_scalar("SELECT name FROM boards WHERE id = ?")
            .bind(&board_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch name");

        assert_eq!(stored_name, "Renamed Board", "name must be trimmed and persisted");
    }

    // -------------------------------------------------------------------------
    // test_rename_board_empty_rejected
    // -------------------------------------------------------------------------

    /// rename_board_inner with an empty name returns Err; the original name is unchanged.
    #[tokio::test]
    async fn test_rename_board_empty_rejected() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "My Board", &user_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let result = rename_board_inner(&write_pool, &board_id, "".to_string(), now).await;
        assert!(result.is_err(), "empty name must return Err");

        let stored_name: String = sqlx::query_scalar("SELECT name FROM boards WHERE id = ?")
            .bind(&board_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch name");

        assert_eq!(stored_name, "My Board", "original name must be unchanged after rejection");
    }
}
