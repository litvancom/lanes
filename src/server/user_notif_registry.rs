//! Per-user notification delivery registry (RT-04).
//!
//! `UserNotifRegistry` holds one `mpsc::UnboundedSender<NotifEvent>` per connected user.
//! The Sender is inserted when the user's WS handler calls `subscribe`, and removed when
//! the handler exits (Pitfall 1 explicit cleanup).
//!
//! Using `mpsc::unbounded` rather than broadcast: a user has at most one WS connection in v1.
//! The Sender lives in the registry; the Receiver lives in the WS handler task.
//! When two tabs open: the second subscribe() overwrites the first Sender (the latest tab
//! receives notifications). Teardown must only remove the entry belonging to this connection
//! — use `remove_if_current` rather than `remove` to avoid the first-tab-to-close wiping the
//! surviving second-tab's sender (CR-01).

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
    /// Returns both the Sender (for connection-scoped teardown via `remove_if_current`) and
    /// the Receiver end (owned by the WS handler task for the lifetime of the connection).
    pub fn subscribe(&self, user_id: &str) -> (mpsc::UnboundedSender<NotifEvent>, mpsc::UnboundedReceiver<NotifEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.insert(user_id.to_string(), tx.clone());
        (tx, rx)
    }

    /// Deliver a notification to a connected user.
    ///
    /// Silently ignores delivery failures (user disconnected between lookup and send).
    pub fn publish(&self, user_id: &str, event: NotifEvent) {
        if let Some(tx) = self.0.get(user_id) {
            let _ = tx.send(event);
        }
    }

    /// Remove the channel for a user only if the currently-stored sender is the one this
    /// connection installed (connection-scoped teardown — CR-01).
    ///
    /// If a second tab has already overwritten the entry with its own sender, this is a no-op
    /// so the second tab's notifications are preserved.
    pub fn remove_if_current(&self, user_id: &str, my_tx: &mpsc::UnboundedSender<NotifEvent>) {
        self.0.remove_if(user_id, |_, cur| cur.same_channel(my_tx));
    }
}
