use leptos::prelude::*;
use crate::models::{Board, BoardWithMeta, TodayCard};

// ─────────────────────────────────────────────────────────────────────────────
// SSR-only constants and types
// ─────────────────────────────────────────────────────────────────────────────

/// Board color swatches available for selection (D-06).
/// Fixed set from the design fixture data.jsx — exactly these 5 values are valid.
/// Validation at the write boundary rejects any color not in this list.
/// Not cfg-gated so the client modal can render swatches without SSR (safe: static design data).
pub const BOARD_COLOR_SWATCHES: &[&str] = &[
    "#7c5cff", // purple
    "#0ea5e9", // blue
    "#f59e0b", // amber
    "#10b981", // green
    "#ec4899", // pink
];

/// A single list definition in a board template (D-07).
#[cfg(feature = "ssr")]
pub struct TemplateList {
    pub name: &'static str,
    pub cards: &'static [&'static str],
}

/// Built-in board templates (D-07).
/// Each template defines named lists plus sample cards demonstrating the template's purpose.
#[cfg(feature = "ssr")]
pub enum BoardTemplate {
    PersonalTodos,
    WeeklyReview,
    TripPlanning,
}

#[cfg(feature = "ssr")]
impl BoardTemplate {
    /// Lists and sample cards for this template.
    pub fn lists(&self) -> &[TemplateList] {
        match self {
            Self::PersonalTodos => &[
                TemplateList {
                    name: "Inbox",
                    cards: &["Capture new tasks here"],
                },
                TemplateList {
                    name: "Today",
                    cards: &["Pick 3 things for today"],
                },
                TemplateList {
                    name: "Done",
                    cards: &["Completed tasks land here"],
                },
            ],
            Self::WeeklyReview => &[
                TemplateList {
                    name: "Wins",
                    cards: &["What went well this week?"],
                },
                TemplateList {
                    name: "Stuck",
                    cards: &["What's blocking you?"],
                },
                TemplateList {
                    name: "Next",
                    cards: &["Top priorities for next week"],
                },
            ],
            Self::TripPlanning => &[
                TemplateList {
                    name: "Ideas",
                    cards: &["Destinations to consider"],
                },
                TemplateList {
                    name: "Booked",
                    cards: &["Flights, hotels, activities"],
                },
                TemplateList {
                    name: "Day-of",
                    cards: &["Packing list, confirmations"],
                },
            ],
        }
    }

    /// Default color for this template (used when not overridden by the user).
    pub fn color(&self) -> &'static str {
        match self {
            Self::PersonalTodos => "#7c5cff",
            Self::WeeklyReview => "#10b981",
            Self::TripPlanning => "#0ea5e9",
        }
    }

    /// Parse a template name string to a BoardTemplate variant.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "personal_todos" => Some(Self::PersonalTodos),
            "weekly_review" => Some(Self::WeeklyReview),
            "trip_planning" => Some(Self::TripPlanning),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inner fns — Board queries
// ─────────────────────────────────────────────────────────────────────────────

