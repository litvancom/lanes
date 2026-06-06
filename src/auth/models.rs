use serde::{Deserialize, Serialize};

/// The authenticated user type, implementing axum_login::AuthUser.
/// Fields match the `users` table after migration 002 (auth_provider, external_id added).
/// Serialize/Deserialize are required because get_current_user() returns it over the wire.
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    pub password_hash: Option<String>,
    pub display_name: String,
    pub avatar_color: String,
    pub auth_provider: String,
    pub created_at: i64,
}

#[cfg(feature = "ssr")]
impl axum_login::AuthUser for AuthUser {
    type Id = String;

    fn id(&self) -> String {
        self.id.clone()
    }

    /// session_auth_hash: axum-login uses this to invalidate sessions when the password changes.
    /// MUST return real hash bytes for password users — returning b"" causes session fixation (Pitfall 1).
    /// OAuth users (no password) intentionally return b"" since they have no password to invalidate.
    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_deref().unwrap_or("").as_bytes()
    }
}

/// Credentials for email/password authentication.
#[derive(Clone, Debug)]
pub struct LoginCredentials {
    pub email: String,
    pub password: String,
}
