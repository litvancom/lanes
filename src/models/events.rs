//! Realtime wire-format types shared between SSR (server) and hydrate (WASM) targets.
//!
//! CRITICAL: No cfg(feature="ssr") gates on ANY type here.
//! These types must compile under both the `ssr` and `hydrate` features.
//! They define the WebSocket message contract between the server and browser.

use serde::{Deserialize, Serialize};
use crate::models::CardLabel;

// ---------------------------------------------------------------------------
// BoardEvent — per-board mutation broadcast (RT-01)
// ---------------------------------------------------------------------------

/// Full variant set for all board mutations. Only `Connected`, `CardMoved`, and `Refresh`
/// are published in Plan 06-01. Later plans add publish hooks for the remaining variants.
/// The full set is declared now to freeze the wire contract.
///
/// D-05: every mutation variant carries `client_id` (the originator's connection ID)
/// so the WASM client can suppress the highlight for its own echo.
///
/// `board_seq` is a per-board monotonically increasing counter (AtomicU64 in BoardRoomRegistry).
/// Clients use it for gap detection: a jump > 1 triggers a full `Refresh`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoardEvent {
    /// Sent directly to a new connection as the initial handshake (not broadcast).
    /// Anchors the client's `last_seen_seq` before the first real event arrives.
    Connected { client_id: String, board_seq: u64 },

    /// A card was moved to a new list and/or position.
    CardMoved {
        board_seq: u64,
        client_id: String,
        card_id: String,
        to_list_id: String,
        position: String,
    },
    /// A new card was created in a list.
    CardAdded {
        board_seq: u64,
        client_id: String,
        card: CardSummary,
    },
    /// One or more card fields were changed (flat optional-field patch).
    CardUpdated {
        board_seq: u64,
        client_id: String,
        card_id: String,
        patch: CardPatch,
    },
    /// A card was archived.
    CardArchived {
        board_seq: u64,
        client_id: String,
        card_id: String,
    },
    /// A comment was added to a card.
    CommentAdded {
        board_seq: u64,
        client_id: String,
        card_id: String,
        comment_id: String,
        author_id: String,
        text: String,
        created_at: i64,
    },
    /// A checklist item was updated.
    ChecklistUpdated {
        board_seq: u64,
        client_id: String,
        card_id: String,
        checklist_done: i64,
        checklist_total: i64,
    },
    /// Labels on a card changed.
    LabelChanged {
        board_seq: u64,
        client_id: String,
        card_id: String,
        labels: Vec<CardLabel>,
    },
    /// Card priority changed.
    PriorityChanged {
        board_seq: u64,
        client_id: String,
        card_id: String,
        priority: Option<String>,
    },
    /// Card due date changed.
    DueDateChanged {
        board_seq: u64,
        client_id: String,
        card_id: String,
        due_at: Option<i64>,
    },
    /// Card members changed.
    MemberChanged {
        board_seq: u64,
        client_id: String,
        card_id: String,
        member_ids: Vec<String>,
    },
    /// A file was attached to a card.
    AttachmentAdded {
        board_seq: u64,
        client_id: String,
        card_id: String,
        attachment_id: String,
        filename: String,
        url: String,
        size_bytes: i64,
    },
    /// A file attachment was removed from a card.
    AttachmentRemoved {
        board_seq: u64,
        client_id: String,
        card_id: String,
        attachment_id: String,
    },
    /// A new list was added to the board.
    ListAdded {
        board_seq: u64,
        client_id: String,
        list_id: String,
        name: String,
        position: String,
    },
    /// A list was renamed.
    ListRenamed {
        board_seq: u64,
        client_id: String,
        list_id: String,
        name: String,
    },
    /// A list was reordered.
    ListReordered {
        board_seq: u64,
        client_id: String,
        list_id: String,
        position: String,
    },
    /// A list was archived.
    ListArchived {
        board_seq: u64,
        client_id: String,
        list_id: String,
    },
    /// The board was renamed.
    BoardRenamed {
        board_seq: u64,
        client_id: String,
        board_id: String,
        name: String,
    },
    /// A card was moved to another board (the card disappeared from this board).
    CardMovedCrossBoard {
        board_seq: u64,
        client_id: String,
        card_id: String,
    },
    /// Triggers an unconditional full board refresh on the client.
    /// Sent when a broadcast receiver falls too far behind (RecvError::Lagged).
    Refresh,
}