/// Internal: fetch all non-archived boards the given user is a member of.
/// Extracted for testability independent of Leptos context machinery.
/// Joins board_members to enforce per-user scoping (COLLAB-01, T-02-13).
#[cfg(feature = "ssr")]
pub async fn fetch_boards_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<Board>, sqlx::Error> {
    sqlx::query_as!(
        Board,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  b.starred as "starred: bool",
                  b.archived as "archived: bool",
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 0
           ORDER BY b.created_at ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Internal: fetch boards with extended meta (card_count, per-user starred, last_viewed_at).
/// Reads `m.starred` (board_members.starred) — NOT `b.starred` (Pitfall 2, D-10).
/// Excludes archived boards.
#[cfg(feature = "ssr")]
pub async fn fetch_boards_with_meta_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<BoardWithMeta>, sqlx::Error> {
    sqlx::query_as!(
        BoardWithMeta,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  m.starred as "starred: bool",
                  b.archived as "archived: bool",
                  (SELECT COUNT(*) FROM cards c
                   JOIN lists l ON l.id = c.list_id
                   WHERE l.board_id = b.id AND c.archived = 0) as "card_count!: i64",
                  m.last_viewed_at,
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 0
           ORDER BY b.created_at ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Internal: fetch the top 3 boards by last_viewed_at DESC (D-01).
/// NULL last_viewed_at sorts last (never-viewed boards after all viewed boards).
#[cfg(feature = "ssr")]
pub async fn fetch_recent_boards_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<BoardWithMeta>, sqlx::Error> {
    sqlx::query_as!(
        BoardWithMeta,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  m.starred as "starred: bool",
                  b.archived as "archived: bool",
                  (SELECT COUNT(*) FROM cards c
                   JOIN lists l ON l.id = c.list_id
                   WHERE l.board_id = b.id AND c.archived = 0) as "card_count!: i64",
                  m.last_viewed_at,
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 0
           ORDER BY (m.last_viewed_at IS NULL), m.last_viewed_at DESC
           LIMIT 3"#,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Internal: fetch boards where the current user has starred them (board_members.starred = 1).
#[cfg(feature = "ssr")]
pub async fn fetch_starred_boards_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<BoardWithMeta>, sqlx::Error> {
    sqlx::query_as!(
        BoardWithMeta,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  m.starred as "starred: bool",
                  b.archived as "archived: bool",
                  (SELECT COUNT(*) FROM cards c
                   JOIN lists l ON l.id = c.list_id
                   WHERE l.board_id = b.id AND c.archived = 0) as "card_count!: i64",
                  m.last_viewed_at,
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND m.starred = 1 AND b.archived = 0
           ORDER BY b.created_at ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Internal: fetch boards that are archived and the user is a member of.
#[cfg(feature = "ssr")]
pub async fn fetch_archived_boards_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<BoardWithMeta>, sqlx::Error> {
    sqlx::query_as!(
        BoardWithMeta,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  m.starred as "starred: bool",
                  b.archived as "archived: bool",
                  (SELECT COUNT(*) FROM cards c
                   JOIN lists l ON l.id = c.list_id
                   WHERE l.board_id = b.id AND c.archived = 0) as "card_count!: i64",
                  m.last_viewed_at,
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 1
           ORDER BY b.created_at ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Internal: search boards by name for the given user (case-insensitive LIKE, D-13).
/// Builds the LIKE pattern in Rust and binds it — never formats into SQL (T-03-02).
/// Returns at most 10 results ordered by created_at ASC.
/// Returns an empty Vec if query trims to empty.
#[cfg(feature = "ssr")]
pub async fn search_boards_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    query: &str,
) -> Result<Vec<BoardWithMeta>, sqlx::Error> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // Build the LIKE term in Rust, not in SQL (T-03-02 parameterized only)
    let like_term = format!("%{}%", trimmed);
    sqlx::query_as!(
        BoardWithMeta,
        r#"SELECT b.id, b.name, b.key_prefix, b.color,
                  m.starred as "starred: bool",
                  b.archived as "archived: bool",
                  (SELECT COUNT(*) FROM cards c
                   JOIN lists l ON l.id = c.list_id
                   WHERE l.board_id = b.id AND c.archived = 0) as "card_count!: i64",
                  m.last_viewed_at,
                  b.created_at, b.updated_at
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 0 AND LOWER(b.name) LIKE LOWER(?)
           ORDER BY b.created_at ASC
           LIMIT 10"#,
        user_id,
        like_term
    )
    .fetch_all(pool)
    .await
}

/// Internal: fetch due-today and overdue non-done cards across the user's boards (D-02).
///
/// ## All-time overdue policy (WORK-02 — intentional, no lower date bound)
///
/// A non-done card stays in the Today strip until it is marked done, regardless of how
/// long ago it was due. There is intentionally NO lower bound on `c.due_at`.
///
/// Two cutoffs are used deliberately:
/// - **Filter cutoff** (`tomorrow_start`): `WHERE c.due_at < tomorrow_start` — admits
///   due-today cards AND all past-due cards (all-time overdue).
/// - **Overdue CASE cutoff** (`today_start`): `CASE WHEN c.due_at < today_start THEN 1 ELSE 0 END`
///   — flags only the strictly-before-today subset as `overdue = true`; cards due today
///   are included in the strip but have `overdue = false`.
///
/// Do NOT add `AND c.due_at >= …` — that would break the all-time overdue contract.
///
/// Results ordered by due_at ASC, limited to 20.
#[cfg(feature = "ssr")]
pub async fn fetch_today_strip_inner(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<TodayCard>, sqlx::Error> {
    // Propagate clock errors instead of panicking (T-03-07-01).
    let now = crate::server::now_millis()
        .map_err(|e| sqlx::Error::Protocol(format!("Clock error: {e}")))?;
    let day_ms = 86_400_000i64;
    let today_start = (now / day_ms) * day_ms;
    let tomorrow_start = today_start + day_ms;

    // Use the non-macro sqlx::query with manual row mapping to handle the CASE WHEN expression.
    // The compile-time type checker has difficulty with CASE expressions returning integer literals;
    // the runtime approach avoids that limitation while keeping parameterized SQL (T-03-02).
    use sqlx::Row;
    let rows = sqlx::query(
        r#"SELECT c.id, c.title, b.id as board_id, b.name as board_name, c.due_at,
                  CASE WHEN c.due_at < ? THEN 1 ELSE 0 END as overdue
           FROM cards c
           JOIN lists l ON l.id = c.list_id
           JOIN boards b ON b.id = l.board_id
           JOIN board_members bm ON bm.board_id = b.id
           WHERE bm.user_id = ?
             AND c.archived = 0 AND b.archived = 0
             AND c.done = 0
             AND c.due_at < ?
           ORDER BY c.due_at ASC
           LIMIT 20"#,
    )
    .bind(today_start)
    .bind(user_id)
    .bind(tomorrow_start)
    .fetch_all(pool)
    .await?;

    // Propagate decode errors instead of silently producing empty-string IDs (T-03-07-02).
    rows.into_iter()
        .map(|row| -> Result<TodayCard, sqlx::Error> {
            let overdue_int: i64 = row.try_get("overdue")?;
            Ok(TodayCard {
                id: row.try_get("id")?,
                title: row.try_get("title")?,
                board_id: row.try_get("board_id")?,
                board_name: row.try_get("board_name")?,
                due_at: row.try_get("due_at").ok(),
                overdue: overdue_int != 0,
            })
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Inner fns — Board mutations
// ─────────────────────────────────────────────────────────────────────────────

/// Derive a board key prefix from its name: first whitespace-delimited word,
/// uppercased, first 6 chars. Single source of truth shared by production and
/// test code (WR-05).
pub fn derive_key_prefix(name: &str) -> String {
    name.split_whitespace()
        .next()
        .unwrap_or("BOARD")
        .to_uppercase()
        .chars()
        .take(6)
        .collect::<String>()
}

/// Internal: validate name and color, insert a new board AND an owner board_members row
/// in a single transaction (Pitfall 4 — never leave a board without an owner).
/// Returns the created Board.
///
/// Validation:
/// - name: trim, reject empty, reject > 120 chars (T-03-01, ASVS V5)
/// - color: must be exactly 7 chars starting with '#' with hex digits AND must be a
///   member of BOARD_COLOR_SWATCHES (D-06 fixed set, T-03-01)
///
/// Uses parameterized SQL only — no format! into SQL (T-03-02 Tampering mitigation).
#[cfg(feature = "ssr")]
pub async fn create_board(
    pool: &sqlx::SqlitePool,
    name: String,
    color: String,
    creator_id: &str,
) -> Result<Board, String> {
    use uuid::Uuid;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Board name cannot be empty".into());
    }
    if name.chars().count() > 120 {
        return Err("Board name must be 120 characters or fewer".into());
    }

    let color = color.trim().to_string();
    // Validate hex format: exactly 7 chars, starts with '#', rest are hex digits
    if !(color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return Err("Board color must be a 6-digit hex value (#rrggbb)".into());
    }
    // Validate against the fixed swatch set (D-06)
    if !BOARD_COLOR_SWATCHES.contains(&color.as_str()) {
        return Err("Board color must be one of the available swatches".into());
    }

    let id = Uuid::now_v7().to_string();
    let key_prefix = derive_key_prefix(&name);

    // Surface a clock error rather than silently writing 0 (WR-03).
    let now = crate::server::now_millis().map_err(|e| format!("Clock error: {e}"))?;

    // Begin a transaction: board INSERT + board_members owner INSERT must be atomic.
    // If the board_members insert fails (e.g. FK violation), the board row is rolled back.
    // This prevents orphan boards with no owner (Pitfall 4, T-02-15).
    let mut tx = pool.begin().await.map_err(|e| format!("DB error: {e}"))?;

    sqlx::query!(
        r#"INSERT INTO boards (id, name, key_prefix, color, next_card_num, starred, archived, created_at, updated_at)
           VALUES (?, ?, ?, ?, 1, 0, 0, ?, ?)"#,
        id,
        name,
        key_prefix,
        color,
        now,
        now,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    // Insert owner board_members row in the same transaction (Pitfall 4).
    sqlx::query!(
        "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
        id,
        creator_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    tx.commit().await.map_err(|e| format!("DB error: {e}"))?;

    Ok(Board {
        id,
        name,
        key_prefix,
        color,
        starred: false,
        archived: false,
        created_at: now,
        updated_at: now,
    })
}

/// Internal: create a board from a template in a single atomic transaction (Pitfall 3).
/// Inserts: board row, owner board_members row, all template lists, all template cards.
/// Rolls back everything if any insert fails.
#[cfg(feature = "ssr")]
pub async fn create_board_from_template(
    pool: &sqlx::SqlitePool,
    name: String,
    color: String,
    template: BoardTemplate,
    creator_id: &str,
) -> Result<Board, String> {
    use uuid::Uuid;
    use fractional_index::FractionalIndex;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Board name cannot be empty".into());
    }
    if name.chars().count() > 120 {
        return Err("Board name must be 120 characters or fewer".into());
    }

    let color = color.trim().to_string();
    if !(color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return Err("Board color must be a 6-digit hex value (#rrggbb)".into());
    }
    if !BOARD_COLOR_SWATCHES.contains(&color.as_str()) {
        return Err("Board color must be one of the available swatches".into());
    }

    let board_id = Uuid::now_v7().to_string();
    let key_prefix = derive_key_prefix(&name);
    let now = crate::server::now_millis().map_err(|e| format!("Clock error: {e}"))?;

    let template_lists = template.lists();
    // Count total cards across all template lists to set next_card_num past them
    let total_cards: i64 = template_lists.iter().map(|l| l.cards.len() as i64).sum();
    let next_card_num: i64 = total_cards + 1;

    // One transaction covers board, owner member, all lists, all cards (Pitfall 3)
    let mut tx = pool.begin().await.map_err(|e| format!("DB error: {e}"))?;

    // INSERT board with next_card_num set past all template cards
    sqlx::query!(
        r#"INSERT INTO boards (id, name, key_prefix, color, next_card_num, starred, archived, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, 0, 0, ?, ?)"#,
        board_id,
        name,
        key_prefix,
        color,
        next_card_num,
        now,
        now,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    // INSERT owner board_members row
    sqlx::query!(
        "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, 'owner')",
        board_id,
        creator_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    // INSERT lists and cards for the template
    let mut prev_list_pos: Option<FractionalIndex> = None;
    let mut card_num: i64 = 1;

    for template_list in template_lists {
        let list_id = Uuid::now_v7().to_string();
        let list_pos = match &prev_list_pos {
            None => FractionalIndex::default(),
            Some(prev) => FractionalIndex::new_after(prev),
        };
        let list_pos_str = list_pos.to_string();

        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
            list_id,
            board_id,
            template_list.name,
            list_pos_str,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("DB error inserting list: {e}"))?;

        prev_list_pos = Some(list_pos);

        // INSERT cards for this list
        let mut prev_card_pos: Option<FractionalIndex> = None;
        for card_title in template_list.cards {
            let card_id = Uuid::now_v7().to_string();
            let card_pos = match &prev_card_pos {
                None => FractionalIndex::default(),
                Some(prev) => FractionalIndex::new_after(prev),
            };
            let card_pos_str = card_pos.to_string();

            sqlx::query!(
                r#"INSERT INTO cards (id, list_id, board_id, card_num, title, position, done, archived, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?)"#,
                card_id,
                list_id,
                board_id,
                card_num,
                card_title,
                card_pos_str,
                now,
                now,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("DB error inserting card: {e}"))?;

            prev_card_pos = Some(card_pos);
            card_num += 1;
        }
    }

    tx.commit().await.map_err(|e| format!("DB error: {e}"))?;

    Ok(Board {
        id: board_id,
        name,
        key_prefix,
        color,
        starred: false,
        archived: false,
        created_at: now,
        updated_at: now,
    })
}

/// Internal: toggle board_members.starred for the given user and board (D-10).
/// Uses `1 - starred` to flip between 0 and 1 atomically.
/// NEVER touches boards.starred (anti-pattern per RESEARCH Pitfall 2).
#[cfg(feature = "ssr")]
pub async fn toggle_star_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE board_members SET starred = 1 - starred WHERE board_id = ? AND user_id = ?",
        board_id,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Internal: set boards.archived to the given value (D-11).
/// Owner-only: rejects non-owner callers with an error message.
/// `role` is the caller's role string from board_members (should come from require_board_member).
#[cfg(feature = "ssr")]
pub async fn set_archived_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    _user_id: &str,
    role: &str,
    archived: bool,
) -> Result<(), String> {
    if role != "owner" {
        return Err("Only the board owner can perform this action".into());
    }
    let now = crate::server::now_millis().map_err(|e| format!("Clock error: {e}"))?;
    let archived_int: i64 = if archived { 1 } else { 0 };
    sqlx::query!(
        "UPDATE boards SET archived = ?, updated_at = ? WHERE id = ?",
        archived_int,
        now,
        board_id,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// #[server] wrappers — Board queries
// ─────────────────────────────────────────────────────────────────────────────

/// List all non-archived boards the authenticated user is a member of (COLLAB-01).
/// Rejects unauthenticated callers.
/// Reads from the read pool via AppState context.
#[server]
pub async fn list_boards() -> Result<Vec<Board>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_boards_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load boards")
    })
}

/// List all non-archived boards with extended meta (card count, per-user starred, last_viewed_at).
/// Reads board_members.starred (per-user, D-10) — not boards.starred.
#[server]
pub async fn list_boards_with_meta() -> Result<Vec<BoardWithMeta>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_boards_with_meta_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_boards_with_meta DB error: {:?}", e);
        ServerFnError::new("Failed to load boards")
    })
}

/// List the top 3 most-recently-viewed boards (D-01).
/// Never-viewed boards (NULL last_viewed_at) sort after all viewed boards.
#[server]
pub async fn list_recent_boards() -> Result<Vec<BoardWithMeta>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_recent_boards_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_recent_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load recent boards")
    })
}

/// List boards the current user has starred (board_members.starred = 1).
#[server]
pub async fn list_starred_boards() -> Result<Vec<BoardWithMeta>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_starred_boards_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_starred_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load starred boards")
    })
}

/// List boards that are archived and the current user is a member of.
#[server]
pub async fn list_archived_boards() -> Result<Vec<BoardWithMeta>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_archived_boards_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_archived_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load archived boards")
    })
}

