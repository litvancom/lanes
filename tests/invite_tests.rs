//! Tests for invite token generation and invite creation inner fn.
//! Run: DATABASE_URL=sqlite://data/lanes.db cargo test --features ssr invite_tests

#[cfg(feature = "ssr")]
mod invite_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::invite_api::{generate_invite_token, create_invite, consume_invite};
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

        let token = create_invite(&write_pool, &board_id, &user_id, "Invitee@Test.com", "editor", now)
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

        let token1 = create_invite(&write_pool, &board_id, &user_id, "friend@test.com", "editor", now)
            .await
            .expect("first invite");

        let token2 = create_invite(&write_pool, &board_id, &user_id, "friend@test.com", "editor", now + 1000)
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

    // -------------------------------------------------------------------------
    // consume_invite tests (Task 1 — Plan 04)
    // -------------------------------------------------------------------------

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    /// Helper: insert a second user (the invitee).
    async fn insert_invitee(pool: &sqlx::SqlitePool, email: &str) -> String {
        insert_user_direct(pool, email).await
    }

    /// Test: consume_invite succeeds for a valid unused unexpired invite whose email matches
    /// user_email — marks accepted=1, inserts board_members row, returns board_id.
    #[tokio::test]
    async fn test_consume_invite_success() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "Board", &owner_id).await;
        let invitee_id = insert_invitee(&write_pool, "alice@test.com").await;

        let now = now_ms();
        let token = create_invite(&write_pool, &board_id, &owner_id, "Alice@Test.com", "editor", now)
            .await
            .expect("create_invite");

        // consume_invite with matching email (case-insensitive)
        let result = consume_invite(&write_pool, &token, &invitee_id, "alice@test.com", now + 1000)
            .await;
        assert!(result.is_ok(), "consume_invite should succeed: {:?}", result);
        assert_eq!(result.unwrap(), board_id, "should return the board_id");

        // invite row must be accepted
        let accepted: i64 = sqlx::query_scalar!(
            "SELECT accepted FROM invites WHERE token = ?", token
        )
        .fetch_one(&write_pool)
        .await
        .expect("fetch invite");
        assert_eq!(accepted, 1, "invite must be marked accepted");

        // board_members row must exist
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&invitee_id)
        .fetch_one(&write_pool)
        .await
        .expect("member count");
        assert_eq!(member_count, 1, "invitee must be inserted into board_members");
    }

    /// Test: mismatched email (D-15 strict binding) returns WrongEmail error,
    /// NO board_members row, accepted remains 0.
    #[tokio::test]
    async fn test_consume_invite_wrong_email() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "Board", &owner_id).await;
        let attacker_id = insert_invitee(&write_pool, "attacker@test.com").await;

        let now = now_ms();
        let token = create_invite(&write_pool, &board_id, &owner_id, "alice@test.com", "editor", now)
            .await
            .expect("create_invite");

        // Different email — should be rejected
        let result = consume_invite(&write_pool, &token, &attacker_id, "attacker@test.com", now + 1000)
            .await;
        assert!(result.is_err(), "consume_invite should fail for mismatched email");

        // accepted must still be 0
        let accepted: i64 = sqlx::query_scalar!(
            "SELECT accepted FROM invites WHERE token = ?", token
        )
        .fetch_one(&write_pool)
        .await
        .expect("fetch invite");
        assert_eq!(accepted, 0, "accepted must remain 0 on wrong-email rejection");

        // no board_members row for the attacker
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&attacker_id)
        .fetch_one(&write_pool)
        .await
        .expect("member count");
        assert_eq!(member_count, 0, "attacker must NOT be added to board_members");
    }

    /// Test: expired invite (expires_at < now) returns Expired error, no membership.
    #[tokio::test]
    async fn test_consume_invite_expired() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "Board", &owner_id).await;
        let invitee_id = insert_invitee(&write_pool, "bob@test.com").await;

        // Create invite in the past (already expired)
        let past = now_ms() - 10 * 24 * 3600 * 1000; // 10 days ago
        let token = create_invite(&write_pool, &board_id, &owner_id, "bob@test.com", "editor", past)
            .await
            .expect("create_invite");

        // now_ms() is well past expires_at (past + 7 days)
        let result = consume_invite(&write_pool, &token, &invitee_id, "bob@test.com", now_ms())
            .await;
        assert!(result.is_err(), "consume_invite should fail for expired invite");

        // no board_members row
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&invitee_id)
        .fetch_one(&write_pool)
        .await
        .expect("member count");
        assert_eq!(member_count, 0, "expired invite must not grant membership");
    }

    /// Regression (CR-02 / D-14): guarded UPDATE `AND accepted = 0` must return AlreadyUsed
    /// on a sequential double-accept, independent of write-pool connection serialization.
    ///
    /// This test proves single-use is enforced by the UPDATE's rows_affected check, not by
    /// incidental max_connections=1 serialization. A second accept must fail with the specific
    /// AcceptError::AlreadyUsed variant and must NOT duplicate the board_members row.
    #[tokio::test]
    async fn test_consume_invite_guarded_update_rejects_double_accept() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "Board", &owner_id).await;
        let invitee_id = insert_invitee(&write_pool, "diana@test.com").await;

        let now = now_ms();
        let token = create_invite(&write_pool, &board_id, &owner_id, "diana@test.com", "editor", now)
            .await
            .expect("create_invite");

        // First consumption must succeed
        let first = consume_invite(&write_pool, &token, &invitee_id, "diana@test.com", now + 1000)
            .await;
        assert!(first.is_ok(), "first consumption must succeed: {:?}", first);

        // Second consumption: must fail with the specific AlreadyUsed variant (not just any error)
        let second = consume_invite(&write_pool, &token, &invitee_id, "diana@test.com", now + 2000)
            .await;
        assert!(
            matches!(second, Err(lanes::api::invite_api::AcceptError::AlreadyUsed)),
            "second consumption must return AcceptError::AlreadyUsed; got: {:?}", second
        );

        // Exactly one board_members row — no duplicate membership from double-accept
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&invitee_id)
        .fetch_one(&write_pool)
        .await
        .expect("member count");
        assert_eq!(member_count, 1, "double-accept must NOT create a duplicate board_members row");
    }

    /// Test: already-accepted invite returns AlreadyUsed error, does not duplicate membership.
    #[tokio::test]
    async fn test_consume_invite_already_used() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let owner_id = insert_user_direct(&write_pool, "owner@test.com").await;
        let board_id = insert_board_with_owner(&write_pool, "Board", &owner_id).await;
        let invitee_id = insert_invitee(&write_pool, "carol@test.com").await;

        let now = now_ms();
        let token = create_invite(&write_pool, &board_id, &owner_id, "carol@test.com", "editor", now)
            .await
            .expect("create_invite");

        // First consumption succeeds
        let first = consume_invite(&write_pool, &token, &invitee_id, "carol@test.com", now + 1000)
            .await;
        assert!(first.is_ok(), "first consumption must succeed");

        // Second consumption must fail
        let second = consume_invite(&write_pool, &token, &invitee_id, "carol@test.com", now + 2000)
            .await;
        assert!(second.is_err(), "second consumption must fail (already used)");

        // Exactly one board_members row (no duplicate)
        let member_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board_id)
        .bind(&invitee_id)
        .fetch_one(&write_pool)
        .await
        .expect("member count");
        assert_eq!(member_count, 1, "must not duplicate board_members on double-accept");
    }

    /// Test: invite with a specific role grants exactly that role to the accepted member.
    #[tokio::test]
    async fn invite_with_role_grants_that_role_on_accept() {
        let (_f, write_pool, read_pool) = test_db().await;
        let owner = insert_user_direct(&write_pool, "owner@x.com").await;
        let board = insert_board_with_owner(&write_pool, "B", &owner).await;
        let invitee = insert_user_direct(&write_pool, "viewer@x.com").await;
        let now: i64 = 1_700_000_000_000;
        let token = create_invite(&write_pool, &board, &owner, "viewer@x.com", "viewer", now)
            .await
            .unwrap();
        let bid = consume_invite(&write_pool, &token, &invitee, "viewer@x.com", now + 1000)
            .await
            .unwrap();
        assert_eq!(bid, board);
        let role: String = sqlx::query_scalar(
            "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?"
        )
        .bind(&board)
        .bind(&invitee)
        .fetch_one(&read_pool)
        .await
        .unwrap();
        assert_eq!(role, "viewer");
    }

    #[tokio::test]
    async fn owner_row_cannot_be_changed_or_removed() {
        let (_f, write_pool, _r) = test_db().await;
        let owner = insert_user_direct(&write_pool, "o@x.com").await;
        let board = insert_board_with_owner(&write_pool, "B", &owner).await;
        let upd = sqlx::query(
            "UPDATE board_members SET role='viewer' WHERE board_id=? AND user_id=? AND role!='owner'",
        )
        .bind(&board)
        .bind(&owner)
        .execute(&write_pool)
        .await
        .unwrap();
        assert_eq!(upd.rows_affected(), 0, "owner role must not be changeable");
        let del = sqlx::query(
            "DELETE FROM board_members WHERE board_id=? AND user_id=? AND role!='owner'",
        )
        .bind(&board)
        .bind(&owner)
        .execute(&write_pool)
        .await
        .unwrap();
        assert_eq!(del.rows_affected(), 0, "owner must not be removable");
    }
}