impl BoardEvent {
    /// Returns the originator client_id for mutation events, or None for bookkeeping events
    /// (Connected, Refresh) that are not originated by a specific client action.
    ///
    /// D-05: the WASM client compares this against `own_client_id` to suppress the highlight
    /// flash for the originator's own actions.
    pub fn client_id(&self) -> Option<&str> {
        match self {
            BoardEvent::Connected { .. } => None,
            BoardEvent::Refresh => None,
            BoardEvent::CardMoved { client_id, .. } => Some(client_id),
            BoardEvent::CardAdded { client_id, .. } => Some(client_id),
            BoardEvent::CardUpdated { client_id, .. } => Some(client_id),
            BoardEvent::CardArchived { client_id, .. } => Some(client_id),
            BoardEvent::CommentAdded { client_id, .. } => Some(client_id),
            BoardEvent::ChecklistUpdated { client_id, .. } => Some(client_id),
            BoardEvent::LabelChanged { client_id, .. } => Some(client_id),
            BoardEvent::PriorityChanged { client_id, .. } => Some(client_id),
            BoardEvent::DueDateChanged { client_id, .. } => Some(client_id),
            BoardEvent::MemberChanged { client_id, .. } => Some(client_id),
            BoardEvent::AttachmentAdded { client_id, .. } => Some(client_id),
            BoardEvent::AttachmentRemoved { client_id, .. } => Some(client_id),
            BoardEvent::ListAdded { client_id, .. } => Some(client_id),
            BoardEvent::ListRenamed { client_id, .. } => Some(client_id),
            BoardEvent::ListReordered { client_id, .. } => Some(client_id),
            BoardEvent::ListArchived { client_id, .. } => Some(client_id),
            BoardEvent::BoardRenamed { client_id, .. } => Some(client_id),
            BoardEvent::CardMovedCrossBoard { client_id, .. } => Some(client_id),
        }
    }

    /// Returns the board_seq for mutation events (events stamped with a sequence number),
    /// or None for Connected (uses its own dedicated field) and Refresh (no sequence).
    pub fn board_seq(&self) -> Option<u64> {
        match self {
            BoardEvent::Connected { .. } => None,
            BoardEvent::Refresh => None,
            BoardEvent::CardMoved { board_seq, .. } => Some(*board_seq),
            BoardEvent::CardAdded { board_seq, .. } => Some(*board_seq),
            BoardEvent::CardUpdated { board_seq, .. } => Some(*board_seq),
            BoardEvent::CardArchived { board_seq, .. } => Some(*board_seq),
            BoardEvent::CommentAdded { board_seq, .. } => Some(*board_seq),
            BoardEvent::ChecklistUpdated { board_seq, .. } => Some(*board_seq),
            BoardEvent::LabelChanged { board_seq, .. } => Some(*board_seq),
            BoardEvent::PriorityChanged { board_seq, .. } => Some(*board_seq),
            BoardEvent::DueDateChanged { board_seq, .. } => Some(*board_seq),
            BoardEvent::MemberChanged { board_seq, .. } => Some(*board_seq),
            BoardEvent::AttachmentAdded { board_seq, .. } => Some(*board_seq),
            BoardEvent::AttachmentRemoved { board_seq, .. } => Some(*board_seq),
            BoardEvent::ListAdded { board_seq, .. } => Some(*board_seq),
            BoardEvent::ListRenamed { board_seq, .. } => Some(*board_seq),
            BoardEvent::ListReordered { board_seq, .. } => Some(*board_seq),
            BoardEvent::ListArchived { board_seq, .. } => Some(*board_seq),
            BoardEvent::BoardRenamed { board_seq, .. } => Some(*board_seq),
            BoardEvent::CardMovedCrossBoard { board_seq, .. } => Some(*board_seq),
        }
    }
}

