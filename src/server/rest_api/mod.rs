//! REST API router assembly and OpenAPI document.
//!
//! `api_router()` takes NO arguments and returns `axum::Router<AppState>` with UNBOUND state.
//! The caller (`main.rs`) supplies state via `.with_state(app_state)` on the merged router.
//! It is merged BEFORE `.layer(auth_layer)` so bearer auth is independent of sessions (Pitfall 2).

#[cfg(feature = "ssr")]
pub mod auth;
#[cfg(feature = "ssr")]
pub mod boards;
#[cfg(feature = "ssr")]
pub mod cards;
#[cfg(feature = "ssr")]
pub mod comments;
#[cfg(feature = "ssr")]
pub mod lists;
#[cfg(feature = "ssr")]
pub mod workspaces;

// api_router() and ApiDoc are implemented in Task 2a.
// Stub here so the module compiles in Task 1.
#[cfg(feature = "ssr")]
pub fn api_router() -> axum::Router<crate::server::state::AppState> {
    axum::Router::new()
}
