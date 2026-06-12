//! REST API Data Transfer Objects (DTOs) for the `/api/v1` surface.
//!
//! All types derive `Serialize`, `Deserialize`, and `utoipa::ToSchema` so they appear
//! in the generated OpenAPI document.  Types are intentionally flat — no nested
//! domain models — to keep the public API stable independently of internal model changes.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Read DTOs
// ---------------------------------------------------------------------------

/// A board visible to the authenticated user.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct BoardDto {
    pub id: String,
    pub name: String,
    pub color: String,
    pub key_prefix: String,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A single list within a board.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct ListDto {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub position: String,
    pub archived: bool,
    pub is_done_list: bool,
}

/// A card stub (title + placement). Full card detail is not part of this API surface.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CardDto {
    pub id: String,
    pub board_id: String,
    pub list_id: String,
    pub card_num: i64,
    pub title: String,
    pub position: String,
    pub priority: Option<String>,
    pub due_at: Option<i64>,
    pub done: bool,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A comment on a card.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CommentDto {
    pub id: String,
    pub card_id: String,
    pub author_id: String,
    pub body: String,
    pub created_at: i64,
}

/// The caller's workspace — a single-workspace model for v1 (all boards the user is a member of).
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct WorkspaceDto {
    /// Workspace identifier — always the authenticated user's own ID for v1.
    pub id: String,
    /// Display name derived from the user's `display_name`.
    pub display_name: String,
    pub board_count: i64,
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// Create a new board.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CreateBoardReq {
    /// Board display name (1–120 characters).
    pub name: String,
    /// One of the five allowed hex swatches: #7c5cff | #0ea5e9 | #f59e0b | #10b981 | #ec4899
    pub color: String,
}

/// Update an existing board (owner-only).  Supply only the fields you want to change.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct UpdateBoardReq {
    /// New display name (1–120 characters).
    pub name: Option<String>,
    /// New color — must be one of the five allowed swatches.
    pub color: Option<String>,
}

/// Create a new list at the end of a board.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CreateListReq {
    /// List name (1–120 characters).
    pub name: String,
}

/// Rename an existing list.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct UpdateListReq {
    /// New list name (1–120 characters).
    pub name: String,
}

/// Create a new card at the end of a list.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CreateCardReq {
    /// List that will receive the card.
    pub list_id: String,
    /// Card title (1–500 characters).
    pub title: String,
}

/// Update card fields.  Supply only the fields you want to change.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct UpdateCardReq {
    /// New title (1–500 characters).
    pub title: Option<String>,
}

/// Move a card to a different list and/or position.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct MoveCardReq {
    /// Target list ID (must belong to the same board).
    pub to_list_id: String,
    /// Fractional-index position string computed by the caller.
    pub position: String,
}

/// Post a comment on a card.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema)]
pub struct CreateCommentReq {
    /// Comment body (non-empty plain text; Markdown rendered on the UI side).
    pub body: String,
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Query parameters for endpoints that support cursor-based pagination.
#[derive(Serialize, Deserialize, Clone, Debug, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return (default 50, max 200).
    pub limit: Option<i64>,
    /// Cursor from a previous response (`next_cursor` field).
    pub cursor: Option<String>,
}
