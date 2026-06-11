//! Per-board broadcast channel registry.
//!
//! `BoardRoomRegistry` holds one `tokio::broadcast::Sender<BoardEvent>` per board
//! and a per-board monotonically increasing sequence counter (`Arc<AtomicU64>`).
//!
//! The sequence counter powers gap detection on the WASM client (Flag 1 resolution):
//! a client that sees `board_seq > last_seen_seq + 1` knows it missed events and
//! triggers a full board refresh.
//!
//! The registry is held in `AppState` and cloned cheaply (outer `Arc<DashMap>`).

#[cfg(feature = "ssr")]
use dashmap::DashMap;
#[cfg(feature = "ssr")]
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
#[cfg(feature = "ssr")]
use tokio::sync::broadcast;
#[cfg(feature = "ssr")]
use crate::models::events::BoardEvent;

/// Inner per-board state: broadcast sender + monotonic sequence counter.
#[cfg(feature = "ssr")]
struct BoardRoom {
    tx: broadcast::Sender<BoardEvent>,
    seq: Arc<AtomicU64>,
}

/// Concurrent registry of per-board broadcast channels.
///
/// Cloning is cheap — the inner `Arc<DashMap>` is reference-counted.
/// All operations are lock-free at the DashMap level (shard-striped).
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct BoardRoomRegistry(pub Arc<DashMap<String, BoardRoom>>);

#[cfg(feature = "ssr")]
impl BoardRoomRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self(Arc::new(DashMap::new()))
    }

    /// Subscribe to a board's broadcast channel.
    ///
    /// Creates the channel and counter on first access (entry-or-insert).
    /// The returned `Receiver` starts at the current tail — it will not receive
    /// events published before this call.
    pub fn subscribe(&self, board_id: &str) -> broadcast::Receiver<BoardEvent> {
        self.0
            .entry(board_id.to_string())
            .or_insert_with(|| BoardRoom {
                tx: broadcast::channel(256).0,
                seq: Arc::new(AtomicU64::new(0)),
            })
            .tx
            .subscribe()
    }

    /// Increment the board's sequence counter and return the new value.
    ///
    /// Called once per mutation immediately before `publish`. The returned value
    /// is stamped on the `BoardEvent` so clients can detect gaps.
    /// Creates the board entry if it doesn't exist yet.
    pub fn next_seq(&self, board_id: &str) -> u64 {
        self.0
            .entry(board_id.to_string())
            .or_insert_with(|| BoardRoom {
                tx: broadcast::channel(256).0,
                seq: Arc::new(AtomicU64::new(0)),
            })
            .seq
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }

    /// Read the current sequence number without incrementing.
    ///
    /// Returned in the `Connected` handshake so new clients anchor `last_seen_seq`
    /// before their first event arrives.
    pub fn current_seq(&self, board_id: &str) -> u64 {
        self.0
            .get(board_id)
            .map(|room| room.seq.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Broadcast a `BoardEvent` to all subscribers on this board.
    ///
    /// Ignores `SendError` — zero active receivers is a normal state (no one viewing).
    /// Does nothing if the board has no entry yet (shouldn't happen after `subscribe`/`next_seq`).
    pub fn publish(&self, board_id: &str, event: BoardEvent) {
        if let Some(room) = self.0.get(board_id) {
            let _ = room.tx.send(event);
        }
    }

    /// Return the current number of active broadcast receivers for a board.
    ///
    /// Used in the ws_handler cleanup log (SC4 diagnostics): after all tabs close,
    /// this should return 0, confirming no subscriber/task leak under open/close churn.
    pub fn receiver_count(&self, board_id: &str) -> usize {
        self.0
            .get(board_id)
            .map(|room| room.tx.receiver_count())
            .unwrap_or(0)
    }
}