// ---------------------------------------------------------------------------
// CardPatch — flat optional-field struct for CardUpdated (avoids variant-per-field)
// ---------------------------------------------------------------------------

/// Flat optional-field patch for `BoardEvent::CardUpdated`.
/// Only fields present (Some) in the patch are to be applied.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CardPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub cover: Option<String>,
    pub done: Option<bool>,
    pub card_num: Option<i64>,
}

// ---------------------------------------------------------------------------
// CardSummary — lightweight card representation for CardAdded
// ---------------------------------------------------------------------------

/// Lightweight card representation sent in `BoardEvent::CardAdded`.
/// Contains enough data to render the card thumbnail without a fetch.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CardSummary {
    pub id: String,
    pub list_id: String,
    pub board_id: String,
    pub card_num: i64,
    pub title: String,
    pub position: String,
    pub priority: Option<String>,
    pub due_at: Option<i64>,
    pub done: bool,
    pub cover: Option<String>,
    pub labels: Vec<CardLabel>,
    pub member_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// NotifEvent — per-user notification delivery (RT-04)
// ---------------------------------------------------------------------------

/// Events delivered to a specific user via their `UserNotifRegistry` channel.
/// Multiplexed over the board WebSocket via `WsEnvelope::User`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotifEvent {
    /// A user was @mentioned in a comment.
    MentionReceived {
        notification_id: String,
        card_id: String,
        card_title: String,
        board_id: String,
        from_user_name: String,
    },
    /// The unread notification count changed.
    UnreadCountUpdated { count: i64 },
}

// ---------------------------------------------------------------------------
// PresenceEvent — ephemeral presence channel (RT-03)
// ---------------------------------------------------------------------------

/// Viewer's presence snapshot (sent to new joiners via ViewersSnapshot).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PresenceSnapshot {
    pub user_id: String,
    pub display_name: String,
    pub avatar_color: String,
    pub editing_card_id: Option<String>,
    pub typing_in_card_id: Option<String>,
}

/// Events delivered to board viewers via `PresenceRegistry`.
/// Multiplexed over the board WebSocket via `WsEnvelope::Presence`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PresenceEvent {
    /// A new viewer joined the board.
    ViewerJoined {
        user_id: String,
        display_name: String,
        avatar_color: String,
    },
    /// A viewer left the board (navigated away, tab closed, heartbeat timeout).
    ViewerLeft { user_id: String },
    /// A viewer started or stopped editing a specific card (D-10).
    /// `card_id = None` means the viewer stopped editing.
    EditingCard {
        user_id: String,
        card_id: Option<String>,
    },
    /// A viewer is typing in a card's comment field (D-10).
    Typing {
        user_id: String,
        card_id: String,
        is_typing: bool,
    },
    /// Full snapshot of current viewers — sent to a new joiner (Pitfall 3: subscribe first).
    ViewersSnapshot { viewers: Vec<PresenceSnapshot> },
}

// ---------------------------------------------------------------------------
// WsEnvelope — single-socket multiplexer for all three channels
// ---------------------------------------------------------------------------

/// Envelope type wrapping all server→browser WebSocket messages.
/// The `channel` tag routes each message to the correct WASM handler.
///
/// All server→browser messages are wrapped in this envelope.
/// Browser→server messages are small JSON objects (`{"type":"heartbeat"}` etc.)
/// and are NOT wrapped.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "channel", rename_all = "snake_case")]
pub enum WsEnvelope {
    /// Board mutation events and the initial Connected handshake.
    Board { payload: BoardEvent },
    /// Per-user notification events.
    User { payload: NotifEvent },
    /// Ephemeral presence events (viewer join/leave, typing, editing).
    Presence { payload: PresenceEvent },
}
