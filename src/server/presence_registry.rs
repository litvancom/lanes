//! Ephemeral presence channel registry (RT-03).
//!
//! `PresenceRegistry` tracks who is viewing each board (D-10/D-12/D-13).
//! It is completely isolated from `BoardRoomRegistry` — presence events are
//! high-churn and must not cause `RecvError::Lagged` in DB-mutation receivers.
//!
//! Keys in `viewers` are `"{board_id}:{user_id}"` — allows the same user to view
//! multiple boards (not multi-tab: a second WS for the same user on the same board
//! overwrites the presence entry and the viewer appears once in the list).
//!
//! Plan 06-01 stubs `join`, `leave`, and `heartbeat` — only `subscribe` and
//! `snapshot` are needed so `ws_handler` compiles. Plan 06-04 fills the full logic.

#[cfg(feature = "ssr")]
use dashmap::DashMap;
#[cfg(feature = "ssr")]
use std::sync::Arc;
#[cfg(feature = "ssr")]
use std::time::Instant;
#[cfg(feature = "ssr")]
use tokio::sync::broadcast;
#[cfg(feature = "ssr")]
use crate::models::events::{PresenceEvent, PresenceSnapshot};
#[cfg(feature = "ssr")]
use crate::auth::models::AuthUser;

/// Per-viewer presence state.
#[cfg(feature = "ssr")]
#[derive(Clone, Debug)]
pub struct PresenceState {
    pub user_id: String,
    pub display_name: String,
    pub avatar_color: String,
    pub last_heartbeat: Instant,
    pub board_id: String,
    pub editing_card_id: Option<String>,
    pub typing_in_card_id: Option<String>,
}

/// Ephemeral presence registry.
///
/// `viewers` maps `"{board_id}:{user_id}"` → `PresenceState`.
/// `presence_tx` maps `board_id` → `broadcast::Sender<PresenceEvent>` for per-board fan-out.
///
/// Cloning is cheap — all fields are `Arc`-backed.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct PresenceRegistry {
    pub viewers: Arc<DashMap<String, PresenceState>>,
    pub presence_tx: Arc<DashMap<String, broadcast::Sender<PresenceEvent>>>,
}

#[cfg(feature = "ssr")]
impl PresenceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            viewers: Arc::new(DashMap::new()),
            presence_tx: Arc::new(DashMap::new()),
        }
    }

    /// Subscribe to presence events for a board.
    ///
    /// Creates the broadcast channel on first access. The WS handler subscribes
    /// BEFORE building the snapshot (Pitfall 3: subscribe first, then snapshot,
    /// so any ViewerLeft emitted between the two is not missed).
    pub fn subscribe(&self, board_id: &str) -> broadcast::Receiver<PresenceEvent> {
        self.presence_tx
            .entry(board_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    /// Build a ViewersSnapshot for a board (sent to new joiners).
    ///
    /// MUST be called AFTER subscribe (Pitfall 3).
    pub fn snapshot(&self, board_id: &str) -> PresenceEvent {
        let viewers: Vec<PresenceSnapshot> = self
            .viewers
            .iter()
            .filter(|entry| entry.value().board_id == board_id)
            .map(|entry| {
                let v = entry.value();
                PresenceSnapshot {
                    user_id: v.user_id.clone(),
                    display_name: v.display_name.clone(),
                    avatar_color: v.avatar_color.clone(),
                    editing_card_id: v.editing_card_id.clone(),
                    typing_in_card_id: v.typing_in_card_id.clone(),
                }
            })
            .collect();
        PresenceEvent::ViewersSnapshot { viewers }
    }

    /// Record a viewer joining a board and broadcast ViewerJoined to others.
    ///
    /// Stubbed in 06-01 — full logic in 06-04.
    pub fn join(&self, board_id: &str, user: &AuthUser, _client_id: &str) {
        let key = format!("{}:{}", board_id, user.id);
        self.viewers.insert(
            key,
            PresenceState {
                user_id: user.id.clone(),
                display_name: user.display_name.clone(),
                avatar_color: user.avatar_color.clone(),
                last_heartbeat: Instant::now(),
                board_id: board_id.to_string(),
                editing_card_id: None,
                typing_in_card_id: None,
            },
        );
        // Broadcast ViewerJoined to other viewers (06-04 will wire the full channel logic)
        if let Some(tx) = self.presence_tx.get(board_id) {
            let _ = tx.send(PresenceEvent::ViewerJoined {
                user_id: user.id.clone(),
                display_name: user.display_name.clone(),
                avatar_color: user.avatar_color.clone(),
            });
        }
    }

    /// Record a viewer leaving a board and broadcast ViewerLeft to others.
    ///
    /// Stubbed in 06-01 — full logic in 06-04.
    pub fn leave(&self, board_id: &str, user_id: &str) {
        let key = format!("{}:{}", board_id, user_id);
        self.viewers.remove(&key);
        if let Some(tx) = self.presence_tx.get(board_id) {
            let _ = tx.send(PresenceEvent::ViewerLeft {
                user_id: user_id.to_string(),
            });
        }
    }

    /// Update the heartbeat timestamp for a viewer.
    ///
    /// Stubbed in 06-01 — full logic in 06-04.
    pub fn heartbeat(&self, board_id: &str, user_id: &str) {
        let key = format!("{}:{}", board_id, user_id);
        if let Some(mut entry) = self.viewers.get_mut(&key) {
            entry.last_heartbeat = Instant::now();
        }
    }
}
