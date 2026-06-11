use leptos::prelude::*;
use crate::models::{BoardWithMeta, List, Card};
use serde::{Deserialize, Serialize};

/// Full board data returned to the board route (D-08).
/// Contains the board metadata, all non-archived lists ordered by position,
/// and all non-archived card stubs (title-bearing rows for Phase 3 rendering).
/// `board_seq` is the current per-board sequence number at fetch time — the WASM client
/// stores this as `last_seen_seq` to anchor gap detection before the first WS event arrives.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardData {
    pub board: BoardWithMeta,
    pub lists: Vec<List>,
    pub cards: Vec<Card>,
    pub board_seq: u64,
}

/// Internal: fetch a BoardData for a given (board_id, user_id) pair.
/// Membership is verified by the caller (#[server] wrapper, not duplicated here).
/// The board query scopes by user_id so per-user columns (starred, last_viewed_at)
/// are correct; non-members would receive a "board not found" Err.
#[cfg(feature = "ssr")]
pub async fn get_board_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    user_id: &str,
) -> Result<BoardData, sqlx::Error> {
    // Fetch BoardWithMeta for this user — scoped by both board_id AND user_id.
    // Returns None if the user is not a member (D-12 enforced in wrapper).
    let board_row: Option<(String, String, String, String, bool, bool, i64, Option<i64>, i64, i64)> =
        sqlx::query_as(
            r#"SELECT b.id, b.name, b.key_prefix, b.color,
                      CAST(m.starred AS BOOLEAN) as starred,
                      CAST(b.archived AS BOOLEAN) as archived,
                      (SELECT COUNT(*) FROM cards c
                       JOIN lists l ON l.id = c.list_id
                       WHERE l.board_id = b.id AND c.archived = 0) as card_count,
                      m.last_viewed_at,
                      b.created_at, b.updated_at
               FROM boards b
               JOIN board_members m ON m.board_id = b.id
               WHERE b.id = ? AND m.user_id = ?"#
        )
        .bind(board_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

    let row = board_row.ok_or_else(|| {
        // D-12: generic "board not found" for non-members — does not reveal existence
        sqlx::Error::Decode("board not found".into())
    })?;

    // Decompose the tuple into BoardWithMeta (maps to the pattern)
    let board = BoardWithMeta {
        id: row.0,
        name: row.1,
        key_prefix: row.2,
        color: row.3,
        starred: row.4,
        archived: row.5,
        card_count: row.6,
        last_viewed_at: row.7,
        created_at: row.8,
        updated_at: row.9,
    };

    // Fetch non-archived lists ordered by position ASC (BOARD-03/04/05 contract)
    // is_done_list added in migration 004 (D-13).
    let lists_raw: Vec<(String, String, String, String, bool, bool)> = sqlx::query_as(
        r#"SELECT id, board_id, name, position,
                  CAST(archived AS BOOLEAN) as archived,
                  CAST(is_done_list AS BOOLEAN) as is_done_list
           FROM lists
           WHERE board_id = ? AND archived = 0
           ORDER BY position ASC"#
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    let lists: Vec<List> = lists_raw.into_iter().map(|(id, board_id, name, position, archived, is_done_list)| {
        List { id, board_id, name, position, archived, is_done_list }
    }).collect();

    // Fetch non-archived cards with all Phase-4 columns.
    // cover was in 001; checklist_done/total/comment_count/attachment_count added in 004.
    let cards_raw: Vec<(String, String, String, i64, String, String, Option<String>, Option<String>, Option<i64>, bool, bool, i64, i64, i64, i64)> =
        sqlx::query_as(
            r#"SELECT id, list_id, board_id, card_num, title, position,
                      cover, priority, due_at,
                      CAST(done AS BOOLEAN) as done,
                      CAST(archived AS BOOLEAN) as archived,
                      checklist_done, checklist_total,
                      comment_count, attachment_count
               FROM cards
               WHERE board_id = ? AND archived = 0
               ORDER BY list_id, position ASC"#
        )
        .bind(board_id)
        .fetch_all(pool)
        .await?;

    // Fetch card labels: (card_id, label_id, label_name, label_color)
    // Scoped to board_id via labels.board_id (T-04-01 — no cross-board label leakage).
    let card_labels_raw: Vec<(String, String, String, String)> = sqlx::query_as(
        r#"SELECT cl.card_id, l.id, l.name, l.color
           FROM card_labels cl
           JOIN labels l ON l.id = cl.label_id
           WHERE l.board_id = ?"#
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    // Fetch card members: (card_id, user_id)
    // Scoped to board_id via cards join (T-04-01 — no cross-board member leakage).
    let card_members_raw: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT cm.card_id, cm.user_id
           FROM card_members cm
           JOIN cards c ON c.id = cm.card_id
           WHERE c.board_id = ?"#
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    // Group labels by card_id
    use std::collections::HashMap;
    use crate::models::CardLabel;
    let mut labels_by_card: HashMap<String, Vec<CardLabel>> = HashMap::new();
    for (card_id, label_id, label_name, label_color) in card_labels_raw {
        labels_by_card
            .entry(card_id)
            .or_default()
            .push(CardLabel { id: label_id, name: label_name, color: label_color });
    }

    // Group member_ids by card_id
    let mut members_by_card: HashMap<String, Vec<String>> = HashMap::new();
    for (card_id, user_id) in card_members_raw {
        members_by_card.entry(card_id).or_default().push(user_id);
    }

    let cards: Vec<Card> = cards_raw.into_iter().map(|(id, list_id, board_id, card_num, title, position, cover, priority, due_at, done, archived, checklist_done, checklist_total, comment_count, attachment_count)| {
        let labels = labels_by_card.remove(&id).unwrap_or_default();
        let member_ids = members_by_card.remove(&id).unwrap_or_default();
        Card { id, list_id, board_id, card_num, title, position, cover, priority, due_at, done, archived, labels, checklist_done, checklist_total, comment_count, attachment_count, member_ids }
    }).collect();

    // board_seq is set by the #[server] get_board wrapper (which has AppState access).
    // get_board_inner is called from tests directly — board_seq defaults to 0 there.
    Ok(BoardData { board, lists, cards, board_seq: 0 })
}

/// Internal: update board_members.last_viewed_at for a specific (board, user) pair.
/// Scoped by both board_id AND user_id — never touches other users' rows (T-03-11).
#[cfg(feature = "ssr")]
pub async fn touch_last_viewed_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    user_id: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    // UPDATE board_members SET last_viewed_at (T-03-11 scope: per board + per user)
    sqlx::query(
        "UPDATE board_members SET last_viewed_at = ? WHERE board_id = ? AND user_id = ?"
    )
    .bind(now)
    .bind(board_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Server functions (Leptos #[server] wrappers around inner fns)
// ---------------------------------------------------------------------------

/// Fetch the full board data for the board route.
/// Returns BoardData (board + lists + card stubs) for members only (BOARD-01, T-03-10).
/// Read-only — never writes last_viewed_at (Pitfall 6); that's touch_last_viewed's job.
///
/// `board_seq` is read from the in-memory `BoardRoomRegistry` and stamped on the response
/// so the WASM client can anchor `last_seen_seq` before the first WS event arrives.
#[server]
pub async fn get_board(board_id: String) -> Result<BoardData, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-03-10, D-12: generic "board not found" for non-members)
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    let mut data = get_board_inner(&state.read_pool.0, &board_id, &user.id).await.map_err(|e| {
        if matches!(e, sqlx::Error::RowNotFound) {
            return ServerFnError::new("board not found");
        }
        tracing::error!("get_board_inner error: {e}");
        ServerFnError::new("Failed to load board")
    })?;

    // Stamp the current board_seq so the client can anchor last_seen_seq (Flag 1 resolution).
    data.board_seq = state.board_rooms.current_seq(&board_id);

    Ok(data)
}

/// Update last_viewed_at for the current user on a board (D-01).
/// Called client-side from an Effect after hydration — NEVER from SSR get_board (Pitfall 6).
/// Enforces board membership before writing (T-03-11).
#[server]
pub async fn touch_last_viewed(board_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    use crate::server::now_millis;

    let state = expect_context::<AppState>();

    // Auth + membership gate — rejects non-members before the write (T-03-11)
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    let now = now_millis().map_err(|e| {
        tracing::error!("clock error: {e}");
        ServerFnError::new("Clock error")
    })?;

    touch_last_viewed_inner(&state.write_pool.0, &board_id, &user.id, now).await.map_err(|e| {
        tracing::error!("touch_last_viewed_inner error: {e}");
        ServerFnError::new("Failed to update last viewed")
    })
}
