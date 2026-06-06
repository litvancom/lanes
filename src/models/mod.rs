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
