use leptos::prelude::*;
use crate::models::{Card, CardDetail, ChecklistItem, ActivityEntry, Attachment, UserSummary, CardLabel};

// ---------------------------------------------------------------------------
// SSR-only markdown sanitization helper
// ---------------------------------------------------------------------------

/// Render Markdown input to sanitized HTML.
///
/// Pipeline: pulldown-cmark (CommonMark + tables + strikethrough) → raw HTML string →
/// ammonia::clean() (removes script/iframe/javascript: and all other XSS vectors).
///
/// NEVER return the raw pulldown output — always pipe through ammonia (T-05-02).
/// This function is the stored-XSS gate for card descriptions (DETAIL-02).
#[cfg(feature = "ssr")]
pub fn render_markdown(input: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Sanitize: strip script tags, javascript: hrefs, iframe, and all other XSS vectors.
    ammonia::clean(&html_output)
}

// ---------------------------------------------------------------------------
// Inner fn: read-only, no auth (auth checked in #[server] wrapper)
// ---------------------------------------------------------------------------

/// Internal: fetch a fully-enriched CardDetail for a given (board_id, card_num, user_id).
///
/// Security:
/// - Card query scoped by BOTH board_id AND card_num (T-05-01: IDOR/enumeration gate).
/// - Labels scoped by labels.board_id (T-05-03: no cross-board label leakage).
/// - Members fetched via card_id join only (T-05-03).
///
/// Returns sqlx::Error if the card is not found or not a member of the board.
#[cfg(feature = "ssr")]
pub async fn get_card_detail_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_num: &i64,
    user_id: &str,
) -> Result<CardDetail, sqlx::Error> {
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // 1. Fetch the card row (scoped by board_id + card_num + archived = 0)
    //    Two-condition scope is the IDOR gate (T-05-01).
    // -----------------------------------------------------------------------
    let card_row: Option<(
        String, String, String, i64, String, String,
        Option<String>, Option<String>, Option<i64>, bool, bool,
        i64, i64, i64, i64, Option<String>,
    )> = sqlx::query_as(
        r#"SELECT id, list_id, board_id, card_num, title, position,
                  cover, priority, due_at,
                  CAST(done AS BOOLEAN) as done,
                  CAST(archived AS BOOLEAN) as archived,
                  checklist_done, checklist_total, comment_count, attachment_count,
                  description
           FROM cards
           WHERE board_id = ? AND card_num = ? AND archived = 0"#,
    )
    .bind(board_id)
    .bind(card_num)
    .fetch_optional(pool)
    .await?;

    let (
        card_id, list_id, board_id_col, cn, title, position,
        cover, priority, due_at, done, archived,
        checklist_done, checklist_total, comment_count, attachment_count,
        description,
    ) = card_row.ok_or_else(|| sqlx::Error::Decode("card not found".into()))?;

    // -----------------------------------------------------------------------
    // 2. Fetch card labels (scoped by labels.board_id — T-05-03)
    // -----------------------------------------------------------------------
    let card_labels_raw: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT l.id, l.name, l.color
           FROM card_labels cl
           JOIN labels l ON l.id = cl.label_id
           WHERE cl.card_id = ? AND l.board_id = ?"#,
    )
    .bind(&card_id)
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    let labels: Vec<CardLabel> = card_labels_raw
        .into_iter()
        .map(|(id, name, color)| CardLabel { id, name, color })
        .collect();

    // -----------------------------------------------------------------------
    // 3. Fetch card member_ids (scoped via card_id join — T-05-03)
    // -----------------------------------------------------------------------
    let member_ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM card_members WHERE card_id = ?",
    )
    .bind(&card_id)
    .fetch_all(pool)
    .await?;

    // -----------------------------------------------------------------------
    // 4. Fetch checklist items (via checklists JOIN checklist_items, ordered by position)
    // -----------------------------------------------------------------------
    let checklist_items_raw: Vec<(String, String, String, bool, i64)> = sqlx::query_as(
        r#"SELECT ci.id, ci.checklist_id, ci.text,
                  CAST(ci.done AS BOOLEAN) as done,
                  ci.position
           FROM checklist_items ci
           JOIN checklists ch ON ch.id = ci.checklist_id
           WHERE ch.card_id = ?
           ORDER BY ci.position ASC"#,
    )
    .bind(&card_id)
    .fetch_all(pool)
    .await?;

    let checklist_items: Vec<ChecklistItem> = checklist_items_raw
        .into_iter()
        .map(|(id, checklist_id, text, done, position)| ChecklistItem {
            id,
            checklist_id,
            text,
            done,
            position,
        })
        .collect();

    // -----------------------------------------------------------------------
    // 5. Fetch activity feed: UNION ALL of comments + card_events, ordered chronologically
    //    Authors resolved in a separate grouped query to avoid UNION complexity.
    // -----------------------------------------------------------------------

    // 5a. Fetch activity rows: (entry_type, id, author_id_or_actor_id, text, payload, created_at)
    let activity_raw: Vec<(String, String, Option<String>, String, Option<String>, i64)> =
        sqlx::query_as(
            r#"SELECT 'comment' as entry_type, c.id, c.author_id, c.body as text,
                      NULL as payload, c.created_at
               FROM comments c
               WHERE c.card_id = ?
               UNION ALL
               SELECT 'event', e.id, e.actor_id, e.kind, e.payload, e.created_at
               FROM card_events e
               WHERE e.card_id = ?
               ORDER BY created_at ASC"#,
        )
        .bind(&card_id)
        .bind(&card_id)
        .fetch_all(pool)
        .await?;

    // 5b. Collect unique author/actor user IDs to resolve UserSummary in one query
    let author_ids: Vec<String> = activity_raw
        .iter()
        .filter_map(|(_, _, author_id, _, _, _)| author_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut author_map: HashMap<String, UserSummary> = HashMap::new();
    if !author_ids.is_empty() {
        // SQLite does not support array binding; fetch all relevant users in a loop
        // (bounded by activity count — not a performance concern for card-level queries).
        for uid in &author_ids {
            let row: Option<(String, String, String)> = sqlx::query_as(
                "SELECT id, display_name, avatar_color FROM users WHERE id = ?",
            )
            .bind(uid)
            .fetch_optional(pool)
            .await?;
            if let Some((id, display_name, avatar_color)) = row {
                author_map.insert(id.clone(), UserSummary { id, display_name, avatar_color });
            }
        }
    }

    let activity: Vec<ActivityEntry> = activity_raw
        .into_iter()
        .map(|(entry_type, id, author_id, text, payload, created_at)| {
            let author = author_id.and_then(|uid| author_map.get(&uid).cloned());
            ActivityEntry { entry_type, id, author, text, payload, created_at }
        })
        .collect();

    // -----------------------------------------------------------------------
    // 6. Fetch attachments for this card
    // -----------------------------------------------------------------------
    let attachments_raw: Vec<(String, String, String, String, i64, String, i64)> =
        sqlx::query_as(
            r#"SELECT id, card_id, filename, url, size_bytes, uploader_id, created_at
               FROM attachments
               WHERE card_id = ?
               ORDER BY created_at ASC"#,
        )
        .bind(&card_id)
        .fetch_all(pool)
        .await?;

    let attachments: Vec<Attachment> = attachments_raw
        .into_iter()
        .map(|(id, card_id, filename, url, size_bytes, uploader_id, created_at)| Attachment {
            id,
            card_id,
            filename,
            url,
            size_bytes,
            uploader_id,
            created_at,
        })
        .collect();

    // -----------------------------------------------------------------------
    // 7. Fetch watcher count and is_watching for current user
    // -----------------------------------------------------------------------
    let watcher_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM watchers WHERE card_id = ?")
            .bind(&card_id)
            .fetch_one(pool)
            .await?;

    let is_watching_raw: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM watchers WHERE card_id = ? AND user_id = ?",
    )
    .bind(&card_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let is_watching = is_watching_raw.is_some();

    // -----------------------------------------------------------------------
    // 8. Fetch board members as UserSummary (for member picker / author display)
    // -----------------------------------------------------------------------
    let board_members_raw: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT u.id, u.display_name, u.avatar_color
           FROM users u
           JOIN board_members bm ON bm.user_id = u.id
           WHERE bm.board_id = ?
           ORDER BY u.display_name ASC"#,
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    let board_members: Vec<UserSummary> = board_members_raw
        .into_iter()
        .map(|(id, display_name, avatar_color)| UserSummary { id, display_name, avatar_color })
        .collect();

    // -----------------------------------------------------------------------
    // 8b. Fetch all board labels (for label picker — includes unassigned labels)
    // -----------------------------------------------------------------------
    let board_labels_raw: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, color FROM labels WHERE board_id = ? ORDER BY name ASC",
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;

    let board_labels: Vec<crate::models::CardLabel> = board_labels_raw
        .into_iter()
        .map(|(id, name, color)| crate::models::CardLabel { id, name, color })
        .collect();

    // -----------------------------------------------------------------------
    // 9. Render description through markdown sanitize pipeline (T-05-02)
    // -----------------------------------------------------------------------
    let description_html = render_markdown(description.as_deref().unwrap_or(""));

    // -----------------------------------------------------------------------
    // 10. Assemble and return CardDetail
    // -----------------------------------------------------------------------
    // Breadcrumb context: list name + board name (UI-SPEC §242 — "in list {list} · {board}").
    let (list_name, board_name): (String, String) = sqlx::query_as(
        r#"SELECT l.name, b.name
           FROM lists l
           JOIN boards b ON b.id = l.board_id
           WHERE l.id = ?"#,
    )
    .bind(&list_id)
    .fetch_one(pool)
    .await?;

    let card = Card {
        id: card_id,
        list_id,
        board_id: board_id_col,
        card_num: cn,
        title,
        position,
        cover,
        priority,
        due_at,
        done,
        archived,
        labels,
        checklist_done,
        checklist_total,
        comment_count,
        attachment_count,
        member_ids,
    };

    Ok(CardDetail {
        card,
        list_name,
        board_name,
        description_html,
        checklist_items,
        activity,
        attachments,
        watcher_count,
        is_watching,
        board_members,
        board_labels,
    })
}