/// Search boards by name for the current user (case-insensitive, D-13).
/// Returns at most 10 results. Empty query returns empty Vec.
#[server]
pub async fn search_boards(query: String) -> Result<Vec<BoardWithMeta>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    search_boards_for_user(pool, &user.id, &query).await.map_err(|e| {
        tracing::error!("search_boards DB error: {:?}", e);
        ServerFnError::new("Failed to search boards")
    })
}

/// Fetch due-today and overdue non-done cards across the user's boards (D-02).
/// Results are ordered by due_at ASC and limited to 20.
#[server]
pub async fn fetch_today_strip() -> Result<Vec<TodayCard>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_today_strip_inner(pool, &user.id).await.map_err(|e| {
        tracing::error!("fetch_today_strip DB error: {:?}", e);
        ServerFnError::new("Failed to load today strip")
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// #[server] wrappers — Board mutations
// ─────────────────────────────────────────────────────────────────────────────

/// Create a new board with a user-chosen color from the fixed swatch set (BOARD-01, D-06).
/// Validates name and color server-side (T-03-01).
/// The authenticated user becomes the board owner (Pitfall 4).
/// Rejects unauthenticated callers.
/// Uses a parameterized INSERT + owner-row in a single transaction (no format! into SQL).
#[server]
pub async fn add_board(name: String, color: String) -> Result<Board, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.write_pool.0;

    create_board(pool, name, color, &user.id).await.map_err(|e| {
        tracing::error!("add_board error: {}", e);
        ServerFnError::new("Failed to create board")
    })
}

/// Create a board from a built-in template (BOARD-02, D-07).
/// Inserts board, owner member, all template lists, and sample cards in one transaction.
/// `template` must be one of: "personal_todos", "weekly_review", "trip_planning".
#[server]
pub async fn add_board_from_template(
    name: String,
    color: String,
    template: String,
) -> Result<Board, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.write_pool.0;

    let board_template = BoardTemplate::from_str(&template)
        .ok_or_else(|| ServerFnError::new("Invalid template name"))?;

    create_board_from_template(pool, name, color, board_template, &user.id)
        .await
        .map_err(|e| {
            tracing::error!("add_board_from_template error: {}", e);
            ServerFnError::new("Failed to create board from template")
        })
}

