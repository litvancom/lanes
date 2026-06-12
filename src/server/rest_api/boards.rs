//! REST API handlers for the `/api/v1/boards` resource.
//!
//! All handlers take `State<AppState>` + `ApiUser` and return `Response`.
//! Board-membership is enforced by replicating the board_members SELECT from
//! `attachments.rs` directly — `require_board_member` uses `leptos_axum::extract`
//! which is unavailable in plain Axum handlers (D-16, plan constraint).
//!
//! Generic 404 is returned for non-members to avoid IDOR existence leaks.

#![cfg(feature = "ssr")]

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    models::rest_dto::{BoardDto, CreateBoardReq, UpdateBoardReq},
    server::{
        rest_api::auth::ApiUser,
        state::AppState,
    },
};

// ---------------------------------------------------------------------------
// Membership helper — attachments.rs pattern (D-16 / plan constraint)
// ---------------------------------------------------------------------------

/// Verify the given user is a member of `board_id`.
/// Returns the user's `role` on success, or an Err(Response) for the handler to return.
///
/// Generic 404 for non-members — never leak existence (no IDOR, D-16).
pub async fn require_member(
    pool: &sqlx::SqlitePool,
    board_id: &str,
    user_id: &str,
) -> Result<String, Response> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
    )
    .bind(board_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("require_member DB error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
    })?;

    row.map(|(role,)| role).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "board not found"}))).into_response()
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/boards
// ---------------------------------------------------------------------------

