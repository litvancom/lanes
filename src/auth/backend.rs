use axum_login::{AuthnBackend, UserId};
use sqlx::SqlitePool;
use tokio::task;
use crate::auth::models::{AuthUser, LoginCredentials};

// AuthnBackend in axum-login 0.18 uses Rust's native async-in-trait (RPITIT); no #[async_trait] needed.

/// Error type for EmailPasswordBackend operations.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Task join error: {0}")]
    TaskJoin(#[from] task::JoinError),
}

/// Authentication backend implementing axum_login::AuthnBackend for email/password auth (AUTH-04, D-01).
/// EmailPasswordBackend is NOT stored in AppState — axum-login manages it via AuthManagerLayer.
#[derive(Debug, Clone)]
pub struct EmailPasswordBackend {
    pub write_pool: SqlitePool,
    pub read_pool: SqlitePool,
}

impl EmailPasswordBackend {
    pub fn new(write_pool: SqlitePool, read_pool: SqlitePool) -> Self {
        Self { write_pool, read_pool }
    }
}

impl AuthnBackend for EmailPasswordBackend {
    type User = AuthUser;
    type Credentials = LoginCredentials;
    type Error = BackendError;

    async fn authenticate(
        &self,
        creds: LoginCredentials,
    ) -> Result<Option<AuthUser>, BackendError> {
        let user: Option<AuthUser> = sqlx::query_as!(
            AuthUser,
            "SELECT id, email, password_hash, display_name, avatar_color, auth_provider, created_at \
             FROM users WHERE email = ? AND auth_provider = 'password'",
            creds.email
        )
        .fetch_optional(&self.read_pool)
        .await?;

        // Argon2id verification MUST run in spawn_blocking — it is CPU-intensive (Pitfall 9, T-02-02).
        task::spawn_blocking(move || {
            Ok(user.filter(|u| {
                u.password_hash
                    .as_ref()
                    .map(|h| password_auth::verify_password(&creds.password, h).is_ok())
                    .unwrap_or(false)
            }))
        })
        .await?
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<AuthUser>, BackendError> {
        let user = sqlx::query_as!(
            AuthUser,
            "SELECT id, email, password_hash, display_name, avatar_color, auth_provider, created_at \
             FROM users WHERE id = ?",
            user_id
        )
        .fetch_optional(&self.read_pool)
        .await?;
        Ok(user)
    }
}
