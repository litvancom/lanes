use leptos::prelude::*;
use crate::models::List;

/// Resolve the board_id for a given list_id.
/// Returns None if the list does not exist.
/// Used to enforce board membership before list mutations (T-03-07).
#[cfg(feature = "ssr")]
pub async fn board_id_for_list(
    pool: &sqlx::SqlitePool,
    list_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT board_id FROM lists WHERE id = ?"
    )
    .bind(list_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(b,)| b))
}

/// Compute the fractional position for the next list appended to a board.
/// Queries the current maximum position for the board (non-archived), then
/// returns new_after that position. If no lists exist, returns FractionalIndex::default().
#[cfg(feature = "ssr")]
pub async fn next_list_position(
    pool: &sqlx::SqlitePool,
    board_id: &str,
) -> Result<String, sqlx::Error> {
    use fractional_index::FractionalIndex;

    let max_pos: Option<String> = sqlx::query_scalar(
        "SELECT position FROM lists WHERE board_id = ? AND archived = 0 ORDER BY position DESC LIMIT 1"
    )
    .bind(board_id)
    .fetch_optional(pool)
    .await?;

    let pos = match max_pos {
        None => FractionalIndex::default(),
        Some(p) => {
            let fi = FractionalIndex::from_string(&p).map_err(|e| {
                sqlx::Error::Decode(format!("invalid fractional index in DB: {e}").into())
            })?;
            FractionalIndex::new_after(&fi)
        }
    };
    Ok(pos.to_string())
}

/// Internal: validate name and insert a new list row.
/// Validation: trim, reject empty, reject > 120 chars (T-03-09 Tampering mitigation).
/// Returns the created List.
#[cfg(feature = "ssr")]
pub async fn create_list_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    name: String,
    position: &str,
) -> Result<List, sqlx::Error> {
    use uuid::Uuid;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(sqlx::Error::Decode("List name cannot be empty".into()));
    }
    if name.chars().count() > 120 {
        return Err(sqlx::Error::Decode("List name must be 120 characters or fewer".into()));
    }

    let id = Uuid::now_v7().to_string();

    sqlx::query(
        "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)"
    )
    .bind(&id)
    .bind(board_id)
    .bind(&name)
    .bind(position)
    .execute(pool)
    .await?;

    Ok(List {
        id,
        board_id: board_id.to_string(),
        name,
        position: position.to_string(),
        archived: false,
        is_done_list: false,
    })
}

/// Internal: update a list's name.
/// Validation: trim, reject empty, reject > 120 chars (T-03-09).
#[cfg(feature = "ssr")]
pub async fn rename_list_inner(
    pool: &sqlx::SqlitePool,
    list_id: &str,
    name: String,
) -> Result<(), sqlx::Error> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(sqlx::Error::Decode("List name cannot be empty".into()));
    }
    if name.chars().count() > 120 {
        return Err(sqlx::Error::Decode("List name must be 120 characters or fewer".into()));
    }

    sqlx::query("UPDATE lists SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(list_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Internal: update a list's fractional position.
/// Validates the position string before writing (T-03-08 Tampering mitigation).
/// Returns Err if the position is not a valid FractionalIndex.
#[cfg(feature = "ssr")]
pub async fn reorder_list_inner(
    pool: &sqlx::SqlitePool,
    list_id: &str,
    new_position: String,
) -> Result<(), sqlx::Error> {
    use fractional_index::FractionalIndex;

    // Validate position before any DB write (T-03-08)
    FractionalIndex::from_string(&new_position).map_err(|_| {
        sqlx::Error::Decode("invalid position: not a valid fractional index".into())
    })?;

    // UPDATE lists SET position (verified pattern from PLAN.md key_links)
    sqlx::query("UPDATE lists SET position = ? WHERE id = ?")
        .bind(&new_position)
        .bind(list_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Server functions (Leptos #[server] wrappers around inner fns)
// ---------------------------------------------------------------------------

/// Create a list at the end of a board's list sequence.
/// Enforces board membership (T-03-07). Validates name (T-03-09).
/// Computes fractional position via next_list_position (BOARD-03).
/// `client_id`: opaque per-connection UUID for D-05 self-echo suppression (T-6-03).
#[server]
pub async fn create_list(board_id: String, name: String, client_id: String) -> Result<List, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    use crate::models::events::BoardEvent;

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-03-07)
    require_board_member(&board_id, &state.read_pool.0).await?;

    // Validate name before computing position (fail fast)
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("List name cannot be empty"));
    }
    if name.chars().count() > 120 {
        return Err(ServerFnError::new("List name must be 120 characters or fewer"));
    }

    // Compute append position on read pool (SELECT only)
    let position = next_list_position(&state.read_pool.0, &board_id).await.map_err(|e| {
        tracing::error!("next_list_position error: {e}");
        ServerFnError::new("Failed to compute list position")
    })?;

    // Insert on write pool
    let list = create_list_inner(&state.write_pool.0, &board_id, name, &position).await.map_err(|e| {
        tracing::error!("create_list_inner error: {e}");
        ServerFnError::new("Failed to create list")
    })?;

    // Publish ListAdded after successful DB write (T-6-07).
    let seq = state.board_rooms.next_seq(&board_id);
    state.board_rooms.publish(
        &board_id,
        BoardEvent::ListAdded {
            board_seq: seq,
            client_id,
            list_id: list.id.clone(),
            name: list.name.clone(),
            position: list.position.clone(),
        },
    );

    Ok(list)
}

