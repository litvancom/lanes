use leptos::prelude::*;
use crate::models::Card;

/// Compute the fractional position for the next card appended to a list.
/// Queries the current maximum position for the list (non-archived), then
/// returns new_after that position. If no cards exist, returns FractionalIndex::default().
#[cfg(feature = "ssr")]
pub async fn next_card_position(
    pool: &sqlx::SqlitePool,
    list_id: &str,
) -> Result<String, sqlx::Error> {
    use fractional_index::FractionalIndex;

    let max_pos: Option<String> = sqlx::query_scalar(
        "SELECT position FROM cards WHERE list_id = ? AND archived = 0 ORDER BY position DESC LIMIT 1"
    )
    .bind(list_id)
    .fetch_optional(pool)
    .await?;

    let pos = match max_pos {
        None => FractionalIndex::default(),
        Some(p) => {
            FractionalIndex::from_string(&p)
                .map(|fi| FractionalIndex::new_after(&fi))
                .unwrap_or_default()
        }
    };
    Ok(pos.to_string())
}

/// Internal: validate title and insert a new card row.
/// Validation: trim, reject empty, reject > 500 chars (T-04-05 DoS mitigation).
/// Allocates `card_num` atomically from `boards.next_card_num`.
/// Returns the created Card.
#[cfg(feature = "ssr")]
pub async fn create_card_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    list_id: &str,
    title: String,
    position: &str,
) -> Result<Card, sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;

    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(sqlx::Error::Decode("Card title cannot be empty".into()));
    }
    if title.chars().count() > 500 {
        return Err(sqlx::Error::Decode("Card title must be 500 characters or fewer".into()));
    }

    let id = Uuid::now_v7().to_string();
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    // Allocate card_num atomically: read next_card_num, increment it, return the allocated value
    // The UPDATE returns the new value; the card uses the pre-increment value (next_card_num before increment)
    let card_num: i64 = sqlx::query_scalar(
        "UPDATE boards SET next_card_num = next_card_num + 1 WHERE id = ? RETURNING next_card_num - 1"
    )
    .bind(board_id)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, position,
         priority, due_at, cover, done, archived, checklist_done, checklist_total,
         comment_count, attachment_count, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, NULL, 0, 0, 0, 0, 0, 0, ?, ?)"
    )
    .bind(&id)
    .bind(list_id)
    .bind(board_id)
    .bind(card_num)
    .bind(&title)
    .bind(position)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(Card {
        id,
        list_id: list_id.to_string(),
        board_id: board_id.to_string(),
        card_num,
        title,
        position: position.to_string(),
        priority: None,
        due_at: None,
        done: false,
        archived: false,
        cover: None,
        labels: Vec::new(),
        checklist_done: 0,
        checklist_total: 0,
        comment_count: 0,
        attachment_count: 0,
        member_ids: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Server functions (Leptos #[server] wrappers around inner fns)
// ---------------------------------------------------------------------------

/// Create a card at the end of a list.
/// Enforces board membership (T-04-04). Validates title (T-04-05).
/// Computes fractional position via next_card_position (CARD-04).
/// Allocates card_num atomically from boards.next_card_num.
#[server]
pub async fn create_card(
    board_id: String,
    list_id: String,
    title: String,
) -> Result<Card, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-04-04)
    require_board_member(&board_id, &state.read_pool.0).await?;

    // Validate title before computing position (fail fast)
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ServerFnError::new("Card title cannot be empty"));
    }
    if title.chars().count() > 500 {
        return Err(ServerFnError::new("Card title must be 500 characters or fewer"));
    }

    // Compute append position on read pool (SELECT only)
    let position = next_card_position(&state.read_pool.0, &list_id).await.map_err(|e| {
        tracing::error!("next_card_position error: {e}");
        ServerFnError::new("Failed to compute card position")
    })?;

    // Insert on write pool
    create_card_inner(&state.write_pool.0, &board_id, &list_id, title, &position)
        .await
        .map_err(|e| {
            tracing::error!("create_card_inner error: {e}");
            ServerFnError::new("Failed to create card")
        })
}
