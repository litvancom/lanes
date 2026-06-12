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

    // The card_num allocation (UPDATE boards) and the card INSERT must be atomic.
    // Outside a transaction, an INSERT failure would leave next_card_num already
    // advanced, permanently leaking a card number in the per-board sequence (WR-02).
    let mut tx = pool.begin().await?;

    // Allocate card_num atomically: read next_card_num, increment it, return the allocated value
    // The UPDATE returns the new value; the card uses the pre-increment value (next_card_num before increment)
    let card_num: i64 = sqlx::query_scalar(
        "UPDATE boards SET next_card_num = next_card_num + 1 WHERE id = ? RETURNING next_card_num - 1"
    )
    .bind(board_id)
    .fetch_one(&mut *tx)
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
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

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

/// Move a card to a new list and position, deriving done from the target list's is_done_list flag.
///
/// Security: UPDATE is scoped by card_id AND board_id (T-04-11).
/// Position: validated before any write (T-04-09).
/// Done: server-derived from target list's is_done_list — client cannot force done=true (T-04-10).
#[cfg(feature = "ssr")]
pub async fn move_card_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    to_list_id: &str,
    new_position: &str,
) -> Result<(), sqlx::Error> {
    use fractional_index::FractionalIndex;
    use crate::server::now_millis;

    // 1. Validate position before any DB write (mirrors reorder_list_inner, T-04-09)
    FractionalIndex::from_string(new_position).map_err(|_| {
        sqlx::Error::Decode("invalid position: not a valid fractional index".into())
    })?;

    // 2. Resolve the target list's owning board AND is_done_list in a single query.
    //    The board-membership gate authorizes the caller against the client-supplied
    //    board_id, but does NOT verify the client-supplied to_list_id belongs to that
    //    board. Without this check a member of board A could move a card to a list on
    //    board B, corrupting board/list invariants (CR-01). A single row also avoids a
    //    TOCTOU window and a second round-trip. (D-14, T-04-10)
    let (target_board, target_is_done): (String, bool) = sqlx::query_as(
        "SELECT board_id, CAST(is_done_list AS BOOLEAN) FROM lists WHERE id = ?"
    )
    .bind(to_list_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| sqlx::Error::Decode("target list not found".into()))?;

    if target_board != board_id {
        return Err(sqlx::Error::Decode("target list not on this board".into()));
    }

    // 3. UPDATE scoped by id AND board_id (T-04-11: cross-board card_id matches no row)
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;
    sqlx::query(
        "UPDATE cards SET list_id = ?, position = ?, done = ?, updated_at = ? WHERE id = ? AND board_id = ?"
    )
    .bind(to_list_id)
    .bind(new_position)
    .bind(target_is_done as i64)
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Server functions (Leptos #[server] wrappers around inner fns)
// ---------------------------------------------------------------------------

/// Create a card at the end of a list.
/// Enforces board membership (T-04-04). Validates title (T-04-05).
/// Computes fractional position via next_card_position (CARD-04).
/// Allocates card_num atomically from boards.next_card_num.
///
/// `client_id`: opaque per-connection UUID for D-05 self-echo suppression (T-6-03).
#[server]
pub async fn create_card(
    board_id: String,
    list_id: String,
    title: String,
    client_id: String,
) -> Result<Card, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::api::list_api::board_id_for_list;
    use crate::server::state::AppState;
    use crate::models::events::{BoardEvent, CardSummary};

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-04-04)
    require_board_member(&board_id, &state.read_pool.0).await?;

    // Verify the target list actually belongs to the authorized board. The membership
    // gate authorizes against the client-supplied board_id but does NOT tie the
    // client-supplied list_id to that board. Without this check a member of board A
    // could create a card on a list belonging to board B (CR-02).
    let owning_board = board_id_for_list(&state.read_pool.0, &list_id).await
        .map_err(|e| {
            tracing::error!("board_id_for_list error: {e}");
            ServerFnError::new("Failed to load list")
        })?
        .ok_or_else(|| ServerFnError::new("list not found"))?;
    if owning_board != board_id {
        return Err(ServerFnError::new("list not on this board"));
    }

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
    let card = create_card_inner(&state.write_pool.0, &board_id, &list_id, title, &position)
        .await
        .map_err(|e| {
            tracing::error!("create_card_inner error: {e}");
            ServerFnError::new("Failed to create card")
        })?;

    // Publish CardAdded after successful DB write (T-6-07: publish after ? propagation).
    // CR-04: publish_seq allocates seq and sends atomically under the same entry guard.
    state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardAdded {
        board_seq: seq,
        client_id,
        card: CardSummary {
            id: card.id.clone(),
            list_id: card.list_id.clone(),
            board_id: card.board_id.clone(),
            card_num: card.card_num,
            title: card.title.clone(),
            position: card.position.clone(),
            priority: card.priority.clone(),
            due_at: card.due_at,
            done: card.done,
            cover: card.cover.clone(),
            labels: card.labels.clone(),
            member_ids: card.member_ids.clone(),
        },
    });

    Ok(card)
}

