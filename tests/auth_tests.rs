//! Auth seam tests — EmailPasswordBackend authenticate behaviors + require_board_member helper.
//! Run: DATABASE_URL=sqlite:///tmp/test.db cargo test --features ssr auth_tests

#[cfg(feature = "ssr")]
mod auth_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::auth::backend::EmailPasswordBackend;
    use lanes::auth::models::{AuthUser, LoginCredentials};
    use axum_login::AuthnBackend;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    /// Create a temp DB with migrations applied; return (file guard, write_pool, read_pool).
    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool, read_pool)
    }

    /// Insert a user with a real Argon2id hash for the given password.
    async fn insert_password_user(
        pool: &sqlx::SqlitePool,
        email: &str,
        password: &str,
        display_name: &str,
    ) -> String {
        let id = Uuid::now_v7().to_string();
        let password_owned = password.to_string();
        let hash = tokio::task::spawn_blocking(move || {
            password_auth::generate_hash(password_owned)
        })
        .await
        .expect("spawn_blocking hash");

        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        sqlx::query!(
            "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
             VALUES (?, ?, ?, ?, '#7c5cff', 'password', ?)",
            id,
            email,
            hash,
            display_name,
            now
        )
        .execute(pool)
        .await
        .expect("insert user");

        id
    }

    /// Test: authenticate() returns Some(AuthUser) for valid credentials.
    #[tokio::test]
    async fn test_authenticate_valid_credentials() {
        let (_file, write_pool, read_pool) = test_db().await;
        insert_password_user(&write_pool, "alice@example.com", "correct-horse", "Alice").await;

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
        let creds = LoginCredentials {
            email: "alice@example.com".to_string(),
            password: "correct-horse".to_string(),
        };
        let result = backend.authenticate(creds).await.expect("authenticate should not error");
        assert!(result.is_some(), "valid credentials should return Some(AuthUser)");
        let user = result.unwrap();
        assert_eq!(user.email, "alice@example.com");
    }

    /// Test: authenticate() returns None for wrong password.
    #[tokio::test]
    async fn test_authenticate_wrong_password_returns_none() {
        let (_file, write_pool, read_pool) = test_db().await;
        insert_password_user(&write_pool, "bob@example.com", "correct-horse", "Bob").await;

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
        let creds = LoginCredentials {
            email: "bob@example.com".to_string(),
            password: "wrong-password".to_string(),
        };
        let result = backend.authenticate(creds).await.expect("authenticate should not error");
        assert!(result.is_none(), "wrong password should return None");
    }

    /// Test: authenticate() returns None for an unknown email.
    #[tokio::test]
    async fn test_authenticate_unknown_email_returns_none() {
        let (_file, write_pool, read_pool) = test_db().await;

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
        let creds = LoginCredentials {
            email: "nobody@example.com".to_string(),
            password: "any-password".to_string(),
        };
        let result = backend.authenticate(creds).await.expect("authenticate should not error");
        assert!(result.is_none(), "unknown email should return None");
    }

    /// Test: require_board_member query returns (user, role) for a member.
    /// Tests the DB query directly by inserting a board_members row.
    #[tokio::test]
    async fn test_require_board_member_query_returns_role_for_member() {
        let (_file, write_pool, read_pool) = test_db().await;
        let user_id = insert_password_user(&write_pool, "carol@example.com", "passw0rd!", "Carol").await;

        // Insert a board
        let board_id = Uuid::now_v7().to_string();
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            "INSERT INTO boards (id, name, key_prefix, color, next_card_num, starred, archived, created_at, updated_at) \
             VALUES (?, 'Test Board', 'TEST', '#6366f1', 1, 0, 0, ?, ?)",
            board_id, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert board");

        // Insert membership
        sqlx::query!(
            "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
            board_id, user_id
        )
        .execute(&write_pool)
        .await
        .expect("insert board_member");

        // Run the query that require_board_member executes
        let role: Option<String> = sqlx::query_scalar!(
            "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
            board_id,
            user_id
        )
        .fetch_optional(&read_pool)
        .await
        .expect("query_scalar");

        assert_eq!(role, Some("owner".to_string()), "member should have role 'owner'");
    }

    /// Test: require_board_member query returns None for a non-member.
    #[tokio::test]
    async fn test_require_board_member_query_returns_none_for_non_member() {
        let (_file, write_pool, read_pool) = test_db().await;
        let user_id = insert_password_user(&write_pool, "dave@example.com", "passw0rd!", "Dave").await;

        let board_id = Uuid::now_v7().to_string();
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        sqlx::query!(
            "INSERT INTO boards (id, name, key_prefix, color, next_card_num, starred, archived, created_at, updated_at) \
             VALUES (?, 'Board', 'BOARD', '#6366f1', 1, 0, 0, ?, ?)",
            board_id, now, now
        )
        .execute(&write_pool)
        .await
        .expect("insert board");

        // No board_members row — Dave is not a member

        let role: Option<String> = sqlx::query_scalar!(
            "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
            board_id,
            user_id
        )
        .fetch_optional(&read_pool)
        .await
        .expect("query_scalar");

        assert!(role.is_none(), "non-member should return None");
    }
}

