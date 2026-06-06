//! Integration tests for list_boards and add_board server functions.
//! Run: DATABASE_URL=sqlite://data/lanes.db cargo test --features ssr api_tests

#[cfg(feature = "ssr")]
mod api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::{create_board, derive_key_prefix, fetch_boards_for_user};
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

    /// Direct board insert used to set up test data (archived flag control).
    async fn insert_board_direct(pool: &sqlx::SqlitePool, name: &str, archived: bool) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let key_prefix = derive_key_prefix(name);
        let color = "#6366f1".to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let archived_int: i64 = if archived { 1 } else { 0 };

        sqlx::query!(
            r#"INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
               VALUES (?, ?, ?, ?, 0, ?, ?, ?)"#,
            id, name, key_prefix, color, archived_int, now, now
        )
        .execute(pool)
        .await
        .expect("insert board");
        id
    }

    /// Insert a board_members row linking user to board.
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
    // Task 1 TDD: fetch_boards_for_user — per-user scoping
    // -------------------------------------------------------------------------

    /// Test: fetch_boards_for_user returns only boards where a board_members row links the user.
    #[tokio::test]
    async fn test_fetch_boards_for_user_only_returns_own_boards() {
        let (_file, write_pool, read_pool) = test_db().await;

        let user_a = insert_user_direct(&write_pool, "a@test.com").await;
        let user_b = insert_user_direct(&write_pool, "b@test.com").await;

        // user_a board
        let board_a_id = insert_board_direct(&write_pool, "Alpha", false).await;
        insert_member_direct(&write_pool, &board_a_id, &user_a, "owner").await;

        // user_b board — user_a is NOT a member
        let board_b_id = insert_board_direct(&write_pool, "Beta", false).await;
        insert_member_direct(&write_pool, &board_b_id, &user_b, "owner").await;

        let boards = fetch_boards_for_user(&read_pool, &user_a).await.expect("fetch_boards_for_user");
        assert_eq!(boards.len(), 1, "user_a should see only their own board");
        assert_eq!(boards[0].id, board_a_id, "should be Alpha board");
    }

    /// Test: fetch_boards_for_user excludes archived boards even if user is a member.
    #[tokio::test]
    async fn test_fetch_boards_for_user_excludes_archived() {
        let (_file, write_pool, read_pool) = test_db().await;

        let user_a = insert_user_direct(&write_pool, "a@test.com").await;

        let active_id = insert_board_direct(&write_pool, "Active", false).await;
        insert_member_direct(&write_pool, &active_id, &user_a, "owner").await;

        let archived_id = insert_board_direct(&write_pool, "Archived", true).await;
        insert_member_direct(&write_pool, &archived_id, &user_a, "member").await;

        let boards = fetch_boards_for_user(&read_pool, &user_a).await.expect("fetch_boards_for_user");
        assert_eq!(boards.len(), 1, "archived board should be excluded");
        assert_eq!(boards[0].id, active_id);
    }

    /// Test: create_board inserts one boards row AND one board_members owner row; returned Board.id matches.
    #[tokio::test]
    async fn test_create_board_inserts_board_and_owner_member() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let creator_id = insert_user_direct(&write_pool, "creator@test.com").await;

        let board = create_board(&write_pool, "Test Board".to_string(), &creator_id)
            .await
            .expect("create_board should succeed");

        // Returned id must parse as a UUID
        uuid::Uuid::parse_str(&board.id).expect("id must be a valid UUID");

        // Exactly one board row
        let board_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count boards");
        assert_eq!(board_count, 1, "should have exactly one board row");

        // Exactly one board_members row with role 'owner'
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ? AND role = 'owner'"
        )
        .bind(&board.id)
        .bind(&creator_id)
        .fetch_one(&write_pool)
        .await
        .expect("count members");
        assert_eq!(member_count, 1, "should have exactly one owner board_members row");
    }

    /// Test: transaction rollback — no orphan board if board_members insert fails.
    /// We simulate this by using an invalid creator_id that violates the FK constraint.
    #[tokio::test]
    async fn test_create_board_transaction_rollback_on_member_failure() {
        let (_file, write_pool, _read_pool) = test_db().await;

        // Enable FK enforcement for this connection
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&write_pool)
            .await
            .expect("pragma");

        // Use a non-existent creator_id to trigger FK violation on board_members insert
        let result = create_board(&write_pool, "Orphan Board".to_string(), "non-existent-user-id")
            .await;

        // Should fail (FK violation)
        assert!(result.is_err(), "should fail with invalid creator_id");

        // No orphan board row should remain
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no orphan board row should remain after transaction rollback");
    }

    // -------------------------------------------------------------------------
    // Existing validation tests — updated for new 3-arg create_board signature
    // -------------------------------------------------------------------------

    /// Test: create_board with empty name returns Err; no row inserted.
    #[tokio::test]
    async fn test_add_board_empty_name_returns_err_no_row() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let creator_id = insert_user_direct(&write_pool, "user@test.com").await;

        let result = create_board(&write_pool, "".to_string(), &creator_id).await;
        assert!(result.is_err(), "empty name should return Err");
        assert!(
            result.unwrap_err().contains("empty"),
            "error must mention empty"
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted");
    }

    /// Test: create_board with whitespace-only name returns Err; no row inserted.
    #[tokio::test]
    async fn test_add_board_whitespace_name_returns_err_no_row() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let creator_id = insert_user_direct(&write_pool, "user@test.com").await;

        let result = create_board(&write_pool, "   ".to_string(), &creator_id).await;
        assert!(result.is_err(), "whitespace-only name should return Err");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted");
    }

    /// Test: create_board with name > 120 chars returns Err; no row inserted.
    #[tokio::test]
    async fn test_add_board_too_long_name_returns_err_no_row() {
        let (_file, write_pool, _read_pool) = test_db().await;
        let creator_id = insert_user_direct(&write_pool, "user@test.com").await;

        let long_name = "A".repeat(121);
        let result = create_board(&write_pool, long_name, &creator_id).await;
        assert!(result.is_err(), "name > 120 chars should return Err");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted");
    }

    /// Test: fetch_boards_for_user returns boards ordered by created_at ASC.
    #[tokio::test]
    async fn test_fetch_boards_for_user_ordered_by_created_at_asc() {
        let (_file, write_pool, read_pool) = test_db().await;

        let user = insert_user_direct(&write_pool, "user@test.com").await;

        let alpha_id = insert_board_direct(&write_pool, "Alpha", false).await;
        insert_member_direct(&write_pool, &alpha_id, &user, "member").await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let beta_id = insert_board_direct(&write_pool, "Beta", false).await;
        insert_member_direct(&write_pool, &beta_id, &user, "member").await;

        let boards = fetch_boards_for_user(&read_pool, &user).await.expect("fetch_boards_for_user");
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].name, "Alpha");
        assert_eq!(boards[1].name, "Beta");
    }
}