/// List all non-archived boards the authenticated user is a member of.
#[utoipa::path(
    get,
    path = "/api/v1/boards",
    responses(
        (status = 200, description = "Board list", body = Vec<BoardDto>),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn list_boards(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
) -> Response {
    let rows: Result<Vec<(String, String, String, String, bool, bool, i64, i64)>, sqlx::Error> =
        sqlx::query_as(
            r#"SELECT b.id, b.name, b.color, b.key_prefix,
                      CAST(b.starred AS BOOLEAN) as starred,
                      CAST(b.archived AS BOOLEAN) as archived,
                      b.created_at, b.updated_at
               FROM boards b
               JOIN board_members m ON m.board_id = b.id
               WHERE m.user_id = ? AND b.archived = 0
               ORDER BY b.created_at ASC"#,
        )
        .bind(&user.id)
        .fetch_all(&state.read_pool.0)
        .await;

    match rows {
        Ok(rows) => {
            let dtos: Vec<BoardDto> = rows
                .into_iter()
                .map(|(id, name, color, key_prefix, _starred, archived, created_at, updated_at)| {
                    BoardDto { id, name, color, key_prefix, archived, created_at, updated_at }
                })
                .collect();
            (StatusCode::OK, Json(dtos)).into_response()
        }
        Err(e) => {
            tracing::error!("list_boards REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/boards
// ---------------------------------------------------------------------------

/// Create a new board. The authenticated user becomes the owner.
#[utoipa::path(
    post,
    path = "/api/v1/boards",
    request_body = CreateBoardReq,
    responses(
        (status = 201, description = "Board created", body = BoardDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn create_board(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Json(body): Json<CreateBoardReq>,
) -> Response {
    use crate::api::workspace_api::create_board as create_board_inner;

    match create_board_inner(&state.write_pool.0, body.name, body.color, &user.id).await {
        Ok(board) => {
            let dto = BoardDto {
                id: board.id,
                name: board.name,
                color: board.color,
                key_prefix: board.key_prefix,
                archived: board.archived,
                created_at: board.created_at,
                updated_at: board.updated_at,
            };
            (StatusCode::CREATED, Json(dto)).into_response()
        }
        Err(msg) => {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/boards/{board_id}
// ---------------------------------------------------------------------------

/// Get a single board by ID.
#[utoipa::path(
    get,
    path = "/api/v1/boards/{board_id}",
    params(("board_id" = String, Path, description = "Board ID")),
    responses(
        (status = 200, description = "Board", body = BoardDto),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn get_board(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
) -> Response {
    // Membership gate (D-16 — no IDOR)
    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    let row: Result<Option<(String, String, String, String, bool, bool, i64, i64)>, sqlx::Error> =
        sqlx::query_as(
            r#"SELECT b.id, b.name, b.color, b.key_prefix,
                      CAST(b.starred AS BOOLEAN),
                      CAST(b.archived AS BOOLEAN),
                      b.created_at, b.updated_at
               FROM boards b WHERE b.id = ?"#,
        )
        .bind(&board_id)
        .fetch_optional(&state.read_pool.0)
        .await;

    match row {
        Ok(Some((id, name, color, key_prefix, _starred, archived, created_at, updated_at))) => {
            let dto = BoardDto { id, name, color, key_prefix, archived, created_at, updated_at };
            (StatusCode::OK, Json(dto)).into_response()
        }
        Ok(None) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "board not found"}))).into_response()
        }
        Err(e) => {
            tracing::error!("get_board REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/boards/{board_id}
// ---------------------------------------------------------------------------

/// Update a board's name and/or color (owner-only).
#[utoipa::path(
    patch,
    path = "/api/v1/boards/{board_id}",
    params(("board_id" = String, Path, description = "Board ID")),
    request_body = UpdateBoardReq,
    responses(
        (status = 200, description = "Updated board", body = BoardDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Owner-only operation"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn update_board(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
    Json(body): Json<UpdateBoardReq>,
) -> Response {
    use crate::api::workspace_api::BOARD_COLOR_SWATCHES;
    use crate::server::now_millis;

    // Membership + role gate
    let role = match require_member(&state.read_pool.0, &board_id, &user.id).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if role != "owner" {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "owner only"}))).into_response();
    }

    // Fetch current board values so we can apply partial updates
    let current: Option<(String, String, String, String, bool, i64, i64)> = match sqlx::query_as(
        "SELECT id, name, color, key_prefix, CAST(archived AS BOOLEAN), created_at, updated_at FROM boards WHERE id = ?",
    )
    .bind(&board_id)
    .fetch_optional(&state.read_pool.0)
    .await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("update_board fetch current board error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    let (cur_id, cur_name, cur_color, cur_key_prefix, cur_archived, cur_created_at, _) = match current {
        Some(row) => row,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "board not found"}))).into_response(),
    };

    // Apply name patch
    let new_name = if let Some(n) = body.name {
        let n = n.trim().to_string();
        if n.is_empty() {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "name cannot be empty"}))).into_response();
        }
        if n.chars().count() > 120 {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "name too long"}))).into_response();
        }
        n
    } else {
        cur_name
    };

    // Apply color patch
    let new_color = if let Some(c) = body.color {
        let c = c.trim().to_string();
        if !(c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid color format"}))).into_response();
        }
        if !BOARD_COLOR_SWATCHES.contains(&c.as_str()) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "color must be one of the allowed swatches"}))).into_response();
        }
        c
    } else {
        cur_color
    };

    let now = match now_millis() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("clock error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    let result = sqlx::query(
        "UPDATE boards SET name = ?, color = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&new_name)
    .bind(&new_color)
    .bind(now)
    .bind(&board_id)
    .execute(&state.write_pool.0)
    .await;

    match result {
        Ok(_) => {
            // Publish Refresh so any open board tabs pick up the name/color change (D-20).
            state.board_rooms.publish(&board_id, crate::models::events::BoardEvent::Refresh);

            let dto = BoardDto {
                id: cur_id,
                name: new_name,
                color: new_color,
                key_prefix: cur_key_prefix,
                archived: cur_archived,
                created_at: cur_created_at,
                updated_at: now,
            };
            (StatusCode::OK, Json(dto)).into_response()
        }
        Err(e) => {
            tracing::error!("update_board REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/boards/{board_id}
// ---------------------------------------------------------------------------

/// Permanently delete a board and all its data (owner-only).
#[utoipa::path(
    delete,
    path = "/api/v1/boards/{board_id}",
    params(("board_id" = String, Path, description = "Board ID")),
    responses(
        (status = 204, description = "Board deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Owner-only operation"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn delete_board(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
) -> Response {
    use crate::api::workspace_api::delete_board_inner;

    // Membership + role gate
    let role = match require_member(&state.read_pool.0, &board_id, &user.id).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if role != "owner" {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "owner only"}))).into_response();
    }

    match delete_board_inner(&state.write_pool.0, &board_id).await {
        Ok(()) => {
            // Publish Refresh so any open board tabs are notified the board is gone (D-20).
            state.board_rooms.publish(&board_id, crate::models::events::BoardEvent::Refresh);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!("delete_board REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}
