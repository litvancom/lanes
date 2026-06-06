use leptos::prelude::*;
use crate::models::Board;

/// Internal: fetch all non-archived boards from the given pool.
/// Extracted for testability independent of Leptos context machinery.
#[cfg(feature = "ssr")]
pub async fn fetch_boards(pool: &sqlx::SqlitePool) -> Result<Vec<Board>, sqlx::Error> {
    sqlx::query_as!(
        Board,
        r#"SELECT id, name, key_prefix, color,
                  starred as "starred: bool",
                  archived as "archived: bool",
                  created_at, updated_at
           FROM boards WHERE archived = 0 ORDER BY created_at ASC"#
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

/// Internal: validate name and insert a new board. Returns the created Board.
/// Validation: trim, reject empty, reject > 120 chars (T-03-01, ASVS V5).
/// Uses parameterized INSERT only — no format! into SQL.
#[cfg(feature = "ssr")]
pub async fn create_board(pool: &sqlx::SqlitePool, name: String) -> Result<Board, String> {
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

    // Parameterized INSERT — no format! into SQL (T-03-01 Tampering mitigation).
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
    .execute(pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

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

/// List all non-archived boards, ordered by created_at ASC.
/// Reads from the read pool via AppState context.
#[server]
pub async fn list_boards() -> Result<Vec<Board>, ServerFnError> {
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let pool = &state.read_pool.0;

    fetch_boards(pool).await.map_err(|e| {
        tracing::error!("list_boards DB error: {:?}", e);
        ServerFnError::new("Failed to load boards")
    })
}

/// Create a new board. Validates name server-side (T-03-01).
/// Uses a parameterized INSERT (no format! into SQL) on the write pool.
#[server]
pub async fn add_board(name: String) -> Result<Board, ServerFnError> {
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let pool = &state.write_pool.0;

    create_board(pool, name).await.map_err(|e| {
        // Log full error server-side; return generic message to client (T-03-02)
        tracing::error!("add_board error: {}", e);
        ServerFnError::new("Failed to create board")
    })
}
