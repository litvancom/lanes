//! Integration tests for the `lanes seed` CLI subcommand and seed data.
//! Run: DATABASE_URL=sqlite://data/test_seed.db cargo test --features ssr seed_tests

#[cfg(feature = "ssr")]
mod seed_tests {
    use lanes::server::db::{make_write_pool, run_migrations};
    use sqlx::Row;
    use tempfile::NamedTempFile;

    /// Create a temporary SQLite URL backed by a NamedTempFile (auto-cleaned on drop).
    fn temp_db_url() -> (NamedTempFile, String) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        (file, url)
    }

    // ---------------------------------------------------------------------------
    // Task 1 tests: CLI dispatch
    // ---------------------------------------------------------------------------

    /// Verify the Cli struct can parse `seed` as a known subcommand without error.
    /// We cannot run the binary in a unit test, but we can confirm the parse logic
    /// compiles and handles the `seed` variant.
    #[test]
    fn dispatch_seed_variant_exists() {
        use clap::Parser;
        use lanes::cli::{Cli, Commands};

        // "lanes seed" — should parse successfully
        let cli = Cli::try_parse_from(["lanes", "seed"]).expect("seed subcommand should parse");
        assert!(matches!(cli.command, Some(Commands::Seed)));
    }

    /// Verify that `lanes` with no subcommand parses to `None` (server path).
    #[test]
    fn dispatch_no_subcommand_is_none() {
        use clap::Parser;
        use lanes::cli::Cli;

        let cli = Cli::try_parse_from(["lanes"]).expect("no subcommand should parse");
        assert!(cli.command.is_none());
    }

    /// Verify that an unknown subcommand is rejected (non-zero exit).
    #[test]
    fn dispatch_unknown_subcommand_is_rejected() {
        use clap::Parser;
        use lanes::cli::Cli;

        let result = Cli::try_parse_from(["lanes", "badcmd"]);
        assert!(result.is_err(), "unknown subcommand must be rejected");
    }

    // ---------------------------------------------------------------------------
    // Task 2 tests: run_seed fixtures and non-empty guard
    // ---------------------------------------------------------------------------

    /// run_seed on a freshly-migrated DB inserts the required minimum rows.
    #[tokio::test]
    async fn seed_inserts_representative_fixtures() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");
        run_migrations(&pool).await.expect("migrations");

        lanes::seed::run_seed(&pool)
            .await
            .expect("run_seed should succeed on empty DB");

        // users >= 1
        let (user_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("count users");
        assert!(user_count >= 1, "expected >= 1 user, got {}", user_count);

        // boards = 1
        let (board_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM boards")
            .fetch_one(&pool)
            .await
            .expect("count boards");
        assert_eq!(board_count, 1, "expected 1 board, got {}", board_count);

        // lists >= 3
        let (list_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM lists")
            .fetch_one(&pool)
            .await
            .expect("count lists");
        assert!(list_count >= 3, "expected >= 3 lists, got {}", list_count);

        // cards >= 6
        let (card_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cards")
            .fetch_one(&pool)
            .await
            .expect("count cards");
        assert!(card_count >= 6, "expected >= 6 cards, got {}", card_count);

        // labels >= 1
        let (label_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM labels")
            .fetch_one(&pool)
            .await
            .expect("count labels");
        assert!(label_count >= 1, "expected >= 1 label, got {}", label_count);

        // card_labels >= 1
        let (card_label_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM card_labels")
            .fetch_one(&pool)
            .await
            .expect("count card_labels");
        assert!(
            card_label_count >= 1,
            "expected >= 1 card_label, got {}",
            card_label_count
        );

        // checklists >= 1
        let (checklist_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM checklists")
            .fetch_one(&pool)
            .await
            .expect("count checklists");
        assert!(
            checklist_count >= 1,
            "expected >= 1 checklist, got {}",
            checklist_count
        );

        // checklist_items >= 1
        let (item_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM checklist_items")
            .fetch_one(&pool)
            .await
            .expect("count checklist_items");
        assert!(
            item_count >= 1,
            "expected >= 1 checklist_item, got {}",
            item_count
        );

        // comments >= 1
        let (comment_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM comments")
            .fetch_one(&pool)
            .await
            .expect("count comments");
        assert!(
            comment_count >= 1,
            "expected >= 1 comment, got {}",
            comment_count
        );

        // card_members >= 1
        let (member_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM card_members")
            .fetch_one(&pool)
            .await
            .expect("count card_members");
        assert!(
            member_count >= 1,
            "expected >= 1 card_member, got {}",
            member_count
        );
    }

    /// At least one seeded card has a non-NULL due_at.
    /// At least one seeded card has priority P1 or P2.
    #[tokio::test]
    async fn seed_cards_have_due_date_and_priority() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");
        run_migrations(&pool).await.expect("migrations");

        lanes::seed::run_seed(&pool)
            .await
            .expect("run_seed on empty DB");

        let (due_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM cards WHERE due_at IS NOT NULL")
                .fetch_one(&pool)
                .await
                .expect("count cards with due_at");
        assert!(
            due_count >= 1,
            "expected >= 1 card with due_at set, got {}",
            due_count
        );

        let (priority_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM cards WHERE priority IN ('P1', 'P2')")
                .fetch_one(&pool)
                .await
                .expect("count cards with P1/P2 priority");
        assert!(
            priority_count >= 1,
            "expected >= 1 card with P1 or P2 priority, got {}",
            priority_count
        );
    }

    /// Second call to run_seed (DB non-empty) returns Err and inserts no extra rows.
    #[tokio::test]
    async fn seed_refuses_non_empty_database() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");
        run_migrations(&pool).await.expect("migrations");

        // First seed — should succeed
        lanes::seed::run_seed(&pool)
            .await
            .expect("first seed should succeed");

        let (user_count_after_first,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("count users after first seed");

        // Second seed — must fail
        let result = lanes::seed::run_seed(&pool).await;
        assert!(
            result.is_err(),
            "second run_seed on non-empty DB must return Err"
        );

        // User count must be unchanged
        let (user_count_after_second,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("count users after second seed attempt");
        assert_eq!(
            user_count_after_first, user_count_after_second,
            "user count must not change after refused seed"
        );
    }

    /// List positions returned by ORDER BY position ASC are in insertion order.
    #[tokio::test]
    async fn seed_list_positions_sort_correctly() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");
        run_migrations(&pool).await.expect("migrations");

        lanes::seed::run_seed(&pool)
            .await
            .expect("run_seed on empty DB");

        // Fetch the board id
        let (board_id,): (String,) = sqlx::query_as("SELECT id FROM boards LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("fetch board id");

        // Fetch list names and positions ordered by position ASC
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT name, position FROM lists WHERE board_id = ? ORDER BY position ASC")
                .bind(&board_id)
                .fetch_all(&pool)
                .await
                .expect("fetch list positions");

        assert!(
            rows.len() >= 3,
            "expected >= 3 lists in position order, got {}",
            rows.len()
        );

        // Verify lexicographic ascending order (since FractionalIndex::to_string maintains order)
        for i in 0..(rows.len() - 1) {
            assert!(
                rows[i].1 < rows[i + 1].1,
                "list positions must be strictly ascending: '{}' (list '{}') < '{}' (list '{}')",
                rows[i].1,
                rows[i].0,
                rows[i + 1].1,
                rows[i + 1].0
            );
        }

        // Verify the first list is "Inbox" (our seed order)
        assert_eq!(
            rows[0].0, "Inbox",
            "first list (by position) should be 'Inbox'"
        );
    }
}
