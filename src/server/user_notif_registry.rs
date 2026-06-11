//! Per-user notification delivery registry (RT-04).
//!
//! `UserNotifRegistry` holds one `mpsc::UnboundedSender<NotifEvent>` per connected user.
//! The Sender is inserted when the user's WS handler calls `subscribe`, and removed when
//! the handler exits (Pitfall 1 explicit cleanup).
//!
//! Using `mpsc::unbounded` rather than broadcast: a user has at most one WS connection in v1.
//! The Sender lives in the registry; the Receiver lives in the WS handler task.
//! When two tabs open: the second subscribe() overwrites the first Sender (first tab loses
//! notifications). This is consistent with D-12 (tab-visible = present).

#[cfg(feature = "ssr")]
use dashmap::DashMap;
#[cfg(feature = "ssr")]
use std::sync::Arc;
#[cfg(feature = "ssr")]
use tokio::sync::mpsc;
#[cfg(feature = "ssr")]
use crate::models::events::NotifEvent;

/// Concurrent registry mapping user IDs to their notification channel Senders.
///
/// Cloning is cheap — the inner `Arc<DashMap>` is reference-counted.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct UserNotifRegistry(pub Arc<DashMap<String, mpsc::UnboundedSender<NotifEvent>>>);

#[cfg(feature = "ssr")]
impl UserNotifRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self(Arc::new(DashMap::new()))
    }

    /// Register a notification channel for the given user.
    ///
    /// Overwrites any existing entry (multi-tab: only the latest tab receives notifications).
    /// Returns the Receiver end — the caller (WS handler) owns it for the lifetime of the task.
    pub fn subscribe(&self, user_id: &str) -> mpsc::UnboundedReceiver<NotifEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.insert(user_id.to_string(), tx);
        rx
    }

    /// Deliver a notification to a connected user.
    ///
    /// Silently ignores delivery failures (user disconnected between lookup and send).
    pub fn publish(&self, user_id: &str, event: NotifEvent) {
        if let Some(tx) = self.0.get(user_id) {
            let _ = tx.send(event);
        }
    }

    /// Remove the channel for a user (called on WS handler exit — Pitfall 1 cleanup).
    ///
    /// Without this, the Sender stays in the map; future publishes succeed on a dead channel
    /// and the Receiver is leaked in the (already-returned) WS task.
    pub fn remove(&self, user_id: &str) {
        self.0.remove(user_id);
    }
}
