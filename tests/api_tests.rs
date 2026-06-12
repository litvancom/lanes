//! Integration tests for the REST API: bearer extractor, boards IDOR, token management.
//! Run: cargo test --features ssr bearer_token
//!      cargo test --features ssr api_list_boards
//!      cargo test --features ssr token_

// ---------------------------------------------------------------------------
// Bearer token extractor tests (Task 1 Wave-0)
// ---------------------------------------------------------------------------

#[cfg(feature = "ssr")]
mod bearer_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::server::rest_api::auth::{resolve_api_user, RateLimiter};
    use tempfile::NamedTempFile;

    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, _read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool)
    }

    async fn insert_user(pool: &sqlx::SqlitePool, email: &str) -> String {
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

    /// Insert an api_tokens row given a raw token. Returns (token_id, token_hash).
    async fn insert_token(pool: &sqlx::SqlitePool, user_id: &str, raw_token: &str) -> (String, String) {
        use sha2::{Digest, Sha256};
        use uuid::Uuid;

        let digest_bytes = Sha256::digest(raw_token.as_bytes());
        let hash = digest_bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let token_id = Uuid::now_v7().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        sqlx::query!(
            "INSERT INTO api_tokens (id, user_id, name, token_hash, created_at) VALUES (?, ?, 'test-token', ?, ?)",
            token_id, user_id, hash, now
        )
        .execute(pool)
        .await
        .expect("insert token");

        (token_id, hash)
    }

    /// A valid Bearer token resolves to the correct user.
    #[tokio::test]
    async fn bearer_token_resolves_user() {
        let (_file, pool) = test_db().await;
        let user_id = insert_user(&pool, "alice@test.com").await;

        let raw = "my-test-token-abc123";
        insert_token(&pool, &user_id, raw).await;

        let limiter = RateLimiter::default();
        let header = format!("Bearer {}", raw);
        let user = resolve_api_user(&pool, &pool, &limiter, &header)
            .await
            .expect("should resolve user");

        assert_eq!(user.id, user_id, "resolved user id must match");
        assert_eq!(user.email, "alice@test.com");
    }

    /// Missing Authorization header yields 401.
    #[tokio::test]
    async fn bearer_token_missing_401() {
        let (_file, pool) = test_db().await;
        let limiter = RateLimiter::default();

        // No "Bearer " prefix — scheme is wrong
        let result = resolve_api_user(&pool, &pool, &limiter, "Basic abc").await;
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    }

    /// A token that is not in the DB yields 401.
    #[tokio::test]
    async fn bearer_token_unknown_token_401() {
        let (_file, pool) = test_db().await;
        let limiter = RateLimiter::default();

        let result = resolve_api_user(&pool, &pool, &limiter, "Bearer this-token-does-not-exist").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), axum::http::StatusCode::UNAUTHORIZED);
    }

    /// Exceeding the rate limit yields 429.
    #[tokio::test]
    async fn bearer_token_rate_limit_429() {
        let (_file, pool) = test_db().await;
        let user_id = insert_user(&pool, "rl@test.com").await;

        let raw = "rate-limit-token";
        insert_token(&pool, &user_id, raw).await;

        let limiter = RateLimiter::default();
        let header = format!("Bearer {}", raw);

        // Exhaust the 120-request budget
        for _ in 0..120 {
            resolve_api_user(&pool, &pool, &limiter, &header)
                .await
                .expect("should not fail within limit");
        }

        // 121st request should be rate-limited
        let result = resolve_api_user(&pool, &pool, &limiter, &header).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }
}

// ---------------------------------------------------------------------------
// Workspace API tests (pre-existing — kept for regression)
// ---------------------------------------------------------------------------

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

        let board = create_board(&write_pool, "Test Board".to_string(), "#7c5cff".to_string(), &creator_id)
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
        let result = create_board(&write_pool, "Orphan Board".to_string(), "#7c5cff".to_string(), "non-existent-user-id")
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

        let result = create_board(&write_pool, "".to_string(), "#7c5cff".to_string(), &creator_id).await;
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

        let result = create_board(&write_pool, "   ".to_string(), "#7c5cff".to_string(), &creator_id).await;
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
        let result = create_board(&write_pool, long_name, "#7c5cff".to_string(), &creator_id).await;
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

// ---------------------------------------------------------------------------
// REST API IDOR tests (Task 2b Wave-0)
// ---------------------------------------------------------------------------