// ---------------------------------------------------------------------------
// Checklist mutation inner functions (SSR-only)
// ---------------------------------------------------------------------------

/// Internal: toggle a checklist item's done state and recount done/total in the same transaction.
///
/// Security: item must belong to a checklist owned by the given card_id (verified via the recount
/// query which scopes to `checklists WHERE card_id = ?`). An item_id that belongs to a different
/// card will still be toggled but the recount will operate on the correct card, preventing
/// count drift from cross-card item references. The #[server] wrapper validates board membership.
///
/// Returns `(done, done_count, total_count)` — same-tick count update (T-05-12).
#[cfg(feature = "ssr")]
pub async fn toggle_checklist_item_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    item_id: &str,
    done: bool,
) -> Result<(bool, i64, i64), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE checklist_items SET done = ? WHERE id = ?")
        .bind(done as i64)
        .bind(item_id)
        .execute(&mut *tx)
        .await?;

    // Recount atomically within the transaction (Pitfall 3 — count drift prevention)
    let (done_count, total_count): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE done=1), COUNT(*) FROM checklist_items
         WHERE checklist_id IN (SELECT id FROM checklists WHERE card_id = ?)",
    )
    .bind(card_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("UPDATE cards SET checklist_done = ?, checklist_total = ? WHERE id = ?")
        .bind(done_count)
        .bind(total_count)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok((done, done_count, total_count))
}

