//! Tests for the v1 schema via PRAGMA introspection.
//! Phase 1 had 16 tables including `sessions`.
//! Phase 2 migration 002 drops `sessions` (replaced by tower-sessions' `tower_sessions` table
//! created at runtime via SqliteStore::migrate() — not visible to sqlx migration tests).
//! Run: DATABASE_URL=sqlite://data/test_schema.db cargo test --features ssr schema_tests

#[cfg(feature = "ssr")]
mod schema_tests {
    use lanes::server::db::{make_write_pool, run_migrations};
    use sqlx::Row;
    use tempfile::NamedTempFile;

    fn temp_db_url() -> (NamedTempFile, String) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        (file, url)
    }

    async fn migrated_pool() -> (NamedTempFile, sqlx::SqlitePool) {
        let (file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        (file, pool)
    }

    // Expected tables after Phase 2 migrations.
    // Phase 1 had 16 tables including `sessions`.
    // Migration 002 drops `sessions` (replaced by tower-sessions' `tower_sessions` at runtime).
    // Remaining domain tables: 15.
    const EXPECTED_TABLES: &[&str] = &[
        "users",
        // "sessions" dropped by migration 002 — replaced by tower_sessions at runtime
        "boards",
        "board_members",
        "lists",
        "cards",
        "card_members",
        "labels",
        "card_labels",
        "checklists",
        "checklist_items",
        "comments",
        "attachments",
        "notifications",
        "invites",
        "api_tokens",
        "watchers",
    ];

    #[tokio::test]
    async fn test_all_16_tables_exist() {
        let (_file, pool) = migrated_pool().await;

        let rows = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '_sqlx%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("query");

        let table_names: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();

        for expected in EXPECTED_TABLES {
            assert!(
                table_names.contains(&expected.to_string()),
                "Expected table '{}' to exist. Found: {:?}",
                expected,
                table_names
            );
        }
        assert_eq!(
            table_names.len(),
            EXPECTED_TABLES.len(),
            "Expected exactly {} tables, found {}: {:?}",
            EXPECTED_TABLES.len(),
            table_names.len(),
            table_names
        );
    }

    #[tokio::test]
    async fn test_cards_position_is_text_not_null() {
        let (_file, pool) = migrated_pool().await;

        let rows = sqlx::query("PRAGMA table_info(cards)")
            .fetch_all(&pool)
            .await
            .expect("pragma");

        let pos_col = rows
            .iter()
            .find(|r| r.get::<String, _>(1) == "position")
            .expect("cards.position column must exist");

        let col_type: String = pos_col.get(2);
        let not_null: i64 = pos_col.get(3);

        assert_eq!(col_type, "TEXT", "cards.position must be TEXT");
        assert_eq!(not_null, 1, "cards.position must be NOT NULL");
    }

    #[tokio::test]
    async fn test_lists_position_is_text_not_null() {
        let (_file, pool) = migrated_pool().await;

        let rows = sqlx::query("PRAGMA table_info(lists)")
            .fetch_all(&pool)
            .await
            .expect("pragma");

        let pos_col = rows
            .iter()
            .find(|r| r.get::<String, _>(1) == "position")
            .expect("lists.position column must exist");

        let col_type: String = pos_col.get(2);
        let not_null: i64 = pos_col.get(3);

        assert_eq!(col_type, "TEXT", "lists.position must be TEXT");
        assert_eq!(not_null, 1, "lists.position must be NOT NULL");
    }

    #[tokio::test]
    async fn test_boards_lists_cards_ids_are_text() {
        let (_file, pool) = migrated_pool().await;

        for table in &["boards", "lists", "cards"] {
            let rows = sqlx::query(&format!("PRAGMA table_info({})", table))
                .fetch_all(&pool)
                .await
                .expect("pragma");

            let id_col = rows
                .iter()
                .find(|r| r.get::<String, _>(1) == "id")
                .expect(&format!("{}.id column must exist", table));

            let col_type: String = id_col.get(2);
            assert_eq!(
                col_type, "TEXT",
                "{}.id must be TEXT (Postgres-portable UUIDv7)",
                table
            );
        }
    }

    #[tokio::test]
    async fn test_cards_unique_board_id_card_num() {
        let (_file, pool) = migrated_pool().await;

        // Insert two users and a board first (FK constraints)
        let now = 1_700_000_000_000i64;
        sqlx::query(
            "INSERT INTO users (id, email, display_name, avatar_color, created_at) VALUES ('u1', 'a@a.com', 'A', '#fff', ?)"
        )
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert user");

        sqlx::query(
            "INSERT INTO boards (id, name, key_prefix, color, created_at, updated_at) VALUES ('b1', 'Test Board', 'TB', '#7c5cff', ?, ?)"
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert board");

        sqlx::query(
            "INSERT INTO lists (id, board_id, name, position) VALUES ('l1', 'b1', 'List 1', '80')"
        )
        .execute(&pool)
        .await
        .expect("insert list");

        // Insert first card (card_num=1)
        sqlx::query(
            "INSERT INTO cards (id, list_id, board_id, card_num, title, position, created_at, updated_at) \
             VALUES ('c1', 'l1', 'b1', 1, 'Card 1', '80', ?, ?)"
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert first card");

        // Insert second card with same board_id+card_num must fail
        let result = sqlx::query(
            "INSERT INTO cards (id, list_id, board_id, card_num, title, position, created_at, updated_at) \
             VALUES ('c2', 'l1', 'b1', 1, 'Card 2', '90', ?, ?)"
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await;

        assert!(
            result.is_err(),
            "UNIQUE(board_id, card_num) constraint must reject duplicate card numbers"
        );
    }

    #[tokio::test]
    async fn test_boards_has_next_card_num() {
        let (_file, pool) = migrated_pool().await;

        let rows = sqlx::query("PRAGMA table_info(boards)")
            .fetch_all(&pool)
            .await
            .expect("pragma");

        let col = rows
            .iter()
            .find(|r| r.get::<String, _>(1) == "next_card_num")
            .expect("boards.next_card_num must exist");

        let col_type: String = col.get(2);
        assert_eq!(col_type, "INTEGER", "boards.next_card_num must be INTEGER");
    }

    #[tokio::test]
    async fn test_no_rowid_or_autoincrement_sqlite_constructs() {
        let (_file, _pool) = migrated_pool().await;

        // Read the migration SQL and verify no SQLite-only constructs
        // This is a schema portability check (PLAT-01)
        let migration_sql = include_str!("../migrations/001_init.sql");
        let lower = migration_sql.to_lowercase();

        assert!(
            !lower.contains("rowid"),
            "Migration must not use ROWID (SQLite-specific)"
        );
        assert!(
            !lower.contains("autoincrement"),
            "Migration must not use AUTOINCREMENT (SQLite-specific)"
        );
        assert!(
            !lower.contains("without rowid"),
            "Migration must not use WITHOUT ROWID (SQLite-specific)"
        );
    }

    #[tokio::test]
    async fn test_shared_models_compile() {
        // Verify the shared models can be constructed (they have no cfg gates)
        use lanes::models::{Board, Card, CardLabel, List};

        let board = Board {
            id: "test-id".into(),
            name: "Test Board".into(),
            key_prefix: "TB".into(),
            color: "#7c5cff".into(),
            starred: false,
            archived: false,
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(board.id, "test-id");

        let list = List {
            id: "list-id".into(),
            board_id: "test-id".into(),
            name: "Test List".into(),
            position: "80".into(),
            archived: false,
            is_done_list: false,
        };
        assert_eq!(list.position, "80");
        assert!(!list.is_done_list);

        let label = CardLabel {
            id: "label-id".into(),
            name: "Urgent".into(),
            color: "oklch(72% 0.10 25)".into(),
        };
        assert_eq!(label.name, "Urgent");

        let card = Card {
            id: "card-id".into(),
            list_id: "list-id".into(),
            board_id: "test-id".into(),
            card_num: 1,
            title: "Test Card".into(),
            position: "80".into(),
            priority: None,
            due_at: None,
            done: false,
            archived: false,
            cover: None,
            labels: vec![label],
            checklist_done: 0,
            checklist_total: 0,
            comment_count: 0,
            attachment_count: 0,
            member_ids: vec![],
        };
        assert_eq!(card.card_num, 1);
        assert_eq!(card.labels.len(), 1);
    }

    /// Assert that Migration 004 adds the five new columns:
    /// lists.is_done_list, cards.checklist_done, cards.checklist_total,
    /// cards.comment_count, cards.attachment_count.
    #[tokio::test]
    async fn test_migration_004_columns_exist() {
        let (_file, pool) = migrated_pool().await;

        // Check lists.is_done_list
        let list_cols = sqlx::query("PRAGMA table_info(lists)")
            .fetch_all(&pool)
            .await
            .expect("pragma lists");
        let col_names: Vec<String> = list_cols.iter().map(|r| r.get::<String, _>(1)).collect();
        assert!(
            col_names.contains(&"is_done_list".to_string()),
            "lists.is_done_list must exist after migration 004; found: {:?}",
            col_names
        );

        // Check the four card count columns
        let card_cols = sqlx::query("PRAGMA table_info(cards)")
            .fetch_all(&pool)
            .await
            .expect("pragma cards");
        let card_col_names: Vec<String> = card_cols.iter().map(|r| r.get::<String, _>(1)).collect();

        for col in &["checklist_done", "checklist_total", "comment_count", "attachment_count"] {
            assert!(
                card_col_names.contains(&col.to_string()),
                "cards.{} must exist after migration 004; found: {:?}",
                col,
                card_col_names
            );
        }

        // Ensure cover is NOT re-added by migration 004 (it already exists in 001)
        let cover_count = card_col_names.iter().filter(|c| c.as_str() == "cover").count();
        assert_eq!(
            cover_count, 1,
            "cards.cover must exist exactly once (from 001_init.sql, not duplicated by 004)"
        );
    }
}
