//! REST API handlers for `/api/v1/cards/{card_id}/comments`.
//!
//! Board-membership enforced by resolving the card's board_id and replicating the
//! board_members SELECT (D-16 / no IDOR).  Comment create publishes CommentAdded (D-20).

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
        rest_dto::{CommentDto, CreateCommentReq, PaginationParams},
    },
    server::{rest_api::{auth::ApiUser, boards::{require_member, require_member_commenter}}, state::AppState},
};

/// Resolve (board_id, card_id) from just a card_id, verifying the card exists.
/// Returns `Err(Response)` with a generic 404 if the card does not exist.
async fn resolve_card_board(
    pool: &sqlx::SqlitePool,
    card_id: &str,
) -> Result<String, Response> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT board_id FROM cards WHERE id = ?",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("resolve_card_board DB error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
    })?;

    row.map(|(b,)| b).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "card not found"}))).into_response()
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/cards/{card_id}/comments
// ---------------------------------------------------------------------------

/// List comments on a card.
#[utoipa::path(
    get,
    path = "/api/v1/cards/{card_id}/comments",
    params(
        ("card_id" = String, Path, description = "Card ID"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Comment list", body = Vec<CommentDto>),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Card not found or user not a board member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn list_comments(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(card_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Response {
    // Resolve card → board, then membership check
    let board_id = match resolve_card_board(&state.read_pool.0, &card_id).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    let limit = pagination.limit.unwrap_or(50).min(200);

    let rows: Result<Vec<(String, String, String, String, i64)>, sqlx::Error> = sqlx::query_as(
        r#"SELECT id, card_id, author_id, body, created_at
           FROM comments
           WHERE card_id = ?
           ORDER BY created_at ASC
           LIMIT ?"#,
    )
    .bind(&card_id)
    .bind(limit)
    .fetch_all(&state.read_pool.0)
    .await;

    match rows {
        Ok(rows) => {
            let dtos: Vec<CommentDto> = rows
                .into_iter()
                .map(|(id, card_id, author_id, body, created_at)| CommentDto {
                    id,
                    card_id,
                    author_id,
                    body,
                    created_at,
                })
                .collect();
            (StatusCode::OK, Json(dtos)).into_response()
        }
        Err(e) => {
            tracing::error!("list_comments REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/cards/{card_id}/comments
// ---------------------------------------------------------------------------

/// Post a comment on a card.
#[utoipa::path(
    post,
    path = "/api/v1/cards/{card_id}/comments",
    params(("card_id" = String, Path, description = "Card ID")),
    request_body = CreateCommentReq,
    responses(
        (status = 201, description = "Comment created", body = CommentDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Card not found or user not a board member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn create_comment(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(card_id): Path<String>,
    Json(body): Json<CreateCommentReq>,
) -> Response {
    use crate::api::card_detail_api::add_comment_inner;

    // Resolve card → board, then membership check
    let board_id = match resolve_card_board(&state.read_pool.0, &card_id).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_member_commenter(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    // API tokens don't carry mention parsing — no @mentions processing.
    match add_comment_inner(&state.write_pool.0, &board_id, &card_id, &user.id, body.body, vec![]).await {
        Ok((entry, _notified_ids)) => {
            // D-20: publish CommentAdded
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CommentAdded {
                board_seq: seq,
                client_id: "api".to_string(),
                card_id: card_id.clone(),
                comment_id: entry.id.clone(),
                author_id: user.id.clone(),
                text: entry.text.clone(),
                created_at: entry.created_at,
            });

            let dto = CommentDto {
                id: entry.id,
                card_id: card_id.clone(),
                author_id: user.id.clone(),
                body: entry.text,
                created_at: entry.created_at,
            };
            (StatusCode::CREATED, Json(dto)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("cannot be empty") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                tracing::error!("create_comment REST error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
            }
        }
    }
}
