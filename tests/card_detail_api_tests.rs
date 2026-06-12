//! Wave 1 test scaffold for card detail foundation (Plan 01).
//! Tests: render_markdown XSS sanitization + get_card_detail_inner card_num contract.
//! Run: cargo test --features ssr --test card_detail_api_tests

#[cfg(feature = "ssr")]
mod card_detail_api_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::workspace_api::derive_key_prefix;
    use tempfile::NamedTempFile;

    // -------------------------------------------------------------------------
    // Shared fixtures (duplicated from card_api_tests — shared fixture pattern)
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
    // render_markdown tests (RED — Task 2 makes these compile and pass)
    // -------------------------------------------------------------------------

    /// render_markdown must sanitize XSS: script tags removed, bold preserved.
    ///
    /// Note: pulldown-cmark treats `<script>` as an HTML block (type 6), consuming
    /// text up to a blank line. With a blank line separator, the next paragraph is
    /// processed as Markdown. Tests verify the two independent properties:
    /// (1) script is stripped, (2) Markdown bold renders as <strong>.
    #[tokio::test]
    async fn test_render_markdown_strips_xss() {
        use lanes::api::card_detail_api::render_markdown;

        // XSS: script tags must be stripped (inline HTML block followed by blank line + bold)
        let xss_output = render_markdown("<script>alert(1)</script>\n\n**hi**");
        assert!(
            !xss_output.contains("<script>"),
            "script tag must be stripped by ammonia, got: {xss_output}"
        );
        assert!(
            xss_output.contains("<strong>hi</strong>"),
            "bold after script block should render as <strong>hi</strong>, got: {xss_output}"
        );

        // Markdown rendering: pure markdown input without raw HTML
        let md_output = render_markdown("**bold**");
        assert!(
            md_output.contains("<strong>bold</strong>"),
            "pure markdown bold must render as <strong>bold</strong>, got: {md_output}"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation tests (RED for TDD Task 3 — update_card_title_inner / update_card_description_inner)
    // -------------------------------------------------------------------------

    /// Helper: insert a list and card directly for mutation tests.
    async fn insert_card_direct(
        pool: &sqlx::SqlitePool,
        board_id: &str,
        list_id: &str,
        title: &str,
        card_num: i64,
    ) -> String {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        let card_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, checklist_done, checklist_total, comment_count, attachment_count,
               created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, card_num, title, pos, now, now
        )
        .execute(pool)
        .await
        .expect("insert card");
        card_id
    }

    /// Helper: insert a list directly.
    async fn insert_list_direct(pool: &sqlx::SqlitePool, board_id: &str, name: &str) -> String {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        let list_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id, board_id, name, pos
        )
        .execute(pool)
        .await
        .expect("insert list");
        list_id
    }

    /// update_card_title_inner validates: empty rejected, >500 chars rejected, wrong-board = 0 rows.
    #[tokio::test]
    async fn test_update_card_title_validates_and_scopes() {
        use lanes::api::card_detail_api::update_card_title_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "title_owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Title Test Board").await;
        let other_board_id = insert_board_direct(&write_pool, "Other Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        let list_id = insert_list_direct(&write_pool, &board_id, "Test List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Original Title", 1).await;

        // Empty title → Err
        let result = update_card_title_inner(&write_pool, &board_id, &card_id, "".to_string()).await;
        assert!(result.is_err(), "empty title must be rejected");

        // Whitespace-only title → Err (trims to empty)
        let result = update_card_title_inner(&write_pool, &board_id, &card_id, "   ".to_string()).await;
        assert!(result.is_err(), "whitespace-only title must be rejected");

        // >500 chars → Err
        let long_title = "a".repeat(501);
        let result = update_card_title_inner(&write_pool, &board_id, &card_id, long_title).await;
        assert!(result.is_err(), "title >500 chars must be rejected");

        // Valid update on correct board → Ok
        let result = update_card_title_inner(&write_pool, &board_id, &card_id, "New Title".to_string()).await;
        assert!(result.is_ok(), "valid title update must succeed: {:?}", result.err());
        assert_eq!(result.unwrap(), "New Title");

        // Verify DB row updated
        let stored: String = sqlx::query_scalar("SELECT title FROM cards WHERE id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch card title");
        assert_eq!(stored, "New Title", "DB title must match updated value");

        // IDOR scope: same card_id but wrong board_id → affects 0 rows (returns Ok with no change)
        // update_card_title_inner returns Ok with the trimmed title even if 0 rows were updated
        // (sqlx::query execute doesn't error on 0 rows affected), but we verify no cross-board write
        let result = update_card_title_inner(&write_pool, &other_board_id, &card_id, "Injected".to_string()).await;
        // Should succeed (no DB error) but affect 0 rows
        if result.is_ok() {
            let still_stored: String = sqlx::query_scalar("SELECT title FROM cards WHERE id = ?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch card title after cross-board attempt");
            assert_eq!(still_stored, "New Title", "cross-board title injection must affect 0 rows");
        }
    }

    /// update_card_description_inner stores raw markdown — not rendered HTML.
    #[tokio::test]
    async fn test_update_card_description_stores_raw_markdown() {
        use lanes::api::card_detail_api::update_card_description_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "desc_owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Desc Test Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;

        let list_id = insert_list_direct(&write_pool, &board_id, "Desc List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Card with Description", 2).await;

        let raw_markdown = "**bold** and _italic_\n\n- item 1\n- item 2";

        let result = update_card_description_inner(
            &write_pool,
            &board_id,
            &card_id,
            raw_markdown.to_string(),
        ).await;
        assert!(result.is_ok(), "description update must succeed: {:?}", result.err());

        // Stored value must be the raw markdown, NOT rendered HTML
        let stored: Option<String> = sqlx::query_scalar("SELECT description FROM cards WHERE id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch card description");

        let stored_desc = stored.unwrap_or_default();
        assert_eq!(stored_desc, raw_markdown, "raw markdown must be stored as-is, not rendered HTML");
        assert!(!stored_desc.contains("<strong>"), "stored value must NOT contain rendered HTML tags");
        assert!(!stored_desc.contains("<em>"), "stored value must NOT contain rendered HTML tags");
    }

    // -------------------------------------------------------------------------
    // get_card_detail_inner tests (RED — Task 2 makes these compile and pass)
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Checklist mutation tests (Task 1 — RED phase)
    // -------------------------------------------------------------------------

    /// Helper: insert a checklist directly for test setup.
    async fn insert_checklist_direct(pool: &sqlx::SqlitePool, card_id: &str) -> String {
        use uuid::Uuid;
        let checklist_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO checklists (id, card_id, title, position) VALUES (?, ?, 'Checklist', 0)",
            checklist_id, card_id
        )
        .execute(pool)
        .await
        .expect("insert checklist");
        checklist_id
    }

    /// Helper: insert a checklist item directly for test setup.
    async fn insert_checklist_item_direct(
        pool: &sqlx::SqlitePool,
        checklist_id: &str,
        text: &str,
        done: bool,
        position: i64,
    ) -> String {
        use uuid::Uuid;
        let item_id = Uuid::now_v7().to_string();
        let done_val = done as i64;
        sqlx::query!(
            "INSERT INTO checklist_items (id, checklist_id, text, done, position) VALUES (?, ?, ?, ?, ?)",
            item_id, checklist_id, text, done_val, position
        )
        .execute(pool)
        .await
        .expect("insert checklist item");
        item_id
    }

    /// toggle_checklist_item_inner sets done on the item AND recounts in the same transaction.
    /// Asserts: returned counts match DB counts (no drift).
    #[tokio::test]
    async fn test_toggle_checklist_item_updates_counts() {
        use lanes::api::card_detail_api::toggle_checklist_item_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "checklist_toggle@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Checklist Toggle Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Toggle List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Checklist Card", 1).await;

        // Seed checklist with 3 items, 1 already done
        let checklist_id = insert_checklist_direct(&write_pool, &card_id).await;
        let item1 = insert_checklist_item_direct(&write_pool, &checklist_id, "Item 1", false, 0).await;
        let _item2 = insert_checklist_item_direct(&write_pool, &checklist_id, "Item 2", true, 1).await;
        let _item3 = insert_checklist_item_direct(&write_pool, &checklist_id, "Item 3", false, 2).await;

        // Seed cards.checklist_total = 3, checklist_done = 1
        sqlx::query!("UPDATE cards SET checklist_total=3, checklist_done=1 WHERE id=?", card_id)
            .execute(&write_pool).await.expect("seed counts");

        // Toggle item1 from false → true
        let result = toggle_checklist_item_inner(&write_pool, &board_id, &card_id, &item1, true).await;
        assert!(result.is_ok(), "toggle should succeed: {:?}", result.err());
        let (done_flag, done_c, total_c) = result.unwrap();
        assert!(done_flag, "returned done flag must be true");
        assert_eq!(total_c, 3, "total must be 3");
        assert_eq!(done_c, 2, "done must be 2 after toggling item1 done");

        // Verify DB cards row matches returned counts (no drift — T-05-12)
        let (db_done, db_total): (i64, i64) =
            sqlx::query_as("SELECT checklist_done, checklist_total FROM cards WHERE id=?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch card counts");
        assert_eq!(db_done, done_c, "DB checklist_done must match returned value");
        assert_eq!(db_total, total_c, "DB checklist_total must match returned value");

        // Verify the item itself is marked done
        let item_done: i64 =
            sqlx::query_scalar("SELECT done FROM checklist_items WHERE id=?")
                .bind(&item1)
                .fetch_one(&write_pool)
                .await
                .expect("fetch item done");
        assert_eq!(item_done, 1, "checklist_items.done must be 1 after toggle");
    }

    /// add_checklist_item_inner creates the checklist if none exists and bumps cards.checklist_total.
    #[tokio::test]
    async fn test_add_checklist_item_creates_checklist_and_bumps_total() {
        use lanes::api::card_detail_api::add_checklist_item_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "checklist_add@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Checklist Add Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Add List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Add Card", 2).await;

        // No checklist exists yet — add_checklist_item_inner must create one
        let result = add_checklist_item_inner(&write_pool, &board_id, &card_id, "First item".to_string()).await;
        assert!(result.is_ok(), "add_checklist_item should succeed: {:?}", result.err());
        let (item, done_c, total_c) = result.unwrap();
        assert_eq!(item.text, "First item", "returned item text must match");
        assert!(!item.done, "new item must be undone");
        assert_eq!(total_c, 1, "total must be 1 after adding first item");
        assert_eq!(done_c, 0, "done count must be 0 for fresh item");

        // Verify DB cards.checklist_total was bumped
        let db_total: i64 =
            sqlx::query_scalar("SELECT checklist_total FROM cards WHERE id=?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch card total");
        assert_eq!(db_total, total_c, "DB checklist_total must match returned total");

        // A checklist row must have been created
        let checklist_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM checklists WHERE card_id=?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("count checklists");
        assert_eq!(checklist_count, 1, "one checklist must have been auto-created");

        // Adding a second item does NOT create another checklist
        let result2 = add_checklist_item_inner(&write_pool, &board_id, &card_id, "Second item".to_string()).await;
        assert!(result2.is_ok(), "second add should succeed");
        let (_, _, total2) = result2.unwrap();
        assert_eq!(total2, 2, "total must be 2 after adding second item");

        let checklist_count2: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM checklists WHERE card_id=?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("count checklists again");
        assert_eq!(checklist_count2, 1, "still only one checklist after second add");

        // Reject empty text
        let result_empty = add_checklist_item_inner(&write_pool, &board_id, &card_id, "  ".to_string()).await;
        assert!(result_empty.is_err(), "empty item text must be rejected");
    }

    /// get_card_detail_inner returns CardDetail with the correct card_num for the seeded card.
    #[tokio::test]
    async fn test_get_card_detail_inner_returns_card_num() {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        use lanes::api::card_detail_api::get_card_detail_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "detail_owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Detail Test Board").await;
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

        // Insert card with known card_num
        let card_id = Uuid::now_v7().to_string();
        let card_pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let expected_card_num: i64 = 42;
        // Set boards.next_card_num so that subsequent creates don't collide
        sqlx::query!("UPDATE boards SET next_card_num = 43 WHERE id = ?", board_id)
            .execute(&write_pool)
            .await
            .expect("update next_card_num");
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, checklist_done, checklist_total, comment_count, attachment_count,
               created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, expected_card_num, "Detail Test Card", card_pos, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert card");

        let detail = get_card_detail_inner(&write_pool, &board_id, &expected_card_num, &user_id)
            .await
            .expect("get_card_detail_inner should succeed");

        assert_eq!(
            detail.card.card_num, expected_card_num,
            "card_num must match the seeded value"
        );
    }

    // -------------------------------------------------------------------------
    // Property mutation tests (Task 2 — RED phase)
    // -------------------------------------------------------------------------

    /// Helper: insert a label directly.
    async fn insert_label_direct(pool: &sqlx::SqlitePool, board_id: &str, name: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, 'oklch(72% 0.10 25)')",
            id, board_id, name
        )
        .execute(pool)
        .await
        .expect("insert label");
        id
    }

    /// assign_label_inner rejects labels that don't belong to the card's board (cross-board injection).
    #[tokio::test]
    async fn test_assign_label_is_board_scoped() {
        use lanes::api::card_detail_api::assign_label_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "label_scoped@test.com").await;
        let board_a = insert_board_direct(&write_pool, "Board A").await;
        let board_b = insert_board_direct(&write_pool, "Board B").await;
        insert_member_direct(&write_pool, &board_a, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_a, "List A").await;
        let card_id = insert_card_direct(&write_pool, &board_a, &list_id, "Label Card", 1).await;

        let label_b = insert_label_direct(&write_pool, &board_b, "Board B Label").await;
        let label_a = insert_label_direct(&write_pool, &board_a, "Board A Label").await;

        // Cross-board: label_b belongs to board_b, not board_a — must NOT insert
        let result = assign_label_inner(&write_pool, &board_a, &card_id, &label_b, true).await;
        // assign_label_inner returns Ok(()) with no row inserted (verified below) OR returns Err
        let count_b: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM card_labels WHERE card_id=? AND label_id=?")
                .bind(&card_id).bind(&label_b)
                .fetch_one(&write_pool).await.expect("count b");
        assert_eq!(count_b, 0, "cross-board label must not be assigned (T-05-08)");
        // We accept either Ok(()) (silently skipped) or Err — both are correct security behaviors
        let _ = result;

        // Same-board: label_a belongs to board_a — must insert
        let result_ok = assign_label_inner(&write_pool, &board_a, &card_id, &label_a, true).await;
        assert!(result_ok.is_ok(), "same-board label assign must succeed: {:?}", result_ok.err());

        let count_a: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM card_labels WHERE card_id=? AND label_id=?")
                .bind(&card_id).bind(&label_a)
                .fetch_one(&write_pool).await.expect("count a");
        assert_eq!(count_a, 1, "same-board label must be assigned");

        // Idempotent re-assign (INSERT OR IGNORE — no duplicate PK error)
        let result_dupe = assign_label_inner(&write_pool, &board_a, &card_id, &label_a, true).await;
        assert!(result_dupe.is_ok(), "duplicate assign must not error (INSERT OR IGNORE)");

        let count_dupe: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM card_labels WHERE card_id=? AND label_id=?")
                .bind(&card_id).bind(&label_a)
                .fetch_one(&write_pool).await.expect("count dupe");
        assert_eq!(count_dupe, 1, "duplicate assign must not create duplicate row");

        // Unassign
        let result_unassign = assign_label_inner(&write_pool, &board_a, &card_id, &label_a, false).await;
        assert!(result_unassign.is_ok(), "unassign must succeed");
        let count_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM card_labels WHERE card_id=? AND label_id=?")
                .bind(&card_id).bind(&label_a)
                .fetch_one(&write_pool).await.expect("count after unassign");
        assert_eq!(count_after, 0, "label must be removed after unassign");
    }

    /// set_priority_inner rejects values not in P1/P2/P3/None.
    #[tokio::test]
    async fn test_set_priority_rejects_invalid() {
        use lanes::api::card_detail_api::set_priority_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "priority@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Priority Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Prio List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Prio Card", 1).await;

        // Valid values
        for prio in &[Some("P1"), Some("P2"), Some("P3"), None] {
            let result = set_priority_inner(&write_pool, &board_id, &card_id, prio.map(|s| s.to_string())).await;
            assert!(result.is_ok(), "valid priority {:?} must succeed: {:?}", prio, result.err());
        }

        // Invalid values
        for bad in &["p1", "HIGH", "P4", "P0", "urgent", ""] {
            let result = set_priority_inner(&write_pool, &board_id, &card_id, Some(bad.to_string())).await;
            assert!(result.is_err(), "invalid priority {:?} must be rejected", bad);
        }

        // Verify DB stores None correctly
        let _ = set_priority_inner(&write_pool, &board_id, &card_id, None).await;
        let stored_prio: Option<String> =
            sqlx::query_scalar("SELECT priority FROM cards WHERE id=?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch priority");
        assert!(stored_prio.is_none(), "priority must be NULL after set_priority_inner(None)");
    }

    // -------------------------------------------------------------------------
    // Comment + mention tests (Task 1 of Plan 04 — RED phase)
    // -------------------------------------------------------------------------

    /// add_comment_inner bumps cards.comment_count and inserts a watchers row for the author.
    #[tokio::test]
    async fn test_add_comment_increments_count_and_auto_watches() {
        use lanes::api::card_detail_api::add_comment_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let author_id = insert_user_direct(&write_pool, "comment_author@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Comment Count Board").await;
        insert_member_direct(&write_pool, &board_id, &author_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Comment List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Comment Card", 1).await;

        // comment_count starts at 0
        let count_before: i64 = sqlx::query_scalar("SELECT comment_count FROM cards WHERE id=?")
            .bind(&card_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch initial count");
        assert_eq!(count_before, 0, "initial comment_count must be 0");

        let result = add_comment_inner(
            &write_pool,
            &board_id,
            &card_id,
            &author_id,
            "Hello, world!".to_string(),
            vec![],
        ).await;
        assert!(result.is_ok(), "add_comment_inner must succeed: {:?}", result.err());
        let (entry, _mentions) = result.unwrap();
        assert_eq!(entry.entry_type, "comment", "returned entry_type must be 'comment'");
        assert_eq!(entry.text, "Hello, world!", "returned text must match body");

        // comment_count must have been bumped to 1
        let count_after: i64 = sqlx::query_scalar("SELECT comment_count FROM cards WHERE id=?")
            .bind(&card_id)
            .fetch_one(&write_pool)
            .await
            .expect("fetch count after comment");
        assert_eq!(count_after, 1, "comment_count must be 1 after posting a comment");

        // Author must be in watchers (D-12 auto-watch)
        let watcher_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM watchers WHERE card_id=? AND user_id=?")
                .bind(&card_id)
                .bind(&author_id)
                .fetch_one(&write_pool)
                .await
                .expect("watchers count");
        assert_eq!(watcher_count, 1, "author must be auto-watched after commenting (D-12)");

        // Empty body must be rejected
        let result_empty = add_comment_inner(
            &write_pool,
            &board_id,
            &card_id,
            &author_id,
            "   ".to_string(),
            vec![],
        ).await;
        assert!(result_empty.is_err(), "empty comment body must be rejected");
    }

    /// mention of another board member creates a notification; self-mention is suppressed.
    #[tokio::test]
    async fn test_mention_creates_notification_skips_self() {
        use lanes::api::card_detail_api::add_comment_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let author_id = insert_user_direct(&write_pool, "mention_author@test.com").await;
        let other_id = insert_user_direct(&write_pool, "mention_other@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Mention Board").await;
        insert_member_direct(&write_pool, &board_id, &author_id, "owner").await;
        insert_member_direct(&write_pool, &board_id, &other_id, "member").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Mention List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Mention Card", 1).await;

        // Mention another board member → 1 notification row
        let result = add_comment_inner(
            &write_pool,
            &board_id,
            &card_id,
            &author_id,
            "@other hello".to_string(),
            vec![other_id.clone()],
        ).await;
        assert!(result.is_ok(), "add_comment with mention must succeed: {:?}", result.err());

        let notif_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=? AND card_id=? AND kind='mention'")
                .bind(&other_id)
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("notifications count");
        assert_eq!(notif_count, 1, "mentioning another board member must create 1 notification row");

        // Self-mention: author_id in mention_user_ids → 0 notifications for self
        let result2 = add_comment_inner(
            &write_pool,
            &board_id,
            &card_id,
            &author_id,
            "@me again".to_string(),
            vec![author_id.clone()],
        ).await;
        assert!(result2.is_ok(), "self-mention comment must succeed");

        let self_notif: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=? AND card_id=? AND kind='mention'")
                .bind(&author_id)
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("self notif count");
        assert_eq!(self_notif, 0, "self-mention must NOT create a notification row (D-11)");
    }

    /// mention_user_id that is not a board member produces no notification row.
    #[tokio::test]
    async fn test_mention_non_member_no_notification() {
        use lanes::api::card_detail_api::add_comment_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let author_id = insert_user_direct(&write_pool, "nonmember_author@test.com").await;
        let outsider_id = insert_user_direct(&write_pool, "outsider@test.com").await;
        // outsider is a user but NOT a board member
        let board_id = insert_board_direct(&write_pool, "Nonmember Board").await;
        insert_member_direct(&write_pool, &board_id, &author_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "NM List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "NM Card", 1).await;

        let result = add_comment_inner(
            &write_pool,
            &board_id,
            &card_id,
            &author_id,
            "@outsider hello".to_string(),
            vec![outsider_id.clone()],
        ).await;
        assert!(result.is_ok(), "comment with non-member mention must succeed (just no notification)");

        let notif_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=? AND card_id=?")
                .bind(&outsider_id)
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("outsider notif count");
        assert_eq!(notif_count, 0, "non-board-member must NOT receive a notification (T-05-14)");
    }

    /// record_attachment_inner inserts an attachments row and bumps cards.attachment_count.
    #[tokio::test]
    async fn test_record_attachment_bumps_count() {
        use lanes::api::card_detail_api::record_attachment_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "attach_owner@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Attachment Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Attach List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Attach Card", 1).await;

        // attachment_count starts at 0
        let count_before: i64 =
            sqlx::query_scalar("SELECT attachment_count FROM cards WHERE id = ?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch initial count");
        assert_eq!(count_before, 0, "initial attachment_count must be 0");

        let url = format!("/api/attachments/{}/{}/abc-uuid.pdf", board_id, card_id);
        let result = record_attachment_inner(
            &write_pool,
            &card_id,
            &user_id,
            "document.pdf",
            &url,
            1024,
        )
        .await;
        assert!(result.is_ok(), "record_attachment_inner must succeed: {:?}", result.err());

        let attachment = result.unwrap();
        assert_eq!(attachment.filename, "document.pdf", "filename must match");
        assert_eq!(attachment.url, url, "url must match");
        assert_eq!(attachment.size_bytes, 1024, "size must match");
        assert_eq!(attachment.card_id, card_id, "card_id must match");

        // attachment_count must have been bumped to 1
        let count_after: i64 =
            sqlx::query_scalar("SELECT attachment_count FROM cards WHERE id = ?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch count after");
        assert_eq!(count_after, 1, "attachment_count must be 1 after inserting an attachment");

        // Insert a second attachment — count must be 2
        let url2 = format!("/api/attachments/{}/{}/def-uuid.png", board_id, card_id);
        let result2 = record_attachment_inner(
            &write_pool,
            &card_id,
            &user_id,
            "image.png",
            &url2,
            2048,
        )
        .await;
        assert!(result2.is_ok(), "second record_attachment_inner must succeed");

        let count_two: i64 =
            sqlx::query_scalar("SELECT attachment_count FROM cards WHERE id = ?")
                .bind(&card_id)
                .fetch_one(&write_pool)
                .await
                .expect("fetch count after second");
        assert_eq!(count_two, 2, "attachment_count must be 2 after two attachments");
    }

    /// assign_member_inner inserts card_members AND watchers rows in the same transaction.
    #[tokio::test]
    async fn test_assign_member_auto_watches() {
        use lanes::api::card_detail_api::assign_member_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "assign_owner@test.com").await;
        let member_id = insert_user_direct(&write_pool, "assign_member@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Assign Board").await;
        insert_member_direct(&write_pool, &board_id, &owner_id, "owner").await;
        insert_member_direct(&write_pool, &board_id, &member_id, "member").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Assign List").await;
        let card_id = insert_card_direct(&write_pool, &board_id, &list_id, "Assign Card", 1).await;

        // Assign member (who is a board member) — should insert both card_members AND watchers
        let result = assign_member_inner(&write_pool, &board_id, &card_id, &member_id).await;
        assert!(result.is_ok(), "assign_member must succeed: {:?}", result.err());

        let cm_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM card_members WHERE card_id=? AND user_id=?")
                .bind(&card_id).bind(&member_id)
                .fetch_one(&write_pool).await.expect("card_members count");
        assert_eq!(cm_count, 1, "card_members row must be inserted");

        let watcher_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM watchers WHERE card_id=? AND user_id=?")
                .bind(&card_id).bind(&member_id)
                .fetch_one(&write_pool).await.expect("watchers count");
        assert_eq!(watcher_count, 1, "watchers row must be inserted (auto-watch D-12)");

        // Idempotent: assign again — no duplicate PK error
        let result2 = assign_member_inner(&write_pool, &board_id, &card_id, &member_id).await;
        assert!(result2.is_ok(), "duplicate assign must not error (INSERT OR IGNORE)");

        // Non-board-member: should fail
        let outsider_id = insert_user_direct(&write_pool, "outsider@test.com").await;
        let result_out = assign_member_inner(&write_pool, &board_id, &card_id, &outsider_id).await;
        assert!(result_out.is_err(), "non-board-member assign must be rejected (T-05-09)");
    }

    // -------------------------------------------------------------------------
    // Plan 06: move_card_cross_board_inner, watch/unwatch, archive_card_inner
    // -------------------------------------------------------------------------

    /// Helper: insert a card with card_num (ensuring boards.next_card_num is set correctly).
    async fn insert_card_with_num(
        pool: &sqlx::SqlitePool,
        board_id: &str,
        list_id: &str,
        title: &str,
        card_num: i64,
    ) -> String {
        use uuid::Uuid;
        use fractional_index::FractionalIndex;
        let card_id = Uuid::now_v7().to_string();
        let pos = FractionalIndex::default().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position,
               done, archived, checklist_done, checklist_total, comment_count, attachment_count,
               created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, ?, ?)"#,
            card_id, list_id, board_id, card_num, title, pos, now, now
        )
        .execute(pool)
        .await
        .expect("insert card with num");
        card_id
    }

    /// move_card_cross_board_inner:
    /// - reallocates card_num on target board
    /// - updates board_id + list_id
    /// - strips all card_labels (board-scoped, D-05)
    /// - strips non-target-board card_members (D-05)
    /// - keeps comments and attachments (child rows keyed by card_id — A3)
    /// - logs a card_events 'moved' entry (D-06)
    #[tokio::test]
    async fn test_cross_board_move_reallocates_and_strips() {
        use lanes::api::card_detail_api::move_card_cross_board_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        // Board A setup
        let owner_id = insert_user_direct(&write_pool, "cross_owner@test.com").await;
        let only_a_user_id = insert_user_direct(&write_pool, "only_a@test.com").await;
        let both_user_id = insert_user_direct(&write_pool, "both_boards@test.com").await;
        let board_a_id = insert_board_direct(&write_pool, "Board A").await;
        let board_b_id = insert_board_direct(&write_pool, "Board B").await;

        // Members: owner + only_a in board A; owner + both_user in board B
        insert_member_direct(&write_pool, &board_a_id, &owner_id, "owner").await;
        insert_member_direct(&write_pool, &board_a_id, &only_a_user_id, "member").await;
        insert_member_direct(&write_pool, &board_a_id, &both_user_id, "member").await;
        insert_member_direct(&write_pool, &board_b_id, &owner_id, "owner").await;
        insert_member_direct(&write_pool, &board_b_id, &both_user_id, "member").await;
        // NOTE: only_a_user_id is NOT a member of board B

        let list_a_id = insert_list_direct(&write_pool, &board_a_id, "List A").await;
        let list_b_id = insert_list_direct(&write_pool, &board_b_id, "List B").await;

        // Set next_card_num so we can verify reallocation
        sqlx::query!("UPDATE boards SET next_card_num = 1 WHERE id = ?", board_a_id)
            .execute(&write_pool).await.expect("set next_card_num A");
        sqlx::query!("UPDATE boards SET next_card_num = 5 WHERE id = ?", board_b_id)
            .execute(&write_pool).await.expect("set next_card_num B (5 = first on B)");

        let card_id = insert_card_with_num(&write_pool, &board_a_id, &list_a_id, "Lisbon Trip", 1).await;

        // Add a label on board A
        let label_a_id: String = {
            use uuid::Uuid;
            let id = Uuid::now_v7().to_string();
            sqlx::query!("INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, 'Travel', '#abc')", id, board_a_id)
                .execute(&write_pool).await.expect("insert label A");
            sqlx::query!("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)", card_id, id)
                .execute(&write_pool).await.expect("insert card_label");
            id
        };
        let _ = label_a_id;

        // Add card members: owner (on B), only_a (NOT on B), both_user (on B)
        sqlx::query!("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)", card_id, owner_id)
            .execute(&write_pool).await.expect("cm owner");
        sqlx::query!("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)", card_id, only_a_user_id)
            .execute(&write_pool).await.expect("cm only_a");
        sqlx::query!("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)", card_id, both_user_id)
            .execute(&write_pool).await.expect("cm both");

        // Add a comment (child row — should survive)
        {
            use uuid::Uuid;
            let cmt_id = Uuid::now_v7().to_string();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap().as_millis() as i64;
            sqlx::query!("INSERT INTO comments (id, card_id, author_id, body, created_at) VALUES (?, ?, ?, 'test comment', ?)",
                cmt_id, card_id, owner_id, now)
                .execute(&write_pool).await.expect("insert comment");
        }

        // Add an attachment (child row — should survive)
        {
            use uuid::Uuid;
            let att_id = Uuid::now_v7().to_string();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap().as_millis() as i64;
            sqlx::query!("INSERT INTO attachments (id, card_id, uploader_id, filename, url, size_bytes, created_at) VALUES (?, ?, ?, 'file.pdf', '/api/a/1', 100, ?)",
                att_id, card_id, owner_id, now)
                .execute(&write_pool).await.expect("insert attachment");
        }

        // --- Perform cross-board move ---
        use fractional_index::FractionalIndex;
        let new_pos = FractionalIndex::default().to_string();
        let result = move_card_cross_board_inner(
            &write_pool,
            &board_a_id,
            &card_id,
            &board_b_id,
            &list_b_id,
            &new_pos,
        ).await;
        assert!(result.is_ok(), "cross-board move must succeed: {:?}", result.err());
        let new_card_num = result.unwrap();

        // 1. New card_num equals what board B allocated (5)
        assert_eq!(new_card_num, 5, "new card_num must be the pre-move next_card_num (5)");

        // 2. boards.next_card_num on board B must be 6 now
        let b_next: i64 = sqlx::query_scalar("SELECT next_card_num FROM boards WHERE id = ?")
            .bind(&board_b_id)
            .fetch_one(&write_pool).await.expect("next_card_num B");
        assert_eq!(b_next, 6, "boards.next_card_num on B must be incremented to 6");

        // 3. Card row updated: board_id = board_b, list_id = list_b, card_num = 5
        let (db_board, db_list, db_num): (String, String, i64) = sqlx::query_as(
            "SELECT board_id, list_id, card_num FROM cards WHERE id = ?"
        )
        .bind(&card_id)
        .fetch_one(&write_pool).await.expect("card row after move");
        assert_eq!(db_board, board_b_id, "card must be on board B after cross-board move");
        assert_eq!(db_list, list_b_id, "card must be in list B after cross-board move");
        assert_eq!(db_num, 5, "card_num must be the new allocation");

        // 4. card_labels stripped (T-05-24)
        let label_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM card_labels WHERE card_id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool).await.expect("label count");
        assert_eq!(label_count, 0, "all board-scoped labels must be stripped on cross-board move (D-05)");

        // 5. Only target-board members remain
        let remaining_members: Vec<String> = sqlx::query_scalar(
            "SELECT user_id FROM card_members WHERE card_id = ? ORDER BY user_id ASC"
        )
        .bind(&card_id)
        .fetch_all(&write_pool).await.expect("card_members after move");
        // only_a is NOT on board B — must be removed
        assert!(!remaining_members.contains(&only_a_user_id),
            "only_a (not on board B) must be removed from card_members (D-05)");
        // owner and both_user are on board B — must remain
        assert!(remaining_members.contains(&owner_id),
            "owner (on board B) must remain in card_members");
        assert!(remaining_members.contains(&both_user_id),
            "both_user (on board B) must remain in card_members");

        // 6. Comments still present (keyed by card_id — A3)
        let comment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE card_id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool).await.expect("comment count");
        assert_eq!(comment_count, 1, "comments must survive cross-board move (A3)");

        // 7. Attachments still present (keyed by card_id — A3)
        let attachment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE card_id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool).await.expect("attachment count");
        assert_eq!(attachment_count, 1, "attachments must survive cross-board move (A3)");

        // 8. card_events 'moved' entry exists (D-06)
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM card_events WHERE card_id = ? AND kind = 'moved'"
        )
        .bind(&card_id)
        .fetch_one(&write_pool).await.expect("event count");
        assert_eq!(event_count, 1, "a 'moved' card_event must be logged (D-06)");
    }

    /// watch_card_inner / unwatch_card_inner return updated distinct watcher count.
    #[tokio::test]
    async fn test_watch_unwatch_count() {
        use lanes::api::card_detail_api::{watch_card_inner, unwatch_card_inner};

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "watcher@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Watch Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Watch List").await;
        let card_id = insert_card_with_num(&write_pool, &board_id, &list_id, "Watch Card", 1).await;

        // Watch → count 1
        let count_after_watch = watch_card_inner(&write_pool, &card_id, &user_id).await
            .expect("watch_card_inner must succeed");
        assert_eq!(count_after_watch, 1, "watcher count must be 1 after watch");

        // Idempotent watch (INSERT OR IGNORE) → count still 1
        let count_dupe = watch_card_inner(&write_pool, &card_id, &user_id).await
            .expect("duplicate watch must not error");
        assert_eq!(count_dupe, 1, "watcher count must still be 1 after duplicate watch");

        // Unwatch → count 0
        let count_after_unwatch = unwatch_card_inner(&write_pool, &card_id, &user_id).await
            .expect("unwatch_card_inner must succeed");
        assert_eq!(count_after_unwatch, 0, "watcher count must be 0 after unwatch");

        // Unwatch again (idempotent) → count still 0
        let count_dupe_unwatch = unwatch_card_inner(&write_pool, &card_id, &user_id).await
            .expect("duplicate unwatch must not error");
        assert_eq!(count_dupe_unwatch, 0, "watcher count must still be 0 after duplicate unwatch");
    }

    // -------------------------------------------------------------------------
    // list_move_targets_inner tests (gap-fix: Move popover dropdowns)
    // -------------------------------------------------------------------------

    /// Helper: insert a list with a custom position string (avoids UNIQUE(board_id, position) collision).
    async fn insert_list_with_pos(pool: &sqlx::SqlitePool, board_id: &str, name: &str, pos: &str) -> String {
        use uuid::Uuid;
        let list_id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id, board_id, name, pos
        )
        .execute(pool)
        .await
        .expect("insert list with pos");
        list_id
    }

    /// list_move_targets_inner returns only boards the user is a member of,
    /// each with their non-archived lists.
    #[tokio::test]
    async fn test_list_move_targets_returns_only_user_boards() {
        use lanes::api::card_detail_api::list_move_targets_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_a = insert_user_direct(&write_pool, "move_targets_a@test.com").await;
        let user_b = insert_user_direct(&write_pool, "move_targets_b@test.com").await;

        let board_a = insert_board_direct(&write_pool, "Board A Move").await;
        let board_b = insert_board_direct(&write_pool, "Board B Move").await;
        let board_c = insert_board_direct(&write_pool, "Board C Move").await;

        // user_a is a member of board_a and board_b (but NOT board_c)
        insert_member_direct(&write_pool, &board_a, &user_a, "owner").await;
        insert_member_direct(&write_pool, &board_b, &user_a, "member").await;
        insert_member_direct(&write_pool, &board_c, &user_b, "owner").await;

        // Use distinct positions to avoid UNIQUE(board_id, position) constraint
        let _list_a1 = insert_list_with_pos(&write_pool, &board_a, "List A1", "a0").await;
        let _list_a2 = insert_list_with_pos(&write_pool, &board_a, "List A2", "a1").await;
        let _list_b1 = insert_list_with_pos(&write_pool, &board_b, "List B1", "a0").await;

        let targets = list_move_targets_inner(&write_pool, &user_a)
            .await
            .expect("list_move_targets_inner must succeed");

        // Must return exactly board_a and board_b (user_a is a member of those)
        let board_ids: Vec<&str> = targets.iter().map(|b| b.id.as_str()).collect();
        assert!(board_ids.contains(&board_a.as_str()), "board_a must be in targets (user_a is a member)");
        assert!(board_ids.contains(&board_b.as_str()), "board_b must be in targets (user_a is a member)");
        assert!(!board_ids.contains(&board_c.as_str()), "board_c must NOT be in targets (user_a is not a member)");

        // board_a must have 2 lists
        let board_a_entry = targets.iter().find(|b| b.id == board_a).expect("board_a must be present");
        assert_eq!(board_a_entry.lists.len(), 2, "board_a must have 2 non-archived lists");

        // board_b must have 1 list
        let board_b_entry = targets.iter().find(|b| b.id == board_b).expect("board_b must be present");
        assert_eq!(board_b_entry.lists.len(), 1, "board_b must have 1 non-archived list");
    }

    /// list_move_targets_inner excludes archived boards and archived lists.
    #[tokio::test]
    async fn test_list_move_targets_excludes_archived() {
        use lanes::api::card_detail_api::list_move_targets_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user = insert_user_direct(&write_pool, "move_targets_arch@test.com").await;

        // Active board
        let active_board = insert_board_direct(&write_pool, "Active Board").await;
        // Archived board — insert directly (insert_board_direct always inserts non-archived)
        let archived_board_id = {
            use uuid::Uuid;
            let id = Uuid::now_v7().to_string();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            sqlx::query!(
                r#"INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
                   VALUES (?, 'Archived Board', 'ARCH', '#aaa', 0, 1, ?, ?)"#,
                id, now, now
            )
            .execute(&write_pool)
            .await
            .expect("insert archived board");
            id
        };

        insert_member_direct(&write_pool, &active_board, &user, "owner").await;
        insert_member_direct(&write_pool, &archived_board_id, &user, "member").await;

        // Active board has one active list and one archived list (distinct positions)
        let _active_list = insert_list_with_pos(&write_pool, &active_board, "Active List", "a0").await;
        let archived_list_id = {
            use uuid::Uuid;
            let id = Uuid::now_v7().to_string();
            sqlx::query!(
                "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, 'Archived List', 'a1', 1)",
                id, active_board
            )
            .execute(&write_pool)
            .await
            .expect("insert archived list");
            id
        };
        let _ = archived_list_id;

        let targets = list_move_targets_inner(&write_pool, &user)
            .await
            .expect("list_move_targets_inner must succeed");

        // Archived board must not appear
        let board_ids: Vec<&str> = targets.iter().map(|b| b.id.as_str()).collect();
        assert!(!board_ids.contains(&archived_board_id.as_str()),
            "archived board must be excluded from move targets");

        // Active board must appear with only its non-archived list
        let active_entry = targets.iter().find(|b| b.id == active_board)
            .expect("active board must be in targets");
        assert_eq!(active_entry.lists.len(), 1,
            "active board must have exactly 1 list (the archived list is excluded)");
        assert_eq!(active_entry.lists[0].name, "Active List",
            "only the active list must be returned");
    }

    /// archive_card_inner sets archived=1 scoped by id AND board_id,
    /// logs a card_events 'archived' entry, and the card is absent from get_board_inner.
    #[tokio::test]
    async fn test_archive_card_absent_from_get_board() {
        use lanes::api::card_detail_api::archive_card_inner;
        use lanes::api::board_api::get_board_inner;

        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "archiver@test.com").await;
        let board_id = insert_board_direct(&write_pool, "Archive Board").await;
        insert_member_direct(&write_pool, &board_id, &user_id, "owner").await;
        let list_id = insert_list_direct(&write_pool, &board_id, "Archive List").await;

        // Set up a board name/key_prefix since get_board_inner needs the board row
        sqlx::query!("UPDATE boards SET name = 'Archive Board', key_prefix = 'ARB' WHERE id = ?", board_id)
            .execute(&write_pool).await.expect("update board");

        let card_id = insert_card_with_num(&write_pool, &board_id, &list_id, "To Archive", 1).await;

        // Card is present before archive
        let board_before = get_board_inner(&write_pool, &board_id, &user_id).await
            .expect("get_board before archive must succeed");
        let card_ids_before: Vec<String> = board_before.cards.iter()
            .map(|c| c.id.clone()).collect();
        assert!(card_ids_before.contains(&card_id),
            "card must be present before archiving");

        // Archive
        let result = archive_card_inner(&write_pool, &board_id, &card_id).await;
        assert!(result.is_ok(), "archive_card_inner must succeed: {:?}", result.err());

        // DB row: archived = 1
        let archived: i64 = sqlx::query_scalar("SELECT archived FROM cards WHERE id = ?")
            .bind(&card_id)
            .fetch_one(&write_pool).await.expect("fetch archived flag");
        assert_eq!(archived, 1, "cards.archived must be 1 after archive");

        // card_events 'archived' entry
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM card_events WHERE card_id = ? AND kind = 'archived'"
        )
        .bind(&card_id)
        .fetch_one(&write_pool).await.expect("archived event count");
        assert_eq!(event_count, 1, "a 'archived' card_event must be logged");

        // Card absent from get_board_inner (get_board filters archived = 0)
        let board_after = get_board_inner(&write_pool, &board_id, &user_id).await
            .expect("get_board after archive must succeed");
        let card_ids_after: Vec<String> = board_after.cards.iter()
            .map(|c| c.id.clone()).collect();
        assert!(!card_ids_after.contains(&card_id),
            "archived card must be absent from get_board_inner");
    }
}
