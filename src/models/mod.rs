use serde::{Deserialize, Serialize};

pub mod events;
pub mod rest_dto;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub color: String,
    pub starred: bool,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Extended board view used by workspace home, sidebar starred section, and archive view.
/// Collapses the boards JOIN board_members JOIN card_count into a single queryable type.
/// `starred` reads from board_members.starred (per-user, D-10) — NOT boards.starred.
/// `last_viewed_at` is epoch millis; None = board has never been opened (D-01).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardWithMeta {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub color: String,
    /// Per-user starred flag from board_members.starred (D-10).
    /// One member's star never affects another member's sidebar.
    pub starred: bool,
    pub archived: bool,
    pub card_count: i64,
    /// Epoch millis; None = never viewed (D-01).
    pub last_viewed_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A card returned by the Today strip query (D-02).
/// Represents a non-done card whose due_at falls within today or is in the past.
/// `overdue` is true when due_at < today's midnight epoch millis.
/// Results are ordered by due_at ASC.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TodayCard {
    pub id: String,
    pub title: String,
    pub board_id: String,
    pub board_name: String,
    pub due_at: Option<i64>,
    pub overdue: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct List {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub position: String,
    pub archived: bool,
    /// Phase 4 — D-13: cards moved into this list have done=1 set automatically.
    /// NOT derived from the list name.
    pub is_done_list: bool,
}

/// A label attached to a card (display only in Phase 4; editing UI is Phase 5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CardLabel {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Card {
    pub id: String,
    pub list_id: String,
    pub board_id: String,
    pub card_num: i64,
    pub title: String,
    pub position: String,
    pub priority: Option<String>,
    pub due_at: Option<i64>,
    pub done: bool,
    pub archived: bool,
    // Phase 4 additions
    /// CSS color/gradient string for the cover band, or None (D-12).
    pub cover: Option<String>,
    /// Labels attached to this card (populated from card_labels JOIN labels).
    pub labels: Vec<CardLabel>,
    /// Denormalized checklist progress — done items (D-11).
    pub checklist_done: i64,
    /// Denormalized checklist progress — total items (D-11).
    pub checklist_total: i64,
    /// Denormalized comment count (D-11).
    pub comment_count: i64,
    /// Denormalized attachment count (D-11).
    pub attachment_count: i64,
    /// IDs of users assigned to this card (from card_members).
    pub member_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Phase 5: Card Detail DTOs
// ---------------------------------------------------------------------------

/// Lightweight user representation used in card detail views (board members, activity authors).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserSummary {
    pub id: String,
    pub display_name: String,
    pub avatar_color: String,
}

/// A single item in a checklist (belongs to a checklist, which belongs to a card).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChecklistItem {
    pub id: String,
    pub checklist_id: String,
    pub text: String,
    pub done: bool,
    pub position: i64,
}

/// A single entry in the card activity feed — either a user comment or a system event.
/// `entry_type` is "comment" | "event".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ActivityEntry {
    /// "comment" or "event"
    pub entry_type: String,
    pub id: String,
    /// Author of the comment or actor who triggered the event; None for system-generated events.
    pub author: Option<UserSummary>,
    /// Comment body text, or event kind string (e.g. "moved", "archived").
    pub text: String,
    /// JSON payload for system events (e.g. `{"from_list":"...","to_list":"..."}`); None for comments.
    pub payload: Option<String>,
    pub created_at: i64,
}

/// A file attached to a card.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub card_id: String,
    pub filename: String,
    pub url: String,
    pub size_bytes: i64,
    pub uploader_id: String,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Phase 7: Calendar DTO
// ---------------------------------------------------------------------------

