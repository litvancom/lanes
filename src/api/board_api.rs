use leptos::prelude::*;
use crate::models::{BoardWithMeta, List, Card};
use serde::{Deserialize, Serialize};

/// Full board data returned to the board route (D-08).
/// Contains the board metadata, all non-archived lists ordered by position,
/// and all non-archived card stubs (title-bearing rows for Phase 3 rendering).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardData {
    pub board: BoardWithMeta,
    pub lists: Vec<List>,
    pub cards: Vec<Card>,
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
    let lists: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        r#"SELECT id, board_id, name, position,
                  CAST(archived AS BOOLEAN) as archived
           FROM lists
           WHERE board_id = ? AND archived = 0
           ORDER BY position ASC"#
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    let lists: Vec<List> = lists.into_iter().map(|(id, board_id, name, position, archived)| {
        List { id, board_id, name, position, archived }
    }).collect();

    // Fetch non-archived card stubs ordered by position ASC (D-08: title-only stubs)
    // All Card fields selected to match the Card model exactly.
    let cards_raw: Vec<(String, String, String, i64, String, String, Option<String>, Option<i64>, bool, bool)> =
        sqlx::query_as(
            r#"SELECT id, list_id, board_id, card_num, title, position,
                      priority, due_at,
                      CAST(done AS BOOLEAN) as done,
                      CAST(archived AS BOOLEAN) as archived
               FROM cards
               WHERE board_id = ? AND archived = 0
               ORDER BY position ASC"#
        )
        .bind(board_id)
        .fetch_all(pool)
        .await?;

    let cards: Vec<Card> = cards_raw.into_iter().map(|(id, list_id, board_id, card_num, title, position, priority, due_at, done, archived)| {
        Card { id, list_id, board_id, card_num, title, position, priority, due_at, done, archived }
    }).collect();

    Ok(BoardData { board, lists, cards })
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
#[server]
pub async fn get_board(board_id: String) -> Result<BoardData, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-03-10, D-12: generic "board not found" for non-members)
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    get_board_inner(&state.read_pool.0, &board_id, &user.id).await.map_err(|e| {
        if matches!(e, sqlx::Error::RowNotFound) {
            return ServerFnError::new("board not found");
        }
        tracing::error!("get_board_inner error: {e}");
        ServerFnError::new("Failed to load board")
    })
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