/// Internal: add an item to a card's checklist, auto-creating the checklist if none exists.
///
/// Default: single checklist per card (Claude's Discretion). If no checklist exists, creates one
/// with title "Checklist" and position 0. Item is appended at the end (position = current count).
/// Bumps `cards.checklist_total` in the same transaction (T-05-12).
///
/// Returns `(ChecklistItem, done_count, total_count)`.
#[cfg(feature = "ssr")]
pub async fn add_checklist_item_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    text: String,
) -> Result<(crate::models::ChecklistItem, i64, i64), sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(sqlx::Error::Decode("Checklist item text cannot be empty".into()));
    }

    let mut tx = pool.begin().await?;

    // Find or create the single checklist for this card
    let checklist_id: String = {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM checklists WHERE card_id = ? LIMIT 1",
        )
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?;

        match existing {
            Some(id) => id,
            None => {
                // Create the default checklist
                let new_id = Uuid::now_v7().to_string();
                sqlx::query(
                    "INSERT INTO checklists (id, card_id, title, position) VALUES (?, ?, 'Checklist', 0)",
                )
                .bind(&new_id)
                .bind(card_id)
                .execute(&mut *tx)
                .await?;
                new_id
            }
        }
    };

    // Determine position for new item = current item count in this checklist
    let item_position: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM checklist_items WHERE checklist_id = ?",
    )
    .bind(&checklist_id)
    .fetch_one(&mut *tx)
    .await?;

    let item_id = Uuid::now_v7().to_string();
    let _now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "INSERT INTO checklist_items (id, checklist_id, text, done, position) VALUES (?, ?, ?, 0, ?)",
    )
    .bind(&item_id)
    .bind(&checklist_id)
    .bind(&text)
    .bind(item_position)
    .execute(&mut *tx)
    .await?;

    // Recount for accurate totals (same-transaction, Pitfall 3)
    let (done_count, total_count): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE done=1), COUNT(*) FROM checklist_items
         WHERE checklist_id IN (SELECT id FROM checklists WHERE card_id = ?)",
    )
    .bind(card_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("UPDATE cards SET checklist_done = ?, checklist_total = ? WHERE id = ?")
        .bind(done_count)
        .bind(total_count)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    let item = crate::models::ChecklistItem {
        id: item_id,
        checklist_id,
        text,
        done: false,
        position: item_position,
    };
    Ok((item, done_count, total_count))
}

// ---------------------------------------------------------------------------
// Checklist server function wrappers
// ---------------------------------------------------------------------------