/// A due-date-bearing card returned by `get_calendar_cards` (CAL-01).
///
/// Aggregated across all boards the user is a member of (D-11).
/// `due_at` is epoch millis (UTC); always Some because the query filters on IS NOT NULL.
/// `done` drives the struck-through/muted chip styling (D-14).
/// `board_color` is the raw color string from boards.color — callers must sanitize
/// with `safe_hex()` before CSS interpolation (T-07-11).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CalendarCard {
    pub id: String,
    pub title: String,
    pub card_num: i64,
    pub due_at: Option<i64>,
    pub done: bool,
    pub board_id: String,
    pub board_name: String,
    pub board_color: String,
}

// ---------------------------------------------------------------------------
// Phase 7: Inbox DTOs
// ---------------------------------------------------------------------------

/// A single row in the inbox list (INBOX-01).
///
/// Returned by `list_notifications`.  All optional fields come from JOINs that
/// may not match (deleted card, anonymous actor, etc.).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NotificationRow {
    pub id: String,
    /// Notification kind: "mention" | "assigned" | "due_soon" | "overdue" | "watch_activity"
    pub kind: String,
    pub card_id: Option<String>,
    pub card_title: Option<String>,
    pub board_id: Option<String>,
    pub board_name: Option<String>,
    /// Per-board sequential card number (for deep-link `/board/{board_id}/card/{card_num}`).
    pub card_num: Option<i64>,
    pub actor_name: Option<String>,
    pub actor_color: Option<String>,
    pub read: bool,
    /// Creation timestamp as epoch milliseconds (UTC).
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Phase 5 gap-fix: Move popover target DTOs
// ---------------------------------------------------------------------------

/// A list shown in the Move popover's list selector for a given board.
/// Excludes archived lists.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MoveTargetList {
    pub id: String,
    pub name: String,
}

/// A board shown in the Move popover's board selector.
/// Only boards the current user is a member of; excludes archived boards.
/// Includes the board's non-archived lists.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MoveTargetBoard {
    pub id: String,
    pub name: String,
    pub lists: Vec<MoveTargetList>,
}

// ---------------------------------------------------------------------------
// Phase 7: API Token DTOs (API-03)
// ---------------------------------------------------------------------------

/// Returned exactly once at token creation (D-17 / D-18: raw never stored, shown once only).
///
/// `raw_token` is the 64-char hex string shown to the user once; it is never persisted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CreatedToken {
    pub id: String,
    pub name: String,
    /// The raw 64-char hex token.  Shown to the user once; never retrievable again.
    pub raw_token: String,
}

/// Token metadata returned by `list_api_tokens`.
/// Does NOT contain the token hash — callers never see the stored hash (D-17).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ApiTokenMeta {
    pub id: String,
    pub name: String,
    /// Creation timestamp as epoch milliseconds (UTC).
    pub created_at: i64,
    /// Last-used timestamp as epoch milliseconds (UTC); None if never used via API.
    pub last_used_at: Option<i64>,
}

/// Full card detail payload returned by `get_card_detail`.
/// Consumed by the card-detail modal and all Phase 5 slices.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CardDetail {
    /// The card itself (with labels, member_ids, denormalized counts).
    pub card: Card,
    /// Name of the list the card belongs to (breadcrumb — UI-SPEC §242).
    pub list_name: String,
    /// Name of the board the card belongs to (breadcrumb — UI-SPEC §242).
    pub board_name: String,
    /// Card description rendered from Markdown through ammonia (pre-sanitized HTML).
    pub description_html: String,
    /// Checklist items for this card, ordered by position ASC.
    pub checklist_items: Vec<ChecklistItem>,
    /// Activity feed: comments + system events, ordered by created_at ASC.
    pub activity: Vec<ActivityEntry>,
    /// File attachments for this card.
    pub attachments: Vec<Attachment>,
    /// Number of users watching this card.
    pub watcher_count: i64,
    /// Whether the current user is watching this card.
    pub is_watching: bool,
    /// All board members (for member picker and author resolution).
    pub board_members: Vec<UserSummary>,
    /// All labels on this board (for label picker — includes unassigned labels).
    pub board_labels: Vec<CardLabel>,
    /// When the card was created (epoch millis UTC) — for the modal header "created" label.
    pub created_at: i64,
}
