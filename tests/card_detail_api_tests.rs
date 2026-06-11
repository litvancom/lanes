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
}
