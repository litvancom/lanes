use axum_macros::FromRef;
use leptos::config::LeptosOptions;
use sqlx::SqlitePool;
use std::sync::Arc;
use crate::mailer::Mailer;
use crate::server::board_rooms::BoardRoomRegistry;
use crate::server::user_notif_registry::UserNotifRegistry;
use crate::server::presence_registry::PresenceRegistry;
use crate::server::rest_api::auth::RateLimiter;

/// Newtype wrappers to allow both pools to coexist in AppState with FromRef.
/// Without these, two fields of the same `SqlitePool` type would produce conflicting FromRef impls.
#[derive(Clone)]
pub struct WritePool(pub SqlitePool);

#[derive(Clone)]
pub struct ReadPool(pub SqlitePool);

/// Shared application state held by Axum (D-05).
/// `FromRef` allows `LeptosOptions` and the pool newtypes to be extracted
/// individually via `State<...>` in handlers.
/// Note: EmailPasswordBackend is NOT stored here — axum-login manages it via AuthManagerLayer.
/// Note: `Arc<dyn ObjectStore>` does NOT derive FromRef — upload/download handlers extract
/// `State<AppState>` and read `state.storage` directly (same approach as `mailer`).
/// Note: Realtime registries (board_rooms, user_notifs, presence) are NOT FromRef-extractable —
/// handlers read them via `State<AppState>` like `mailer` and `storage` (avoids FromRef conflicts).
#[derive(Clone, FromRef)]
pub struct AppState {
    pub leptos_options: LeptosOptions,
    pub write_pool: WritePool,
    pub read_pool: ReadPool,
    pub mailer: Arc<dyn Mailer>,                          // pluggable mailer (D-13, COLLAB-02)
    pub storage: Arc<dyn object_store::ObjectStore>,      // pluggable attachment store (DETAIL-08)
    pub board_rooms: BoardRoomRegistry,                   // per-board broadcast (RT-01)
    pub user_notifs: UserNotifRegistry,                   // per-user notification delivery (RT-04)
    pub presence: PresenceRegistry,                       // ephemeral presence (RT-03)
    pub rate_limiter: RateLimiter,                        // per-token in-memory rate limiter (D-21)
}