/// Toggle the current user's star on a board (D-10, WORK-04).
/// Any board member can star; this only modifies board_members.starred for the current user.
/// Reads board membership from read pool; writes to write pool.
#[server]
pub async fn toggle_star_board(board_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let (user, _role) = require_board_member(&board_id, &state.read_pool.0).await?;

    toggle_star_inner(&state.write_pool.0, &board_id, &user.id)
        .await
        .map_err(|e| {
            tracing::error!("toggle_star_board DB error: {:?}", e);
            ServerFnError::new("Failed to toggle star")
        })
}

/// Archive a board (owner-only, D-11, WORK-05).
/// Sets boards.archived = 1. Only the board owner may archive.
#[server]
pub async fn archive_board(board_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let (user, role) = require_board_member(&board_id, &state.read_pool.0).await?;

    if role != "owner" {
        return Err(ServerFnError::new("Only the board owner can perform this action"));
    }

    set_archived_inner(&state.write_pool.0, &board_id, &user.id, &role, true)
        .await
        .map_err(|e| {
            tracing::error!("archive_board error: {}", e);
            ServerFnError::new("Failed to archive board")
        })
}

/// Restore a board from archive (owner-only, D-11, WORK-05).
/// Sets boards.archived = 0. Only the board owner may restore.
#[server]
pub async fn restore_board(board_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let (user, role) = require_board_member(&board_id, &state.read_pool.0).await?;

    if role != "owner" {
        return Err(ServerFnError::new("Only the board owner can perform this action"));
    }

    set_archived_inner(&state.write_pool.0, &board_id, &user.id, &role, false)
        .await
        .map_err(|e| {
            tracing::error!("restore_board error: {}", e);
            ServerFnError::new("Failed to restore board")
        })
}

/// Internal: permanently delete a board by ID.
/// ON DELETE CASCADE in the schema removes all child rows: lists, cards, board_members,
/// labels, etc. — FK enforcement must be ON (write pool has `foreign_keys(true)`).
#[cfg(feature = "ssr")]
pub async fn delete_board_inner(
    pool: &sqlx::SqlitePool,
    board_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM boards WHERE id = ?", board_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Permanently delete a board (owner-only, WORK-05, D-11, T-03-27).
/// Cascades: all lists, cards, board_members, labels for this board are removed.
/// Only the board owner may delete. Non-members receive a generic "board not found" error.
#[server]
pub async fn delete_board(board_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_member;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let (_user, role) = require_board_member(&board_id, &state.read_pool.0).await?;

    if role != "owner" {
        return Err(ServerFnError::new("Only the board owner can delete this board"));
    }

    delete_board_inner(&state.write_pool.0, &board_id)
        .await
        .map_err(|e| {
            tracing::error!("delete_board error: {:?}", e);
            ServerFnError::new("Failed to delete board")
        })
}
