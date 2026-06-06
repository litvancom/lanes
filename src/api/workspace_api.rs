use leptos::prelude::*;
use crate::models::Board;

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

/// Internal: validate name, insert a new board AND an owner board_members row
/// in a single transaction (Pitfall 4 — never leave a board without an owner).
/// Returns the created Board.
/// Validation: trim, reject empty, reject > 120 chars (T-03-01, ASVS V5).
/// Uses parameterized SQL only — no format! into SQL (T-02-17 Tampering mitigation).
#[cfg(feature = "ssr")]
pub async fn create_board(
    pool: &sqlx::SqlitePool,
    name: String,
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

    let id = Uuid::now_v7().to_string();

    let key_prefix = derive_key_prefix(&name);

    // Color is hardcoded today; validate at the write boundary so an invalid
    // color can never reach the DB once this becomes user-controlled (CR-01).
    let color = "#6366f1".to_string();
    if !(color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return Err("Board color must be a 6-digit hex value (#rrggbb)".into());
    }
    // Surface a clock error rather than silently writing 0 (WR-03).
    let now = crate::server::now_millis().map_err(|e| format!("Clock error: {e}"))?;

    // Begin a transaction: board INSERT + board_members owner INSERT must be atomic.
    // If the board_members insert fails (e.g. FK violation), the board row is rolled back.
    // This prevents orphan boards with no owner (Pitfall 4, T-02-15).
    let mut tx = pool.begin().await.map_err(|e| format!("DB error: {e}"))?;

    // Parameterized INSERT — no format! into SQL (T-02-17 Tampering mitigation).
    // next_card_num is set explicitly (=1) to match the seed path and make the
    // per-board card counter contract self-documenting rather than relying on
    // the schema DEFAULT (WR-02).
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

/// List all non-archived boards the authenticated user is a member of (COLLAB-01).
/// Rejects unauthenticated callers (D-11).
/// Reads from the read pool via AppState context.
#[server]
pub async fn list_boards() -> Result<Vec<Board>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    // Auth gate FIRST — unauthenticated callers are rejected (D-11)
    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_boards_for_user(pool, &user.id).await.map_err(|e| {
        tracing::error!("list_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load boards")
    })
}

/// Create a new board. Validates name server-side (T-03-01).
/// The authenticated user becomes the board owner (Pitfall 4).
/// Rejects unauthenticated callers (D-11).
/// Uses a parameterized INSERT+owner-row in a single transaction (no format! into SQL).
#[server]
pub async fn add_board(name: String) -> Result<Board, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    // Auth gate FIRST — unauthenticated callers are rejected (D-11)
    let user = require_user().await?;
    let state = expect_context::<AppState>();
    let pool = &state.write_pool.0;

    create_board(pool, name, &user.id).await.map_err(|e| {
        // Log full error server-side; return generic message to client (T-03-02)
        tracing::error!("add_board error: {}", e);
        ServerFnError::new("Failed to create board")
    })
}
