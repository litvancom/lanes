//! Per-user notification delivery registry (RT-04).
//!
//! `UserNotifRegistry` holds one or more `mpsc::UnboundedSender<NotifEvent>` per connected
//! user, keyed by a per-connection `channel_id` (UUID string).  A user with multiple
//! concurrent connections (e.g. a dashboard tab and a board tab) gets independent entries
//! — publish fans out to ALL of them.
//!
//! When a connection opens, `subscribe` appends a new `(channel_id, Sender)` pair to the
//! user's Vec and returns the `channel_id` alongside the Sender/Receiver pair.
//! When the connection closes, `remove_if_current(user_id, channel_id)` removes only that
//! connection's entry, preserving any sibling connections (CR-01 under fan-out).
//!
//! `publish` fans out to every live Sender for the user, pruning dead Senders (dropped
//! Receivers) on send failure.  If the Vec becomes empty after pruning, the user key is
//! removed to avoid unbounded map growth (T-06-SC-03 DoS mitigation).

#[cfg(feature = "ssr")]
use dashmap::DashMap;
#[cfg(feature = "ssr")]
use std::sync::Arc;
#[cfg(feature = "ssr")]
use tokio::sync::mpsc;
#[cfg(feature = "ssr")]
use crate::models::events::NotifEvent;

/// Concurrent registry mapping user IDs to their per-connection notification channel Senders.
///
/// Each entry is a `Vec<(channel_id, Sender)>` so that multiple concurrent connections for
/// the same user all receive published events (fan-out).  Cloning is cheap — the inner
/// `Arc<DashMap>` is reference-counted.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct UserNotifRegistry(
    pub Arc<DashMap<String, Vec<(String, mpsc::UnboundedSender<NotifEvent>)>>>,
);

#[cfg(feature = "ssr")]
impl UserNotifRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self(Arc::new(DashMap::new()))
    }

    /// Register a notification channel for the given user.
    ///
    /// Creates a new unbounded channel, generates a unique `channel_id`, appends the Sender
    /// to the user's Vec (APPEND — never overwrites siblings), and returns
    /// `(channel_id, Sender, Receiver)`.
    ///
    /// The `channel_id` must be passed to `remove_if_current` when the connection closes so
    /// only this connection's Sender is removed (CR-01 under fan-out).
    pub fn subscribe(
        &self,
        user_id: &str,
    ) -> (String, mpsc::UnboundedSender<NotifEvent>, mpsc::UnboundedReceiver<NotifEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let channel_id = uuid::Uuid::new_v4().to_string();
        self.0
            .entry(user_id.to_string())
            .or_default()
            .push((channel_id.clone(), tx.clone()));
        (channel_id, tx, rx)
    }

    /// Deliver a notification to all connections for the given user (fan-out).
    ///
    /// Iterates the user's Sender Vec, cloning the event for each Sender.
    /// Prunes any Sender whose `send` returns `Err` (receiver dropped — stale connection).
    /// If the Vec becomes empty after pruning, the user key is removed (T-06-SC-03).
    pub fn publish(&self, user_id: &str, event: NotifEvent) {
        if let Some(mut entry) = self.0.get_mut(user_id) {
            entry.retain(|(_, tx)| tx.send(event.clone()).is_ok());
            if entry.is_empty() {
                drop(entry); // release the DashMap shard lock before remove
                self.0.remove(user_id);
            }
        }
    }

    /// Remove the channel for this specific connection only (channel-scoped teardown).
    ///
    /// Retains all other connections for the same user (CR-01 under fan-out).
    /// If the Vec becomes empty after removal, the user key is removed.
    pub fn remove_if_current(&self, user_id: &str, channel_id: &str) {
        if let Some(mut entry) = self.0.get_mut(user_id) {
            entry.retain(|(cid, _)| cid != channel_id);
            if entry.is_empty() {
                drop(entry);
                self.0.remove(user_id);
            }
        }
    }
}
