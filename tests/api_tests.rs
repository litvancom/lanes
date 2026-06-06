//! Integration tests for list_boards and add_board server functions.
//! Run: DATABASE_URL=sqlite://data/test_api.db cargo test --features ssr api_tests

#[cfg(feature = "ssr")]
mod api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::{create_board, derive_key_prefix, fetch_boards};
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

    /// Test: create_board inserts exactly one row; returned Board.id is a valid UUID.
    #[tokio::test]
    async fn test_add_board_inserts_one_row_with_valid_uuid() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let board = create_board(&write_pool, "Test Board".to_string())
            .await
            .expect("create_board should succeed");

        // Returned id must parse as a UUID
        uuid::Uuid::parse_str(&board.id).expect("id must be a valid UUID");

        // Exactly one row
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "should have exactly one board row");
    }

    /// Test: after seeding + create_board, fetch_boards returns all non-archived boards.
    #[tokio::test]
    async fn test_list_boards_includes_new_board_excludes_archived() {
        let (_file, write_pool, read_pool) = test_db().await;

        insert_board_direct(&write_pool, "Seeded Board", false).await;
        insert_board_direct(&write_pool, "Archived Board", true).await;

        let new_board = create_board(&write_pool, "New Board".to_string())
            .await
            .expect("create_board should succeed");

        let boards = fetch_boards(&read_pool).await.expect("fetch_boards");

        assert_eq!(boards.len(), 2, "should have 2 non-archived boards");

        assert!(
            boards.iter().any(|b| b.id == new_board.id),
            "new board must be in list"
        );
        assert!(
            boards.iter().all(|b| b.name != "Archived Board"),
            "archived board must be excluded"
        );
    }

    /// Test: fetch_boards returns boards ordered by created_at ASC.
    #[tokio::test]
    async fn test_list_boards_ordered_by_created_at_asc() {
        let (_file, write_pool, read_pool) = test_db().await;

        insert_board_direct(&write_pool, "Alpha", false).await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        insert_board_direct(&write_pool, "Beta", false).await;

        let boards = fetch_boards(&read_pool).await.expect("fetch_boards");
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].name, "Alpha");
        assert_eq!(boards[1].name, "Beta");
    }

    /// Test: create_board with empty name returns Err; no row inserted.
    #[tokio::test]
    async fn test_add_board_empty_name_returns_err_no_row() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let result = create_board(&write_pool, "".to_string()).await;
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

        let result = create_board(&write_pool, "   ".to_string()).await;
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

        let long_name = "A".repeat(121);
        let result = create_board(&write_pool, long_name).await;
        assert!(result.is_err(), "name > 120 chars should return Err");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted");
    }
}
