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
