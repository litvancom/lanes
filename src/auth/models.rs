use serde::{Deserialize, Serialize};

/// Client-safe DTO returned by `get_current_user()` over the wire (AUTH-04 / CR-01).
///
/// Contains only the four non-secret fields the UI needs: id, email, display_name, avatar_color.
/// No `password_hash`, `auth_provider`, or `created_at` — credentials never cross the
/// server→client trust boundary.
///
/// No `#[cfg(feature = "ssr")]` gate: this struct compiles on both the server (SSR) and the
/// WASM client target, because the client-side Leptos Resource deserializes it from JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrentUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub avatar_color: String,
}

/// The authenticated user type, implementing axum_login::AuthUser.
/// Fields match the `users` table after migration 002 (auth_provider, external_id added).
///
/// Server/session-only: `AuthUser` is the full DB record used by the auth backend and
/// axum-login session machinery. It is NOT returned over the wire by `get_current_user()`.
/// The client-facing DTO is `CurrentUser` — it carries only the four non-secret fields.
///
/// Serialize/Deserialize are required for axum-login session serialization; the session
/// cookie transport does NOT cross the HTTP response body trust boundary in the same way.
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
