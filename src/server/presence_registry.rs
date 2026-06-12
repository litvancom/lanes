//! Ephemeral presence channel registry (RT-03).
//!
//! `PresenceRegistry` tracks who is viewing each board (D-10/D-12/D-13).
//! It is completely isolated from `BoardRoomRegistry` — presence events are
//! high-churn and must not cause `RecvError::Lagged` in DB-mutation receivers.
//!
//! Keys in `viewers` are `"{board_id}:{user_id}"` — allows the same user to view
//! multiple boards simultaneously. For multi-tab on the *same* board, `conn_count`
//! reference-counts connections per `(board_id, user_id)` so `leave` only removes
//! the entry and broadcasts `ViewerLeft` when the last connection for that user on
//! that board closes (CR-02).
//!
//! Full implementation for 06-04: join/leave/heartbeat/set_editing/set_typing/
//! snapshot/sweep_once. The background sweep task is spawned once in start_server.

#[cfg(feature = "ssr")]
use dashmap::DashMap;
#[cfg(feature = "ssr")]
use std::sync::Arc;
#[cfg(feature = "ssr")]
use std::time::{Duration, Instant};
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
/// `conn_count` maps the same key → active connection count for ref-counted teardown (CR-02).
/// `presence_tx` maps `board_id` → `broadcast::Sender<PresenceEvent>` for per-board fan-out.
///
/// Cloning is cheap — all fields are `Arc`-backed.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct PresenceRegistry {
    pub viewers: Arc<DashMap<String, PresenceState>>,
    pub conn_count: Arc<DashMap<String, usize>>,
    pub presence_tx: Arc<DashMap<String, broadcast::Sender<PresenceEvent>>>,
}

