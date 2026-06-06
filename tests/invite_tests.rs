//! Tests for invite token generation and invite creation inner fn.
//! Run: DATABASE_URL=sqlite://data/lanes.db cargo test --features ssr invite_tests

#[cfg(feature = "ssr")]
mod invite_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::invite_api::{generate_invite_token, create_invite};
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

    /// Insert a board + owner member row for test setup.
    async fn insert_board_with_owner(pool: &sqlx::SqlitePool, name: &str, owner_id: &str) -> String {
        use uuid::Uuid;
        let board_id = Uuid::now_v7().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            r#"INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
               VALUES (?, ?, ?, '#6366f1', 0, 0, ?, ?)"#,
            board_id, name, name, now, now
        )
        .execute(pool)
        .await
        .expect("insert board");
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
            board_id, owner_id
        )
        .execute(pool)
        .await
        .expect("insert board_member");
        board_id
    }

    // -------------------------------------------------------------------------
    // generate_invite_token tests
    // -------------------------------------------------------------------------

    /// Test: generate_invite_token returns a 32-char base62 string.
    #[tokio::test]
    async fn test_generate_invite_token_length_and_charset() {
        let token = generate_invite_token();
        assert_eq!(token.len(), 32, "token must be 32 chars");
        // base62 charset: 0-9, a-z, A-Z
        assert!(
            token.chars().all(|c| c.is_alphanumeric()),
            "token must only contain alphanumeric (base62) chars"
        );
    }

    /// Test: two successive calls return different tokens (CSPRNG, not sequential).
    #[tokio::test]
    async fn test_generate_invite_token_is_unique() {
        let t1 = generate_invite_token();
        let t2 = generate_invite_token();
        assert_ne!(t1, t2, "successive tokens must differ (CSPRNG)");
    }

    // -------------------------------------------------------------------------
    // create_invite tests
    // -------------------------------------------------------------------------

    /// Test: create_invite inserts one invites row with accepted=0, lowercased email,
    /// 7-day expiry, and a unique token; returns the token.
    #[tokio::test]
    async fn test_create_invite_inserts_correct_row() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "TestBoard", &user_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let token = create_invite(&write_pool, &board_id, &user_id, "Invitee@Test.com", now)
            .await
            .expect("create_invite should succeed");

        // Row must exist
        let row = sqlx::query!(
            "SELECT email, token, accepted, expires_at FROM invites WHERE board_id = ? AND inviter_id = ?",
            board_id,
            user_id
        )
        .fetch_one(&write_pool)
        .await
        .expect("invite row must exist");

        // Email lowercased
        assert_eq!(row.email, "invitee@test.com", "email should be lowercased");
        // Token matches returned value
        assert_eq!(row.token, token, "stored token should match returned token");
        // accepted = 0
        assert_eq!(row.accepted, 0, "new invite must have accepted=0");
        // 7-day expiry: expires_at ≈ now + 7*24*3600*1000
        let expected_expiry = now + 7 * 24 * 3600 * 1000;
        let diff = (row.expires_at - expected_expiry).abs();
        assert!(diff < 5000, "expires_at should be approximately 7 days from now");
    }

    /// Test: re-inviting the same email creates a fresh row with a NEW token (D-14).
    #[tokio::test]
    async fn test_create_invite_reinvite_creates_fresh_token() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let user_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "TestBoard", &user_id).await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let token1 = create_invite(&write_pool, &board_id, &user_id, "friend@test.com", now)
            .await
            .expect("first invite");

        let token2 = create_invite(&write_pool, &board_id, &user_id, "friend@test.com", now + 1000)
            .await
            .expect("second invite");

        assert_ne!(token1, token2, "re-invite must produce a new token");

        // Two rows should exist (no upsert — each invite is a fresh row)
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invites WHERE board_id = ? AND email = 'friend@test.com'"
        )
        .bind(&board_id)
        .fetch_one(&write_pool)
        .await
        .expect("count");
        assert_eq!(count, 2, "re-invite should create a second row");
    }
}