/// Move a card to a different list and/or position.
///
/// Enforces board membership first (T-04-08).
/// Re-validates position string (T-04-09 — double validation mirrors reorder_list).
/// Does NOT accept a `done` parameter — done is server-derived from is_done_list (T-04-10).
/// UPDATE scoped by card_id AND board_id (T-04-11).
/// Verifies the target list belongs to the authorized board (CR-01) before writing.
///
/// `client_id`: opaque per-connection UUID supplied by the browser (D-05/Flag 2).
/// It is stamped on the `BoardEvent::CardMoved` broadcast so the originator's own WS
/// client can suppress the highlight flash for its own move (self-echo suppression).
/// Security: T-6-03 — `client_id` is untrusted; worst case spoofing only affects
/// highlight suppression, not authorization.
///
/// Note (WR-05): the server validates the fractional position string but does NOT
/// validate that `new_position` is consistent with the target list's neighbor ordering.
/// Intra-list ordering is intentionally client-trusted (by-design for fractional
/// indexing); with the target-list/board check above, a client can only reorder within
/// lists it is authorized to mutate.
#[server]
pub async fn move_card(
    board_id: String,
    card_id: String,
    to_list_id: String,
    new_position: String,
    client_id: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    use crate::models::events::BoardEvent;

    let state = expect_context::<AppState>();

    // Auth + membership gate first (T-04-08)
    let (actor, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    // Re-validate position (T-04-09: mirrors reorder_list wrapper lines 237-242)
    {
        use fractional_index::FractionalIndex;
        FractionalIndex::from_string(&new_position)
            .map_err(|_| ServerFnError::new("invalid position"))?;
    }

    // DB write
    move_card_inner(&state.write_pool.0, &board_id, &card_id, &to_list_id, &new_position)
        .await
        .map_err(|e| {
            tracing::error!("move_card_inner error: {e}");
            ServerFnError::new("Failed to move card")
        })?;

    // Publish CardMoved event after successful DB write (Pattern 2).
    // CR-04: publish_seq allocates seq and sends atomically under the same entry guard.
    // D-05: stamp client_id so the originator's WASM client can suppress its own highlight.
    state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardMoved {
        board_seq: seq,
        client_id,
        card_id: card_id.clone(),
        to_list_id,
        position: new_position,
    });

    // D-03: watch_activity notification for watchers on card move (self-suppressed D-07).
    {
        use crate::api::notification_api::notify_watchers_inner;
        use crate::models::events::NotifEvent;

        match notify_watchers_inner(&state.write_pool.0, &card_id, &board_id, &actor.id).await {
            Ok(notified_ids) => {
                for uid in notified_ids {
                    let count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read = 0",
                    )
                    .bind(&uid)
                    .fetch_one(&state.read_pool.0)
                    .await
                    .unwrap_or(0);
                    state.user_notifs.publish(&uid, NotifEvent::UnreadCountUpdated { count });
                }
            }
            Err(e) => tracing::error!("move_card watch_activity error: {e}"),
        }
    }

    Ok(())
}
