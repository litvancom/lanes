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