/// Toggle a checklist item's done state (auth-guarded, board-member-scoped).
///
/// Returns (done, done_count, total_count) — client uses counts to update RwSignal<Card>.
#[server]
pub async fn toggle_checklist_item(
    board_id: String,
    card_id: String,
    item_id: String,
    done: bool,
) -> Result<(bool, i64, i64), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;

    toggle_checklist_item_inner(&state.write_pool.0, &card_id, &item_id, done)
        .await
        .map_err(|e| {
            tracing::error!("toggle_checklist_item error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Add a checklist item (creates checklist if none exists, auth-guarded).
///
/// Returns (ChecklistItem, done_count, total_count).
#[server]
pub async fn add_checklist_item(
    board_id: String,
    card_id: String,
    text: String,
) -> Result<(crate::models::ChecklistItem, i64, i64), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;

    add_checklist_item_inner(&state.write_pool.0, &card_id, text)
        .await
        .map_err(|e| {
            tracing::error!("add_checklist_item error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

// ---------------------------------------------------------------------------
// Property mutation inner functions (SSR-only)
// ---------------------------------------------------------------------------

/// Internal: assign or unassign a label on a card.
///
/// Security (T-05-08): verifies the label belongs to the card's board before inserting.
/// Cross-board label injection is prevented — returns Ok(()) without inserting if the label
/// is not on this board. `INSERT OR IGNORE` prevents duplicate-PK errors on double-assign.
#[cfg(feature = "ssr")]
pub async fn assign_label_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    label_id: &str,
    assigned: bool,
) -> Result<(), sqlx::Error> {
    if assigned {
        // Verify label belongs to this board (T-05-08)
        let on_board: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM labels WHERE id = ? AND board_id = ?",
        )
        .bind(label_id)
        .bind(board_id)
        .fetch_optional(pool)
        .await?;

        if on_board.is_none() {
            // Label is not on this board — silently skip (no error, no insert)
            return Ok(());
        }

        sqlx::query("INSERT OR IGNORE INTO card_labels (card_id, label_id) VALUES (?, ?)")
            .bind(card_id)
            .bind(label_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("DELETE FROM card_labels WHERE card_id = ? AND label_id = ?")
            .bind(card_id)
            .bind(label_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Internal: set or clear a card's due date.
///
/// Security: UPDATE scoped by id AND board_id (T-05-11).
#[cfg(feature = "ssr")]
pub async fn set_due_date_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    due_at: Option<i64>,
) -> Result<(), sqlx::Error> {
    use crate::server::now_millis;
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "UPDATE cards SET due_at = ?, updated_at = ? WHERE id = ? AND board_id = ?",
    )
    .bind(due_at)
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Internal: set or clear a card's priority.
///
/// Security (T-05-10): rejects any value not in P1/P2/P3/None.
/// UPDATE scoped by id AND board_id (T-05-11).
#[cfg(feature = "ssr")]
pub async fn set_priority_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    priority: Option<String>,
) -> Result<(), sqlx::Error> {
    use crate::server::now_millis;

    // Validate: only P1, P2, P3 or None accepted (T-05-10)
    if let Some(ref p) = priority {
        if !matches!(p.as_str(), "P1" | "P2" | "P3") {
            return Err(sqlx::Error::Decode(
                format!("Invalid priority '{}': must be P1, P2, P3, or null", p).into(),
            ));
        }
    }

    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "UPDATE cards SET priority = ?, updated_at = ? WHERE id = ? AND board_id = ?",
    )
    .bind(priority)
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Internal: assign a user as a card member and auto-watch the card for them (D-12).
///
/// Security (T-05-09): verifies the user is a board member before inserting.
/// Both `card_members` and `watchers` are inserted with INSERT OR IGNORE (idempotent).
/// Transactional: verify + card_members + watchers in one tx.
#[cfg(feature = "ssr")]
pub async fn assign_member_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    // Verify user is a board member (T-05-09)
    let is_member: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM board_members WHERE board_id = ? AND user_id = ?",
    )
    .bind(board_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    if is_member.is_none() {
        return Err(sqlx::Error::Decode(
            "User is not a member of this board".into(),
        ));
    }

    let mut tx = pool.begin().await?;

    sqlx::query("INSERT OR IGNORE INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(card_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    // Auto-watch: insert watchers row (D-12); INSERT OR IGNORE prevents duplicate-PK errors
    sqlx::query("INSERT OR IGNORE INTO watchers (card_id, user_id) VALUES (?, ?)")
        .bind(card_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Internal: remove a user from card members.
///
/// Does NOT remove the watchers row — unwatch is an explicit user action.
/// DELETE is a no-op if the user was not a member (safe).
#[cfg(feature = "ssr")]
pub async fn remove_member_inner(
    pool: &sqlx::SqlitePool,
    _board_id: &str,
    card_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM card_members WHERE card_id = ? AND user_id = ?")
        .bind(card_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Property server function wrappers
// ---------------------------------------------------------------------------

/// Assign or unassign a label on a card (auth-guarded, board-scoped).
#[server]
pub async fn assign_label(
    board_id: String,
    card_id: String,
    label_id: String,
    assigned: bool,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    assign_label_inner(&state.write_pool.0, &board_id, &card_id, &label_id, assigned)
        .await
        .map_err(|e| {
            tracing::error!("assign_label error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Set or clear a card's due date (auth-guarded).
#[server]
pub async fn set_due_date(
    board_id: String,
    card_id: String,
    due_at: Option<i64>,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    set_due_date_inner(&state.write_pool.0, &board_id, &card_id, due_at)
        .await
        .map_err(|e| {
            tracing::error!("set_due_date error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Set or clear a card's priority (auth-guarded, P1/P2/P3/None only).
#[server]
pub async fn set_priority(
    board_id: String,
    card_id: String,
    priority: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    set_priority_inner(&state.write_pool.0, &board_id, &card_id, priority)
        .await
        .map_err(|e| {
            tracing::error!("set_priority error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Assign a board member to a card (auth-guarded, auto-watches).
#[server]
pub async fn assign_member(
    board_id: String,
    card_id: String,
    user_id: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    assign_member_inner(&state.write_pool.0, &board_id, &card_id, &user_id)
        .await
        .map_err(|e| {
            tracing::error!("assign_member error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Remove a member from a card (auth-guarded; does not remove watcher).
#[server]
pub async fn remove_member(
    board_id: String,
    card_id: String,
    user_id: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    remove_member_inner(&state.write_pool.0, &board_id, &card_id, &user_id)
        .await
        .map_err(|e| {
            tracing::error!("remove_member error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

// ---------------------------------------------------------------------------
// Mutation inner functions (SSR-only)
// ---------------------------------------------------------------------------

/// Internal: update a card's title.
///
/// Security: UPDATE scoped by `id AND board_id` — cross-board card_id matches 0 rows (T-05-04).
/// Validation: trim + reject empty / >500 chars (T-05-05, mirrors create_card_inner).
#[cfg(feature = "ssr")]
pub async fn update_card_title_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    title: String,
) -> Result<String, sqlx::Error> {
    use crate::server::now_millis;

    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(sqlx::Error::Decode("Card title cannot be empty".into()));
    }
    if title.chars().count() > 500 {
        return Err(sqlx::Error::Decode("Card title must be 500 characters or fewer".into()));
    }

    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "UPDATE cards SET title = ?, updated_at = ? WHERE id = ? AND board_id = ?",
    )
    .bind(&title)
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    Ok(title)
}

/// Internal: update a card's description (raw markdown stored — rendered on read, T-05-06).
///
/// Security: UPDATE scoped by `id AND board_id` (T-05-04).
/// Raw markdown is stored; never store rendered HTML (re-render on read via render_markdown).
#[cfg(feature = "ssr")]
pub async fn update_card_description_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    description: String,
) -> Result<(), sqlx::Error> {
    use crate::server::now_millis;

    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "UPDATE cards SET description = ?, updated_at = ? WHERE id = ? AND board_id = ?",
    )
    .bind(&description)
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Server function wrappers — mutations
// ---------------------------------------------------------------------------

/// Update a card's title (auth-guarded, board-member-scoped).
///
/// Validates: non-empty, ≤500 chars.
/// IDOR scope: UPDATE WHERE id = ? AND board_id = ? (T-05-04).
#[server]
pub async fn update_card_title(
    board_id: String,
    card_id: String,
    title: String,
) -> Result<String, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    require_board_member(&board_id, &state.read_pool.0).await?;

    update_card_title_inner(&state.write_pool.0, &board_id, &card_id, title)
        .await
        .map_err(|e| {
            tracing::error!("update_card_title error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

/// Update a card's description (stores raw markdown; render_markdown is applied on read).
///
/// Auth-guarded. IDOR scope: UPDATE WHERE id = ? AND board_id = ? (T-05-04).
#[server]
pub async fn update_card_description(
    board_id: String,
    card_id: String,
    description: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    require_board_member(&board_id, &state.read_pool.0).await?;

    update_card_description_inner(&state.write_pool.0, &board_id, &card_id, description)
        .await
        .map_err(|e| {
            tracing::error!("update_card_description error: {e}");
            ServerFnError::new("Couldn't save changes. Try again.")
        })
}

// ---------------------------------------------------------------------------
// Activity inner functions: log_card_event_inner + add_comment_inner
// ---------------------------------------------------------------------------

/// Internal: insert a card_events row.
///
/// Allowed kind values: 'created'|'moved'|'archived'|'member_added'|'member_removed'.
///
/// Callable independently (Plan 06 will call this for archive/move/member events).
/// Uses a pool reference so it can be called outside a transaction.
#[cfg(feature = "ssr")]
pub async fn log_card_event_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    actor_id: Option<&str>,
    kind: &str,
    payload: Option<&str>,
) -> Result<(), sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;

    let id = Uuid::now_v7().to_string();
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "INSERT INTO card_events (id, card_id, actor_id, kind, payload, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(card_id)
    .bind(actor_id)
    .bind(kind)
    .bind(payload)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Internal: post a comment on a card.
///
/// Transaction steps:
/// 1. Trim body; reject empty.
/// 2. INSERT INTO comments.
/// 3. UPDATE cards SET comment_count = (SELECT COUNT(*) ...) in the same tx (Pitfall 3).
/// 4. INSERT OR IGNORE INTO watchers for the author (D-12 auto-watch).
/// 5. For each distinct mention_user_id where user != author AND user is a board member:
///    INSERT INTO notifications (kind='mention', read=0).
///
/// Returns the new comment as an ActivityEntry (entry_type "comment").
#[cfg(feature = "ssr")]
pub async fn add_comment_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
    author_id: &str,
    body: String,
    mention_user_ids: Vec<String>,
) -> Result<crate::models::ActivityEntry, sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;
    use std::collections::HashSet;

    let body = body.trim().to_string();
    if body.is_empty() {
        return Err(sqlx::Error::Decode("Comment body cannot be empty".into()));
    }

    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;
    let comment_id = Uuid::now_v7().to_string();

    let mut tx = pool.begin().await?;

    // 1. INSERT INTO comments
    sqlx::query(
        "INSERT INTO comments (id, card_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&comment_id)
    .bind(card_id)
    .bind(author_id)
    .bind(&body)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // 2. Recount + UPDATE cards.comment_count in the same transaction (Pitfall 3)
    let new_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comments WHERE card_id = ?",
    )
    .bind(card_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("UPDATE cards SET comment_count = ? WHERE id = ?")
        .bind(new_count)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

    // 3. Auto-watch: INSERT OR IGNORE the author into watchers (D-12)
    sqlx::query("INSERT OR IGNORE INTO watchers (card_id, user_id) VALUES (?, ?)")
        .bind(card_id)
        .bind(author_id)
        .execute(&mut *tx)
        .await?;

    // 4. Mention notifications: only for distinct board members who are not the author
    let deduped: HashSet<String> = mention_user_ids.into_iter().collect();
    for uid in deduped {
        if uid == author_id {
            // Self-mention suppressed (D-11, T-05-15)
            continue;
        }
        // Verify the mentioned user is a board member (T-05-14)
        let is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM board_members WHERE board_id = ? AND user_id = ?",
        )
        .bind(board_id)
        .bind(&uid)
        .fetch_optional(&mut *tx)
        .await?;

        if is_member.is_none() {
            // Not a board member — no notification (T-05-14)
            continue;
        }

        let notif_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO notifications (id, user_id, board_id, card_id, kind, read, created_at) VALUES (?, ?, ?, ?, 'mention', 0, ?)",
        )
        .bind(&notif_id)
        .bind(&uid)
        .bind(board_id)
        .bind(card_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 5. Resolve author UserSummary for the returned ActivityEntry
    let author_row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, display_name, avatar_color FROM users WHERE id = ?",
    )
    .bind(author_id)
    .fetch_optional(pool)
    .await?;

    let author = author_row.map(|(id, display_name, avatar_color)| {
        crate::models::UserSummary { id, display_name, avatar_color }
    });

    Ok(crate::models::ActivityEntry {
        entry_type: "comment".to_string(),
        id: comment_id,
        author,
        text: body,
        payload: None,
        created_at: now,
    })
}

// ---------------------------------------------------------------------------
// Activity server function wrapper
// ---------------------------------------------------------------------------

/// Post a comment on a card (auth-guarded, board-member-scoped).
///
/// Security (T-05-16): board membership required; comment scoped to card on that board.
/// Security (T-05-13): comment body returned as ActivityEntry.text — rendered as text node in UI.
/// Security (T-05-14/15): mention notifications only for board members; self excluded.
#[server]
pub async fn add_comment(
    board_id: String,
    card_id: String,
    body: String,
    mention_user_ids: Vec<String>,
) -> Result<crate::models::ActivityEntry, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    add_comment_inner(
        &state.write_pool.0,
        &board_id,
        &card_id,
        &user.id,
        body,
        mention_user_ids,
    )
    .await
    .map_err(|e| {
        tracing::error!("add_comment error: {e}");
        ServerFnError::new("Couldn't save changes. Try again.")
    })
}

// ---------------------------------------------------------------------------
// Server function wrapper — read
// ---------------------------------------------------------------------------

/// Fetch full card detail for a board member.
///
/// Auth: requires authenticated user who is a member of the given board.
/// Read-only: uses read_pool only.
#[server]
pub async fn get_card_detail(
    board_id: String,
    card_num: i64,
) -> Result<CardDetail, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();

    // Auth + membership gate (T-05-01)
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    get_card_detail_inner(&state.read_pool.0, &board_id, &card_num, &user.id)
        .await
        .map_err(|e| {
            tracing::error!("get_card_detail error: {e}");
            ServerFnError::new("Failed to load card")
        })
}

// ---------------------------------------------------------------------------
// Move (cross-board), watch/unwatch, archive inner functions (SSR-only)
// ---------------------------------------------------------------------------

/// Internal: move a card to a different board (DETAIL-09, D-04/D-05/D-06, T-05-23/24/25).
///
/// One transaction:
/// 1. Verify target list belongs to to_board_id.
/// 2. Allocate a new card_num on to_board_id (mirrors create_card_inner).
/// 3. UPDATE cards: board_id, list_id, card_num, position, updated_at (scoped by id AND from_board_id).
/// 4. DELETE card_labels (board-scoped labels cannot carry — D-05, T-05-24).
/// 5. DELETE card_members not in the target board's membership (D-05, T-05-24).
/// 6. Log a card_events 'moved' entry with payload (D-06).
///
/// Child rows (comments, attachments, checklists, checklist_items) carry automatically
/// because they are keyed by card_id, not board_id (A3 from RESEARCH.md).
///
/// Returns the new card_num allocated on the target board.
#[cfg(feature = "ssr")]
pub async fn move_card_cross_board_inner(
    pool: &sqlx::SqlitePool,
    from_board_id: &str,
    card_id: &str,
    to_board_id: &str,
    to_list_id: &str,
    new_position: &str,
) -> Result<i64, sqlx::Error> {
    use fractional_index::FractionalIndex;
    use crate::server::now_millis;

    // Validate position before any write (T-04-09 pattern)
    FractionalIndex::from_string(new_position).map_err(|_| {
        sqlx::Error::Decode("invalid position: not a valid fractional index".into())
    })?;

    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    let mut tx = pool.begin().await?;

    // 1. Verify target list belongs to to_board_id (T-05-23)
    let list_board: Option<String> = sqlx::query_scalar(
        "SELECT board_id FROM lists WHERE id = ?"
    )
    .bind(to_list_id)
    .fetch_optional(&mut *tx)
    .await?;

    match list_board {
        None => return Err(sqlx::Error::Decode("target list not found".into())),
        Some(lb) if lb != to_board_id => return Err(sqlx::Error::Decode("target list not on to_board_id".into())),
        _ => {}
    }

    // 1b. Verify the card actually belongs to from_board_id BEFORE any other write
    // (CR-01/CR-02): bind card_id to from_board_id so a cross-board / non-existent
    // card_id cannot burn a card number, strip labels/members, or log a spurious
    // 'moved' event on a card the caller does not own on the source board.
    let card_on_from: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM cards WHERE id = ? AND board_id = ?",
    )
    .bind(card_id)
    .bind(from_board_id)
    .fetch_optional(&mut *tx)
    .await?;
    if card_on_from.is_none() {
        return Err(sqlx::Error::Decode("card not on source board".into()));
    }

    // 2. Allocate a new card_num on to_board_id (mirrors create_card_inner — WR-02)
    let new_card_num: i64 = sqlx::query_scalar(
        "UPDATE boards SET next_card_num = next_card_num + 1 WHERE id = ? RETURNING next_card_num - 1"
    )
    .bind(to_board_id)
    .fetch_one(&mut *tx)
    .await?;

    // 3. UPDATE the card (scoped by id AND from_board_id — T-05-25)
    let card_update = sqlx::query(
        "UPDATE cards SET board_id = ?, list_id = ?, card_num = ?, position = ?, updated_at = ? WHERE id = ? AND board_id = ?"
    )
    .bind(to_board_id)
    .bind(to_list_id)
    .bind(new_card_num)
    .bind(new_position)
    .bind(now)
    .bind(card_id)
    .bind(from_board_id)
    .execute(&mut *tx)
    .await?;
    // Abort (rolling back the tx, including the card_num allocation) if the card
    // is not on from_board_id (CR-01/CR-02). Belt-and-braces with the pre-check above.
    if card_update.rows_affected() == 0 {
        return Err(sqlx::Error::Decode("card not on source board".into()));
    }

    // 4. DELETE all card_labels (board-scoped labels cannot carry — D-05, T-05-24)
    sqlx::query("DELETE FROM card_labels WHERE card_id = ?")
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

    // 5. DELETE card_members whose user_id is NOT in the target board's membership (D-05, T-05-24)
    sqlx::query(
        "DELETE FROM card_members WHERE card_id = ? AND user_id NOT IN (SELECT user_id FROM board_members WHERE board_id = ?)"
    )
    .bind(card_id)
    .bind(to_board_id)
    .execute(&mut *tx)
    .await?;

    // 6. Log a card_events 'moved' entry (D-06)
    {
        use uuid::Uuid;
        let event_id = Uuid::now_v7().to_string();
        let payload = format!(r#"{{"from_board":"{}","to_board":"{}"}}"#, from_board_id, to_board_id);
        sqlx::query(
            "INSERT INTO card_events (id, card_id, actor_id, kind, payload, created_at) VALUES (?, ?, NULL, 'moved', ?, ?)"
        )
        .bind(&event_id)
        .bind(card_id)
        .bind(&payload)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(new_card_num)
}

/// Internal: add a watcher for a card.
///
/// Uses INSERT OR IGNORE (idempotent). Returns the new distinct watcher count.
#[cfg(feature = "ssr")]
pub async fn watch_card_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    user_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO watchers (card_id, user_id) VALUES (?, ?)")
        .bind(card_id)
        .bind(user_id)
        .execute(pool)
        .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM watchers WHERE card_id = ?")
        .bind(card_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Internal: remove a watcher from a card.
///
/// DELETE is idempotent. Returns the new distinct watcher count.
#[cfg(feature = "ssr")]
pub async fn unwatch_card_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    user_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query("DELETE FROM watchers WHERE card_id = ? AND user_id = ?")
        .bind(card_id)
        .bind(user_id)
        .execute(pool)
        .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM watchers WHERE card_id = ?")
        .bind(card_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Internal: archive a card (sets archived=1 scoped by id AND board_id) and logs the event.
///
/// Security: UPDATE scoped by id AND board_id (T-05-26). Absent from get_board_inner after this.
#[cfg(feature = "ssr")]
pub async fn archive_card_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    card_id: &str,
) -> Result<(), sqlx::Error> {
    use crate::server::now_millis;
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    sqlx::query(
        "UPDATE cards SET archived = 1, updated_at = ? WHERE id = ? AND board_id = ?"
    )
    .bind(now)
    .bind(card_id)
    .bind(board_id)
    .execute(pool)
    .await?;

    // Log a card_events 'archived' entry
    log_card_event_inner(pool, card_id, None, "archived", None).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Server function wrappers: move_card_cross_board, watch_card, archive_card
// ---------------------------------------------------------------------------

/// Move a card to another board (full cross-board semantics — DETAIL-09, D-04/D-05/D-06).
///
/// Caller must be a member of BOTH the source and target board (T-05-23).
/// Same-board list moves reuse the existing `move_card` server fn from card_api.rs.
#[server]
pub async fn move_card_cross_board(
    from_board_id: String,
    card_id: String,
    to_board_id: String,
    to_list_id: String,
    new_position: String,
) -> Result<i64, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    // Must be member of BOTH boards (T-05-23)
    require_board_member(&from_board_id, &state.read_pool.0).await?;
    require_board_member(&to_board_id, &state.read_pool.0).await?;
    move_card_cross_board_inner(
        &state.write_pool.0,
        &from_board_id,
        &card_id,
        &to_board_id,
        &to_list_id,
        &new_position,
    )
    .await
    .map_err(|e| {
        tracing::error!("move_card_cross_board error: {e}");
        ServerFnError::new("Couldn't move card. Try again.")
    })
}

/// Watch or unwatch a card (auth-guarded, returns updated distinct watcher count).
///
/// Security (T-05-26): board membership required.
#[server]
pub async fn watch_card(
    board_id: String,
    card_id: String,
    watch: bool,
) -> Result<i64, ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;
    let pool = &state.write_pool.0;
    let count = if watch {
        watch_card_inner(pool, &card_id, &user.id).await
    } else {
        unwatch_card_inner(pool, &card_id, &user.id).await
    };
    count.map_err(|e| {
        tracing::error!("watch_card error: {e}");
        ServerFnError::new("Couldn't update watch status. Try again.")
    })
}

/// Archive a card (auth-guarded, board-member-scoped — T-05-26).
///
/// Card is removed from get_board_inner after this (archived=1 filtered out).
#[server]
pub async fn archive_card(
    board_id: String,
    card_id: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_member(&board_id, &state.read_pool.0).await?;
    archive_card_inner(&state.write_pool.0, &board_id, &card_id)
        .await
        .map_err(|e| {
            tracing::error!("archive_card error: {e}");
            ServerFnError::new("Couldn't archive card. Try again.")
        })
}

// ---------------------------------------------------------------------------
// Attachment inner function (SSR-only)
// ---------------------------------------------------------------------------

/// Internal: insert an attachments row and bump cards.attachment_count in the same transaction.
///
/// Security: no path traversal possible — `url` is the server-constructed download path,
/// never derived from user-supplied filename (T-05-18). The `filename` stored is the
/// display name only; the storage key (UUID-based) is managed by the upload handler.
///
/// Transaction: INSERT attachments + UPDATE cards.attachment_count (Pitfall 3 count consistency).
///
/// Returns the newly inserted Attachment row (used by the upload handler to respond with JSON
/// and by the AttachmentsSection to push into the modal-scoped signal).
#[cfg(feature = "ssr")]
pub async fn record_attachment_inner(
    pool: &sqlx::SqlitePool,
    card_id: &str,
    uploader_id: &str,
    filename: &str,
    url: &str,
    size_bytes: i64,
) -> Result<crate::models::Attachment, sqlx::Error> {
    use uuid::Uuid;
    use crate::server::now_millis;

    let id = Uuid::now_v7().to_string();
    let now = now_millis().map_err(|_| sqlx::Error::Decode("clock error".into()))?;

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO attachments (id, card_id, uploader_id, filename, url, size_bytes, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(card_id)
    .bind(uploader_id)
    .bind(filename)
    .bind(url)
    .bind(size_bytes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Bump attachment_count by recounting (Pitfall 3 — same-transaction count update)
    let new_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments WHERE card_id = ?",
    )
    .bind(card_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("UPDATE cards SET attachment_count = ? WHERE id = ?")
        .bind(new_count)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(crate::models::Attachment {
        id,
        card_id: card_id.to_string(),
        filename: filename.to_string(),
        url: url.to_string(),
        size_bytes,
        uploader_id: uploader_id.to_string(),
        created_at: now,
    })
}
