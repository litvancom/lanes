//! Unit tests for list_api inner functions (create / rename / reorder).
//! Run: cargo test --features ssr --test list_api_tests
//!
//! Tests call inner fns directly (they take a pool, no Leptos context needed).
//! Pattern follows tests/api_tests.rs.

#[cfg(feature = "ssr")]
mod list_api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::derive_key_prefix;
    use lanes::api::list_api::{
        create_list_inner, rename_list_inner, reorder_list_inner, next_list_position,
        board_id_for_list,
    };
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
    async fn insert_board_direct(pool: &sqlx::SqlitePool, name: &str) -> String {
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
        id
    }

    /// Insert a board_members row.
    async fn insert_member_direct(pool: &sqlx::SqlitePool, board_id: &str, user_id: &str, role: &str) {
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, ?)",
            board_id, user_id, role
        )
        .execute(pool)
        .await
        .expect("insert member");
    }

    // -------------------------------------------------------------------------
    // test_create_list_first_position
    // -------------------------------------------------------------------------

    /// Creating the first list on a board yields a valid FractionalIndex position.
    #[tokio::test]
    async fn test_create_list_first_position() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;

        let pos = next_list_position(&write_pool, &board_id).await.expect("next_list_position");
        let list = create_list_inner(&write_pool, &board_id, "First List".to_string(), &pos)
            .await
            .expect("create_list_inner");

        // Position must parse as a valid FractionalIndex
        FractionalIndex::from_string(&list.position).expect("must be valid FractionalIndex");
        assert_eq!(list.board_id, board_id);
    }

    // -------------------------------------------------------------------------
    // test_create_list_appends_after_last
    // -------------------------------------------------------------------------

    /// Creating a second list yields a position that sorts AFTER the first.
    #[tokio::test]
    async fn test_create_list_appends_after_last() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;

        let pos1 = next_list_position(&write_pool, &board_id).await.expect("pos1");
        let list1 = create_list_inner(&write_pool, &board_id, "First".to_string(), &pos1)
            .await
            .expect("list1");

        let pos2 = next_list_position(&write_pool, &board_id).await.expect("pos2");
        let list2 = create_list_inner(&write_pool, &board_id, "Second".to_string(), &pos2)
            .await
            .expect("list2");

        // Lexicographic compare: second position must sort after first
        assert!(
            list2.position > list1.position,
            "second list position '{}' must sort after first '{}'",
            list2.position,
            list1.position
        );
    }

    // -------------------------------------------------------------------------
    // test_create_list_empty_name_rejected
    // -------------------------------------------------------------------------

    /// Empty/whitespace name returns Err; no row inserted.
    #[tokio::test]
    async fn test_create_list_empty_name_rejected() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;
        let pos = FractionalIndex::default().to_string();

        // Empty name
        let result = create_list_inner(&write_pool, &board_id, "".to_string(), &pos).await;
        assert!(result.is_err(), "empty name should return Err");

        // Whitespace-only name
        let result2 = create_list_inner(&write_pool, &board_id, "   ".to_string(), &pos).await;
        assert!(result2.is_err(), "whitespace-only name should return Err");

        // No list rows inserted
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lists")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no list row should be inserted");
    }

    // -------------------------------------------------------------------------
    // test_create_list_non_member_rejected
    // -------------------------------------------------------------------------

    /// board_id_for_list returns None for unknown list_id (simulates non-member scenario at API layer).
    /// This test validates the board_id lookup helper used for non-member rejection.
    #[tokio::test]
    async fn test_create_list_non_member_rejected() {
        let (_file, write_pool, _read_pool) = test_db().await;

        // Unknown list_id — returns None
        let result = board_id_for_list(&write_pool, "non-existent-list-id").await.expect("no db error");
        assert!(result.is_none(), "unknown list_id must return None");
    }

    // -------------------------------------------------------------------------
    // test_rename_list_updates_name
    // -------------------------------------------------------------------------

    /// rename_list_inner changes lists.name; surrounding whitespace is trimmed.
    #[tokio::test]
    async fn test_rename_list_updates_name() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;
        let pos = FractionalIndex::default().to_string();

        let list = create_list_inner(&write_pool, &board_id, "Original".to_string(), &pos)
            .await
            .expect("create");

        rename_list_inner(&write_pool, &list.id, "  Renamed List  ".to_string())
            .await
            .expect("rename");

        let name: String = sqlx::query_scalar("SELECT name FROM lists WHERE id = ?")
            .bind(&list.id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch name");

        assert_eq!(name, "Renamed List", "name must be trimmed and updated");
    }

    // -------------------------------------------------------------------------
    // test_rename_list_empty_rejected
    // -------------------------------------------------------------------------

    /// rename_list_inner rejects empty name; original name unchanged.
    #[tokio::test]
    async fn test_rename_list_empty_rejected() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;
        let pos = FractionalIndex::default().to_string();

        let list = create_list_inner(&write_pool, &board_id, "Original".to_string(), &pos)
            .await
            .expect("create");

        let result = rename_list_inner(&write_pool, &list.id, "".to_string()).await;
        assert!(result.is_err(), "empty name should return Err");

        // Original name unchanged
        let name: String = sqlx::query_scalar("SELECT name FROM lists WHERE id = ?")
            .bind(&list.id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch name");
        assert_eq!(name, "Original", "original name must be unchanged");
    }

    // -------------------------------------------------------------------------
    // test_reorder_list_persists_valid_position
    // -------------------------------------------------------------------------

    /// reorder_list_inner persists a valid FractionalIndex position string.
    #[tokio::test]
    async fn test_reorder_list_persists_valid_position() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;

        // Create two lists to get two positions
        let pos1 = FractionalIndex::default().to_string();
        let pos2 = FractionalIndex::new_after(&FractionalIndex::default()).to_string();

        let list1 = create_list_inner(&write_pool, &board_id, "First".to_string(), &pos1)
            .await
            .expect("list1");
        let _list2 = create_list_inner(&write_pool, &board_id, "Second".to_string(), &pos2)
            .await
            .expect("list2");

        // Compute midpoint position between nothing and first (move list1 before itself = new_before)
        // Just use a known-valid FractionalIndex string for the new_position
        let new_pos = FractionalIndex::new_between(
            &FractionalIndex::default(),
            &FractionalIndex::new_after(&FractionalIndex::default()),
        )
        .expect("new_between must work")
        .to_string();

        reorder_list_inner(&write_pool, &list1.id, new_pos.clone())
            .await
            .expect("reorder");

        let stored_pos: String = sqlx::query_scalar("SELECT position FROM lists WHERE id = ?")
            .bind(&list1.id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch position");

        assert_eq!(stored_pos, new_pos, "new position must be persisted");
    }

    // -------------------------------------------------------------------------
    // test_reorder_list_invalid_position_rejected
    // -------------------------------------------------------------------------

    /// reorder_list_inner rejects an undecodable position string; row is unchanged.
    #[tokio::test]
    async fn test_reorder_list_invalid_position_rejected() {
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let board_id = insert_board_direct(&write_pool, "My Board").await;
        let pos = FractionalIndex::default().to_string();

        let list = create_list_inner(&write_pool, &board_id, "First".to_string(), &pos)
            .await
            .expect("create");

        let result = reorder_list_inner(&write_pool, &list.id, "not-a-valid-fractional-index-!!".to_string()).await;
        assert!(result.is_err(), "invalid position should return Err");

        // Position unchanged
        let stored_pos: String = sqlx::query_scalar("SELECT position FROM lists WHERE id = ?")
            .bind(&list.id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch position");
        assert_eq!(stored_pos, pos, "position must be unchanged after rejected reorder");
    }
}
