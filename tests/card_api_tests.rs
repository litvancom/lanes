//! Wave 0 test scaffold for card server-fn tests (Plan 02/03 home fixture).
//! Smoke-tests get_board_inner enriched path: labels, cover, counts returned correctly.
//! Run: cargo test --features ssr --test card_api_tests

#[cfg(feature = "ssr")]
mod card_api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::derive_key_prefix;
    use lanes::api::board_api::get_board_inner;
    use tempfile::NamedTempFile;

    // -------------------------------------------------------------------------
    // Shared fixtures (Plan 02/03 will reuse these helpers)
    // -------------------------------------------------------------------------

    /// Create a temp DB with migrations applied; return (file guard, write_pool, read_pool).
    pub async fn test_db() -> (NamedTempFile, sqlx::SqlitePool, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool, read_pool)
    }

    /// Insert a user row directly for test setup.
    pub async fn insert_user_direct(pool: &sqlx::SqlitePool, email: &str) -> String {
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
    pub async fn insert_board_direct(pool: &sqlx::SqlitePool, name: &str) -> String {
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
    pub async fn insert_member_direct(pool: &sqlx::SqlitePool, board_id: &str, user_id: &str, role: &str) {
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, ?)",
            board_id, user_id, role
        )
        .execute(pool)
        .await
        .expect("insert member");
    }

    // -------------------------------------------------------------------------
    // Smoke test: get_board_inner returns enriched card with label + cover + counts
    // -------------------------------------------------------------------------

    /// A card with one attached label, a cover, and set counts is returned
    /// enriched from get_board_inner with labels.len() == 1, correct cover,
    /// and correct count fields.
    #[tokio::test]
    async fn test_get_board_inner_returns_enriched_card() {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;

        // Setup: user, board, member row
        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Test Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        // Insert list
        let list_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id, board_id, "Test List", pos
        )
        .execute(&write_pool)
        .await
        .expect("insert list");

        // Insert card with cover and count values
        let card_id = Uuid::now_v7().to_string();
        let card_pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let cover = "#f5e6d3";
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, cover, position,
               checklist_done, checklist_total, comment_count, attachment_count,
               done, archived, created_at, updated_at)
               VALUES (?, ?, ?, 1, ?, ?, ?, 3, 8, 1, 0, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, "Plan holiday menu", cover, card_pos, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert card");

        // Insert a label and link it to the card
        let label_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
            label_id, board_id, "Home", "oklch(68% 0.10 240)"
        )
        .execute(&write_pool)
        .await
        .expect("insert label");

        sqlx::query!(
            "INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)",
            card_id, label_id
        )
        .execute(&write_pool)
        .await
        .expect("insert card_label");

        // Fetch board data via get_board_inner
        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.cards.len(), 1, "one non-archived card expected");
        let card = &data.cards[0];

        // Labels populated from join
        assert_eq!(card.labels.len(), 1, "card should have 1 label");
        assert_eq!(card.labels[0].name, "Home");
        assert_eq!(card.labels[0].color, "oklch(68% 0.10 240)");

        // Cover populated
        assert_eq!(
            card.cover.as_deref(),
            Some("#f5e6d3"),
            "cover must be returned"
        );

        // Counts populated
        assert_eq!(card.checklist_done, 3);
        assert_eq!(card.checklist_total, 8);
        assert_eq!(card.comment_count, 1);
        assert_eq!(card.attachment_count, 0);
    }

    /// A card with no labels returns an empty labels vec.
    #[tokio::test]
    async fn test_get_board_inner_card_no_labels_returns_empty() {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "owner2@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Empty Label Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        let list_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id, board_id, "Todo", pos
        )
        .execute(&write_pool)
        .await
        .expect("insert list");

        let card_id = Uuid::now_v7().to_string();
        let card_pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, created_at, updated_at)
               VALUES (?, ?, ?, 1, ?, ?, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, "Simple card", card_pos, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert card");

        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.cards.len(), 1);
        let card = &data.cards[0];
        assert!(card.labels.is_empty(), "card with no labels should have empty labels vec");
        assert!(card.cover.is_none(), "card with no cover should return None");
        assert_eq!(card.member_ids.len(), 0, "card with no members should have empty member_ids");
    }

    /// A card with two labels returns both in the labels vec.
    #[tokio::test]
    async fn test_get_board_inner_card_with_two_labels() {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "owner3@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Two Label Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        let list_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id, board_id, "Work", pos
        )
        .execute(&write_pool)
        .await
        .expect("insert list");

        let card_id = Uuid::now_v7().to_string();
        let card_pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, created_at, updated_at)
               VALUES (?, ?, ?, 1, ?, ?, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, "Multi-label card", card_pos, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert card");

        // Insert two labels and link both
        let label1_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
            label1_id, board_id, "Urgent", "oklch(72% 0.10 25)"
        )
        .execute(&write_pool)
        .await
        .expect("insert label 1");

        let label2_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
            label2_id, board_id, "Travel", "oklch(70% 0.07 200)"
        )
        .execute(&write_pool)
        .await
        .expect("insert label 2");

        sqlx::query!(
            "INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)",
            card_id, label1_id
        )
        .execute(&write_pool)
        .await
        .expect("insert card_label 1");

        sqlx::query!(
            "INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)",
            card_id, label2_id
        )
        .execute(&write_pool)
        .await
        .expect("insert card_label 2");

        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.cards.len(), 1);
        let card = &data.cards[0];
        assert_eq!(card.labels.len(), 2, "card should have 2 labels");
    }

    // -------------------------------------------------------------------------
    // Helper: insert a list row directly for test setup.
    // -------------------------------------------------------------------------
    pub async fn insert_list_direct(pool: &sqlx::SqlitePool, board_id: &str, name: &str) -> String {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        let id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            id, board_id, name, pos
        )
        .execute(pool)
        .await
        .expect("insert list");
        id
    }

    // -------------------------------------------------------------------------
    // create_card_inner tests (Task 1, Plan 02)
    // -------------------------------------------------------------------------

    /// Empty title is rejected before any DB write.
    #[tokio::test]
    async fn test_create_card_empty_title_rejected() {
        use lanes::api::card_api::create_card_inner;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner_card1@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Card Test Board 1").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Backlog").await;

        let result = create_card_inner(&write_pool, &board_id, &list_id, "   ".to_string(), "a0").await;
        assert!(result.is_err(), "empty/whitespace title must be rejected");

        // Confirm no row was inserted
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE list_id = ?")
            .bind(&list_id)
            .fetch_one(&write_pool)
            .await
            .expect("count query");
        assert_eq!(count, 0, "no card row should be inserted for empty title");
    }

    /// Title longer than 500 characters is rejected.
    #[tokio::test]
    async fn test_create_card_title_too_long_rejected() {
        use lanes::api::card_api::create_card_inner;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner_card2@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Card Test Board 2").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Backlog").await;

        let long_title = "a".repeat(501);
        let result = create_card_inner(&write_pool, &board_id, &list_id, long_title, "a0").await;
        assert!(result.is_err(), "title > 500 chars must be rejected");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE list_id = ?")
            .bind(&list_id)
            .fetch_one(&write_pool)
            .await
            .expect("count query");
        assert_eq!(count, 0, "no card row should be inserted for too-long title");
    }

    /// Created card has a valid fractional position string.
    #[tokio::test]
    async fn test_create_card_returns_valid_position() {
        use lanes::api::card_api::{create_card_inner, next_card_position};
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner_card3@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Card Test Board 3").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Backlog").await;

        let position = next_card_position(&write_pool, &list_id).await.expect("position");
        let card = create_card_inner(&write_pool, &board_id, &list_id, "Test card".to_string(), &position).await.expect("create card");

        // Position must parse as a valid FractionalIndex
        let fi = FractionalIndex::from_string(&card.position);
        assert!(fi.is_ok(), "card position '{}' must be a valid FractionalIndex", card.position);
    }

    /// Two sequential creates yield positions where the second sorts after the first.
    #[tokio::test]
    async fn test_create_card_appends_after_last() {
        use lanes::api::card_api::{create_card_inner, next_card_position};
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner_card4@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Card Test Board 4").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Backlog").await;

        let pos1 = next_card_position(&write_pool, &list_id).await.expect("pos1");
        let card1 = create_card_inner(&write_pool, &board_id, &list_id, "First card".to_string(), &pos1).await.expect("card1");

        let pos2 = next_card_position(&write_pool, &list_id).await.expect("pos2");
        let card2 = create_card_inner(&write_pool, &board_id, &list_id, "Second card".to_string(), &pos2).await.expect("card2");

        let fi1 = FractionalIndex::from_string(&card1.position).expect("fi1");
        let fi2 = FractionalIndex::from_string(&card2.position).expect("fi2");
        assert!(fi2 > fi1, "second card position must sort after first");
    }

    /// create_card_inner allocates card_num atomically from boards.next_card_num.
    #[tokio::test]
    async fn test_create_card_allocates_card_num() {
        use lanes::api::card_api::{create_card_inner, next_card_position};

        let (_file, write_pool, _read_pool) = test_db().await;
        let user_id = insert_user_direct(&write_pool, "owner_card5@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Card Test Board 5").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Backlog").await;

        let pos1 = next_card_position(&write_pool, &list_id).await.expect("pos1");
        let card1 = create_card_inner(&write_pool, &board_id, &list_id, "Card One".to_string(), &pos1).await.expect("card1");

        let pos2 = next_card_position(&write_pool, &list_id).await.expect("pos2");
        let card2 = create_card_inner(&write_pool, &board_id, &list_id, "Card Two".to_string(), &pos2).await.expect("card2");

        // Each card gets a unique, sequential card_num
        assert_ne!(card1.card_num, card2.card_num, "cards must have different card_nums");
        assert!(card2.card_num > card1.card_num, "second card_num must be greater than first");

        // Verify the counter was incremented in the DB
        let next_num: i64 = sqlx::query_scalar("SELECT next_card_num FROM boards WHERE id = ?")
            .bind(&board_id)
            .fetch_one(&write_pool)
            .await
            .expect("next_card_num");
        assert!(next_num > card2.card_num, "boards.next_card_num must be greater than last allocated num");
    }

    /// is_done_list is populated on the list returned by get_board_inner.
    #[tokio::test]
    async fn test_get_board_inner_list_is_done_list_populated() {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "owner4@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Done List Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        let pos1 = FractionalIndex::default().to_string();
        let pos2 = FractionalIndex::new_after(&FractionalIndex::default()).to_string();

        // Regular list
        let regular_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived, is_done_list) VALUES (?, ?, ?, ?, 0, 0)",
            regular_id, board_id, "In Progress", pos1
        )
        .execute(&write_pool)
        .await
        .expect("insert regular list");

        // Done list
        let done_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived, is_done_list) VALUES (?, ?, ?, ?, 0, 1)",
            done_id, board_id, "Done", pos2
        )
        .execute(&write_pool)
        .await
        .expect("insert done list");

        let data = get_board_inner(&write_pool, &board_id, &user_id)
            .await
            .expect("get_board_inner");

        assert_eq!(data.lists.len(), 2);
        let regular = data.lists.iter().find(|l| l.id == regular_id).unwrap();
        let done = data.lists.iter().find(|l| l.id == done_id).unwrap();

        assert!(!regular.is_done_list, "regular list must have is_done_list=false");
        assert!(done.is_done_list, "done list must have is_done_list=true");
    }
}