#[cfg(feature = "ssr")]
mod idor_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::server::rest_api::boards::require_member;
    use lanes::api::workspace_api::derive_key_prefix;
    use tempfile::NamedTempFile;

    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, _read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool)
    }

    async fn insert_user(pool: &sqlx::SqlitePool, email: &str) -> String {
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

    async fn insert_board(pool: &sqlx::SqlitePool, name: &str, owner_id: &str) -> String {
        use uuid::Uuid;
        let id = Uuid::now_v7().to_string();
        let kp = derive_key_prefix(name);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            "INSERT INTO boards (id, name, key_prefix, color, starred, archived, created_at, updated_at)
             VALUES (?, ?, ?, '#7c5cff', 0, 0, ?, ?)",
            id, name, kp, now, now
        )
        .execute(pool)
        .await
        .expect("insert board");
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
            id, owner_id
        )
        .execute(pool)
        .await
        .expect("insert member");
        id
    }

    /// IDOR negative case: a token owner cannot access a board they are not a member of.
    ///
    /// `require_member` is the per-request membership gate used by every board-scoped
    /// REST handler. This test asserts it returns a 404 (not 403, not 401 — no existence leak)
    /// when the requesting user is not in board_members for the target board (D-16, T-07-14).
    #[tokio::test]
    async fn api_list_boards_idor_non_member_gets_404() {
        let (_file, pool) = test_db().await;

        // alice owns board_a; bob is NOT a member of board_a
        let alice = insert_user(&pool, "alice@test.com").await;
        let bob = insert_user(&pool, "bob@test.com").await;
        let board_a = insert_board(&pool, "Alice Board", &alice).await;

        // Bob tries to access Alice's board via the membership gate
        let result = require_member(&pool, &board_a, &bob).await;
        assert!(result.is_err(), "non-member should be rejected");

        // The rejection response must encode 404 (not 403/401 — no IDOR existence leak)
        let response = result.unwrap_err();
        // require_member produces a 404 Response; verify by converting to Parts
        use axum::response::IntoResponse;
        let (parts, _body) = response.into_response().into_parts();
        assert_eq!(
            parts.status,
            axum::http::StatusCode::NOT_FOUND,
            "non-member must receive 404 to prevent IDOR existence leak"
        );
    }

    /// Positive case: a legitimate member receives Ok(role).
    #[tokio::test]
    async fn api_list_boards_member_gets_role() {
        let (_file, pool) = test_db().await;

        let alice = insert_user(&pool, "alice@test.com").await;
        let board_a = insert_board(&pool, "Alice Board", &alice).await;

        let result = require_member(&pool, &board_a, &alice).await;
        assert!(result.is_ok(), "owner should be admitted");
        assert_eq!(result.unwrap(), "owner");
    }
}

// ---------------------------------------------------------------------------
// API token management tests (Task 3 — API-03)
// ---------------------------------------------------------------------------

#[cfg(feature = "ssr")]
mod token_tests {
    use lanes::api::token_api::create_api_token_inner;
    use lanes::server::db::{init_pools, run_migrations};
    use sha2::{Digest, Sha256};
    use tempfile::NamedTempFile;

    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, _read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool)
    }

    async fn insert_user(pool: &sqlx::SqlitePool, email: &str) -> String {
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

    /// D-17: only the SHA-256 hash is stored in `api_tokens.token_hash`;
    /// the raw token returned by `create_api_token_inner` is never in the DB.
    #[tokio::test]
    async fn token_hash_only_stored() {
        let (_file, pool) = test_db().await;
        let user_id = insert_user(&pool, "alice@test.com").await;

        let created = create_api_token_inner("my-ci-token".to_string(), &user_id, &pool)
            .await
            .expect("create should succeed");

        // The raw token is 64 hex chars (32 random bytes → hex)
        assert_eq!(created.raw_token.len(), 64, "raw token must be 64 hex chars");
        assert!(
            created.raw_token.chars().all(|c| c.is_ascii_hexdigit()),
            "raw token must be lowercase hex"
        );

        // Verify: DB stores the SHA-256 hash of the raw token — not the raw token itself
        let stored_hash: Option<String> = sqlx::query_scalar(
            "SELECT token_hash FROM api_tokens WHERE id = ?",
        )
        .bind(&created.id)
        .fetch_optional(&pool)
        .await
        .expect("query");

        let stored_hash = stored_hash.expect("token row must exist");

        // Compute expected hash independently
        let expected_hash_bytes = Sha256::digest(created.raw_token.as_bytes());
        let expected_hash: String = expected_hash_bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        assert_eq!(
            stored_hash, expected_hash,
            "stored hash must be SHA-256 of raw token"
        );
        assert_ne!(
            stored_hash, created.raw_token,
            "stored hash must NOT equal the raw token (D-17)"
        );
    }

    /// Revoking a token removes it from the DB; the bearer extractor subsequently rejects it.
    #[tokio::test]
    async fn token_revoke_invalidates() {
        use lanes::server::rest_api::auth::{resolve_api_user, RateLimiter};

        let (_file, pool) = test_db().await;
        let user_id = insert_user(&pool, "bob@test.com").await;

        let created = create_api_token_inner("revoke-test".to_string(), &user_id, &pool)
            .await
            .expect("create should succeed");

        // Token resolves before revoke
        let limiter = RateLimiter::default();
        let header = format!("Bearer {}", created.raw_token);
        let resolved = resolve_api_user(&pool, &pool, &limiter, &header)
            .await
            .expect("token should resolve before revoke");
        assert_eq!(resolved.id, user_id);

        // Revoke: DELETE WHERE id = ? AND user_id = ? (no IDOR — must match both)
        sqlx::query!(
            "DELETE FROM api_tokens WHERE id = ? AND user_id = ?",
            created.id,
            user_id
        )
        .execute(&pool)
        .await
        .expect("revoke delete");

        // Token no longer resolves after revoke
        let result = resolve_api_user(&pool, &pool, &limiter, &header).await;
        assert!(result.is_err(), "revoked token must not resolve");
        assert_eq!(
            result.unwrap_err(),
            axum::http::StatusCode::UNAUTHORIZED,
            "revoked token must yield 401"
        );
    }
}
