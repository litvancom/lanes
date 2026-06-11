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
    // 9. Render description through markdown sanitize pipeline (T-05-02)
    // -----------------------------------------------------------------------
    let description_html = render_markdown(description.as_deref().unwrap_or(""));

    // -----------------------------------------------------------------------
    // 10. Assemble and return CardDetail
    // -----------------------------------------------------------------------
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
        description_html,
        checklist_items,
        activity,
        attachments,
        watcher_count,
        is_watching,
        board_members,
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