/// Rename a list. Resolves board_id from the list, then enforces board membership.
/// Validates the new name (trim, reject empty, reject > 120 chars) (BOARD-04, T-03-09).
/// `client_id`: opaque per-connection UUID for D-05 self-echo suppression (T-6-03).
#[server]
pub async fn rename_list(list_id: String, name: String, client_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    use crate::models::events::BoardEvent;

    let state = expect_context::<AppState>();

    // Resolve board_id from list_id (need to auth against the board)
    let board_id = board_id_for_list(&state.read_pool.0, &list_id).await
        .map_err(|e| {
            tracing::error!("board_id_for_list error: {e}");
            ServerFnError::new("Failed to load list")
        })?
        .ok_or_else(|| ServerFnError::new("list not found"))?;

    // Auth + membership gate (T-03-07)
    require_board_member(&board_id, &state.read_pool.0).await?;

    // Validate name (fail fast before write)
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("List name cannot be empty"));
    }
    if name.chars().count() > 120 {
        return Err(ServerFnError::new("List name must be 120 characters or fewer"));
    }

    rename_list_inner(&state.write_pool.0, &list_id, name.clone()).await.map_err(|e| {
        tracing::error!("rename_list_inner error: {e}");
        ServerFnError::new("Failed to rename list")
    })?;

    // Publish ListRenamed after successful DB write (T-6-07).
    let seq = state.board_rooms.next_seq(&board_id);
    state.board_rooms.publish(
        &board_id,
        BoardEvent::ListRenamed {
            board_seq: seq,
            client_id,
            list_id,
            name,
        },
    );

    Ok(())
}

/// Reorder a list by persisting a client-computed fractional position.
/// The position midpoint is computed client-side (Wave 2 DnD / Phase 3 Move left/right);
/// this fn validates and persists it (BOARD-05, T-03-08, D-15).
/// `client_id`: opaque per-connection UUID for D-05 self-echo suppression (T-6-03).
#[server]
pub async fn reorder_list(list_id: String, new_position: String, client_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    use crate::models::events::BoardEvent;

    let state = expect_context::<AppState>();

    // Resolve board_id for membership check
    let board_id = board_id_for_list(&state.read_pool.0, &list_id).await
        .map_err(|e| {
            tracing::error!("board_id_for_list error: {e}");
            ServerFnError::new("Failed to load list")
        })?
        .ok_or_else(|| ServerFnError::new("list not found"))?;

    // Auth + membership gate (T-03-07)
    require_board_member(&board_id, &state.read_pool.0).await?;

    // Validate position string (T-03-08): reject undecodable fractional index
    {
        use fractional_index::FractionalIndex;
        FractionalIndex::from_string(&new_position)
            .map_err(|_| ServerFnError::new("invalid position"))?;
    }

    reorder_list_inner(&state.write_pool.0, &list_id, new_position.clone()).await.map_err(|e| {
        tracing::error!("reorder_list_inner error: {e}");
        ServerFnError::new("Failed to reorder list")
    })?;

    // Publish ListReordered after successful DB write (T-6-07).
    let seq = state.board_rooms.next_seq(&board_id);
    state.board_rooms.publish(
        &board_id,
        BoardEvent::ListReordered {
            board_seq: seq,
            client_id,
            list_id,
            position: new_position,
        },
    );

    Ok(())
}