#[cfg(feature = "ssr")]
impl PresenceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            viewers: Arc::new(DashMap::new()),
            conn_count: Arc::new(DashMap::new()),
            presence_tx: Arc::new(DashMap::new()),
        }
    }

    /// Subscribe to presence events for a board.
    ///
    /// Creates the broadcast channel on first access (capacity 1024 — presence is
    /// high-churn; Lagged just resends a snapshot rather than Refresh).
    /// The WS handler subscribes BEFORE building the snapshot (Pitfall 3: subscribe first,
    /// then snapshot, so any ViewerLeft emitted between the two is not missed).
    pub fn subscribe(&self, board_id: &str) -> broadcast::Receiver<PresenceEvent> {
        self.presence_tx
            .entry(board_id.to_string())
            .or_insert_with(|| broadcast::channel(1024).0)
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
    /// Increments the connection ref-count for this (board_id, user_id) pair (CR-02).
    /// On first join (count was 0), inserts the viewer entry and broadcasts ViewerJoined.
    /// On subsequent joins (multi-tab), only updates the heartbeat — no duplicate event.
    pub fn join(&self, board_id: &str, user: &AuthUser, _client_id: &str) {
        let key = format!("{}:{}", board_id, user.id);

        // Increment connection ref-count.
        let mut count_entry = self.conn_count.entry(key.clone()).or_insert(0);
        *count_entry += 1;
        let is_first = *count_entry == 1;
        drop(count_entry);

        if is_first {
            // First connection for this user on this board — insert viewer entry.
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
            // Broadcast ViewerJoined to other viewers on this board.
            let tx = self.presence_tx
                .entry(board_id.to_string())
                .or_insert_with(|| broadcast::channel(1024).0);
            let _ = tx.send(PresenceEvent::ViewerJoined {
                user_id: user.id.clone(),
                display_name: user.display_name.clone(),
                avatar_color: user.avatar_color.clone(),
            });
        } else {
            // Subsequent connection (multi-tab) — refresh heartbeat only.
            if let Some(mut entry) = self.viewers.get_mut(&key) {
                entry.last_heartbeat = Instant::now();
            }
        }
    }

    /// Record a viewer leaving a board.
    ///
    /// Decrements the connection ref-count (CR-02). Only removes the viewer entry and
    /// broadcasts `ViewerLeft` when the count reaches 0 (last connection for this user
    /// on this board closed).
    pub fn leave(&self, board_id: &str, user_id: &str) {
        let key = format!("{}:{}", board_id, user_id);

        let remaining = {
            let mut count_entry = self.conn_count.entry(key.clone()).or_insert(0);
            if *count_entry > 0 {
                *count_entry -= 1;
            }
            *count_entry
        };

        if remaining == 0 {
            self.conn_count.remove(&key);
            self.viewers.remove(&key);
            if let Some(tx) = self.presence_tx.get(board_id) {
                let _ = tx.send(PresenceEvent::ViewerLeft {
                    user_id: user_id.to_string(),
                });
            }
        }
    }

    /// Update the heartbeat timestamp for a viewer.
    ///
    /// No broadcast — heartbeats are purely a liveness signal.
    pub fn heartbeat(&self, board_id: &str, user_id: &str) {
        let key = format!("{}:{}", board_id, user_id);
        if let Some(mut entry) = self.viewers.get_mut(&key) {
            entry.last_heartbeat = Instant::now();
        }
    }

    /// Update the card being edited by a viewer and broadcast EditingCard.
    ///
    /// `card_id = None` means the viewer stopped editing.
    /// T-6-14: user_id comes from the validated AuthSession in the WS handler, not from the message.
    pub fn set_editing(&self, board_id: &str, user_id: &str, card_id: Option<String>) {
        let key = format!("{}:{}", board_id, user_id);
        if let Some(mut entry) = self.viewers.get_mut(&key) {
            entry.editing_card_id = card_id.clone();
        }
        if let Some(tx) = self.presence_tx.get(board_id) {
            let _ = tx.send(PresenceEvent::EditingCard {
                user_id: user_id.to_string(),
                card_id,
            });
        }
    }

    /// Update the typing state for a viewer in a card's comment field and broadcast Typing.
    ///
    /// T-6-14: user_id comes from the validated AuthSession in the WS handler, not from the message.
    pub fn set_typing(&self, board_id: &str, user_id: &str, card_id: &str, is_typing: bool) {
        let key = format!("{}:{}", board_id, user_id);
        if let Some(mut entry) = self.viewers.get_mut(&key) {
            entry.typing_in_card_id = if is_typing {
                Some(card_id.to_string())
            } else {
                None
            };
        }
        if let Some(tx) = self.presence_tx.get(board_id) {
            let _ = tx.send(PresenceEvent::Typing {
                user_id: user_id.to_string(),
                card_id: card_id.to_string(),
                is_typing,
            });
        }
    }

    /// Reap viewers whose last_heartbeat is older than 15 seconds (D-13 / T-6-15).
    ///
    /// Takes an explicit `now: Instant` so tests can control time deterministically
    /// without requiring tokio::time::pause to affect std::time::Instant.
    ///
    /// Called by the background sweep loop in start_server every 10 seconds (Anti-Pattern §717).
    /// The sweep removes the viewer entry AND resets the conn_count to 0, treating a stale
    /// heartbeat as equivalent to all connections having gone away.
    pub fn sweep_once(&self, now: Instant) {
        let threshold = Duration::from_secs(15);

        // Collect stale keys first to avoid holding DashMap locks across the remove + broadcast.
        let stale: Vec<(String, String)> = self
            .viewers
            .iter()
            .filter(|entry| {
                now.duration_since(entry.value().last_heartbeat) > threshold
            })
            .map(|entry| {
                let v = entry.value();
                (entry.key().clone(), v.board_id.clone())
            })
            .collect();

        for (key, board_id) in stale {
            // Extract user_id from the key (format: "{board_id}:{user_id}")
            let user_id = if let Some(colon) = key.find(':') {
                key[colon + 1..].to_string()
            } else {
                continue;
            };

            // Remove viewer entry and reset ref-count.
            self.viewers.remove(&key);
            self.conn_count.remove(&key);

            // Broadcast ViewerLeft to remaining board viewers (T-6-15 reap)
            if let Some(tx) = self.presence_tx.get(&board_id) {
                let _ = tx.send(PresenceEvent::ViewerLeft {
                    user_id,
                });
            }
        }
    }
}