/// Task 1 (Plan 02) tests: seed Mira with real password + reset_password behaviors.
#[cfg(feature = "ssr")]
mod seed_auth_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::auth::backend::EmailPasswordBackend;
    use lanes::auth::models::LoginCredentials;
    use lanes::seed::run_seed;
    use axum_login::AuthnBackend;
    use tempfile::NamedTempFile;

    async fn test_db() -> (NamedTempFile, sqlx::SqlitePool, sqlx::SqlitePool) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        let (write_pool, read_pool) = init_pools(&url).await.expect("init pools");
        run_migrations(&write_pool).await.expect("migrations");
        (file, write_pool, read_pool)
    }

    /// Test: after seeding, mira@example.com authenticates with password "lanes-demo" (D-10).
    #[tokio::test]
    async fn test_mira_authenticates_after_seed() {
        let (_file, write_pool, read_pool) = test_db().await;

        run_seed(&write_pool).await.expect("seed should succeed");

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
        let creds = LoginCredentials {
            email: "mira@example.com".to_string(),
            password: "lanes-demo".to_string(),
        };
        let result = backend.authenticate(creds).await.expect("authenticate should not error");
        assert!(result.is_some(), "mira@example.com should authenticate with lanes-demo");
        let user = result.unwrap();
        assert_eq!(user.email, "mira@example.com");
    }

    /// Test: wrong password fails for Mira post-seed (D-10).
    #[tokio::test]
    async fn test_mira_wrong_password_fails_after_seed() {
        let (_file, write_pool, read_pool) = test_db().await;

        run_seed(&write_pool).await.expect("seed should succeed");

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
        let creds = LoginCredentials {
            email: "mira@example.com".to_string(),
            password: "wrong-password".to_string(),
        };
        let result = backend.authenticate(creds).await.expect("authenticate should not error");
        assert!(result.is_none(), "wrong password should return None");
    }

    /// Test: reset_password rotates hash; old password no longer works, new one does (D-20).
    #[tokio::test]
    async fn test_reset_password_rotates_hash() {
        let (_file, write_pool, read_pool) = test_db().await;

        run_seed(&write_pool).await.expect("seed should succeed");

        // Rotate the password
        lanes::seed::reset_password(&write_pool, "mira@example.com", "new-password-123")
            .await
            .expect("reset_password should succeed");

        let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());

        // Old password should fail
        let old_creds = LoginCredentials {
            email: "mira@example.com".to_string(),
            password: "lanes-demo".to_string(),
        };
        let old_result = backend.authenticate(old_creds).await.expect("no error");
        assert!(old_result.is_none(), "old password should no longer work after reset");

        // New password should succeed
        let new_creds = LoginCredentials {
            email: "mira@example.com".to_string(),
            password: "new-password-123".to_string(),
        };
        let new_result = backend.authenticate(new_creds).await.expect("no error");
        assert!(new_result.is_some(), "new password should authenticate after reset");
    }

    /// Test: reset_password rejects passwords shorter than 8 chars (D-20).
    #[tokio::test]
    async fn test_reset_password_rejects_short_password() {
        let (_file, write_pool, _read_pool) = test_db().await;

        run_seed(&write_pool).await.expect("seed should succeed");

        let result = lanes::seed::reset_password(&write_pool, "mira@example.com", "short7!")
            .await;
        assert!(result.is_err(), "short password should be rejected");
        let err = result.unwrap_err();
        assert!(err.contains("8"), "error should mention 8 characters: {}", err);
    }

    /// Test: reset_password returns error when email not found (D-20).
    #[tokio::test]
    async fn test_reset_password_unknown_email_returns_error() {
        let (_file, write_pool, _read_pool) = test_db().await;

        run_seed(&write_pool).await.expect("seed should succeed");

        let result = lanes::seed::reset_password(
            &write_pool,
            "nobody@example.com",
            "validpass123",
        )
        .await;
        assert!(result.is_err(), "unknown email should return error");
        let err = result.unwrap_err();
        assert!(err.contains("not found") || err.contains("No user"),
            "error should indicate user not found: {}", err);
    }
}

/// CR-01 regression tests: CurrentUser DTO must not expose password_hash over the wire (AUTH-04).
/// Run: DATABASE_URL=sqlite://data/lanes.db cargo test --features ssr auth_tests -- current_user_dto
#[cfg(feature = "ssr")]
mod current_user_dto_tests {
    use lanes::auth::models::CurrentUser;

