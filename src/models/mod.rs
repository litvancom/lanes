use serde::{Deserialize, Serialize};

/// Extended board view used by workspace home, sidebar starred section, board route,
/// and archive view. Collapses JOIN into a single queryable type.
/// `starred` reads from board_members.starred (per-user, D-10) NOT boards.starred.
/// `last_viewed_at` is None when the board has never been opened by this user (D-01).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardWithMeta {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub color: String,
    pub starred: bool,           // from board_members.starred (per user, D-10)
    pub archived: bool,
    pub card_count: i64,
    pub last_viewed_at: Option<i64>,  // epoch millis; None = never viewed (D-01)
    pub created_at: i64,
    pub updated_at: i64,
}

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
