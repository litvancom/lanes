use axum_login::{AuthnBackend, UserId};
use sqlx::SqlitePool;
use std::sync::LazyLock;
use tokio::task;
use crate::auth::models::{AuthUser, LoginCredentials};

/// Fixed dummy Argon2id PHC hash used to equalize login timing when no user row exists (WR-01).
///
/// Without this, `authenticate()` only pays the (deliberately expensive) Argon2id cost when a
/// matching user is found, so login latency leaks account existence — defeating the generic
/// "Invalid email or password." message (D-18, T-02-08). We verify the supplied password against
/// this dummy hash whenever the email is unknown, burning equivalent CPU.
///
/// Computed once at first use from the same `password_auth` defaults (Argon2id, OWASP params) that
/// `hash_password` uses, so the verification cost matches real hashes rather than being hardcoded.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| password_auth::generate_hash("lanes-dummy-password"));

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
        // Always perform a hash verification so login timing is constant regardless of account
        // existence; otherwise an attacker can enumerate users by latency (WR-01, D-18, T-02-08).
        task::spawn_blocking(move || match user {
            Some(u) => {
                let ok = u
                    .password_hash
                    .as_deref()
                    .map(|h| password_auth::verify_password(&creds.password, h).is_ok())
                    .unwrap_or(false);
                Ok(if ok { Some(u) } else { None })
            }
            None => {
                // Burn equivalent CPU against a fixed dummy hash to equalize timing.
                let _ = password_auth::verify_password(&creds.password, &DUMMY_HASH);
                Ok(None)
            }
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
