use serde::{Deserialize, Serialize};

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
}
