use axum_macros::FromRef;
use leptos::config::LeptosOptions;
use sqlx::SqlitePool;

/// Newtype wrappers to allow both pools to coexist in AppState with FromRef.
/// Without these, two fields of the same `SqlitePool` type would produce conflicting FromRef impls.
#[derive(Clone)]
pub struct WritePool(pub SqlitePool);

#[derive(Clone)]
pub struct ReadPool(pub SqlitePool);

/// Shared application state held by Axum (D-05).
/// `FromRef` allows `LeptosOptions` and the pool newtypes to be extracted
/// individually via `State<...>` in handlers.
#[derive(Clone, FromRef)]
pub struct AppState {
    pub leptos_options: LeptosOptions,
    pub write_pool: WritePool,
    pub read_pool: ReadPool,
}
