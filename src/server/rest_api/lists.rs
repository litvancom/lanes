//! REST API handlers for `/api/v1/boards/{board_id}/lists`.
//!
//! Board-membership enforced by replicating the board_members SELECT (D-16 / no IDOR).
//! Mutations publish BoardEvents so web clients stay live (D-20).

#![cfg(feature = "ssr")]

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    models::{
        events::BoardEvent,
        rest_dto::{CreateListReq, ListDto, PaginationParams, UpdateListReq},
    },
    server::{rest_api::{auth::ApiUser, boards::{require_member, require_member_editor}}, state::AppState},
};

// ---------------------------------------------------------------------------
// GET /api/v1/boards/{board_id}/lists
// ---------------------------------------------------------------------------

/// List all non-archived lists in a board.
#[utoipa::path(
    get,
    path = "/api/v1/boards/{board_id}/lists",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "List of lists", body = Vec<ListDto>),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn list_lists(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Response {
    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    let limit = pagination.limit.unwrap_or(50).min(200);

    let rows: Result<Vec<(String, String, String, String, bool, bool)>, sqlx::Error> =
        sqlx::query_as(
            r#"SELECT id, board_id, name, position,
                      CAST(archived AS BOOLEAN),
                      CAST(is_done_list AS BOOLEAN)
               FROM lists
               WHERE board_id = ? AND archived = 0
               ORDER BY position ASC
               LIMIT ?"#,
        )
        .bind(&board_id)
        .bind(limit)
        .fetch_all(&state.read_pool.0)
        .await;

    match rows {
        Ok(rows) => {
            let dtos: Vec<ListDto> = rows
                .into_iter()
                .map(|(id, board_id, name, position, archived, is_done_list)| ListDto {
                    id,
                    board_id,
                    name,
                    position,
                    archived,
                    is_done_list,
                })
                .collect();
            (StatusCode::OK, Json(dtos)).into_response()
        }
        Err(e) => {
            tracing::error!("list_lists REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/boards/{board_id}/lists
// ---------------------------------------------------------------------------

/// Create a new list at the end of a board.
#[utoipa::path(
    post,
    path = "/api/v1/boards/{board_id}/lists",
    params(("board_id" = String, Path, description = "Board ID")),
    request_body = CreateListReq,
    responses(
        (status = 201, description = "List created", body = ListDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn create_list(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
    Json(body): Json<CreateListReq>,
) -> Response {
    use crate::api::list_api::{create_list_inner, next_list_position};

    if let Err(resp) = require_member_editor(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    let position = match next_list_position(&state.read_pool.0, &board_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("next_list_position REST error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    match create_list_inner(&state.write_pool.0, &board_id, body.name, &position).await {
        Ok(list) => {
            // D-20: publish so web clients see the new list
            let list_id = list.id.clone();
            let list_name = list.name.clone();
            let list_pos = list.position.clone();
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::ListAdded {
                board_seq: seq,
                client_id: "api".to_string(),
                list_id,
                name: list_name,
                position: list_pos,
            });

            let dto = ListDto {
                id: list.id,
                board_id: list.board_id,
                name: list.name,
                position: list.position,
                archived: list.archived,
                is_done_list: list.is_done_list,
            };
            (StatusCode::CREATED, Json(dto)).into_response()
        }
        Err(e) => {
            // Decode errors are validation failures from create_list_inner
            let msg = e.to_string();
            if msg.contains("cannot be empty") || msg.contains("or fewer") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                tracing::error!("create_list REST error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/boards/{board_id}/lists/{list_id}
// ---------------------------------------------------------------------------

/// Rename a list.
#[utoipa::path(
    patch,
    path = "/api/v1/boards/{board_id}/lists/{list_id}",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        ("list_id" = String, Path, description = "List ID"),
    ),
    request_body = UpdateListReq,
    responses(
        (status = 200, description = "Updated list", body = ListDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn update_list(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path((board_id, list_id)): Path<(String, String)>,
    Json(body): Json<UpdateListReq>,
) -> Response {
    use crate::api::list_api::rename_list_inner;

    if let Err(resp) = require_member_editor(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    // Verify list belongs to this board (IDOR — list_id could be on another board)
    let owning_board: Option<(String,)> = match sqlx::query_as(
        "SELECT board_id FROM lists WHERE id = ?",
    )
    .bind(&list_id)
    .fetch_optional(&state.read_pool.0)
    .await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("update_list list lookup error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    match owning_board {
        Some((b,)) if b == board_id => {}
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "list not found"}))).into_response(),
    }

    let new_name = body.name.clone();
    match rename_list_inner(&state.write_pool.0, &list_id, body.name).await {
        Ok(()) => {
            // D-20: publish ListRenamed
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::ListRenamed {
                board_seq: seq,
                client_id: "api".to_string(),
                list_id: list_id.clone(),
                name: new_name.clone(),
            });

            // Fetch updated list to return
            let row: Option<(String, String, String, String, bool, bool)> = sqlx::query_as(
                r#"SELECT id, board_id, name, position,
                          CAST(archived AS BOOLEAN),
                          CAST(is_done_list AS BOOLEAN)
                   FROM lists WHERE id = ?"#,
            )
            .bind(&list_id)
            .fetch_optional(&state.read_pool.0)
            .await
            .unwrap_or(None);

            match row {
                Some((id, board_id, name, position, archived, is_done_list)) => {
                    let dto = ListDto { id, board_id, name, position, archived, is_done_list };
                    (StatusCode::OK, Json(dto)).into_response()
                }
                None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "list not found"}))).into_response(),
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("cannot be empty") || msg.contains("or fewer") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                tracing::error!("update_list REST error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/boards/{board_id}/lists/{list_id}
// ---------------------------------------------------------------------------

/// Archive a list (soft delete — cards are preserved).
#[utoipa::path(
    delete,
    path = "/api/v1/boards/{board_id}/lists/{list_id}",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        ("list_id" = String, Path, description = "List ID"),
    ),
    responses(
        (status = 204, description = "List archived"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn delete_list(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path((board_id, list_id)): Path<(String, String)>,
) -> Response {
    if let Err(resp) = require_member_editor(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    // Verify list belongs to this board
    let owning_board: Option<(String,)> = match sqlx::query_as(
        "SELECT board_id FROM lists WHERE id = ?",
    )
    .bind(&list_id)
    .fetch_optional(&state.read_pool.0)
    .await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("delete_list list lookup error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    match owning_board {
        Some((b,)) if b == board_id => {}
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "list not found"}))).into_response(),
    }

    let result = sqlx::query("UPDATE lists SET archived = 1 WHERE id = ? AND board_id = ?")
        .bind(&list_id)
        .bind(&board_id)
        .execute(&state.write_pool.0)
        .await;

    match result {
        Ok(_) => {
            // D-20: publish ListArchived
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::ListArchived {
                board_seq: seq,
                client_id: "api".to_string(),
                list_id: list_id.clone(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!("delete_list REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}