    /// Regression (CR-01): serialized CurrentUser JSON must NOT contain a `password_hash` key.
    /// If someone accidentally adds password_hash to CurrentUser, this test will catch it.
    #[test]
    fn test_get_current_user_dto_omits_password_hash() {
        let user = CurrentUser {
            id: "user-id-123".to_string(),
            email: "alice@example.com".to_string(),
            display_name: "Alice".to_string(),
            avatar_color: "#7c5cff".to_string(),
        };

        let json = serde_json::to_string(&user).expect("CurrentUser must serialize to JSON");

        assert!(
            !json.contains("password_hash"),
            "serialized CurrentUser must NOT contain 'password_hash'; got: {json}"
        );
    }

    /// Round-trip: CurrentUser serializes and deserializes back to an equal value.
    /// This proves it is a valid wire type usable by the WASM-side Resource.
    #[test]
    fn test_current_user_dto_round_trip() {
        let original = CurrentUser {
            id: "user-id-456".to_string(),
            email: "bob@example.com".to_string(),
            display_name: "Bob Builder".to_string(),
            avatar_color: "#2dd4bf".to_string(),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: CurrentUser = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.id, original.id);
        assert_eq!(deserialized.email, original.email);
        assert_eq!(deserialized.display_name, original.display_name);
        assert_eq!(deserialized.avatar_color, original.avatar_color);
    }
}

/// Task 3 tests: create_user() inner function behaviors (AUTH-01, D-17, D-18).
#[cfg(feature = "ssr")]
mod signup_tests {
    use lanes::server::db::{init_pools, run_migrations};
    use lanes::api::auth_api::create_user;
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

    /// Test: create_user inserts exactly one users row with correct fields (AUTH-01).
    #[tokio::test]
    async fn test_create_user_inserts_one_row() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let result = create_user(
            &write_pool,
            "Alice Smith".to_string(),
            "Alice@Example.COM".to_string(), // should be lowercased+trimmed
            "securepass".to_string(),
        )
        .await;

        assert!(result.is_ok(), "create_user should succeed: {:?}", result);
        let user_id = result.unwrap();

        // Verify exactly one row
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "should have exactly one user row");

        // Verify auth_provider = 'password' and password_hash is non-null
        let row = sqlx::query!(
            "SELECT id, email, password_hash, display_name, avatar_color, auth_provider FROM users WHERE id = ?",
            user_id
        )
        .fetch_one(&write_pool)
        .await
        .expect("fetch user");

        assert_eq!(row.id, user_id);
        assert_eq!(row.email, "alice@example.com", "email should be lowercased");
        assert!(row.password_hash.is_some(), "password_hash should be set");
        assert_eq!(row.auth_provider, "password");
        // avatar_color should be one of the palette colors (non-empty string)
        assert!(!row.avatar_color.is_empty(), "avatar_color should be set");
    }

    /// Test: create_user returns email_taken on duplicate email; no second row inserted (D-18).
    #[tokio::test]
    async fn test_create_user_duplicate_email_returns_email_taken() {
        let (_file, write_pool, _read_pool) = test_db().await;

        // First insert succeeds
        create_user(&write_pool, "Bob".to_string(), "bob@example.com".to_string(), "passw0rd1".to_string())
            .await
            .expect("first create_user should succeed");

        // Second insert with same email should fail
        let result = create_user(
            &write_pool,
            "Bobby".to_string(),
            "bob@example.com".to_string(),
            "different_pass".to_string(),
        )
        .await;

        assert!(result.is_err(), "duplicate email should return Err");
        let err = result.unwrap_err();
        // Should be EmailTaken variant — check debug representation
        assert!(
            format!("{:?}", err).contains("EmailTaken"),
            "error should be EmailTaken, got: {:?}", err
        );

        // Exactly one user row
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "no second row should be inserted");
    }

    /// Test: create_user returns PasswordTooShort for a 7-char password; no row inserted (D-17).
    #[tokio::test]
    async fn test_create_user_short_password_returns_error() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let result = create_user(
            &write_pool,
            "Charlie".to_string(),
            "charlie@example.com".to_string(),
            "short7!".to_string(), // 7 chars — one below the 8-char minimum
        )
        .await;

        assert!(result.is_err(), "short password should return Err");
        let err = result.unwrap_err();
        assert!(
            format!("{:?}", err).contains("PasswordTooShort"),
            "error should be PasswordTooShort, got: {:?}", err
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted on validation error");
    }

    /// Test: create_user returns NameRequired for empty/whitespace display name; no row inserted (D-17).
    #[tokio::test]
    async fn test_create_user_empty_name_returns_error() {
        let (_file, write_pool, _read_pool) = test_db().await;

        let result = create_user(
            &write_pool,
            "   ".to_string(), // whitespace-only display name
            "dana@example.com".to_string(),
            "validpassword".to_string(),
        )
        .await;

        assert!(result.is_err(), "empty name should return Err");
        let err = result.unwrap_err();
        assert!(
            format!("{:?}", err).contains("NameRequired"),
            "error should be NameRequired, got: {:?}", err
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&write_pool)
            .await
            .expect("count");
        assert_eq!(count, 0, "no row should be inserted on validation error");
    }
}
