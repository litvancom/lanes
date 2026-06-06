use leptos::prelude::*;
use crate::auth::models::AuthUser;

/// Internal: signup validation + user creation. Returns new user id.
/// Separated for testability independent of Leptos context machinery (workspace_api.rs pattern).
#[cfg(feature = "ssr")]
pub async fn create_user(
    pool: &sqlx::SqlitePool,
    display_name: String,
    email: String,
    password: String,
) -> Result<String, SignupError> {
    use uuid::Uuid;
    use tokio::task;
    use crate::auth::helpers::derive_avatar_color;

    // Validate inputs (D-17, D-18, ASVS V5)
    let email = email.trim().to_lowercase();
    let display_name = display_name.trim().to_string();

    if password.len() < 8 {
        return Err(SignupError::PasswordTooShort);
    }
    if display_name.is_empty() {
        return Err(SignupError::NameRequired);
    }

    // Hash password off the async executor — Argon2id is CPU-intensive (Pitfall 9, T-02-02)
    let password_owned = password.clone();
    let hash = task::spawn_blocking(move || password_auth::generate_hash(password_owned))
        .await
        .map_err(|_| SignupError::Internal)?;

    let id = Uuid::now_v7().to_string();
    let avatar_color = derive_avatar_color(&email); // D-21
    let now = crate::server::now_millis().map_err(|_| SignupError::Internal)?;

    // Parameterized INSERT — no format! into SQL (T-02-06, ASVS V5)
    let result = sqlx::query!(
        "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
         VALUES (?, ?, ?, ?, ?, 'password', ?)",
        id,
        email,
        hash,
        display_name,
        avatar_color,
        now,
    )
    .execute(pool)
    .await;

    match result {
        Ok(_) => Ok(id),
        Err(sqlx::Error::Database(e)) if e.message().contains("UNIQUE") => {
            Err(SignupError::EmailTaken) // D-18: field-specific error for signup
        }
        Err(e) => {
            tracing::error!("signup DB error: {e}");
            Err(SignupError::Internal)
        }
    }
}

/// Signup validation error codes — mapped to ServerFnError codes for the UI (D-18).
#[cfg(feature = "ssr")]
#[derive(Debug)]
pub enum SignupError {
    PasswordTooShort,
    NameRequired,
    EmailTaken,
    Internal,
}

/// Signup server function: create account + instant sign-in (D-19, AUTH-01).
/// Plain server function, NOT routed through AuthnBackend (D-03).
#[server]
pub async fn signup(
    display_name: String,
    email: String,
    password: String,
) -> Result<(), ServerFnError> {
    use crate::server::state::AppState;
    use crate::auth::{AuthSession, models::LoginCredentials};
    use leptos_axum::extract;

    let state = expect_context::<AppState>();
    let pool = &state.write_pool.0;

    let email_lower = email.trim().to_lowercase();

    match create_user(pool, display_name, email_lower.clone(), password.clone()).await {
        Ok(_) => {}
        Err(SignupError::EmailTaken) => {
            return Err(ServerFnError::new("email_taken"));
        }
        Err(SignupError::PasswordTooShort) => {
            return Err(ServerFnError::new("password_too_short"));
        }
        Err(SignupError::NameRequired) => {
            return Err(ServerFnError::new("name_required"));
        }
        Err(SignupError::Internal) => {
            return Err(ServerFnError::new("Internal error"));
        }
    }

    // Instant sign-in after signup (D-19)
    let mut auth_session: AuthSession = extract()
        .await
        .map_err(|_| ServerFnError::new("Session error"))?;

    let creds = LoginCredentials { email: email_lower, password };
    let user = auth_session
        .authenticate(creds)
        .await
        .map_err(|_| ServerFnError::new("Session error"))?
        .ok_or_else(|| ServerFnError::new("Session error"))?;

    auth_session
        .login(&user)
        .await
        .map_err(|_| ServerFnError::new("Session error"))?;

    leptos_axum::redirect("/");
    Ok(())
}

/// Login server function: authenticate with email + password; redirect to workspace on success.
/// Login errors are always the SAME generic message to prevent account enumeration (D-18, T-02-08).
/// keep_signed_in is accepted cosmetically so the checkbox POST field doesn't cause a 400 (D-07).
#[server]
pub async fn login(
    email: String,
    password: String,
    keep_signed_in: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::auth::{AuthSession, models::LoginCredentials};
    use leptos_axum::extract;

    // keep_signed_in is accepted but unused (D-07 — cosmetic checkbox, sessions always 30 days)
    let _ = keep_signed_in;

    let mut auth_session: AuthSession = extract()
        .await
        .map_err(|_| ServerFnError::new("Invalid email or password."))?;

    let creds = LoginCredentials {
        email: email.trim().to_lowercase(),
        password,
    };

    // Both "auth backend error" and "no matching user" map to the SAME generic message (D-18, T-02-08).
    let user = auth_session
        .authenticate(creds)
        .await
        .map_err(|_| ServerFnError::new("Invalid email or password."))?
        .ok_or_else(|| ServerFnError::new("Invalid email or password."))?;

    auth_session
        .login(&user)
        .await
        .map_err(|_| ServerFnError::new("Invalid email or password."))?;

    leptos_axum::redirect("/");
    Ok(())
}

/// Logout server function: clear session and redirect to /login (AUTH-03).
#[server]
pub async fn logout() -> Result<(), ServerFnError> {
    use crate::auth::AuthSession;
    use leptos_axum::extract;

    let mut auth_session: AuthSession = extract()
        .await
        .map_err(|_| ServerFnError::new("Session error"))?;

    auth_session
        .logout()
        .await
        .map_err(|_| ServerFnError::new("Session error"))?;

    leptos_axum::redirect("/login");
    Ok(())
}

/// Return the currently authenticated user, or None if not logged in.
/// Used by workspace and route guards to decide whether to redirect (RESEARCH Pattern 5).
#[server]
pub async fn get_current_user() -> Result<Option<AuthUser>, ServerFnError> {
    use crate::auth::AuthSession;
    use leptos_axum::extract;

    let auth_session: AuthSession = extract()
        .await
        .map_err(|_| ServerFnError::new("Session error"))?;

    Ok(auth_session.user)
}
